use lsp_types::*;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::client::LspEvent;
use crate::transport::{spawn_server, spawn_stderr_drain, LspTransport};
use crate::types::*;

/// 默认请求超时（秒）。
///
/// 大多数 LSP 请求应在 30 秒内完成。initialize 可能更慢，单独设置。
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// initialize 请求超时（秒）。
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);

/// 语言服务器实例管理
/// 负责单个语言服务器的完整生命周期：发现→启动→初始化→运行→关闭
pub struct LanguageServer {
    /// 传输层
    transport: LspTransport,
    /// 服务器配置
    config: ServerConfig,
    /// 已缓存的服务器能力
    capabilities: ServerCapabilitiesCache,
    /// 请求ID生成器
    id_generator: RequestIdGenerator,
    /// 等待中的请求
    pending_requests: HashMap<serde_json::Value, String>, // id -> method
    /// 已打开的文档
    open_documents: HashMap<Url, DocumentState>,
    /// 服务器是否已初始化
    initialized: bool,
    /// 语言ID（如 "rust", "python"）
    pub language_id: String,
    /// 事件发送器（向UI层推送诊断等推送通知）
    /// None 时不转发，但不会丢弃导致循环卡死
    event_tx: Option<mpsc::UnboundedSender<LspEvent>>,
}

impl LanguageServer {
    /// 启动并初始化语言服务器
    ///
    /// `event_tx` 用于转发服务器推送的 notifications（如 diagnostics）到 UI 层。
    /// 传 None 时通知将被静默忽略但不会阻塞消息泵。
    pub async fn start(
        config: ServerConfig,
        language_id: String,
        event_tx: Option<mpsc::UnboundedSender<LspEvent>>,
    ) -> std::io::Result<Self> {
        let mut process = spawn_server(&config).await?;
        let stdin = process.stdin.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to capture stdin")
        })?;
        let stdout = process.stdout.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to capture stdout")
        })?;
        let transport = LspTransport::new(stdin, stdout);

        // 启动后台 stderr 读取任务，避免子进程 stderr 缓冲区满后阻塞
        spawn_stderr_drain(process);

        let mut server = Self {
            transport,
            config: config.clone(),
            capabilities: ServerCapabilitiesCache::default(),
            id_generator: RequestIdGenerator::new(),
            pending_requests: HashMap::new(),
            open_documents: HashMap::new(),
            initialized: false,
            language_id,
            event_tx,
        };

        // 发送 initialize 请求
        server.initialize().await?;

        Ok(server)
    }

    /// 序列化参数为 JSON Value，失败时返回 io::Error 而非 panic
    fn serialize_params<T: serde::Serialize>(params: T) -> std::io::Result<serde_json::Value> {
        serde_json::to_value(params).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("JSON serialize error: {}", e),
            )
        })
    }

    /// 发送请求并跟踪 pending 状态
    async fn send_request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> std::io::Result<serde_json::Value> {
        let id = self.id_generator.next();
        let request = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: method.to_string(),
            params,
        });

        self.transport.send(&request).await?;
        self.pending_requests.insert(id.clone(), method.to_string());
        Ok(id)
    }

    /// 接收并匹配指定 id 的响应，期间处理通知和错误响应。
    ///
    /// - 成功响应：反序列化为 T，返回 Ok(Some(T))；result 为 null 时返回 Ok(None)
    /// - 错误响应：返回 Err(io::Error)，携带 LSP 错误码和消息
    /// - 通知消息：转发 diagnostics/logMessage 等到 event_tx，避免静默丢失
    /// - 超时：超过 `timeout` 后返回 Err(io::Error)
    async fn receive_response<T: serde::de::DeserializeOwned>(
        &mut self,
        id: &serde_json::Value,
        request_timeout: Duration,
    ) -> std::io::Result<Option<T>> {
        let fut = async {
            loop {
                let message = self.transport.receive().await?;
                match message {
                    LspMessage::Response(resp) if resp.id == *id => {
                        self.pending_requests.remove(&resp.id);
                        if let Some(err) = resp.error {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("LSP error {}: {}", err.code, err.message),
                            ));
                        }
                        return match resp.result {
                            Some(val) => {
                                let parsed = serde_json::from_value(val).map_err(|e| {
                                    std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        format!("JSON deserialize error: {}", e),
                                    )
                                })?;
                                Ok(Some(parsed))
                            }
                            None => Ok(None),
                        };
                    }
                    LspMessage::Response(resp) => {
                        // 不属于本次请求的响应（可能是过期请求的回包）
                        // 记录后丢弃，避免无限循环
                        self.pending_requests.remove(&resp.id);
                        tracing::debug!("Dropping orphan LSP response for id={:?}", resp.id);
                    }
                    LspMessage::Notification(notif) => {
                        self.handle_notification(notif);
                    }
                    LspMessage::Request(req) => {
                        // 服务器发起的反向请求（如 workspace/configuration）
                        // 当前未实现处理，回 error 避免服务器卡死
                        tracing::debug!("Unhandled server->client request: {}", req.method);
                    }
                }
            }
        };

        timeout(request_timeout, fut).await.map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "LSP request timed out")
        })?
    }

    /// 处理服务器推送的 notification，转发到 UI 层。
    ///
    /// 这是修复「通知静默丢失」缺陷的核心：原实现 `_ => {}` 会丢弃所有
    /// diagnostics、logMessage、showMessage 等推送，导致 UI 永远收不到诊断。
    fn handle_notification(&self, notif: LspNotification) {
        match notif.method.as_str() {
            "textDocument/publishDiagnostics" => {
                if let Some(tx) = &self.event_tx {
                    if let Some(params) = notif.params {
                        if let Ok(p) = serde_json::from_value::<PublishDiagnosticsParams>(params) {
                            let _ = tx.send(LspEvent::Diagnostics {
                                uri: p.uri,
                                diagnostics: p.diagnostics,
                            });
                        }
                    }
                }
            }
            "window/logMessage" => {
                if let Some(tx) = &self.event_tx {
                    if let Some(params) = notif.params {
                        let message = params
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let _ = tx.send(LspEvent::Log {
                            language_id: self.language_id.clone(),
                            message,
                        });
                    }
                }
            }
            _ => {
                tracing::trace!("Unhandled LSP notification: {}", notif.method);
            }
        }
    }

    /// 发送 initialize 请求并等待响应
    #[allow(deprecated)]
    async fn initialize(&mut self) -> std::io::Result<()> {
        let root_uri = self
            .config
            .root_uri
            .clone()
            .unwrap_or_else(|| Url::parse("file:///").unwrap());

        let params = InitializeParams {
            process_id: Some(std::process::id() as u32),
            root_path: None,
            root_uri: Some(root_uri.clone()),
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: root_uri.clone(),
                name: self
                    .config
                    .root_uri
                    .as_ref()
                    .map(|u| u.path().to_string())
                    .unwrap_or_default(),
            }]),
            initialization_options: self.config.initialization_options.clone(),
            capabilities: ClientCapabilities {
                workspace: Some(WorkspaceClientCapabilities {
                    apply_edit: Some(true),
                    workspace_edit: Some(WorkspaceEditClientCapabilities {
                        document_changes: Some(true),
                        ..Default::default()
                    }),
                    did_change_configuration: Some(DynamicRegistrationClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    did_change_watched_files: Some(DidChangeWatchedFilesClientCapabilities {
                        dynamic_registration: Some(true),
                        relative_pattern_support: Some(true),
                    }),
                    ..Default::default()
                }),
                text_document: Some(TextDocumentClientCapabilities {
                    synchronization: Some(TextDocumentSyncClientCapabilities {
                        dynamic_registration: Some(true),
                        will_save: Some(true),
                        will_save_wait_until: Some(true),
                        did_save: Some(true),
                    }),
                    completion: Some(CompletionClientCapabilities {
                        dynamic_registration: Some(true),
                        completion_item: Some(CompletionItemCapability {
                            snippet_support: Some(true),
                            commit_characters_support: Some(true),
                            documentation_format: Some(vec![
                                MarkupKind::Markdown,
                                MarkupKind::PlainText,
                            ]),
                            deprecated_support: Some(true),
                            preselect_support: Some(true),
                            ..Default::default()
                        }),
                        completion_item_kind: Some(CompletionItemKindCapability {
                            value_set: Some(vec![
                                CompletionItemKind::TEXT,
                                CompletionItemKind::METHOD,
                                CompletionItemKind::FUNCTION,
                                CompletionItemKind::CONSTRUCTOR,
                                CompletionItemKind::FIELD,
                                CompletionItemKind::VARIABLE,
                                CompletionItemKind::CLASS,
                                CompletionItemKind::INTERFACE,
                                CompletionItemKind::MODULE,
                                CompletionItemKind::PROPERTY,
                                CompletionItemKind::UNIT,
                                CompletionItemKind::VALUE,
                                CompletionItemKind::ENUM,
                                CompletionItemKind::KEYWORD,
                                CompletionItemKind::SNIPPET,
                                CompletionItemKind::COLOR,
                                CompletionItemKind::FILE,
                                CompletionItemKind::REFERENCE,
                                CompletionItemKind::FOLDER,
                                CompletionItemKind::ENUM_MEMBER,
                                CompletionItemKind::CONSTANT,
                                CompletionItemKind::STRUCT,
                                CompletionItemKind::EVENT,
                                CompletionItemKind::OPERATOR,
                                CompletionItemKind::TYPE_PARAMETER,
                            ]),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    hover: Some(HoverClientCapabilities {
                        dynamic_registration: Some(true),
                        content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                    }),
                    definition: Some(GotoCapability {
                        dynamic_registration: Some(true),
                        link_support: Some(true),
                    }),
                    document_highlight: Some(DynamicRegistrationClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    document_symbol: Some(DocumentSymbolClientCapabilities {
                        dynamic_registration: Some(true),
                        hierarchical_document_symbol_support: Some(true),
                        ..Default::default()
                    }),
                    code_action: Some(CodeActionClientCapabilities {
                        dynamic_registration: Some(true),
                        code_action_literal_support: Some(CodeActionLiteralSupport {
                            code_action_kind: CodeActionKindLiteralSupport {
                                value_set: vec![
                                    CodeActionKind::QUICKFIX.as_str().to_string(),
                                    CodeActionKind::REFACTOR.as_str().to_string(),
                                    CodeActionKind::REFACTOR_EXTRACT.as_str().to_string(),
                                    CodeActionKind::REFACTOR_INLINE.as_str().to_string(),
                                    CodeActionKind::REFACTOR_REWRITE.as_str().to_string(),
                                    CodeActionKind::SOURCE.as_str().to_string(),
                                    CodeActionKind::SOURCE_ORGANIZE_IMPORTS.as_str().to_string(),
                                    CodeActionKind::SOURCE_FIX_ALL.as_str().to_string(),
                                ],
                            },
                        }),
                        ..Default::default()
                    }),
                    formatting: Some(DynamicRegistrationClientCapabilities {
                        dynamic_registration: Some(true),
                    }),
                    rename: Some(RenameClientCapabilities {
                        dynamic_registration: Some(true),
                        prepare_support: Some(true),
                        ..Default::default()
                    }),
                    semantic_tokens: Some(SemanticTokensClientCapabilities {
                        dynamic_registration: Some(true),
                        requests: SemanticTokensClientCapabilitiesRequests {
                            range: Some(true),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                        token_types: SEMANTIC_TOKEN_TYPES.to_vec(),
                        token_modifiers: SEMANTIC_TOKEN_MODIFIERS.to_vec(),
                        formats: vec![TokenFormat::RELATIVE],
                        ..Default::default()
                    }),
                    inlay_hint: Some(InlayHintClientCapabilities {
                        dynamic_registration: Some(true),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            trace: None,
            client_info: Some(ClientInfo {
                name: "Aether".to_string(),
                version: Some("0.1.0".to_string()),
            }),
            locale: None,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let params_value = Self::serialize_params(params)?;
        let id = self.send_request("initialize", Some(params_value)).await?;

        // initialize 允许更长超时（服务器首次启动慢）
        let result: Option<InitializeResult> =
            self.receive_response(&id, INITIALIZE_TIMEOUT).await?;
        if let Some(init_result) = result {
            self.cache_capabilities(&init_result.capabilities);
        }

        // 发送 initialized 通知
        let notification = LspMessage::Notification(LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "initialized".to_string(),
            params: Some(Self::serialize_params(InitializedParams {})?),
        });
        self.transport.send(&notification).await?;
        self.initialized = true;

        Ok(())
    }

    /// 缓存服务器能力
    fn cache_capabilities(&mut self, caps: &ServerCapabilities) {
        self.capabilities = ServerCapabilitiesCache {
            completion_provider: caps.completion_provider.clone(),
            hover_provider: caps.hover_provider.clone(),
            definition_provider: caps.definition_provider.clone(),
            references_provider: caps.references_provider.clone(),
            rename_provider: caps.rename_provider.clone(),
            code_action_provider: caps.code_action_provider.clone(),
            document_formatting_provider: caps.document_formatting_provider.clone(),
            diagnostic_provider: caps.diagnostic_provider.clone(),
            text_document_sync: caps.text_document_sync.clone().and_then(|s| match s {
                TextDocumentSyncCapability::Options(o) => Some(o),
                TextDocumentSyncCapability::Kind(_) => None,
            }),
            semantic_tokens_provider: caps.semantic_tokens_provider.clone(),
            inlay_hint_provider: caps.inlay_hint_provider.clone(),
        };
    }

    /// 打开文档
    pub async fn open_document(
        &mut self,
        uri: Url,
        language_id: String,
        version: i32,
        text: String,
    ) -> std::io::Result<()> {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: language_id.clone(),
                version,
                text: text.clone(),
            },
        };

        let notification = LspMessage::Notification(LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "textDocument/didOpen".to_string(),
            params: Some(Self::serialize_params(params)?),
        });

        self.transport.send(&notification).await?;

        self.open_documents.insert(
            uri.clone(),
            DocumentState {
                uri,
                version,
                language_id,
                text,
            },
        );

        Ok(())
    }

    /// 关闭文档
    pub async fn close_document(&mut self, uri: &Url) -> std::io::Result<()> {
        let params = DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        };

        let notification = LspMessage::Notification(LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "textDocument/didClose".to_string(),
            params: Some(Self::serialize_params(params)?),
        });

        self.transport.send(&notification).await?;
        self.open_documents.remove(uri);

        Ok(())
    }

    /// 发送文档变更通知（增量同步）
    pub async fn change_document(
        &mut self,
        uri: &Url,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) -> std::io::Result<()> {
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version,
            },
            content_changes: changes,
        };

        let notification = LspMessage::Notification(LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "textDocument/didChange".to_string(),
            params: Some(Self::serialize_params(params)?),
        });

        self.transport.send(&notification).await?;

        if let Some(doc) = self.open_documents.get_mut(uri) {
            doc.version = version;
        }

        Ok(())
    }

    /// 请求代码补全
    pub async fn request_completion(
        &mut self,
        uri: &Url,
        position: Position,
    ) -> std::io::Result<Option<CompletionResponse>> {
        let id = self.id_generator.next();
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };

        let request = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: "textDocument/completion".to_string(),
            params: Some(Self::serialize_params(params)?),
        });

        self.transport.send(&request).await?;
        self.pending_requests
            .insert(id.clone(), "textDocument/completion".to_string());

        self.receive_response(&id, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求悬停提示
    pub async fn request_hover(
        &mut self,
        uri: &Url,
        position: Position,
    ) -> std::io::Result<Option<Hover>> {
        let id = self.id_generator.next();
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let request = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: "textDocument/hover".to_string(),
            params: Some(Self::serialize_params(params)?),
        });

        self.transport.send(&request).await?;
        self.pending_requests
            .insert(id.clone(), "textDocument/hover".to_string());

        self.receive_response(&id, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求跳转到定义
    pub async fn request_definition(
        &mut self,
        uri: &Url,
        position: Position,
    ) -> std::io::Result<Option<GotoDefinitionResponse>> {
        let id = self.id_generator.next();
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let request = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: "textDocument/definition".to_string(),
            params: Some(Self::serialize_params(params)?),
        });

        self.transport.send(&request).await?;
        self.pending_requests
            .insert(id.clone(), "textDocument/definition".to_string());

        self.receive_response(&id, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 优雅关闭服务器
    pub async fn shutdown(&mut self) -> std::io::Result<()> {
        if !self.initialized {
            return Ok(());
        }

        let id = self.id_generator.next();
        let request = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: "shutdown".to_string(),
            params: None,
        });

        self.transport.send(&request).await?;

        // shutdown 响应通常很快，但给予充足超时
        let _: Option<serde_json::Value> =
            self.receive_response(&id, Duration::from_secs(10)).await?;

        // 发送 exit 通知
        let notification = LspMessage::Notification(LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "exit".to_string(),
            params: None,
        });
        self.transport.send(&notification).await?;
        self.initialized = false;

        Ok(())
    }

    /// 获取服务器能力
    pub fn capabilities(&self) -> &ServerCapabilitiesCache {
        &self.capabilities
    }

    /// 是否支持补全
    pub fn supports_completion(&self) -> bool {
        self.capabilities.completion_provider.is_some()
    }

    /// 是否支持悬停
    pub fn supports_hover(&self) -> bool {
        self.capabilities.hover_provider.is_some()
    }

    /// 是否支持跳转定义
    pub fn supports_definition(&self) -> bool {
        self.capabilities.definition_provider.is_some()
    }

    /// 请求查找引用
    pub async fn request_references(
        &mut self,
        uri: &Url,
        position: Position,
        include_declaration: bool,
    ) -> std::io::Result<Option<Vec<Location>>> {
        let id = self.id_generator.next();
        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration,
            },
        };

        let request = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: "textDocument/references".to_string(),
            params: Some(Self::serialize_params(params)?),
        });

        self.transport.send(&request).await?;
        self.pending_requests
            .insert(id.clone(), "textDocument/references".to_string());

        self.receive_response(&id, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求重命名
    pub async fn request_rename(
        &mut self,
        uri: &Url,
        position: Position,
        new_name: String,
    ) -> std::io::Result<Option<WorkspaceEdit>> {
        let id = self.id_generator.next();
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            new_name,
        };

        let request = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: "textDocument/rename".to_string(),
            params: Some(Self::serialize_params(params)?),
        });

        self.transport.send(&request).await?;
        self.pending_requests
            .insert(id.clone(), "textDocument/rename".to_string());

        self.receive_response(&id, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求代码操作
    pub async fn request_code_actions(
        &mut self,
        uri: &Url,
        range: Range,
        diagnostics: Vec<Diagnostic>,
    ) -> std::io::Result<Option<CodeActionResponse>> {
        let id = self.id_generator.next();
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range,
            context: CodeActionContext {
                diagnostics,
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let request = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: "textDocument/codeAction".to_string(),
            params: Some(Self::serialize_params(params)?),
        });

        self.transport.send(&request).await?;
        self.pending_requests
            .insert(id.clone(), "textDocument/codeAction".to_string());

        self.receive_response(&id, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求格式化
    pub async fn request_formatting(
        &mut self,
        uri: &Url,
        options: FormattingOptions,
    ) -> std::io::Result<Option<Vec<TextEdit>>> {
        let id = self.id_generator.next();
        let params = DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            options,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let request = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: "textDocument/formatting".to_string(),
            params: Some(Self::serialize_params(params)?),
        });

        self.transport.send(&request).await?;
        self.pending_requests
            .insert(id.clone(), "textDocument/formatting".to_string());

        self.receive_response(&id, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 是否支持查找引用
    pub fn supports_references(&self) -> bool {
        self.capabilities.references_provider.is_some()
    }

    /// 是否支持重命名
    pub fn supports_rename(&self) -> bool {
        self.capabilities.rename_provider.is_some()
    }

    /// 是否支持代码操作
    pub fn supports_code_actions(&self) -> bool {
        self.capabilities.code_action_provider.is_some()
    }

    /// 是否支持格式化
    pub fn supports_formatting(&self) -> bool {
        self.capabilities.document_formatting_provider.is_some()
    }

    /// 是否支持语义令牌
    pub fn supports_semantic_tokens(&self) -> bool {
        self.capabilities.semantic_tokens_provider.is_some()
    }

    /// 是否支持内联提示
    pub fn supports_inlay_hints(&self) -> bool {
        self.capabilities.inlay_hint_provider.is_some()
    }

    /// 请求完整语义令牌
    pub async fn request_semantic_tokens_full(
        &mut self,
        uri: &Url,
    ) -> std::io::Result<Option<SemanticTokens>> {
        let id = self.id_generator.next();
        let params = SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let request = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: "textDocument/semanticTokens/full".to_string(),
            params: Some(Self::serialize_params(params)?),
        });

        self.transport.send(&request).await?;
        self.pending_requests
            .insert(id.clone(), "textDocument/semanticTokens/full".to_string());

        self.receive_response(&id, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求语义令牌delta更新
    pub async fn request_semantic_tokens_delta(
        &mut self,
        uri: &Url,
        previous_result_id: String,
    ) -> std::io::Result<Option<SemanticTokensDelta>> {
        let id = self.id_generator.next();
        let params = SemanticTokensDeltaParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            previous_result_id,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let request = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: "textDocument/semanticTokens/full/delta".to_string(),
            params: Some(Self::serialize_params(params)?),
        });

        self.transport.send(&request).await?;
        self.pending_requests.insert(
            id.clone(),
            "textDocument/semanticTokens/full/delta".to_string(),
        );

        self.receive_response(&id, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求范围语义令牌
    pub async fn request_semantic_tokens_range(
        &mut self,
        uri: &Url,
        range: Range,
    ) -> std::io::Result<Option<SemanticTokens>> {
        let id = self.id_generator.next();
        let params = SemanticTokensRangeParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let request = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: "textDocument/semanticTokens/range".to_string(),
            params: Some(Self::serialize_params(params)?),
        });

        self.transport.send(&request).await?;
        self.pending_requests
            .insert(id.clone(), "textDocument/semanticTokens/range".to_string());

        self.receive_response(&id, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求内联提示
    pub async fn request_inlay_hints(
        &mut self,
        uri: &Url,
        range: Range,
    ) -> std::io::Result<Option<Vec<InlayHint>>> {
        let id = self.id_generator.next();
        let params = InlayHintParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let request = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: "textDocument/inlayHint".to_string(),
            params: Some(Self::serialize_params(params)?),
        });

        self.transport.send(&request).await?;
        self.pending_requests
            .insert(id.clone(), "textDocument/inlayHint".to_string());

        self.receive_response(&id, DEFAULT_REQUEST_TIMEOUT).await
    }
}

/// 语义令牌类型（LSP 3.16+ 标准）
const SEMANTIC_TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::NAMESPACE,
    SemanticTokenType::TYPE,
    SemanticTokenType::CLASS,
    SemanticTokenType::ENUM,
    SemanticTokenType::INTERFACE,
    SemanticTokenType::STRUCT,
    SemanticTokenType::TYPE_PARAMETER,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::EVENT,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::METHOD,
    SemanticTokenType::MACRO,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::MODIFIER,
    SemanticTokenType::COMMENT,
    SemanticTokenType::STRING,
    SemanticTokenType::NUMBER,
    SemanticTokenType::REGEXP,
    SemanticTokenType::OPERATOR,
];

/// 语义令牌修饰符
const SEMANTIC_TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
    SemanticTokenModifier::READONLY,
    SemanticTokenModifier::STATIC,
    SemanticTokenModifier::DEPRECATED,
    SemanticTokenModifier::ABSTRACT,
    SemanticTokenModifier::ASYNC,
    SemanticTokenModifier::MODIFICATION,
    SemanticTokenModifier::DOCUMENTATION,
    SemanticTokenModifier::DEFAULT_LIBRARY,
];
