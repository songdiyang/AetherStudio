//! AI 对话三阶段持久化架构 — 热数据 / 温数据 / 冷数据
//!
//! 热数据：内存结构体 + mmap 追加式日志 + 增量差分状态
//! 温数据：异步归档进 SQLite 主库
//! 冷数据：超长期不访问的会话压缩归档

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::ai_panel::{AiConversation, AiMessage, now_secs};

/// 热数据阶段：活跃会话的内存状态 + mmap 增量日志
///
/// 所有读写优先走内存，零磁盘 IO。
/// 通过 mmap 向临时日志文件只追加写入增量内容。
#[derive(Debug)]
pub struct HotDataStore {
    /// 活跃会话的内存状态（完整消息列表 + 元数据）
    conversations: Vec<AiConversation>,
    /// 当前活动会话下标
    active: usize,
    /// mmap 增量日志写入器
    log_writer: Option<MmapLogWriter>,
    /// 自上次归档以来发生变更的会话 ID 集合（用于温数据阶段增量归档）
    dirty_conversations: std::collections::HashSet<String>,
    /// 是否处于活跃状态（false 时触发温数据归档）
    is_active: AtomicBool,
    /// 最后活跃时间戳（用于判断空闲）
    last_activity_at: AtomicU64,
}

/// mmap 追加式日志写入器
///
/// 每轮对话只写入「新增消息 + 发生变化的元数据」，
/// 已存在的历史消息、固定系统提示绝不重复写入。
#[derive(Debug)]
pub struct MmapLogWriter {
    /// 日志文件路径（%APPDATA%/Aether/conversations/hot/{conv_id}.log）
    log_path: PathBuf,
    /// 内存映射文件
    mmap: Option<memmap2::MmapMut>,
    /// 当前写入偏移（文件末尾）
    write_offset: usize,
}

/// 增量日志条目（追加式写入）
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum LogEntry {
    /// 新增消息
    NewMessage {
        conv_id: String,
        msg_index: usize,
        message: AiMessage,
        timestamp: u64,
    },
    /// 元数据变更（标题、模式、附件等）
    MetaChanged {
        conv_id: String,
        title: Option<String>,
        mode: Option<crate::ai_prompt::AiMode>,
        updated_at: u64,
    },
    /// 会话创建
    ConversationCreated {
        conv_id: String,
        title: String,
        created_at: u64,
        mode: crate::ai_prompt::AiMode,
    },
    /// 会话关闭（标记归档点）
    ConversationClosed {
        conv_id: String,
        closed_at: u64,
    },
}

impl HotDataStore {
    /// 创建热数据存储（自动初始化 mmap 日志目录）
    pub fn new(base_dir: PathBuf) -> Result<Self, String> {
        let hot_dir = base_dir.join("hot");
        if let Err(e) = std::fs::create_dir_all(&hot_dir) {
            return Err(format!("无法创建热数据目录: {}", e));
        }
        Ok(Self {
            conversations: Vec::new(),
            active: 0,
            log_writer: None,
            dirty_conversations: std::collections::HashSet::new(),
            is_active: AtomicBool::new(true),
            last_activity_at: AtomicU64::new(now_secs()),
        })
    }

    /// 从 AiPanel 加载活跃会话状态
    pub fn load_from_panel(&mut self, panel: &crate::ai_panel::AiPanel) {
        self.conversations = panel.conversations.clone();
        self.active = panel.active;
        for conv in &self.conversations {
            self.dirty_conversations.insert(conv.id.clone());
        }
    }

    /// 同步 AiPanel 的当前状态到热数据（每次消息变更后调用）
    /// 
    /// 注意：panel 参数以值传递（克隆）传入，避免借用冲突
    pub fn sync_from_panel(&mut self, panel: crate::ai_panel::AiPanel) {
        self.last_activity_at.store(now_secs(), Ordering::Relaxed);
        self.is_active.store(true, Ordering::Relaxed);

        // 先收集所有需要追加的日志条目，避免在循环中多次借用 self
        let mut entries_to_append: Vec<LogEntry> = Vec::new();
        let mut dirty_ids: Vec<String> = Vec::new();

        // 检测哪些会话发生了变更
        for (i, conv) in panel.conversations.iter().enumerate() {
            if let Some(existing) = self.conversations.get_mut(i) {
                if existing.messages.len() != conv.messages.len() {
                    // 消息数量变化 → 追加新消息到日志
                    let new_count = conv.messages.len().saturating_sub(existing.messages.len());
                    for offset in 0..new_count {
                        let msg_idx = existing.messages.len() + offset;
                        if let Some(msg) = conv.messages.get(msg_idx) {
                            entries_to_append.push(LogEntry::NewMessage {
                                conv_id: conv.id.clone(),
                                msg_index: msg_idx,
                                message: msg.clone(),
                                timestamp: now_secs(),
                            });
                        }
                    }
                    existing.messages = conv.messages.clone();
                    existing.updated_at = conv.updated_at;
                    dirty_ids.push(conv.id.clone());
                }
                if existing.title != conv.title || existing.mode != conv.mode {
                    entries_to_append.push(LogEntry::MetaChanged {
                        conv_id: conv.id.clone(),
                        title: Some(conv.title.clone()),
                        mode: Some(conv.mode),
                        updated_at: conv.updated_at,
                    });
                    existing.title = conv.title.clone();
                    existing.mode = conv.mode;
                    dirty_ids.push(conv.id.clone());
                }
            } else {
                // 新会话
                entries_to_append.push(LogEntry::ConversationCreated {
                    conv_id: conv.id.clone(),
                    title: conv.title.clone(),
                    created_at: conv.created_at,
                    mode: conv.mode,
                });
                self.conversations.push(conv.clone());
                dirty_ids.push(conv.id.clone());
            }
        }

        // 统一追加日志
        for entry in entries_to_append {
            self.append_log(entry);
        }
        for id in dirty_ids {
            self.dirty_conversations.insert(id);
        }
    }

    /// 追加日志条目到 mmap 文件
    fn append_log(&mut self, entry: LogEntry) {
        if let Some(writer) = &mut self.log_writer {
            let _ = writer.append(entry);
        }
    }

    /// 标记会话进入非活跃状态（触发温数据归档）
    pub fn deactivate(&mut self, conv_id: &str) {
        self.is_active.store(false, Ordering::Relaxed);
        self.append_log(LogEntry::ConversationClosed {
            conv_id: conv_id.to_string(),
            closed_at: now_secs(),
        });
    }

    /// 获取需要归档到温数据的会话列表
    pub fn dirty_sessions(&self) -> Vec<&AiConversation> {
        self.conversations
            .iter()
            .filter(|c| self.dirty_conversations.contains(&c.id))
            .collect()
    }

    /// 清除已归档会话的脏标记
    pub fn clear_dirty(&mut self, conv_id: &str) {
        self.dirty_conversations.remove(conv_id);
    }

    /// 判断是否应该触发温数据归档（空闲 30 秒）
    pub fn should_warm_archive(&self) -> bool {
        let elapsed = now_secs() - self.last_activity_at.load(Ordering::Relaxed);
        elapsed >= 30 && !self.dirty_conversations.is_empty()
    }

    /// 关闭并清理热数据（退出应用时）
    pub fn shutdown(&mut self) {
        if let Some(writer) = &mut self.log_writer {
            let _ = writer.flush();
        }
    }
}

impl MmapLogWriter {
    /// 创建或打开指定会话的 mmap 日志文件
    pub fn open(conv_id: &str, hot_dir: &PathBuf) -> Result<Self, String> {
        let log_path = hot_dir.join(format!("{}.log", conv_id));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&log_path)
            .map_err(|e| format!("打开日志文件失败: {}", e))?;

        // 预分配 1MB 增长空间
        let initial_size = 1024 * 1024;
        let current_len = file.metadata().map(|m| m.len()).unwrap_or(0) as usize;
        let _target_size = if current_len < initial_size {
            file.set_len(initial_size as u64)
                .map_err(|e| format!("预分配日志文件失败: {}", e))?;
            initial_size
        } else {
            current_len
        };

        let mmap = unsafe {
            memmap2::MmapMut::map_mut(&file)
                .map_err(|e| format!("mmap 日志文件失败: {}", e))?
        };

        Ok(Self {
            log_path,
            mmap: Some(mmap),
            write_offset: current_len,
        })
    }

    /// 追加单条日志条目（序列化为 JSON Lines 格式）
    pub fn append(&mut self, entry: LogEntry) -> Result<(), String> {
        let line = serde_json::to_string(&entry)
            .map_err(|e| format!("日志序列化失败: {}", e))?;
        let bytes = line.as_bytes();
        let len = bytes.len();
        let newline_len = 1; // '\n'
        let total = len + newline_len;

        // 检查是否需要扩容
        if let Some(mmap) = &mut self.mmap {
            let capacity = mmap.len();
            if self.write_offset + total > capacity {
                // 扩容：先 flush 关闭当前 mmap，扩大文件，重新映射
                if let Err(e) = mmap.flush() {
                    return Err(format!("mmap flush 失败: {}", e));
                }
                self.mmap = None;
                let new_size = (capacity * 2).max(self.write_offset + total + 1024 * 1024);
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&self.log_path)
                    .map_err(|e| format!("重新打开日志文件失败: {}", e))?;
                file.set_len(new_size as u64)
                    .map_err(|e| format!("扩容日志文件失败: {}", e))?;
                let new_mmap = unsafe {
                    memmap2::MmapMut::map_mut(&file)
                        .map_err(|e| format!("重新 mmap 失败: {}", e))?
                };
                self.mmap = Some(new_mmap);
            }

            if let Some(mmap) = &mut self.mmap {
                mmap[self.write_offset..self.write_offset + len].copy_from_slice(bytes);
                mmap[self.write_offset + len] = b'\n';
                self.write_offset += total;
            }
        }
        Ok(())
    }

    /// 强制刷写到磁盘
    pub fn flush(&mut self) -> Result<(), String> {
        if let Some(mmap) = &mut self.mmap {
            mmap.flush()
                .map_err(|e| format!("mmap flush 失败: {}", e))?;
        }
        Ok(())
    }
}

/// 原子 u64（std 中没有 AtomicU64 在部分平台，这里用 Mutex 包装）
#[derive(Debug)]
pub struct AtomicU64 {
    value: Mutex<u64>,
}

impl AtomicU64 {
    pub fn new(v: u64) -> Self {
        Self {
            value: Mutex::new(v),
        }
    }
    pub fn load(&self, _ordering: Ordering) -> u64 {
        *self.value.lock().unwrap()
    }
    pub fn store(&self, v: u64, _ordering: Ordering) {
        *self.value.lock().unwrap() = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_entry_serialization() {
        let entry = LogEntry::NewMessage {
            conv_id: "conv-123".to_string(),
            msg_index: 0,
            message: crate::ai_panel::AiMessage::new(
                crate::ai_panel::AiRole::User,
                "你好".to_string(),
            ),
            timestamp: 1234567890,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("conv-123"));
        assert!(json.contains("你好"));
    }

    #[test]
    fn test_hot_data_should_archive_after_idle() {
        let temp_dir = std::env::temp_dir().join("aether_test_hot");
        let _ = std::fs::remove_dir_all(&temp_dir);
        let mut store = HotDataStore::new(temp_dir.clone()).unwrap();
        store.last_activity_at.store(now_secs() - 31, Ordering::Relaxed);
        assert!(store.should_warm_archive());
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
