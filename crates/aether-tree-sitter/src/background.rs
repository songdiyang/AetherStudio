//! 后台语法高亮器
//!
//! 将 Tree-sitter 解析和高亮移到后台线程，避免阻塞 UI 输入。
//!
//! 工作流程：
//! 1. 主线程调用 `request()` 发送高亮请求（文档全文 + 语言）
//! 2. 后台线程接收请求，调用 `highlight_document` 进行增量解析
//! 3. 主线程在渲染帧中调用 `poll_result()` 非阻塞检查结果
//! 4. 结果未就绪时使用上一帧的缓存（无卡顿）

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use aether_core::lexer::LexemeSpan;

use crate::highlighter::TreeSitterHighlighter;

/// 高亮请求
struct HighlightRequest {
    doc_id: String,
    language: String,
    full_text: String,
}

/// 高亮结果
pub struct HighlightResult {
    pub doc_id: String,
    pub token_lines: Vec<Vec<LexemeSpan>>,
}

/// 后台语法高亮器
///
/// 拥有独立的后台线程，线程内持有专属的 `TreeSitterHighlighter` 实例。
/// 主线程通过 channel 与后台线程通信，完全不阻塞。
pub struct BackgroundHighlighter {
    /// 请求发送端（主线程持有）
    request_tx: Sender<HighlightRequest>,
    /// 结果接收端（主线程持有）
    result_rx: Receiver<HighlightResult>,
    /// 后台线程句柄
    _worker: Option<thread::JoinHandle<()>>,
    /// 是否有待处理请求（避免重复发送）
    pending: bool,
}

impl BackgroundHighlighter {
    /// 创建并启动后台高亮器
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<HighlightRequest>();
        let (res_tx, res_rx) = mpsc::channel::<HighlightResult>();

        let worker = thread::spawn(move || {
            let mut highlighter = TreeSitterHighlighter::new();

            for req in req_rx {
                let token_lines =
                    highlighter.highlight_document(&req.doc_id, &req.language, &req.full_text);
                // 结果发送失败表示主线程已关闭，退出循环
                if res_tx
                    .send(HighlightResult {
                        doc_id: req.doc_id,
                        token_lines,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        Self {
            request_tx: req_tx,
            result_rx: res_rx,
            _worker: Some(worker),
            pending: false,
        }
    }

    /// 发送高亮请求（非阻塞）
    ///
    /// 如果已有待处理请求，跳过避免排队堆积。
    /// 后台线程会处理最新一次请求。
    pub fn request(&mut self, doc_id: &str, language: &str, full_text: &str) {
        if self.pending {
            return;
        }
        let _ = self.request_tx.send(HighlightRequest {
            doc_id: doc_id.to_string(),
            language: language.to_string(),
            full_text: full_text.to_string(),
        });
        self.pending = true;
    }

    /// 非阻塞轮询高亮结果
    ///
    /// 返回 `Some(result)` 表示有新结果就绪；
    /// 返回 `None` 表示仍在处理中，主线程应使用上一帧缓存。
    pub fn poll_result(&mut self) -> Option<HighlightResult> {
        match self.result_rx.try_recv() {
            Ok(result) => {
                self.pending = false;
                // 排空可能残留的旧结果
                while let Ok(_) = self.result_rx.try_recv() {}
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.pending = false;
                None
            }
        }
    }

    /// 是否有待处理请求
    pub fn has_pending(&self) -> bool {
        self.pending
    }
}

impl Default for BackgroundHighlighter {
    fn default() -> Self {
        Self::new()
    }
}
