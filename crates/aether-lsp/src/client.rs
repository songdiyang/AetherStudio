use lsp_types::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, RwLock};

use crate::server::LanguageServer;
use crate::sync::DocumentSync;
use crate::types::*;

/// LSP 客户端管理器
/// 管理多个语言服务器实例，按语言ID路由请求
pub struct LspClient {
    /// 语言ID -> 语言服务器实例
    servers: Arc<RwLock<HashMap<String, LanguageServer>>>,
    /// 文档同步管理器
    document_sync: Arc<RwLock<DocumentSync>>,
    /// 诊断集合（使用 std Mutex 以便 UI 主线程同步读取/更新）
    diagnostics: Arc<Mutex<DiagnosticCollection>>,
    /// 事件发送器（向UI层推送事件）
    event_tx: mpsc::UnboundedSender<LspEvent>,
    /// 工作区根目录
    #[allow(dead_code)]
    root_uri: Option<Url>,
}

/// LSP 事件（推送到UI层）
#[derive(Clone, Debug)]
pub enum LspEvent {
    /// 诊断更新
    Diagnostics {
        uri: Url,
        diagnostics: Vec<Diagnostic>,
    },
    /// 补全结果
    Completion {
        uri: Url,
        items: Vec<CompletionItem>,
    },
    /// 悬停结果
    Hover { uri: Url, hover: Hover },
    /// 查找引用结果
    References { uri: Url, locations: Vec<Location> },
    /// 重命名结果
    Rename { uri: Url, edit: WorkspaceEdit },
    /// 代码操作结果
    CodeActions {
        uri: Url,
        actions: Vec<CodeActionOrCommand>,
    },
    /// 格式化结果
    Formatting { uri: Url, edits: Vec<TextEdit> },
    /// 语义令牌结果
    SemanticTokens { uri: Url, tokens: SemanticTokens },
    /// 语义令牌delta结果
    SemanticTokensDelta {
        uri: Url,
        delta: SemanticTokensDelta,
    },
    /// 内联提示结果
    InlayHints { uri: Url, hints: Vec<InlayHint> },
    /// 服务器已就绪
    ServerReady { language_id: String },
    /// 服务器日志
    Log {
        language_id: String,
        message: String,
    },
}

impl LspClient {
    /// 创建新的 LSP 客户端
    pub fn new(root_uri: Option<Url>) -> (Self, mpsc::UnboundedReceiver<LspEvent>) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let client = Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            document_sync: Arc::new(RwLock::new(DocumentSync::new())),
            diagnostics: Arc::new(Mutex::new(DiagnosticCollection::default())),
            event_tx,
            root_uri,
        };

        (client, event_rx)
    }

    /// 启动指定语言的服务器
    pub async fn start_server(
        &self,
        language_id: &str,
        config: ServerConfig,
    ) -> std::io::Result<()> {
        // 克隆 event_tx 用于接收服务器推送的 diagnostics 等通知
        let server =
            LanguageServer::start(config, language_id.to_string(), Some(self.event_tx.clone()))
                .await?;

        let event = LspEvent::ServerReady {
            language_id: language_id.to_string(),
        };
        let _ = self.event_tx.send(event);

        let mut servers = self.servers.write().await;
        servers.insert(language_id.to_string(), server);

        Ok(())
    }

    /// 打开文档（自动路由到对应语言服务器）
    pub async fn open_document(
        &self,
        uri: Url,
        language_id: String,
        text: String,
    ) -> std::io::Result<()> {
        let version = 1;

        // 记录文档状态
        {
            let mut sync = self.document_sync.write().await;
            sync.open_document(uri.clone(), language_id.clone(), version, text.clone());
        }

        // 发送到对应语言服务器
        let mut servers = self.servers.write().await;
        if let Some(server) = servers.get_mut(&language_id) {
            server
                .open_document(uri, language_id, version, text)
                .await?;
        }

        Ok(())
    }

    /// 关闭文档
    pub async fn close_document(&self, uri: &Url) -> std::io::Result<()> {
        let language_id = {
            let sync = self.document_sync.read().await;
            sync.get_language_id(uri).cloned()
        };

        if let Some(lang_id) = language_id {
            let mut servers = self.servers.write().await;
            if let Some(server) = servers.get_mut(&lang_id) {
                server.close_document(uri).await?;
            }

            let mut sync = self.document_sync.write().await;
            sync.close_document(uri);
        }

        // 文档关闭时清理对应诊断缓存
        self.remove_diagnostics(uri);

        Ok(())
    }

    /// 更新或插入某 URI 的诊断缓存
    pub fn update_diagnostics(&self, uri: &Url, diagnostics: Vec<Diagnostic>) {
        if let Ok(mut coll) = self.diagnostics.lock() {
            if diagnostics.is_empty() {
                coll.by_uri.remove(uri);
            } else {
                coll.by_uri.insert(uri.clone(), diagnostics);
            }
        }
    }

    /// 移除某 URI 的诊断缓存
    pub fn remove_diagnostics(&self, uri: &Url) {
        if let Ok(mut coll) = self.diagnostics.lock() {
            coll.by_uri.remove(uri);
        }
    }

    /// 获取某 URI 的诊断快照（返回克隆，避免长期持有锁）
    pub fn diagnostics_for(&self, uri: &Url) -> Option<Vec<Diagnostic>> {
        self.diagnostics
            .lock()
            .ok()
            .and_then(|coll| coll.by_uri.get(uri).cloned())
    }

    /// 获取所有诊断的快照
    pub fn all_diagnostics(&self) -> HashMap<Url, Vec<Diagnostic>> {
        self.diagnostics
            .lock()
            .map(|coll| coll.by_uri.clone())
            .unwrap_or_default()
    }

    /// 清空所有诊断缓存
    pub fn clear_diagnostics(&self) {
        if let Ok(mut coll) = self.diagnostics.lock() {
            coll.by_uri.clear();
        }
    }

    /// 通知文档变更（增量同步）
    ///
    /// 传入新全文，内部基于 DocumentSync 保存的旧文本计算精确增量变更，
    /// 然后更新缓存文本并发送到语言服务器。
    pub async fn notify_change(&self, uri: &Url, new_text: &str) -> std::io::Result<()> {
        let (language_id, new_version, changes) = {
            let mut sync = self.document_sync.write().await;
            let lang_id = sync.get_language_id(uri).cloned();
            let version = sync.increment_version(uri);
            let old_text = sync
                .get_document(uri)
                .map(|d| d.text.clone())
                .unwrap_or_default();
            let changes = crate::sync::compute_changes(&old_text, new_text);
            sync.update_text(uri, new_text.to_string());
            (lang_id, version, changes)
        };

        if changes.is_empty() {
            return Ok(());
        }

        if let Some(lang_id) = language_id {
            if let Some(version) = new_version {
                let mut servers = self.servers.write().await;
                if let Some(server) = servers.get_mut(&lang_id) {
                    server.change_document(uri, version, changes).await?;
                }
            }
        }

        Ok(())
    }

    /// 直接发送预计算的文档变更事件（高级用法）
    pub async fn notify_change_raw(
        &self,
        uri: &Url,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) -> std::io::Result<()> {
        let (language_id, new_version) = {
            let mut sync = self.document_sync.write().await;
            let lang_id = sync.get_language_id(uri).cloned();
            let version = sync.increment_version(uri);
            (lang_id, version)
        };

        if let Some(lang_id) = language_id {
            if let Some(version) = new_version {
                let mut servers = self.servers.write().await;
                if let Some(server) = servers.get_mut(&lang_id) {
                    server.change_document(uri, version, changes).await?;
                }
            }
        }

        Ok(())
    }

    /// 请求代码补全
    pub async fn request_completion(
        &self,
        uri: &Url,
        position: Position,
    ) -> std::io::Result<Option<CompletionResponse>> {
        let language_id = {
            let sync = self.document_sync.read().await;
            sync.get_language_id(uri).cloned()
        };

        if let Some(lang_id) = language_id {
            let mut servers = self.servers.write().await;
            if let Some(server) = servers.get_mut(&lang_id) {
                return server.request_completion(uri, position).await;
            }
        }

        Ok(None)
    }

    /// 请求悬停提示
    pub async fn request_hover(
        &self,
        uri: &Url,
        position: Position,
    ) -> std::io::Result<Option<Hover>> {
        let language_id = {
            let sync = self.document_sync.read().await;
            sync.get_language_id(uri).cloned()
        };

        if let Some(lang_id) = language_id {
            let mut servers = self.servers.write().await;
            if let Some(server) = servers.get_mut(&lang_id) {
                return server.request_hover(uri, position).await;
            }
        }

        Ok(None)
    }

    /// 请求跳转到定义
    pub async fn request_definition(
        &self,
        uri: &Url,
        position: Position,
    ) -> std::io::Result<Option<GotoDefinitionResponse>> {
        let language_id = {
            let sync = self.document_sync.read().await;
            sync.get_language_id(uri).cloned()
        };

        if let Some(lang_id) = language_id {
            let mut servers = self.servers.write().await;
            if let Some(server) = servers.get_mut(&lang_id) {
                return server.request_definition(uri, position).await;
            }
        }

        Ok(None)
    }

    /// 请求查找引用
    pub async fn request_references(
        &self,
        uri: &Url,
        position: Position,
        include_declaration: bool,
    ) -> std::io::Result<Option<Vec<Location>>> {
        let language_id = {
            let sync = self.document_sync.read().await;
            sync.get_language_id(uri).cloned()
        };

        if let Some(lang_id) = language_id {
            let mut servers = self.servers.write().await;
            if let Some(server) = servers.get_mut(&lang_id) {
                return server
                    .request_references(uri, position, include_declaration)
                    .await;
            }
        }

        Ok(None)
    }

    /// 请求重命名
    pub async fn request_rename(
        &self,
        uri: &Url,
        position: Position,
        new_name: String,
    ) -> std::io::Result<Option<WorkspaceEdit>> {
        let language_id = {
            let sync = self.document_sync.read().await;
            sync.get_language_id(uri).cloned()
        };

        if let Some(lang_id) = language_id {
            let mut servers = self.servers.write().await;
            if let Some(server) = servers.get_mut(&lang_id) {
                return server.request_rename(uri, position, new_name).await;
            }
        }

        Ok(None)
    }

    /// 请求代码操作
    pub async fn request_code_actions(
        &self,
        uri: &Url,
        range: Range,
        diagnostics: Vec<Diagnostic>,
    ) -> std::io::Result<Option<CodeActionResponse>> {
        let language_id = {
            let sync = self.document_sync.read().await;
            sync.get_language_id(uri).cloned()
        };

        if let Some(lang_id) = language_id {
            let mut servers = self.servers.write().await;
            if let Some(server) = servers.get_mut(&lang_id) {
                return server.request_code_actions(uri, range, diagnostics).await;
            }
        }

        Ok(None)
    }

    /// 请求格式化
    pub async fn request_formatting(
        &self,
        uri: &Url,
        options: FormattingOptions,
    ) -> std::io::Result<Option<Vec<TextEdit>>> {
        let language_id = {
            let sync = self.document_sync.read().await;
            sync.get_language_id(uri).cloned()
        };

        if let Some(lang_id) = language_id {
            let mut servers = self.servers.write().await;
            if let Some(server) = servers.get_mut(&lang_id) {
                return server.request_formatting(uri, options).await;
            }
        }

        Ok(None)
    }

    /// 请求完整语义令牌
    pub async fn request_semantic_tokens_full(
        &self,
        uri: &Url,
    ) -> std::io::Result<Option<SemanticTokens>> {
        let language_id = {
            let sync = self.document_sync.read().await;
            sync.get_language_id(uri).cloned()
        };

        if let Some(lang_id) = language_id {
            let mut servers = self.servers.write().await;
            if let Some(server) = servers.get_mut(&lang_id) {
                return server.request_semantic_tokens_full(uri).await;
            }
        }

        Ok(None)
    }

    /// 请求语义令牌delta更新
    pub async fn request_semantic_tokens_delta(
        &self,
        uri: &Url,
        previous_result_id: String,
    ) -> std::io::Result<Option<SemanticTokensDelta>> {
        let language_id = {
            let sync = self.document_sync.read().await;
            sync.get_language_id(uri).cloned()
        };

        if let Some(lang_id) = language_id {
            let mut servers = self.servers.write().await;
            if let Some(server) = servers.get_mut(&lang_id) {
                return server
                    .request_semantic_tokens_delta(uri, previous_result_id)
                    .await;
            }
        }

        Ok(None)
    }

    /// 请求范围语义令牌
    pub async fn request_semantic_tokens_range(
        &self,
        uri: &Url,
        range: Range,
    ) -> std::io::Result<Option<SemanticTokens>> {
        let language_id = {
            let sync = self.document_sync.read().await;
            sync.get_language_id(uri).cloned()
        };

        if let Some(lang_id) = language_id {
            let mut servers = self.servers.write().await;
            if let Some(server) = servers.get_mut(&lang_id) {
                return server.request_semantic_tokens_range(uri, range).await;
            }
        }

        Ok(None)
    }

    /// 请求内联提示
    pub async fn request_inlay_hints(
        &self,
        uri: &Url,
        range: Range,
    ) -> std::io::Result<Option<Vec<InlayHint>>> {
        let language_id = {
            let sync = self.document_sync.read().await;
            sync.get_language_id(uri).cloned()
        };

        if let Some(lang_id) = language_id {
            let mut servers = self.servers.write().await;
            if let Some(server) = servers.get_mut(&lang_id) {
                return server.request_inlay_hints(uri, range).await;
            }
        }

        Ok(None)
    }

    /// 关闭所有服务器
    pub async fn shutdown_all(&self) -> std::io::Result<()> {
        let mut servers = self.servers.write().await;
        for (_, server) in servers.iter_mut() {
            let _ = server.shutdown().await;
        }
        servers.clear();
        // 服务器关闭时清空所有诊断缓存，避免显示过期诊断
        self.clear_diagnostics();
        Ok(())
    }

    /// 检查某语言的服务器是否已启动
    pub async fn is_server_ready(&self, language_id: &str) -> bool {
        let servers = self.servers.read().await;
        servers.contains_key(language_id)
    }

    /// 获取某语言服务器的能力
    pub async fn get_capabilities(&self, language_id: &str) -> Option<ServerCapabilitiesCache> {
        let servers = self.servers.read().await;
        servers.get(language_id).map(|s| s.capabilities().clone())
    }
}

/// 默认服务器配置发现
pub fn default_server_config(language_id: &str) -> Option<ServerConfig> {
    match language_id {
        "rust" => Some(ServerConfig {
            command: Some(PathBuf::from("rust-analyzer")),
            args: vec![],
            env: HashMap::new(),
            root_uri: None,
            initialization_options: None,
        }),
        "python" => Some(ServerConfig {
            command: Some(PathBuf::from("pylsp")),
            args: vec![],
            env: HashMap::new(),
            root_uri: None,
            initialization_options: None,
        }),
        "typescript" | "javascript" => Some(ServerConfig {
            command: Some(PathBuf::from("typescript-language-server")),
            args: vec!["--stdio".to_string()],
            env: HashMap::new(),
            root_uri: None,
            initialization_options: None,
        }),
        "c" | "cpp" => Some(ServerConfig {
            command: Some(PathBuf::from("clangd")),
            args: vec![],
            env: HashMap::new(),
            root_uri: None,
            initialization_options: None,
        }),
        _ => None,
    }
}
