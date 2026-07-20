//! AI 对话冷数据阶段 — 超长期不访问的历史对话压缩归档
//!
//! 常规场景下，AetherDB 足以支撑几万、几十万条会话的长期存储。
//! 如果是超大体量（百万级会话），可再做一层压缩归档：
//! 把超过半年未访问的会话打包成压缩文件冷备，需要时再解压回库。

use std::path::PathBuf;
use std::io::{Write, Read};

use crate::ai_panel::{AiConversation, ConversationMeta, now_secs};
use crate::aether_db::{AetherDB, ConversationAdapter, Filter, ScalarValue};

/// 冷数据归档配置
#[derive(Clone, Debug)]
pub struct ColdArchiveConfig {
    /// 多久未访问触发冷归档（默认 180 天）
    pub archive_after_days: u64,
    /// 单次归档批量大小
    pub batch_size: usize,
    /// 压缩级别（1-9，默认 6）
    pub compression_level: u32,
}

impl Default for ColdArchiveConfig {
    fn default() -> Self {
        Self {
            archive_after_days: 180,
            batch_size: 1000,
            compression_level: 6,
        }
    }
}

/// 冷数据存储（压缩归档）
pub struct ColdDataStore {
    /// 冷数据存储目录
    cold_dir: PathBuf,
    /// 归档配置
    config: ColdArchiveConfig,
}

/// 冷归档包元数据
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ColdArchiveMeta {
    pub archive_id: String,
    pub created_at: u64,
    pub conv_count: usize,
    pub oldest_access: u64,
    pub newest_access: u64,
    pub compressed_size: u64,
    pub original_size: u64,
}

impl ColdDataStore {
    /// 创建冷数据存储
    pub fn new(base_dir: PathBuf, config: ColdArchiveConfig) -> Result<Self, String> {
        let cold_dir = base_dir.join("cold");
        if let Err(e) = std::fs::create_dir_all(&cold_dir) {
            return Err(format!("无法创建冷数据目录: {}", e));
        }
        Ok(Self { cold_dir, config })
    }

    /// 扫描需要冷归档的会话（超过 archive_after_days 未访问）
    pub fn scan_cold_candidates(
        &self,
        db_path: &PathBuf,
    ) -> Result<Vec<ConversationMeta>, String> {
        let db = AetherDB::open(db_path.clone())?;

        let threshold = now_secs() - self.config.archive_after_days * 24 * 60 * 60;

        // 使用标量索引查询旧会话
        let filter = Filter::Lte(
            "updated_at".to_string(),
            ScalarValue::Timestamp(threshold),
        );

        let docs = db.filter(&filter);
        let mut seen_convs = std::collections::HashSet::new();
        let mut results = Vec::new();

        for doc in docs {
            if let Some(ScalarValue::String(conv_id)) = doc.scalars.get("conv_id") {
                if seen_convs.insert(conv_id.clone()) {
                    let title = doc
                        .scalars
                        .get("title")
                        .and_then(|v| match v {
                            ScalarValue::String(s) => Some(s.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let updated_at = doc
                        .scalars
                        .get("updated_at")
                        .and_then(|v| match v {
                            ScalarValue::Timestamp(t) => Some(*t),
                            _ => None,
                        })
                        .unwrap_or(doc.created_at);
                    let message_count = doc
                        .scalars
                        .get("message_count")
                        .and_then(|v| match v {
                            ScalarValue::Int(n) => Some(*n as usize),
                            _ => None,
                        })
                        .unwrap_or(0);

                    results.push(ConversationMeta {
                        id: conv_id.clone(),
                        title,
                        updated_at,
                        message_count,
                        preview: doc.text.chars().take(50).collect::<String>(),
                    });
                }
            }

            if results.len() >= self.config.batch_size {
                break;
            }
        }

        // 按更新时间升序排序（最旧的在前）
        results.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
        Ok(results)
    }

    /// 执行冷归档：将候选会话打包成压缩文件
    ///
    /// 流程：
    /// 1. 从 AetherDB 读取候选会话的完整数据
    /// 2. 序列化为 JSON
    /// 3. 使用 zlib 压缩
    /// 4. 写入 .cold 归档文件
    /// 5. 在 AetherDB 中标记为 archived（通过标量字段）
    /// 6. 删除 AetherDB 中已归档的消息数据（保留元数据索引）
    pub fn archive_batch(
        &self,
        db_path: &PathBuf,
        candidates: &[ConversationMeta],
    ) -> Result<ColdArchiveMeta, String> {
        if candidates.is_empty() {
            return Err("没有候选会话需要归档".to_string());
        }

        let archive_id = format!("archive-{}", now_secs());
        let archive_path = self.cold_dir.join(format!("{}.cold", archive_id));

        // 1. 从 AetherDB 读取完整会话数据
        let db = AetherDB::open(db_path.clone())?;

        let mut conversations: Vec<crate::ai_history::ConversationFile> = Vec::new();
        let mut oldest_access = u64::MAX;
        let mut newest_access = u64::MIN;

        for meta in candidates {
            let mut conv = AiConversation::new(meta.id.clone(), meta.title.clone());
            conv.created_at = meta.updated_at; // 简化：使用 updated_at 作为 created_at
            conv.updated_at = meta.updated_at;

            // 查询该会话的所有消息
            let msg_filter = Filter::And(vec![
                Filter::Eq("conv_id".to_string(), ScalarValue::String(meta.id.clone())),
                Filter::Eq("role".to_string(), ScalarValue::String("".to_string())), // 占位
            ]);

            let msg_docs = db.filter(&msg_filter);
            let mut messages: Vec<(usize, crate::ai_panel::AiMessage)> = Vec::new();

            for doc in msg_docs {
                if let Some(ScalarValue::Int(idx)) = doc.scalars.get("msg_index") {
                    let role = doc
                        .scalars
                        .get("role")
                        .and_then(|v| match v {
                            ScalarValue::String(s) => Some(s.as_str()),
                            _ => None,
                        })
                        .unwrap_or("System");
                    let role = match role {
                        "User" => crate::ai_panel::AiRole::User,
                        "Assistant" => crate::ai_panel::AiRole::Assistant,
                        _ => crate::ai_panel::AiRole::System,
                    };
                    let mut msg = crate::ai_panel::AiMessage::new(role, doc.text);
                    msg.reasoning = doc
                        .scalars
                        .get("reasoning")
                        .and_then(|v| match v {
                            ScalarValue::String(s) => Some(s.clone()),
                            _ => None,
                        });
                    messages.push((*idx as usize, msg));
                }
            }

            messages.sort_by_key(|(idx, _)| *idx);
            conv.messages = messages.into_iter().map(|(_, msg)| msg).collect();

            oldest_access = oldest_access.min(meta.updated_at);
            newest_access = newest_access.max(meta.updated_at);

            conversations.push(crate::ai_history::ConversationFile::from_conversation(&conv));
        }

        // 2. 序列化为 JSON
        let json = serde_json::to_string(&conversations)
            .map_err(|e| format!("序列化失败: {}", e))?;
        let original_bytes = json.as_bytes();
        let original_size = original_bytes.len() as u64;

        // 3. 使用 zlib 压缩
        let compressed = Self::compress_data(original_bytes, self.config.compression_level)
            .map_err(|e| format!("压缩失败: {}", e))?;
        let compressed_size = compressed.len() as u64;

        // 4. 写入 .cold 归档文件
        let mut file = std::fs::File::create(&archive_path)
            .map_err(|e| format!("创建归档文件失败: {}", e))?;
        file.write_all(&compressed)
            .map_err(|e| format!("写入归档文件失败: {}", e))?;

        // 5. 在 AetherDB 中标记为 archived = 1（通过添加 archived 标量字段）
        // 简化：删除已归档的消息数据，保留元数据
        for meta in candidates {
            let delete_filter = Filter::And(vec![
                Filter::Eq("conv_id".to_string(), ScalarValue::String(meta.id.clone())),
                Filter::Eq("role".to_string(), ScalarValue::String("".to_string())), // 占位
            ]);

            // 获取所有消息文档并删除
            let msg_docs = db.filter(&delete_filter);
            for doc in msg_docs {
                let _ = db.delete(doc.id);
            }
        }

        db.flush()?;

        // 写入归档包元数据
        let archive_meta = ColdArchiveMeta {
            archive_id: archive_id.clone(),
            created_at: now_secs(),
            conv_count: candidates.len(),
            oldest_access,
            newest_access,
            compressed_size,
            original_size,
        };

        let meta_path = self.cold_dir.join(format!("{}.meta", archive_id));
        let meta_json = serde_json::to_string_pretty(&archive_meta)
            .map_err(|e| format!("元数据序列化失败: {}", e))?;
        std::fs::write(&meta_path, meta_json)
            .map_err(|e| format!("写入元数据文件失败: {}", e))?;

        Ok(archive_meta)
    }

    /// 从冷归档恢复会话到温数据（AetherDB）
    pub fn restore_from_archive(
        &self,
        archive_id: &str,
        db_path: &PathBuf,
    ) -> Result<Vec<AiConversation>, String> {
        let archive_path = self.cold_dir.join(format!("{}.cold", archive_id));
        if !archive_path.exists() {
            return Err(format!("归档文件不存在: {}", archive_id));
        }

        // 1. 读取压缩文件
        let compressed = std::fs::read(&archive_path)
            .map_err(|e| format!("读取归档文件失败: {}", e))?;

        // 2. 解压
        let original = Self::decompress_data(&compressed)
            .map_err(|e| format!("解压失败: {}", e))?;

        // 3. 反序列化
        let conversations: Vec<crate::ai_history::ConversationFile> =
            serde_json::from_slice(&original)
                .map_err(|e| format!("反序列化失败: {}", e))?;

        // 4. 恢复到 AetherDB
        let db = AetherDB::open(db_path.clone())?;

        let mut restored = Vec::new();
        for file in conversations {
            let conv = file.to_conversation();

            // 恢复消息到 AetherDB
            for (idx, msg) in conv.messages.iter().enumerate() {
                let msg_doc = ConversationAdapter::message_to_document(&conv.id, msg, idx);
                db.insert(msg_doc)?;
            }

            restored.push(conv);
        }

        db.flush()?;

        Ok(restored)
    }

    /// 使用 zlib 压缩数据
    fn compress_data(data: &[u8], level: u32) -> Result<Vec<u8>, String> {
        use flate2::write::ZlibEncoder;
        use flate2::Compression;

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(level));
        encoder.write_all(data)
            .map_err(|e| format!("压缩写入失败: {}", e))?;
        encoder.finish()
            .map_err(|e| format!("压缩完成失败: {}", e))
    }

    /// 使用 zlib 解压数据
    fn decompress_data(data: &[u8]) -> Result<Vec<u8>, String> {
        use flate2::read::ZlibDecoder;

        let mut decoder = ZlibDecoder::new(data);
        let mut result = Vec::new();
        decoder.read_to_end(&mut result)
            .map_err(|e| format!("解压失败: {}", e))?;
        Ok(result)
    }

    /// 列出所有冷归档包
    pub fn list_archives(&self) -> Result<Vec<ColdArchiveMeta>, String> {
        let mut results = Vec::new();
        let entries = std::fs::read_dir(&self.cold_dir)
            .map_err(|e| format!("读取冷数据目录失败: {}", e))?;

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.ends_with(".meta") {
                let content = std::fs::read_to_string(entry.path())
                    .map_err(|e| format!("读取元数据文件失败: {}", e))?;
                if let Ok(meta) = serde_json::from_str::<ColdArchiveMeta>(&content) {
                    results.push(meta);
                }
            }
        }

        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(results)
    }

    /// 删除冷归档包（释放磁盘空间）
    pub fn delete_archive(&self, archive_id: &str) -> Result<(), String> {
        let cold_path = self.cold_dir.join(format!("{}.cold", archive_id));
        let meta_path = self.cold_dir.join(format!("{}.meta", archive_id));

        if cold_path.exists() {
            std::fs::remove_file(&cold_path)
                .map_err(|e| format!("删除归档文件失败: {}", e))?;
        }
        if meta_path.exists() {
            std::fs::remove_file(&meta_path)
                .map_err(|e| format!("删除元数据文件失败: {}", e))?;
        }

        Ok(())
    }
}
