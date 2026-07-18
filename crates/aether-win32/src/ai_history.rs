use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ai_panel::{AiConversation, AiMessage, ConversationMeta};

/// 会话索引文件（conversations/index.json）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ConversationIndex {
    pub conversations: Vec<ConversationMeta>,
    /// 索引版本，便于未来迁移
    #[serde(default)]
    pub version: u32,
}

/// 单个会话持久化文件（conversations/conv-{id}.json）
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConversationFile {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub messages: Vec<AiMessage>,
    pub mode: crate::ai_prompt::AiMode,
}

impl ConversationFile {
    /// 从 AiConversation 创建持久化文件结构
    pub fn from_conversation(conv: &AiConversation) -> Self {
        Self {
            id: conv.id.clone(),
            title: conv.title.clone(),
            created_at: conv.created_at,
            updated_at: conv.updated_at,
            messages: conv.messages.clone(),
            mode: conv.mode,
        }
    }

    /// 转换为 AiConversation（用于恢复）
    pub fn to_conversation(&self) -> AiConversation {
        let mut conv = AiConversation::new(self.id.clone(), self.title.clone());
        conv.created_at = self.created_at;
        conv.updated_at = self.updated_at;
        conv.messages = self.messages.clone();
        conv.mode = self.mode;
        conv
    }
}

/// AI 历史持久化存储
#[derive(Clone, Debug)]
pub struct AiHistoryStore {
    base_dir: PathBuf,
}

impl Default for AiHistoryStore {
    fn default() -> Self {
        Self::with_default_dir().unwrap_or_else(|_| Self {
            base_dir: std::env::temp_dir().join("aether_conversations"),
        })
    }
}

impl AiHistoryStore {
    const INDEX_FILE: &'static str = "index.json";
    const VERSION: u32 = 1;

    /// 创建存储实例（目录不存在则自动创建）
    pub fn new(base_dir: PathBuf) -> Result<Self, String> {
        if let Err(e) = std::fs::create_dir_all(&base_dir) {
            return Err(format!(
                "无法创建历史记录目录 {}: {}",
                base_dir.display(),
                e
            ));
        }
        Ok(Self { base_dir })
    }

    /// 默认存储路径：%APPDATA%/Aether/conversations
    pub fn with_default_dir() -> Result<Self, String> {
        let dir = Self::default_dir();
        Self::new(dir)
    }

    fn default_dir() -> PathBuf {
        let config_dir = dirs::config_dir().unwrap_or_else(std::env::temp_dir);
        config_dir.join("Aether").join("conversations")
    }

    fn index_path(&self) -> PathBuf {
        self.base_dir.join(Self::INDEX_FILE)
    }

    fn conv_path(&self, id: &str) -> PathBuf {
        self.base_dir.join(format!("conv-{}.json", id))
    }

    // ===== 索引读写 =====

    /// 读取索引文件
    pub fn load_index(&self) -> ConversationIndex {
        let path = self.index_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            match serde_json::from_str::<ConversationIndex>(&content) {
                Ok(index) => return index,
                Err(e) => {
                    eprintln!("警告: 历史索引解析失败: {}, 将重建", e);
                }
            }
        }
        ConversationIndex {
            version: Self::VERSION,
            ..Default::default()
        }
    }

    /// 保存索引文件
    pub fn save_index(&self, index: &ConversationIndex) -> Result<(), String> {
        let path = self.index_path();
        let content = match serde_json::to_string_pretty(index) {
            Ok(c) => c,
            Err(e) => return Err(format!("索引序列化失败: {}", e)),
        };
        // 原子写入：先写临时文件再重命名
        let tmp = path.with_extension("tmp");
        if let Err(e) = std::fs::write(&tmp, content) {
            return Err(format!("索引写入失败: {}", e));
        }
        if let Err(e) = std::fs::rename(&tmp, &path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("索引重命名失败: {}", e));
        }
        Ok(())
    }

    // ===== 会话内容读写 =====

    /// 保存单个会话到磁盘
    pub fn save_conversation(&self, conv: &AiConversation) -> Result<(), String> {
        let file = ConversationFile::from_conversation(conv);
        let path = self.conv_path(&conv.id);
        let content = match serde_json::to_string_pretty(&file) {
            Ok(c) => c,
            Err(e) => return Err(format!("会话序列化失败: {}", e)),
        };
        let tmp = path.with_extension("tmp");
        if let Err(e) = std::fs::write(&tmp, content) {
            return Err(format!("会话写入失败: {}", e));
        }
        if let Err(e) = std::fs::rename(&tmp, &path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("会话重命名失败: {}", e));
        }
        Ok(())
    }

    /// 从磁盘加载单个会话
    pub fn load_conversation(&self, id: &str) -> Option<AiConversation> {
        let path = self.conv_path(id);
        if let Ok(content) = std::fs::read_to_string(&path) {
            match serde_json::from_str::<ConversationFile>(&content) {
                Ok(file) => return Some(file.to_conversation()),
                Err(e) => {
                    eprintln!("警告: 会话文件 {} 解析失败: {}", path.display(), e);
                }
            }
        }
        None
    }

    /// 删除会话文件
    pub fn delete_conversation(&self, id: &str) -> Result<(), String> {
        let path = self.conv_path(id);
        if path.exists() {
            if let Err(e) = std::fs::remove_file(&path) {
                return Err(format!("删除会话文件失败: {}", e));
            }
        }
        Ok(())
    }

    // ===== 批量操作 =====

    /// 保存索引 + 所有会话（完整持久化）
    pub fn save_all(&self, history: &[ConversationMeta]) -> Result<(), String> {
        let index = ConversationIndex {
            version: Self::VERSION,
            conversations: history.to_vec(),
        };
        self.save_index(&index)
    }

    /// 从索引加载历史元数据（不含完整消息）
    pub fn load_history_meta(&self) -> Vec<ConversationMeta> {
        let index = self.load_index();
        index.conversations
    }

    /// 清理孤立的会话文件（索引中不存在但磁盘上存在的文件）
    pub fn cleanup_orphans(&self, history: &[ConversationMeta]) -> Result<(), String> {
        let valid_ids: std::collections::HashSet<String> =
            history.iter().map(|h| h.id.clone()).collect();
        let entries = match std::fs::read_dir(&self.base_dir) {
            Ok(e) => e,
            Err(e) => return Err(format!("读取目录失败: {}", e)),
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("conv-") && name_str.ends_with(".json") {
                let id = name_str
                    .trim_start_matches("conv-")
                    .trim_end_matches(".json");
                if !valid_ids.contains(id) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_conversation_file() {
        use crate::ai_panel::{AiMessage, AiRole};
        let mut conv = AiConversation::new("test-123".to_string(), "测试".to_string());
        conv.messages.push(AiMessage::new(AiRole::User, "你好".to_string()));
        conv.messages.push(AiMessage::new(AiRole::Assistant, "你好！".to_string()));

        let file = ConversationFile::from_conversation(&conv);
        let json = serde_json::to_string_pretty(&file).unwrap();
        let restored: ConversationFile = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.id, "test-123");
        assert_eq!(restored.title, "测试");
        assert_eq!(restored.messages.len(), 2);
        assert_eq!(restored.messages[0].content, "你好");
        assert_eq!(restored.messages[1].content, "你好！");
    }
}
