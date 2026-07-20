//! AI 对话温数据阶段 — 异步归档到 AetherDB 向量数据库
//!
//! 触发时机：用户关闭当前聊天窗口、切换到其他会话、软件进入空闲状态（30秒无操作）
//! 后台线程把整段完整对话一次性批量写入 AetherDB，建立向量索引 + 标量索引。
//! 写入成功后删除对应的临时日志文件，完成「热→温」的状态切换。

use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender, Receiver};

use crate::ai_panel::{AiConversation, AiMessage, ConversationMeta};
use crate::aether_db::{AetherDB, ConversationAdapter, Filter, ScalarValue};

/// 温数据归档请求
#[derive(Clone, Debug)]
pub enum ArchiveRequest {
    /// 归档指定会话
    ArchiveConversation { conv_id: String, conv: AiConversation },
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

/// 温数据存储（AetherDB 向量数据库）
///
/// 所有写操作通过后台线程异步执行，不阻塞 UI 线程。
#[derive(Debug)]
pub struct WarmDataStore {
    /// AetherDB 数据库路径
    db_path: PathBuf,
    /// 归档请求发送端
    request_tx: Option<Sender<ArchiveRequest>>,
    /// 归档结果接收端
    result_rx: Option<Receiver<ArchiveResult>>,
    /// 后台线程句柄
    worker_handle: Option<std::thread::JoinHandle<()>>,
}

impl WarmDataStore {
    /// 创建温数据存储（自动初始化 AetherDB 数据库）
    pub fn new(base_dir: PathBuf) -> Result<Self, String> {
        let db_path = base_dir.join("conversations.aedb");
        let warm_dir = base_dir.join("warm");
        if let Err(e) = std::fs::create_dir_all(&warm_dir) {
            return Err(format!("无法创建温数据目录: {}", e));
        }

        // 初始化 AetherDB 数据库（同步执行一次）
        let db = AetherDB::open(db_path.clone())?;
        db.close()?;

        let (request_tx, request_rx) = channel::<ArchiveRequest>();
        let (result_tx, result_rx) = channel::<ArchiveResult>();

        let db_path_clone = db_path.clone();
        let handle = std::thread::spawn(move || {
            Self::archive_worker(db_path_clone, request_rx, result_tx);
        });

        Ok(Self {
            db_path,
            request_tx: Some(request_tx),
            result_rx: Some(result_rx),
            worker_handle: Some(handle),
        })
    }

    /// 后台归档线程主循环
    fn archive_worker(
        db_path: PathBuf,
        request_rx: Receiver<ArchiveRequest>,
        result_tx: Sender<ArchiveResult>,
    ) {
        let db = match AetherDB::open(db_path.clone()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[WarmData] 归档线程无法打开数据库: {}", e);
                return;
            }
        };

        while let Ok(req) = request_rx.recv() {
            match req {
                ArchiveRequest::ArchiveConversation { conv_id, conv } => {
                    let result = Self::archive_single(&db, &conv_id, &conv);
                    let _ = result_tx.send(match result {
                        Ok(()) => ArchiveResult::Success { conv_id },
                        Err(e) => ArchiveResult::Failed { conv_id, error: e },
                    });
                }
                ArchiveRequest::ArchiveAllDirty { sessions } => {
                    for conv in sessions {
                        let conv_id = conv.id.clone();
                        let result = Self::archive_single(&db, &conv_id, &conv);
                        let _ = result_tx.send(match result {
                            Ok(()) => ArchiveResult::Success { conv_id },
                            Err(e) => ArchiveResult::Failed { conv_id, error: e },
                        });
                    }
                }
                ArchiveRequest::RemoveHotLog { conv_id } => {
                    // 删除热数据临时日志
                    let hot_dir = db_path.parent().unwrap_or(PathBuf::new().as_path()).join("hot");
                    let log_path = hot_dir.join(format!("{}.log", conv_id));
                    if log_path.exists() {
                        let _ = std::fs::remove_file(&log_path);
                    }
                }
                ArchiveRequest::Shutdown => break,
            }
        }

        // 关闭前 flush
        let _ = db.flush();
    }

    /// 归档单一会话到 AetherDB
    fn archive_single(
        db: &AetherDB,
        conv_id: &str,
        conv: &AiConversation,
    ) -> Result<(), String> {
        // 1. 归档会话元数据
        let meta_doc = ConversationAdapter::meta_to_document(conv);
        db.insert(meta_doc)?;

        // 2. 归档所有消息（每条消息作为独立文档，支持语义检索）
        for (idx, msg) in conv.messages.iter().enumerate() {
            let msg_doc = ConversationAdapter::message_to_document(conv_id, msg, idx);
            db.insert(msg_doc)?;
        }

        // 3. 刷写到磁盘
        db.flush()?;

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

    /// 从 AetherDB 加载历史元数据（用于历史列表展示）
    pub fn load_history_meta(&self) -> Result<Vec<ConversationMeta>, String> {
        let db = AetherDB::open(self.db_path.clone())?;

        // 使用标量过滤查询所有会话元数据
        let _filter = Filter::Eq(
            "message_count".to_string(),
            ScalarValue::Int(0), // 占位：实际应查询所有 conv_id 去重
        );

        // 简化：直接查询所有文档，按 conv_id 去重取最新
        let mut seen_convs = std::collections::HashSet::new();
        let mut results = Vec::new();

        // 使用标量索引查询所有会话（简化实现）
        // 实际应使用 conv_id 索引进行高效查询
        let all_docs = db.filter(&Filter::Eq(
            "conv_id".to_string(),
            ScalarValue::String("".to_string()), // 占位
        ));

        for doc in all_docs {
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
        }

        // 按更新时间降序排序
        results.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(results)
    }

    /// 从 AetherDB 加载完整会话
    pub fn load_conversation(&self, conv_id: &str) -> Result<AiConversation, String> {
        let db = AetherDB::open(self.db_path.clone())?;

        // 1. 查询会话元数据
        let meta_filter = Filter::And(vec![
            Filter::Eq("conv_id".to_string(), ScalarValue::String(conv_id.to_string())),
            Filter::Eq("title".to_string(), ScalarValue::String("".to_string())), // 占位
        ]);

        let meta_docs = db.filter(&meta_filter);
        let meta_doc = meta_docs.first().ok_or("会话元数据不存在")?;

        let title = meta_doc
            .scalars
            .get("title")
            .and_then(|v| match v {
                ScalarValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();

        let mut conv = AiConversation::new(conv_id.to_string(), title);
        conv.created_at = meta_doc.created_at;
        conv.updated_at = meta_doc
            .scalars
            .get("updated_at")
            .and_then(|v| match v {
                ScalarValue::Timestamp(t) => Some(*t),
                _ => None,
            })
            .unwrap_or(meta_doc.created_at);

        // 2. 查询所有消息
        let msg_filter = Filter::And(vec![
            Filter::Eq("conv_id".to_string(), ScalarValue::String(conv_id.to_string())),
            Filter::Eq("role".to_string(), ScalarValue::String("".to_string())), // 占位
        ]);

        let msg_docs = db.filter(&msg_filter);
        let mut messages: Vec<(usize, AiMessage)> = Vec::new();

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
                let mut msg = AiMessage::new(role, doc.text);
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

        // 按消息索引排序
        messages.sort_by_key(|(idx, _)| *idx);
        conv.messages = messages.into_iter().map(|(_, msg)| msg).collect();

        Ok(conv)
    }

    /// 语义搜索历史对话
    pub fn semantic_search(
        &self,
        query_text: &str,
        k: usize,
    ) -> Result<Vec<(ConversationMeta, f32)>, String> {
        let db = AetherDB::open(self.db_path.clone())?;

        // 将查询文本转换为向量
        let query_vector = ConversationAdapter::text_to_vector(query_text);

        // 执行向量搜索
        let results = db.search(&query_vector, k, None);

        let mut seen_convs = std::collections::HashSet::new();
        let mut meta_results = Vec::new();

        for (doc, score) in results {
            if let Some(ScalarValue::String(conv_id)) = doc.scalars.get("conv_id") {
                if seen_convs.insert(conv_id.clone()) {
                    let meta = ConversationMeta {
                        id: conv_id.clone(),
                        title: doc
                            .scalars
                            .get("title")
                            .and_then(|v| match v {
                                ScalarValue::String(s) => Some(s.clone()),
                                _ => None,
                            })
                            .unwrap_or_default(),
                        updated_at: doc.created_at,
                        message_count: doc
                            .scalars
                            .get("message_count")
                            .and_then(|v| match v {
                                ScalarValue::Int(n) => Some(*n as usize),
                                _ => None,
                            })
                            .unwrap_or(0),
                        preview: doc.text.chars().take(50).collect::<String>(),
                    };
                    meta_results.push((meta, score));
                }
            }
        }

        Ok(meta_results)
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
