//! AI 对话温数据阶段 — 异步归档到 MemoryStore（SQLite + sqlite-vec）
//!
//! 触发时机：用户关闭当前聊天窗口、切换到其他会话、软件进入空闲状态（30秒无操作）
//! 后台线程把整段完整对话一次性批量写入 [`MemoryStore`]，建立向量索引。
//! 写入成功后删除对应的热数据日志文件，完成「热→温」的状态切换。
//!
//! 底层存储通过 [`MemoryStore`] trait 抽象（见 memory_store.rs），
//! 当前实现为 SqliteMemoryStore，后续可整体替换为 Qdrant Edge / LanceDB 等。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};

use crate::ai_panel::{AiConversation, AiRole, ConversationMeta};
use crate::memory_store::{ChatMessage, Conversation, MemoryStore, SqliteMemoryStore};

/// 向量维度（与当前 ONNX 嵌入模型一致；换 bge-small-zh 时改为 512）
const EMBEDDING_DIM: usize = crate::embedding::EmbeddingModel::DIM;

/// 温数据归档请求
#[derive(Clone, Debug)]
pub enum ArchiveRequest {
    /// 归档指定会话
    ArchiveConversation {
        conv_id: String,
        conv: AiConversation,
    },
    /// 归档所有脏会话
    ArchiveAllDirty { sessions: Vec<AiConversation> },
    /// 删除指定会话的临时日志
    RemoveHotLog { conv_id: String },
    /// 关闭归档线程
    Shutdown,
}

/// 温数据归档结果
#[derive(Clone, Debug)]
pub enum ArchiveResult {
    Success { conv_id: String },
    Failed { conv_id: String, error: String },
}

/// 温数据存储（MemoryStore 适配器 + 后台归档线程）
///
/// 所有写操作通过后台线程异步执行，不阻塞 UI 线程。
pub struct WarmDataStore {
    /// 数据根目录（热日志目录、SQLite 库均在其下）
    base_dir: PathBuf,
    /// 存储适配器（Arc 共享给后台线程；类型擦除便于替换实现）
    store: Arc<dyn MemoryStore>,
    /// 归档请求发送端
    request_tx: Option<Sender<ArchiveRequest>>,
    /// 归档结果接收端
    result_rx: Option<Receiver<ArchiveResult>>,
    /// 后台线程句柄
    worker_handle: Option<std::thread::JoinHandle<()>>,
    /// 当前工作区哈希（VS Code workspaceStorage 同款绑定；归档时写入会话元数据）
    workspace_hash: Arc<RwLock<String>>,
    /// ACE Reflector 的 LLM 客户端（None = 未启用反思）
    reflector_client: Arc<Mutex<Option<aether_ai::AiClient>>>,
}

impl WarmDataStore {
    /// 创建温数据存储（自动初始化 SQLite 数据库）
    pub fn new(base_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&base_dir).map_err(|e| format!("无法创建温数据目录: {}", e))?;

        let store: Arc<dyn MemoryStore> =
            Arc::new(SqliteMemoryStore::open(&base_dir, EMBEDDING_DIM)?);

        let (request_tx, request_rx) = channel::<ArchiveRequest>();
        let (result_tx, result_rx) = channel::<ArchiveResult>();

        let workspace_hash = Arc::new(RwLock::new(String::new()));
        let reflector_client: Arc<Mutex<Option<aether_ai::AiClient>>> = Arc::new(Mutex::new(None));

        let worker_store = Arc::clone(&store);
        let worker_base = base_dir.clone();
        let worker_hash = Arc::clone(&workspace_hash);
        let worker_reflector = Arc::clone(&reflector_client);
        let handle = std::thread::spawn(move || {
            Self::archive_worker(
                worker_store,
                worker_base,
                worker_hash,
                worker_reflector,
                request_rx,
                result_tx,
            );
        });

        Ok(Self {
            base_dir,
            store,
            request_tx: Some(request_tx),
            result_rx: Some(result_rx),
            worker_handle: Some(handle),
            workspace_hash,
            reflector_client,
        })
    }

    /// 设置当前工作区（归档时会话元数据将绑定其哈希）
    pub fn set_workspace(&self, path: &Path) {
        let hash = fnv1a_hex(&path.to_string_lossy());
        if let Ok(mut guard) = self.workspace_hash.write() {
            *guard = hash;
        }
    }

    /// 启用 ACE Reflector（用当前 AI 配置在归档后自动反思沉淀策略条目）
    pub fn enable_reflector(&self, settings: &aether_shared::settings::AiSettings) {
        if settings.api_key.is_empty() {
            return; // 无 API Key 时静默禁用
        }
        if let Ok(mut guard) = self.reflector_client.lock() {
            *guard = Some(aether_ai::AiClient::new(settings));
        }
    }

    /// 检索 playbook 条目并格式化为系统提示注入文本
    pub fn playbook_context(&self, query: &str, k: usize) -> Result<String, String> {
        crate::reflector::playbook_context(self.store.as_ref(), query, k)
    }

    /// 按语义检索 playbook 条目（返回完整条目，供注入时记录使用明细以做反馈归因）
    pub fn search_playbook(
        &self,
        query: &str,
        k: usize,
    ) -> Result<Vec<(crate::memory_store::PlaybookBullet, f32)>, String> {
        let embedding = crate::embedding::embed_text(query);
        self.store.search_bullets(&embedding, k)
    }

    /// 条目反馈（helpful=true 记有效，false 记无效）
    pub fn bullet_feedback(&self, bullet_id: &str, helpful: bool) -> Result<(), String> {
        self.store.bullet_feedback(bullet_id, helpful)
    }

    /// 当前工作区哈希（未设置时为空串）
    pub fn current_workspace_hash(&self) -> String {
        self.workspace_hash
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// 检索会话（关键词 + 可选工作区过滤）
    pub fn search_conversations(
        &self,
        keyword: &str,
        workspace_only: bool,
        limit: usize,
    ) -> Result<Vec<Conversation>, String> {
        let ws = if workspace_only {
            let h = self.current_workspace_hash();
            if h.is_empty() {
                None
            } else {
                Some(h)
            }
        } else {
            None
        };
        self.store
            .search_conversations(keyword, ws.as_deref(), limit)
    }

    /// 列出 playbook 条目（管理面板数据源）
    pub fn list_playbook(
        &self,
        section: Option<&str>,
    ) -> Result<Vec<crate::memory_store::PlaybookBullet>, String> {
        self.store.list_bullets(section)
    }

    /// 删除 playbook 条目（管理面板手动干预接口）
    pub fn delete_bullet(&self, bullet_id: &str) -> Result<(), String> {
        self.store.delete_bullet(bullet_id)
    }

    /// 执行 grow-and-refine 剪枝（传 dry_run=true 可先预览候选）
    pub fn prune_bullets(
        &self,
        config: &crate::memory_store::PruneConfig,
    ) -> Result<crate::memory_store::PruneReport, String> {
        self.store.prune_bullets(config)
    }

    /// 剪枝审计日志
    pub fn list_prune_log(
        &self,
        limit: usize,
    ) -> Result<Vec<crate::memory_store::PruneLogEntry>, String> {
        self.store.list_prune_log(limit)
    }

    /// 后台归档线程主循环
    fn archive_worker(
        store: Arc<dyn MemoryStore>,
        base_dir: PathBuf,
        workspace_hash: Arc<RwLock<String>>,
        reflector_client: Arc<Mutex<Option<aether_ai::AiClient>>>,
        request_rx: Receiver<ArchiveRequest>,
        result_tx: Sender<ArchiveResult>,
    ) {
        // grow-and-refine：每次启动执行一次保守剪枝（高 harmful 条目清理 + 审计日志）
        match store.prune_bullets(&crate::memory_store::PruneConfig::default()) {
            Ok(report) if report.pruned > 0 => {
                eprintln!(
                    "[WarmData] 启动剪枝：清理 {} 条高 harmful 条目",
                    report.pruned
                );
            }
            _ => {}
        }

        while let Ok(req) = request_rx.recv() {
            match req {
                ArchiveRequest::ArchiveConversation { conv_id, conv } => {
                    let hash = workspace_hash.read().map(|g| g.clone()).unwrap_or_default();
                    let result = Self::archive_single(store.as_ref(), &conv_id, &conv, &hash);
                    if result.is_ok() {
                        Self::maybe_reflect(&store, &reflector_client, &conv);
                    }
                    let _ = result_tx.send(match result {
                        Ok(()) => ArchiveResult::Success { conv_id },
                        Err(e) => ArchiveResult::Failed { conv_id, error: e },
                    });
                }
                ArchiveRequest::ArchiveAllDirty { sessions } => {
                    let hash = workspace_hash.read().map(|g| g.clone()).unwrap_or_default();
                    for conv in sessions {
                        let conv_id = conv.id.clone();
                        let result = Self::archive_single(store.as_ref(), &conv_id, &conv, &hash);
                        if result.is_ok() {
                            Self::maybe_reflect(&store, &reflector_client, &conv);
                        }
                        let _ = result_tx.send(match result {
                            Ok(()) => ArchiveResult::Success { conv_id },
                            Err(e) => ArchiveResult::Failed { conv_id, error: e },
                        });
                    }
                }
                ArchiveRequest::RemoveHotLog { conv_id } => {
                    // 删除热数据临时日志
                    let log_path = base_dir.join("hot").join(format!("{}.log", conv_id));
                    if log_path.exists() {
                        let _ = std::fs::remove_file(&log_path);
                    }
                }
                ArchiveRequest::Shutdown => break,
            }
        }

        let _ = store.flush();
    }

    /// 归档成功后视情况执行 ACE 反思（仅对含用户消息的真实对话）
    fn maybe_reflect(
        store: &Arc<dyn MemoryStore>,
        reflector_client: &Arc<Mutex<Option<aether_ai::AiClient>>>,
        conv: &AiConversation,
    ) {
        let has_user_msg = conv.messages.iter().any(|m| m.role == AiRole::User);
        if !has_user_msg || conv.messages.len() < 2 {
            return;
        }
        let guard = match reflector_client.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let client = match guard.as_ref() {
            Some(c) => c,
            None => return,
        };
        match crate::reflector::reflect_and_curate(store.as_ref(), client, conv) {
            Ok(n) if n > 0 => {
                eprintln!("[Reflector] 会话 {} 沉淀了 {} 条策略", conv.id, n);
            }
            Ok(_) => {}
            Err(e) => eprintln!("[Reflector] 反思失败 {}: {}", conv.id, e),
        }
    }

    /// 归档单一会话（幂等：消息 ID 由 conv_id + msg_index 派生，重复归档不产生重复数据）
    fn archive_single(
        store: &dyn MemoryStore,
        conv_id: &str,
        conv: &AiConversation,
        workspace_hash: &str,
    ) -> Result<(), String> {
        // 1. 归档会话元数据
        store.upsert_conversation(&Conversation {
            id: conv_id.to_string(),
            title: conv.title.clone(),
            workspace_hash: workspace_hash.to_string(),
            mode: format!("{:?}", conv.mode),
            created_at: conv.created_at,
            updated_at: conv.updated_at,
            message_count: conv.messages.len() as u32,
        })?;

        // 2. 归档所有消息（稳定 ID + 语义向量）
        for (idx, msg) in conv.messages.iter().enumerate() {
            let embedding = embed_text(&msg.content);
            store.append_message(&ChatMessage {
                id: format!("{}:{}", conv_id, idx),
                conv_id: conv_id.to_string(),
                msg_index: idx as u32,
                role: role_to_str(&msg.role).to_string(),
                content: msg.content.clone(),
                embedding: Some(embedding),
                schema_ver: 1,
                created_at: conv.updated_at,
            })?;
        }

        // 3. 刷写（WAL checkpoint）
        store.flush()?;

        Ok(())
    }

    /// 发送归档请求（非阻塞）
    pub fn request_archive(&self, conv_id: String, conv: AiConversation) {
        if let Some(tx) = &self.request_tx {
            let _ = tx.send(ArchiveRequest::ArchiveConversation { conv_id, conv });
        }
    }

    /// 批量归档所有脏会话
    pub fn request_archive_all(&self, sessions: Vec<AiConversation>) {
        if let Some(tx) = &self.request_tx {
            let _ = tx.send(ArchiveRequest::ArchiveAllDirty { sessions });
        }
    }

    /// 删除热数据临时日志
    pub fn request_remove_hot_log(&self, conv_id: String) {
        if let Some(tx) = &self.request_tx {
            let _ = tx.send(ArchiveRequest::RemoveHotLog { conv_id });
        }
    }

    /// 轮询归档结果（在主线程定时调用）
    pub fn poll_results(&self) -> Vec<ArchiveResult> {
        let mut results = Vec::new();
        if let Some(rx) = &self.result_rx {
            while let Ok(result) = rx.try_recv() {
                results.push(result);
            }
        }
        results
    }

    /// 加载历史会话元数据列表（用于历史列表展示）
    pub fn load_history_meta(&self) -> Result<Vec<ConversationMeta>, String> {
        let convs = self.store.list_conversations(500)?;
        let mut results = Vec::with_capacity(convs.len());
        for c in convs {
            // 取最后一条消息作为预览
            let preview = self
                .store
                .get_messages(&c.id)
                .ok()
                .and_then(|msgs| msgs.last().map(|m| m.content.clone()))
                .unwrap_or_default();
            results.push(ConversationMeta {
                id: c.id,
                title: c.title,
                updated_at: c.updated_at,
                message_count: c.message_count as usize,
                preview: preview.chars().take(50).collect(),
            });
        }
        Ok(results)
    }

    /// 加载完整会话
    pub fn load_conversation(&self, conv_id: &str) -> Result<AiConversation, String> {
        let msgs = self.store.get_messages(conv_id)?;
        if msgs.is_empty() {
            return Err("会话不存在或无消息".to_string());
        }

        let title = self
            .store
            .list_conversations(1000)?
            .into_iter()
            .find(|c| c.id == conv_id)
            .map(|c| c.title)
            .unwrap_or_default();

        let mut conv = AiConversation::new(conv_id.to_string(), title);
        conv.messages = msgs
            .into_iter()
            .map(|m| crate::ai_panel::AiMessage::new(str_to_role(&m.role), m.content))
            .collect();
        Ok(conv)
    }

    /// 语义搜索历史对话（sqlite-vec 向量检索，按会话去重）
    pub fn semantic_search(
        &self,
        query_text: &str,
        k: usize,
    ) -> Result<Vec<(ConversationMeta, f32)>, String> {
        let query_embedding = embed_text(query_text);
        // 候选放大，按会话去重后取前 k
        let results = self.store.search_messages(&query_embedding, None, k * 4)?;

        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (msg, distance) in results {
            if !seen.insert(msg.conv_id.clone()) {
                continue;
            }
            out.push((
                ConversationMeta {
                    id: msg.conv_id.clone(),
                    title: String::new(), // 标题在列表层另行加载
                    updated_at: msg.created_at,
                    message_count: 0,
                    preview: msg.content.chars().take(50).collect(),
                },
                // L2 距离 → 相似度得分（越大越相似）
                1.0 / (1.0 + distance),
            ));
            if out.len() >= k {
                break;
            }
        }
        Ok(out)
    }

    /// 关闭归档线程
    pub fn shutdown(&mut self) {
        if let Some(tx) = &self.request_tx {
            let _ = tx.send(ArchiveRequest::Shutdown);
        }
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
        self.request_tx = None;
        self.result_rx = None;
    }
}

impl Drop for WarmDataStore {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl std::fmt::Debug for WarmDataStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WarmDataStore")
            .field("base_dir", &self.base_dir)
            .finish()
    }
}

// ============================================================================
// 工具函数
// ============================================================================

/// 文本 → 向量（经 embedding 模块的全局 ONNX 模型；未初始化时回退 n-gram 哈希）
fn embed_text(text: &str) -> Vec<f32> {
    crate::embedding::embed_text(text)
}

fn role_to_str(role: &AiRole) -> &'static str {
    match role {
        AiRole::User => "user",
        AiRole::Assistant => "assistant",
        AiRole::System => "system",
    }
}

fn str_to_role(s: &str) -> AiRole {
    match s {
        "user" => AiRole::User,
        "assistant" => AiRole::Assistant,
        _ => AiRole::System,
    }
}

/// 工作区路径 → 短哈希（FNV-1a，VS Code workspaceStorage 同款思路）
fn fnv1a_hex(s: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_panel::AiMessage;

    #[test]
    fn test_archive_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "aether_warm_test_{}",
            crate::memory_store::new_id("d")
        ));
        let store = SqliteMemoryStore::open(&dir, EMBEDDING_DIM).unwrap();

        let mut conv = AiConversation::new("c1".to_string(), "测试会话".to_string());
        conv.messages
            .push(AiMessage::new(AiRole::User, "你好".to_string()));
        conv.messages.push(AiMessage::new(
            AiRole::Assistant,
            "你好！有什么可以帮你？".to_string(),
        ));

        WarmDataStore::archive_single(&store, "c1", &conv, "ws-hash-1").unwrap();
        // 重复归档应幂等（消息数不变）
        WarmDataStore::archive_single(&store, "c1", &conv, "ws-hash-1").unwrap();

        let msgs = store.get_messages("c1").unwrap();
        assert_eq!(msgs.len(), 3); // system 欢迎语 + user + assistant
        assert_eq!(msgs[1].content, "你好");

        // 语义检索能找到归档内容
        let hits = store.search_messages(&embed_text("你好"), None, 5).unwrap();
        assert!(!hits.is_empty());

        let convs = store.list_conversations(10).unwrap();
        assert_eq!(convs.len(), 1);
        assert_eq!(convs[0].title, "测试会话");
        assert_eq!(convs[0].workspace_hash, "ws-hash-1");

        std::fs::remove_dir_all(&dir).ok();
    }
}
