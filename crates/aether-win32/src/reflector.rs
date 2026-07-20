//! ACE Reflector / Curator — 对话反思与策略条目沉淀
//!
//! 实现 ACE（arXiv:2510.04618）的核心循环：
//! - Generator：即现有 agent 对话循环（不在此模块）
//! - Reflector：归档后对会话做一次 LLM 反思，蒸馏出可复用策略（[`build_reflect_prompt`]）
//! - Curator：**纯代码确定性合并**（[`curate_bullets`]），向量去重，禁止 LLM 整体改写，
//!   从根源避免 context collapse
//!
//! 条目带 helpful/harmful 计数器，作为权重演化信号；
//! 注入上下文时用 [`playbook_context`] 按语义相关性检索。

use serde::Deserialize;

use crate::ai_panel::AiConversation;
use crate::memory_store::{MemoryStore, PlaybookBullet};

/// 向量去重阈值（L2 距离；归一化向量下约等于余弦相似度 > 0.9）
const DEDUP_MAX_DISTANCE: f32 = 0.45;

/// 反思产物：一条新策略
#[derive(Clone, Debug, Deserialize)]
pub struct NewBullet {
    /// 分类：tool_use / coding_style / pitfalls / project_facts / workflow
    pub section: String,
    pub content: String,
}

// ============================================================================
// Reflector：会话 → 策略候选（LLM 调用在 ai_warm_data 的后台线程中执行）
// ============================================================================

/// 构造反思 prompt（ACE Reflector 角色）
///
/// 输入会话轨迹，要求 LLM 输出 JSON 数组形式的策略条目。
/// 只提取可复用经验，不复述对话内容。
pub fn build_reflect_prompt(conv: &AiConversation) -> String {
    // 截取最近若干轮，控制 token 开销
    let mut transcript = String::new();
    for msg in conv.messages.iter().rev().take(12).rev() {
        let role = format!("{:?}", msg.role);
        let content: String = msg.content.chars().take(600).collect();
        transcript.push_str(&format!("【{}】{}\n\n", role, content));
    }

    format!(
        "你是一个代码编辑器 AI 助手的「反思器」。下面是一段刚结束的对话轨迹。\n\
         请从中提炼**可跨会话复用的经验策略**，供助手以后参考。\n\n\
         要求：\n\
         - 只提炼有长期价值的策略：工具使用技巧、该项目的事实/约定、踩过的坑、有效的协作流程\n\
         - 不要复述对话内容，不要记录一次性的问答\n\
         - 每条策略是一句具体、可执行的陈述（不超过 80 字）\n\
         - 如果没有值得沉淀的内容，返回空数组 []\n\n\
         严格输出 JSON 数组（不要输出其他内容）：\n\
         [{{\"section\": \"tool_use|coding_style|pitfalls|project_facts|workflow\", \"content\": \"...\"}}]\n\n\
         对话轨迹：\n{}",
        transcript
    )
}

/// 解析 LLM 反思输出为策略列表（容错：容忍 markdown 代码围栏和前后杂文本）
pub fn parse_bullets(response: &str) -> Vec<NewBullet> {
    // 提取第一个 '[' 到最后一个 ']' 之间的 JSON
    let start = match response.find('[') {
        Some(i) => i,
        None => return Vec::new(),
    };
    let end = match response.rfind(']') {
        Some(i) if i > start => i,
        _ => return Vec::new(),
    };
    let json_str = &response[start..=end];

    serde_json::from_str::<Vec<NewBullet>>(json_str)
        .unwrap_or_default()
        .into_iter()
        .filter(|b| !b.content.trim().is_empty())
        .collect()
}

// ============================================================================
// Curator：确定性合并（无 LLM，向量去重 + 计数器更新）
// ============================================================================

/// 将策略候选合并进 playbook（确定性操作）
///
/// 语义近似的已有条目 → helpful_count + 1（强化）；否则插入新条目。
/// 返回新插入的条目数。
pub fn curate_bullets(store: &dyn MemoryStore, bullets: Vec<NewBullet>) -> Result<usize, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut inserted = 0;
    for bullet in bullets {
        let embedding = crate::embedding::embed_text(&bullet.content);

        // 向量查重：最近邻距离小于阈值视为同一条策略
        let nearest = store.search_bullets(&embedding, 1)?;
        if let Some((existing, distance)) = nearest.first() {
            if *distance < DEDUP_MAX_DISTANCE {
                store.bullet_feedback(&existing.id, true)?;
                continue;
            }
        }

        store.upsert_bullet(&PlaybookBullet {
            id: crate::memory_store::new_id("b"),
            section: bullet.section,
            content: bullet.content,
            helpful_count: 0,
            harmful_count: 0,
            embedding: Some(embedding),
            created_at: now,
            updated_at: now,
        })?;
        inserted += 1;
    }
    Ok(inserted)
}

/// 完整反思-沉淀流程（在后台线程调用）
///
/// 返回新沉淀的条目数。LLM 调用失败会向上传播，由调用方记录日志。
pub fn reflect_and_curate(
    store: &dyn MemoryStore,
    client: &aether_ai::AiClient,
    conv: &AiConversation,
) -> Result<usize, String> {
    let prompt = build_reflect_prompt(conv);
    let response = client
        .complete(&prompt)
        .map_err(|e| format!("反思 LLM 调用失败: {}", e))?;
    let bullets = parse_bullets(&response);
    if bullets.is_empty() {
        return Ok(0);
    }
    curate_bullets(store, bullets)
}

// ============================================================================
// 检索注入：组装 playbook 上下文（供系统提示注入）
// ============================================================================

/// 按查询语义检索 playbook 条目，格式化为可注入系统提示的文本
///
/// 排序依据向量距离；条目附 helpful/harmful 计数供模型参考权重。
pub fn playbook_context(store: &dyn MemoryStore, query: &str, k: usize) -> Result<String, String> {
    let embedding = crate::embedding::embed_text(query);
    let hits = store.search_bullets(&embedding, k)?;
    Ok(format_bullets(&hits))
}

/// 将检索到的条目格式化为系统提示文本（空列表返回空串）
pub fn format_bullets(hits: &[(PlaybookBullet, f32)]) -> String {
    if hits.is_empty() {
        return String::new();
    }
    let mut out = String::from("## 已沉淀的经验策略（按相关性排序）\n");
    for (bullet, _distance) in hits {
        out.push_str(&format!(
            "- [{}] {}（有效 {} 次 / 无效 {} 次）\n",
            bullet.section, bullet.content, bullet.helpful_count, bullet.harmful_count
        ));
    }
    out
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_panel::{AiConversation, AiMessage, AiRole};
    use crate::memory_store::SqliteMemoryStore;

    #[test]
    fn test_parse_bullets_clean_json() {
        let resp = r#"[{"section": "tool_use", "content": "git 操作前先检查工作区状态"}]"#;
        let bullets = parse_bullets(resp);
        assert_eq!(bullets.len(), 1);
        assert_eq!(bullets[0].section, "tool_use");
    }

    #[test]
    fn test_parse_bullets_with_markdown_fence() {
        let resp = "以下是提炼的策略：\n```json\n[{\"section\": \"pitfalls\", \"content\": \"不要在锁内做 IO\"}]\n```\n希望有帮助";
        let bullets = parse_bullets(resp);
        assert_eq!(bullets.len(), 1);
        assert_eq!(bullets[0].content, "不要在锁内做 IO");
    }

    #[test]
    fn test_parse_bullets_empty_and_garbage() {
        assert!(parse_bullets("[]").is_empty());
        assert!(parse_bullets("没有值得沉淀的内容").is_empty());
        assert!(parse_bullets("{not json}").is_empty());
        // 空内容条目被过滤
        assert!(parse_bullets(r#"[{"section": "x", "content": "  "}]"#).is_empty());
    }

    #[test]
    fn test_build_reflect_prompt_contains_transcript() {
        let mut conv = AiConversation::new("c1".into(), "t".into());
        conv.messages
            .push(AiMessage::new(AiRole::User, "如何配置 LSP？".into()));
        let prompt = build_reflect_prompt(&conv);
        assert!(prompt.contains("如何配置 LSP？"));
        assert!(prompt.contains("JSON"));
    }

    #[test]
    fn test_curate_bullets_dedup() {
        let dir = std::env::temp_dir().join(format!(
            "aether_reflect_test_{}",
            crate::memory_store::new_id("d")
        ));
        let store = SqliteMemoryStore::open(&dir, crate::embedding::EmbeddingModel::DIM).unwrap();

        // 第一次：插入新条目
        let n = curate_bullets(
            &store,
            vec![NewBullet {
                section: "tool_use".into(),
                content: "git 操作前先检查工作区状态".into(),
            }],
        )
        .unwrap();
        assert_eq!(n, 1);

        // 第二次：完全相同的条目应被去重，转为 helpful+1
        let n = curate_bullets(
            &store,
            vec![NewBullet {
                section: "tool_use".into(),
                content: "git 操作前先检查工作区状态".into(),
            }],
        )
        .unwrap();
        assert_eq!(n, 0);

        let bullets = store.list_bullets(None).unwrap();
        assert_eq!(bullets.len(), 1);
        assert_eq!(bullets[0].helpful_count, 1);

        // playbook_context 能检索到
        let ctx = playbook_context(&store, "git 工作区", 5).unwrap();
        assert!(ctx.contains("git 操作前先检查工作区状态"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
