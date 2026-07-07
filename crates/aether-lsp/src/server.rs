use lsp_types::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::ChildStdin;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::client::LspEvent;
use crate::transport::{spawn_server, spawn_stderr_drain, LspReader, LspWriter};
use crate::types::*;
use tokio::process::Child;

/// 默认请求超时（秒）。
///
/// 大多数 LSP 请求应在 30 秒内完成。initialize 可能更慢，单独设置。
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// initialize 请求超时（秒）。
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);

/// 语言服务器实例管理
/// 负责单个语言服务器的完整生命周期：发现→启动→初始化→运行→关闭
///
/// 架构（接线修复后）：
/// - 主线程持有 `LspWriter`，所有出站请求/通知通过它发送
/// - 后台 `reader_loop` task 独占 `LspReader`，持续读 stdout
/// - 请求-响应通过 `oneshot::channel` 配对：调用方 await receiver，
///   reader task 收到 Response 时通过 sender 投递
/// - Notification（如 publishDiagnostics）由 reader task 直接转发到 `event_tx`
///
/// 这样修复了"无后台 reader 时纯通知路径诊断滞留管道"的缺陷。
pub struct LanguageServer<W = ChildStdin> {
    /// 写入器（仅 stdin，不与 reader task 共享）
    writer: LspWriter<W>,
    /// 服务器配置
    config: ServerConfig,
    /// 已缓存的服务器能力
    capabilities: ServerCapabilitiesCache,
    /// 请求ID生成器
    id_generator: RequestIdGenerator,
    /// 等待中的请求：id -> oneshot sender
    ///
    /// reader task 持有 Arc 副本，收到 Response 时通过 sender 投递。
    /// 超时时调用方从此表 remove 对应 sender 以释放资源。
    response_channels: Arc<Mutex<HashMap<serde_json::Value, oneshot::Sender<LspResponse>>>>,
    /// 已打开的文档
    open_documents: HashMap<Url, DocumentState>,
    /// 服务器是否已初始化
    initialized: bool,
    /// 语言ID（如 "rust", "python"）
    pub language_id: String,
    /// 子进程句柄，用于 shutdown 时超时 kill
    child: Option<Child>,
    /// reader task 句柄，Drop 时 abort 防泄漏
    reader_handle: Option<JoinHandle<()>>,
    /// 事件发送器（向UI层推送通知）
    #[allow(dead_code)]
    event_tx: Option<mpsc::UnboundedSender<LspEvent>>,
}

impl LanguageServer<ChildStdin> {
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
        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("Failed to capture stdin"))?;
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("Failed to capture stdout"))?;
        let stderr = process.stderr.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to capture stderr")
        })?;

        let writer = LspWriter::new(stdin);
        let reader = LspReader::new(stdout);

        // 启动后台 stderr 读取任务，避免子进程 stderr 缓冲区满后阻塞
        spawn_stderr_drain(stderr);

        // 共享给 reader task 的响应通道表
        let response_channels: Arc<
            Mutex<HashMap<serde_json::Value, oneshot::Sender<LspResponse>>>,
        > = Arc::new(Mutex::new(HashMap::new()));

        // 启动常驻 stdout reader task，持续解析消息并分发
        let reader_handle = tokio::spawn(reader_loop(
            reader,
            event_tx.clone(),
            response_channels.clone(),
            language_id.clone(),
        ));

        let mut server = Self {
            writer,
            config: config.clone(),
            capabilities: ServerCapabilitiesCache::default(),
            id_generator: RequestIdGenerator::new(),
            response_channels,
            open_documents: HashMap::new(),
            initialized: false,
            language_id,
            child: Some(process),
            reader_handle: Some(reader_handle),
            event_tx,
        };

        // 发送 initialize 请求
        server.initialize().await?;

        Ok(server)
    }
}

impl<W: AsyncWrite + Unpin + Send + 'static> LanguageServer<W> {
    /// 序列化参数为 JSON Value，失败时返回 io::Error 而非 panic
    pub(crate) fn serialize_params<T: serde::Serialize>(
        params: T,
    ) -> std::io::Result<serde_json::Value> {
        serde_json::to_value(params).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("JSON serialize error: {}", e),
            )
        })
    }

    /// 发送请求并返回 (id, receiver)。
    ///
    /// 调用方应随后调用 `receive_response(id, rx, timeout)` 等待响应。
    /// reader task 收到匹配 id 的 Response 时通过 sender 投递。
    async fn send_request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> std::io::Result<(serde_json::Value, oneshot::Receiver<LspResponse>)> {
        let id = self.id_generator.next();
        let request = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: method.to_string(),
            params,
        });

        self.writer.send(&request).await?;

        let (tx, rx) = oneshot::channel();
        self.response_channels.lock().await.insert(id.clone(), tx);
        Ok((id, rx))
    }

    /// 等待指定 id 的响应，超时返回错误。
    ///
    /// - 成功响应：反序列化为 T，返回 Ok(Some(T))；result 为 null 时返回 Ok(None)
    /// - 错误响应：返回 Err(io::Error)，携带 LSP 错误码和消息
    /// - 服务器关闭 stdout：reader task 退出时 drop 所有 sender，receiver 收到 RecvError
    /// - 超时：从 response_channels 移除该 id 的 sender，返回 Err(io::Error)
    async fn receive_response<T: serde::de::DeserializeOwned>(
        &self,
        id: serde_json::Value,
        rx: oneshot::Receiver<LspResponse>,
        request_timeout: Duration,
    ) -> std::io::Result<Option<T>> {
        let fut = async {
            match rx.await {
                Ok(resp) => {
                    if let Some(err) = resp.error {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("LSP error {}: {}", err.code, err.message),
                        ));
                    }
                    match resp.result {
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
                    }
                }
                Err(_) => Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "LSP server closed stdout",
                )),
            }
        };

        match timeout(request_timeout, fut).await {
            Ok(result) => result,
            Err(_) => {
                // 超时：清理 pending sender，避免泄漏
                self.response_channels.lock().await.remove(&id);
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "LSP request timed out",
                ))
            }
        }
    }

    /// 处理服务器发起的反向请求（server -> client），并发送 JSON-RPC 响应。
    ///
    /// 目前实现常见请求的最小可用响应，避免服务器因等待响应而超时/卡死：
    /// - `workspace/configuration`：返回与请求项数对应的空配置
    /// - `client/registerCapability` / `client/unregisterCapability`：返回空结果
    /// - `workspace/applyEdit`：返回 `{ applied: false }`（UI 层尚未支持自动应用）
    /// - `workspace/workspaceFolders`：返回当前 root_uri 作为唯一工作区
    /// - 其他：返回 MethodNotFound 错误
    #[allow(dead_code)]
    pub(crate) async fn handle_server_request(&mut self, req: LspRequest) -> std::io::Result<()> {
        let result: Result<serde_json::Value, LspError> = match req.method.as_str() {
            "workspace/configuration" => {
                let count = req
                    .params
                    .as_ref()
                    .and_then(|p| serde_json::from_value::<ConfigurationParams>(p.clone()).ok())
                    .map(|p| p.items.len())
                    .unwrap_or(0);
                let configs: Vec<serde_json::Value> = vec![serde_json::Value::Null; count];
                Ok(serde_json::to_value(configs).unwrap_or(serde_json::Value::Array(vec![])))
            }
            "client/registerCapability" | "client/unregisterCapability" => {
                Ok(serde_json::Value::Null)
            }
            "workspace/applyEdit" => {
                // 先通过事件通道通知 UI 层（如果有监听器可应用），但默认标记为未应用
                if let Some(tx) = &self.event_tx {
                    // 未来可扩展 LspEvent::ApplyEdit
                    let _ = tx.send(LspEvent::Log {
                        language_id: self.language_id.clone(),
                        message: "Server requested workspace/applyEdit (not yet applied)"
                            .to_string(),
                    });
                }
                Ok(serde_json::to_value(ApplyWorkspaceEditResponse {
                    applied: false,
                    failure_reason: Some(
                        "Aether does not yet auto-apply workspace edits".to_string(),
                    ),
                    failed_change: None,
                })
                .unwrap_or(serde_json::Value::Null))
            }
            "workspace/workspaceFolders" => {
                let folders = self.config.root_uri.as_ref().map(|uri| {
                    vec![WorkspaceFolder {
                        uri: uri.clone(),
                        name: uri.path().to_string(),
                    }]
                });
                Ok(serde_json::to_value(folders).unwrap_or(serde_json::Value::Null))
            }
            _ => Err(LspError {
                code: -32601,
                message: format!("Method not found: {}", req.method),
                data: None,
            }),
        };

        let response = match result {
            Ok(res) => LspMessage::Response(LspResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(res),
                error: None,
            }),
            Err(err) => LspMessage::Response(LspResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: None,
                error: Some(err),
            }),
        };

        self.writer.send(&response).await
    }

    /// 处理服务器推送的 notification，转发到 UI 层。
    ///
    /// 这是修复「通知静默丢失」缺陷的核心：原实现 `_ => {}` 会丢弃所有
    /// diagnostics、logMessage、showMessage 等推送，导致 UI 永远收不到诊断。
    #[allow(dead_code)]
    pub(crate) fn handle_notification(&self, notif: LspNotification) {
        handle_notification(&self.language_id, &self.event_tx, notif);
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
        let (id, rx) = self.send_request("initialize", Some(params_value)).await?;

        // initialize 允许更长超时（服务器首次启动慢）
        let result: Option<InitializeResult> =
            self.receive_response(id, rx, INITIALIZE_TIMEOUT).await?;
        if let Some(init_result) = result {
            self.cache_capabilities(&init_result.capabilities);
        }

        // 发送 initialized 通知
        let notification = LspMessage::Notification(LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "initialized".to_string(),
            params: Some(Self::serialize_params(InitializedParams {})?),
        });
        self.writer.send(&notification).await?;
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

        self.writer.send(&notification).await?;

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

        self.writer.send(&notification).await?;
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

        self.writer.send(&notification).await?;

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
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };

        let (id, rx) = self
            .send_request(
                "textDocument/completion",
                Some(Self::serialize_params(params)?),
            )
            .await?;
        self.receive_response(id, rx, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求悬停提示
    pub async fn request_hover(
        &mut self,
        uri: &Url,
        position: Position,
    ) -> std::io::Result<Option<Hover>> {
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let (id, rx) = self
            .send_request("textDocument/hover", Some(Self::serialize_params(params)?))
            .await?;
        self.receive_response(id, rx, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求跳转到定义
    pub async fn request_definition(
        &mut self,
        uri: &Url,
        position: Position,
    ) -> std::io::Result<Option<GotoDefinitionResponse>> {
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let (id, rx) = self
            .send_request(
                "textDocument/definition",
                Some(Self::serialize_params(params)?),
            )
            .await?;
        self.receive_response(id, rx, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 优雅关闭服务器
    pub async fn shutdown(&mut self) -> std::io::Result<()> {
        if !self.initialized {
            return Ok(());
        }

        let (id, rx) = self.send_request("shutdown", None).await?;

        // shutdown 响应通常很快，但给予充足超时
        let _: Option<serde_json::Value> = self
            .receive_response(id, rx, Duration::from_secs(10))
            .await?;

        // 发送 exit 通知
        let notification = LspMessage::Notification(LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "exit".to_string(),
            params: None,
        });
        self.writer.send(&notification).await?;
        self.initialized = false;

        // H-04: 发送 exit 通知后等待 5 秒，超时则强制 kill 子进程
        if let Some(mut child) = self.child.take() {
            match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                Ok(_) => {}
                Err(_) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }
            }
        }

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

        let (id, rx) = self
            .send_request(
                "textDocument/references",
                Some(Self::serialize_params(params)?),
            )
            .await?;
        self.receive_response(id, rx, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求重命名
    pub async fn request_rename(
        &mut self,
        uri: &Url,
        position: Position,
        new_name: String,
    ) -> std::io::Result<Option<WorkspaceEdit>> {
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            new_name,
        };

        let (id, rx) = self
            .send_request("textDocument/rename", Some(Self::serialize_params(params)?))
            .await?;
        self.receive_response(id, rx, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求代码操作
    pub async fn request_code_actions(
        &mut self,
        uri: &Url,
        range: Range,
        diagnostics: Vec<Diagnostic>,
    ) -> std::io::Result<Option<CodeActionResponse>> {
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

        let (id, rx) = self
            .send_request(
                "textDocument/codeAction",
                Some(Self::serialize_params(params)?),
            )
            .await?;
        self.receive_response(id, rx, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求格式化
    pub async fn request_formatting(
        &mut self,
        uri: &Url,
        options: FormattingOptions,
    ) -> std::io::Result<Option<Vec<TextEdit>>> {
        let params = DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            options,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let (id, rx) = self
            .send_request(
                "textDocument/formatting",
                Some(Self::serialize_params(params)?),
            )
            .await?;
        self.receive_response(id, rx, DEFAULT_REQUEST_TIMEOUT).await
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
        let params = SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let (id, rx) = self
            .send_request(
                "textDocument/semanticTokens/full",
                Some(Self::serialize_params(params)?),
            )
            .await?;
        self.receive_response(id, rx, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求语义令牌delta更新
    pub async fn request_semantic_tokens_delta(
        &mut self,
        uri: &Url,
        previous_result_id: String,
    ) -> std::io::Result<Option<SemanticTokensDelta>> {
        let params = SemanticTokensDeltaParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            previous_result_id,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let (id, rx) = self
            .send_request(
                "textDocument/semanticTokens/full/delta",
                Some(Self::serialize_params(params)?),
            )
            .await?;
        self.receive_response(id, rx, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求范围语义令牌
    pub async fn request_semantic_tokens_range(
        &mut self,
        uri: &Url,
        range: Range,
    ) -> std::io::Result<Option<SemanticTokens>> {
        let params = SemanticTokensRangeParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let (id, rx) = self
            .send_request(
                "textDocument/semanticTokens/range",
                Some(Self::serialize_params(params)?),
            )
            .await?;
        self.receive_response(id, rx, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// 请求内联提示
    pub async fn request_inlay_hints(
        &mut self,
        uri: &Url,
        range: Range,
    ) -> std::io::Result<Option<Vec<InlayHint>>> {
        let params = InlayHintParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let (id, rx) = self
            .send_request(
                "textDocument/inlayHint",
                Some(Self::serialize_params(params)?),
            )
            .await?;
        self.receive_response(id, rx, DEFAULT_REQUEST_TIMEOUT).await
    }
}

/// 常驻 stdout reader task：持续解析子进程 stdout 的 LSP 消息并分发。
///
/// - Response：按 id 查 response_channels，通过 oneshot 投递给等待的请求方
/// - Notification（如 publishDiagnostics）：直接转发到 event_tx
/// - Server->Client Request：当前未实现，记日志忽略
///
/// 退出条件：reader.receive() 返回错误（通常 stdout EOF，子进程已退出）。
/// 退出时清理 response_channels 中所有 pending sender，让等待方收到 RecvError。
async fn reader_loop<R>(
    mut reader: LspReader<R>,
    event_tx: Option<mpsc::UnboundedSender<LspEvent>>,
    response_channels: Arc<Mutex<HashMap<serde_json::Value, oneshot::Sender<LspResponse>>>>,
    language_id: String,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    loop {
        match reader.receive().await {
            Ok(message) => match message {
                LspMessage::Response(resp) => {
                    let id = resp.id.clone();
                    let mut channels = response_channels.lock().await;
                    if let Some(sender) = channels.remove(&id) {
                        // sender send 失败表示请求方已超时放弃，忽略
                        let _ = sender.send(resp);
                    }
                    // 不在 channels 中的响应（已超时清理）直接丢弃
                }
                LspMessage::Notification(notif) => {
                    handle_notification(&language_id, &event_tx, notif);
                }
                LspMessage::Request(req) => {
                    // 服务器发起的反向请求（如 workspace/configuration）
                    // 当前未实现处理，回 error 避免服务器卡死
                    tracing::debug!("Unhandled server->client request: {}", req.method);
                }
            },
            Err(e) => {
                tracing::debug!("LSP reader loop exit ({}): {:?}", language_id, e);
                // 清理所有 pending sender，让等待方收到 RecvError
                let mut channels = response_channels.lock().await;
                channels.clear();
                break;
            }
        }
    }
}

/// 处理服务器推送的 notification，转发到 UI 层。
///
/// 这是修复「通知静默丢失」缺陷的核心：原实现 `_ => {}` 会丢弃所有
/// diagnostics、logMessage、showMessage 等推送，导致 UI 永远收不到诊断。
fn handle_notification(
    language_id: &str,
    event_tx: &Option<mpsc::UnboundedSender<LspEvent>>,
    notif: LspNotification,
) {
    match notif.method.as_str() {
        "textDocument/publishDiagnostics" => {
            if let Some(tx) = event_tx {
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
            if let Some(tx) = event_tx {
                if let Some(params) = notif.params {
                    let message = params
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let _ = tx.send(LspEvent::Log {
                        language_id: language_id.to_string(),
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

/// H-04: 防止 LanguageServer 异常路径下未调用 shutdown 导致僵尸进程。
///
/// tokio::process::Child 默认不会在 drop 时 kill 子进程，
/// 必须显式处理，否则语言服务器进程会一直驻留。
impl<W> Drop for LanguageServer<W> {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
        // 中止 reader task（它会在 stdout EOF 后自然退出，但显式 abort 更快）
        if let Some(handle) = self.reader_handle.take() {
            handle.abort();
        }
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

#[cfg(test)]
impl<W: AsyncWrite + Unpin + Send + 'static> LanguageServer<W> {
    /// 测试用构造函数，不实际启动子进程
    #[cfg(test)]
    pub(crate) fn new_for_test<R>(
        writer: LspWriter<W>,
        reader: R,
        config: ServerConfig,
        language_id: String,
        event_tx: Option<mpsc::UnboundedSender<LspEvent>>,
    ) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let response_channels: Arc<
            Mutex<HashMap<serde_json::Value, oneshot::Sender<LspResponse>>>,
        > = Arc::new(Mutex::new(HashMap::new()));

        let reader_handle = tokio::spawn(reader_loop(
            LspReader::new(reader),
            event_tx.clone(),
            response_channels.clone(),
            language_id.clone(),
        ));

        Self {
            writer,
            config,
            capabilities: ServerCapabilitiesCache::default(),
            id_generator: RequestIdGenerator::new(),
            response_channels,
            open_documents: HashMap::new(),
            initialized: false,
            language_id,
            child: None,
            reader_handle: Some(reader_handle),
            event_tx,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{encode_message, LspTransport};
    use tokio::io::AsyncReadExt;

    #[test]
    fn test_serialize_params() {
        let params = serde_json::json!({"key": "value"});
        let value =
            LanguageServer::<tokio::io::DuplexStream>::serialize_params(params.clone()).unwrap();
        assert_eq!(value, params);
    }

    #[tokio::test]
    async fn test_supports_capabilities() {
        use lsp_types::{CompletionOptions, HoverProviderCapability};
        let (tx, _rx) = mpsc::unbounded_channel();
        let (read, write) = tokio::io::duplex(1024);
        let mut server = LanguageServer::new_for_test(
            LspWriter::new(write),
            read,
            ServerConfig::default(),
            "rust".to_string(),
            Some(tx),
        );

        assert!(!server.supports_completion());
        server.capabilities.completion_provider = Some(CompletionOptions::default());
        assert!(server.supports_completion());

        assert!(!server.supports_hover());
        server.capabilities.hover_provider = Some(HoverProviderCapability::Simple(true));
        assert!(server.supports_hover());
    }

    #[tokio::test]
    async fn test_handle_notification_publish_diagnostics() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (read, write) = tokio::io::duplex(1024);
        let server = LanguageServer::new_for_test(
            LspWriter::new(write),
            read,
            ServerConfig::default(),
            "rust".to_string(),
            Some(tx),
        );

        let uri = Url::parse("file:///test.rs").unwrap();
        server.handle_notification(LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "textDocument/publishDiagnostics".to_string(),
            params: Some(serde_json::json!({
                "uri": uri,
                "diagnostics": []
            })),
        });

        match rx.try_recv().unwrap() {
            LspEvent::Diagnostics {
                uri: event_uri,
                diagnostics,
            } => {
                assert_eq!(event_uri, uri);
                assert!(diagnostics.is_empty());
            }
            _ => panic!("expected Diagnostics event"),
        }
    }

    #[tokio::test]
    async fn test_handle_notification_log_message() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (read, write) = tokio::io::duplex(1024);
        let server = LanguageServer::new_for_test(
            LspWriter::new(write),
            read,
            ServerConfig::default(),
            "rust".to_string(),
            Some(tx),
        );

        server.handle_notification(LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "window/logMessage".to_string(),
            params: Some(serde_json::json!({"message": "hello"})),
        });

        match rx.try_recv().unwrap() {
            LspEvent::Log {
                language_id,
                message,
            } => {
                assert_eq!(language_id, "rust");
                assert_eq!(message, "hello");
            }
            _ => panic!("expected Log event"),
        }
    }

    #[tokio::test]
    async fn test_handle_notification_unknown() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (read, write) = tokio::io::duplex(1024);
        let server = LanguageServer::new_for_test(
            LspWriter::new(write),
            read,
            ServerConfig::default(),
            "rust".to_string(),
            Some(tx),
        );

        server.handle_notification(LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "unknown/notification".to_string(),
            params: None,
        });

        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_handle_server_request_workspace_configuration() {
        // stdin 通道: client_write -> server_read (handle_server_request 通过 writer 写响应)
        let (client_write, mut server_read) = tokio::io::duplex(1024);
        // stdout 通道未使用, reader task 会忽略 EOF，但不影响此测试
        let (_server_write, client_read) = tokio::io::duplex(1024);

        let mut server = LanguageServer::new_for_test(
            LspWriter::new(client_write),
            client_read,
            ServerConfig::default(),
            "rust".to_string(),
            None,
        );

        let req = LspRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::Number(10i64.into()),
            method: "workspace/configuration".to_string(),
            params: Some(serde_json::json!({"items": [{}, {}]})),
        };
        server.handle_server_request(req).await.unwrap();

        let mut buf = vec![0u8; 1024];
        let n = server_read.read(&mut buf).await.unwrap();
        let response_str = String::from_utf8(buf[..n].to_vec()).unwrap();
        assert!(response_str.contains("\"id\":10"));
        assert!(response_str.contains("\"result\""));
    }

    #[tokio::test]
    async fn test_handle_server_request_unknown_method() {
        let (client_write, mut server_read) = tokio::io::duplex(1024);
        let (_server_write, client_read) = tokio::io::duplex(1024);

        let mut server = LanguageServer::new_for_test(
            LspWriter::new(client_write),
            client_read,
            ServerConfig::default(),
            "rust".to_string(),
            None,
        );

        let req = LspRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::Number(11i64.into()),
            method: "client/unknown".to_string(),
            params: None,
        };
        server.handle_server_request(req).await.unwrap();

        let mut buf = vec![0u8; 1024];
        let n = server_read.read(&mut buf).await.unwrap();
        let response_str = String::from_utf8(buf[..n].to_vec()).unwrap();
        assert!(response_str.contains("\"id\":11"));
        assert!(response_str.contains("-32601"));
    }

    #[tokio::test]
    async fn test_cache_capabilities() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let (read, write) = tokio::io::duplex(1024);
        let mut server = LanguageServer::new_for_test(
            LspWriter::new(write),
            read,
            ServerConfig::default(),
            "rust".to_string(),
            Some(tx),
        );

        let caps = ServerCapabilities {
            completion_provider: Some(CompletionOptions::default()),
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            ..Default::default()
        };
        server.cache_capabilities(&caps);
        assert!(server.capabilities.completion_provider.is_some());
        assert!(server.capabilities.hover_provider.is_some());
    }

    // 以下测试使用 mock 双向管道覆盖 LanguageServer 中不依赖真实子进程的分支。
    type TestServer = LanguageServer<tokio::io::DuplexStream>;

    struct Harness {
        server: TestServer,
        client_recv: LspTransport<tokio::io::DuplexStream, tokio::io::DuplexStream>,
        server_write: tokio::io::DuplexStream,
    }

    fn new_harness(event_tx: Option<mpsc::UnboundedSender<LspEvent>>) -> Harness {
        let (client_write, server_read) = tokio::io::duplex(16384);
        let (server_write, client_read) = tokio::io::duplex(16384);
        let (_, dummy_write) = tokio::io::duplex(1);
        let server = LanguageServer::new_for_test(
            LspWriter::new(client_write),
            client_read,
            ServerConfig::default(),
            "rust".to_string(),
            event_tx,
        );
        let client_recv = LspTransport::new_generic(dummy_write, server_read);
        Harness {
            server,
            client_recv,
            server_write,
        }
    }

    async fn write_response(write: &mut tokio::io::DuplexStream, message: &LspMessage) {
        let bytes = encode_message(message).unwrap();
        use tokio::io::AsyncWriteExt;
        write.write_all(&bytes).await.unwrap();
        write.flush().await.unwrap();
    }

    #[test]
    fn test_serialize_params_error() {
        struct AlwaysFails;
        impl serde::Serialize for AlwaysFails {
            fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                Err(serde::ser::Error::custom("fail"))
            }
        }
        let result = TestServer::serialize_params(AlwaysFails);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_send_request_tracks_pending() {
        let mut h = new_harness(None);
        let (id, _rx) = h
            .server
            .send_request("textDocument/definition", Some(serde_json::json!({})))
            .await
            .unwrap();
        assert!(h.server.response_channels.lock().await.contains_key(&id));
        let received = h.client_recv.receive().await.unwrap();
        match received {
            LspMessage::Request(req) => {
                assert_eq!(req.method, "textDocument/definition");
                assert_eq!(req.id, id);
            }
            _ => panic!("expected request"),
        }
    }

    #[tokio::test]
    async fn test_receive_response_success() {
        let mut h = new_harness(None);
        let (id, rx) = h.server.send_request("x", None).await.unwrap();
        let response = LspMessage::Response(LspResponse {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            result: Some(serde_json::json!({"items": []})),
            error: None,
        });
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            let mut write = h.server_write;
            write_response(&mut write, &response).await;
        });
        let result: Option<serde_json::Value> = h
            .server
            .receive_response(id, rx, Duration::from_secs(5))
            .await
            .unwrap();
        assert!(result.is_some());
        assert!(h.server.response_channels.lock().await.is_empty());
    }

    #[tokio::test]
    async fn test_receive_response_error() {
        let mut h = new_harness(None);
        let (id, rx) = h.server.send_request("x", None).await.unwrap();
        let response = LspMessage::Response(LspResponse {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            result: None,
            error: Some(LspError {
                code: -32600,
                message: "bad".to_string(),
                data: None,
            }),
        });
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            let mut write = h.server_write;
            write_response(&mut write, &response).await;
        });
        let result = h
            .server
            .receive_response::<serde_json::Value>(id, rx, Duration::from_secs(5))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_receive_response_orphan_dropped() {
        let mut h = new_harness(None);
        let (id, rx) = h.server.send_request("x", None).await.unwrap();
        let orphan = LspMessage::Response(LspResponse {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::Number(999i64.into()),
            result: Some(serde_json::json!({})),
            error: None,
        });
        let good = LspMessage::Response(LspResponse {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            result: Some(serde_json::Value::Null),
            error: None,
        });
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            let mut write = h.server_write;
            write_response(&mut write, &orphan).await;
            write_response(&mut write, &good).await;
        });
        let result = h
            .server
            .receive_response::<serde_json::Value>(id, rx, Duration::from_secs(5))
            .await
            .unwrap();
        assert!(result.is_none());
        assert!(h.server.response_channels.lock().await.is_empty());
    }

    #[tokio::test]
    async fn test_receive_response_forwards_notification() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut h = new_harness(Some(tx));
        let (id, rx_resp) = h.server.send_request("x", None).await.unwrap();
        let notif = LspMessage::Notification(LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "textDocument/publishDiagnostics".to_string(),
            params: Some(serde_json::json!({"uri": "file:///a.rs", "diagnostics": []})),
        });
        let response = LspMessage::Response(LspResponse {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            result: Some(serde_json::Value::Null),
            error: None,
        });
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            let mut write = h.server_write;
            write_response(&mut write, &notif).await;
            write_response(&mut write, &response).await;
        });
        let _ = h
            .server
            .receive_response::<serde_json::Value>(id, rx_resp, Duration::from_secs(5))
            .await
            .unwrap();
        match rx.try_recv().unwrap() {
            LspEvent::Diagnostics { .. } => {}
            _ => panic!("expected diagnostics"),
        }
    }

    #[tokio::test]
    async fn test_handle_server_request_register_capability() {
        let mut h = new_harness(None);
        let req = LspRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::Number(1i64.into()),
            method: "client/registerCapability".to_string(),
            params: None,
        };
        h.server.handle_server_request(req).await.unwrap();
        let resp = h.client_recv.receive().await.unwrap();
        match resp {
            LspMessage::Response(r) => {
                assert!(r.error.is_none());
                // Some(Value::Null) 在 JSON 中序列化为 null，反序列化后变为 None
                assert!(r.result.is_none());
            }
            _ => panic!("expected response"),
        }
    }

    #[tokio::test]
    async fn test_handle_server_request_apply_edit_logs_event() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut h = new_harness(Some(tx));
        let req = LspRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::Number(2i64.into()),
            method: "workspace/applyEdit".to_string(),
            params: None,
        };
        h.server.handle_server_request(req).await.unwrap();
        let resp = h.client_recv.receive().await.unwrap();
        match resp {
            LspMessage::Response(r) => {
                assert!(r.error.is_none());
            }
            _ => panic!("expected response"),
        }
        match rx.try_recv().unwrap() {
            LspEvent::Log { language_id, .. } => assert_eq!(language_id, "rust"),
            _ => panic!("expected log event"),
        }
    }

    #[tokio::test]
    async fn test_handle_server_request_workspace_folders() {
        let config = ServerConfig {
            root_uri: Some(Url::parse("file:///workspace").unwrap()),
            ..Default::default()
        };
        let (client_write, server_read) = tokio::io::duplex(1024);
        let (server_write, client_read) = tokio::io::duplex(1024);
        let (_, dummy_write) = tokio::io::duplex(1);
        let mut client_recv = LspTransport::new_generic(dummy_write, server_read);
        let mut server = LanguageServer::new_for_test(
            LspWriter::new(client_write),
            client_read,
            config,
            "rust".to_string(),
            None,
        );
        let req = LspRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::Number(3i64.into()),
            method: "workspace/workspaceFolders".to_string(),
            params: None,
        };
        server.handle_server_request(req).await.unwrap();
        let resp = client_recv.receive().await.unwrap();
        match resp {
            LspMessage::Response(r) => {
                let arr = r.result.unwrap().as_array().cloned().unwrap();
                assert_eq!(arr.len(), 1);
            }
            _ => panic!("expected response"),
        }
        // 避免 unused 警告
        let _ = server_write;
    }

    #[tokio::test]
    async fn test_handle_notification_without_event_tx() {
        let h = new_harness(None);
        h.server.handle_notification(LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "textDocument/publishDiagnostics".to_string(),
            params: Some(serde_json::json!({"uri": "file:///a.rs", "diagnostics": []})),
        });
        // 无 event_tx 不应 panic
    }

    #[tokio::test]
    async fn test_handle_notification_diagnostics_invalid_params() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let h = new_harness(Some(tx));
        h.server.handle_notification(LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "textDocument/publishDiagnostics".to_string(),
            params: Some(serde_json::json!({"diagnostics": []})),
        });
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_supports_all_capabilities() {
        let mut h = new_harness(None);
        assert!(!h.server.supports_definition());
        h.server.capabilities.definition_provider = Some(OneOf::Left(true));
        assert!(h.server.supports_definition());

        assert!(!h.server.supports_references());
        h.server.capabilities.references_provider = Some(OneOf::Left(true));
        assert!(h.server.supports_references());

        assert!(!h.server.supports_rename());
        h.server.capabilities.rename_provider = Some(OneOf::Left(true));
        assert!(h.server.supports_rename());

        assert!(!h.server.supports_code_actions());
        h.server.capabilities.code_action_provider =
            Some(CodeActionProviderCapability::Simple(true));
        assert!(h.server.supports_code_actions());

        assert!(!h.server.supports_formatting());
        h.server.capabilities.document_formatting_provider = Some(OneOf::Left(true));
        assert!(h.server.supports_formatting());

        assert!(!h.server.supports_semantic_tokens());
        h.server.capabilities.semantic_tokens_provider =
            Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
                SemanticTokensOptions::default(),
            ));
        assert!(h.server.supports_semantic_tokens());

        assert!(!h.server.supports_inlay_hints());
        h.server.capabilities.inlay_hint_provider = Some(OneOf::Left(true));
        assert!(h.server.supports_inlay_hints());
    }

    #[tokio::test]
    async fn test_open_close_change_document() {
        let mut h = new_harness(None);
        let uri = Url::parse("file:///a.rs").unwrap();
        h.server
            .open_document(
                uri.clone(),
                "rust".to_string(),
                1,
                "fn main() {}".to_string(),
            )
            .await
            .unwrap();
        let msg = h.client_recv.receive().await.unwrap();
        match msg {
            LspMessage::Notification(n) => assert_eq!(n.method, "textDocument/didOpen"),
            _ => panic!("expected didOpen"),
        }
        assert!(h.server.open_documents.contains_key(&uri));

        h.server.change_document(&uri, 2, vec![]).await.unwrap();
        let msg = h.client_recv.receive().await.unwrap();
        match msg {
            LspMessage::Notification(n) => assert_eq!(n.method, "textDocument/didChange"),
            _ => panic!("expected didChange"),
        }
        assert_eq!(h.server.open_documents[&uri].version, 2);

        h.server.close_document(&uri).await.unwrap();
        let msg = h.client_recv.receive().await.unwrap();
        match msg {
            LspMessage::Notification(n) => assert_eq!(n.method, "textDocument/didClose"),
            _ => panic!("expected didClose"),
        }
        assert!(!h.server.open_documents.contains_key(&uri));
    }

    #[tokio::test]
    async fn test_shutdown_not_initialized_is_noop() {
        let mut h = new_harness(None);
        assert!(h.server.shutdown().await.is_ok());
    }

    #[tokio::test]
    async fn test_shutdown_initialized() {
        let mut h = new_harness(None);
        h.server.initialized = true;
        let response = LspMessage::Response(LspResponse {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::Number(1i64.into()),
            result: Some(serde_json::Value::Null),
            error: None,
        });
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            let mut write = h.server_write;
            write_response(&mut write, &response).await;
        });
        h.server.shutdown().await.unwrap();
        assert!(!h.server.initialized);
        let msg = h.client_recv.receive().await.unwrap();
        match msg {
            LspMessage::Request(req) => assert_eq!(req.method, "shutdown"),
            _ => panic!("expected shutdown request"),
        }
        let msg = h.client_recv.receive().await.unwrap();
        match msg {
            LspMessage::Notification(n) => assert_eq!(n.method, "exit"),
            _ => panic!("expected exit"),
        }
    }

    #[tokio::test]
    async fn test_request_completion_sends_message() {
        let mut h = new_harness(None);
        let uri = Url::parse("file:///a.rs").unwrap();
        let _ = h
            .server
            .request_completion(
                &uri,
                Position {
                    line: 0,
                    character: 0,
                },
            )
            .await;
        let msg = h.client_recv.receive().await.unwrap();
        match msg {
            LspMessage::Request(req) => assert_eq!(req.method, "textDocument/completion"),
            _ => panic!("expected completion request"),
        }
    }

    #[tokio::test]
    async fn test_request_hover_sends_message() {
        let mut h = new_harness(None);
        let uri = Url::parse("file:///a.rs").unwrap();
        let _ = h
            .server
            .request_hover(
                &uri,
                Position {
                    line: 0,
                    character: 0,
                },
            )
            .await;
        let msg = h.client_recv.receive().await.unwrap();
        match msg {
            LspMessage::Request(req) => assert_eq!(req.method, "textDocument/hover"),
            _ => panic!("expected hover request"),
        }
    }

    #[tokio::test]
    async fn test_initialize_caches_capabilities() {
        let mut h = new_harness(None);
        let response = LspMessage::Response(LspResponse {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::Number(1i64.into()),
            result: Some(serde_json::json!({
                "capabilities": {
                    "completionProvider": {},
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "textDocumentSync": {
                        "openClose": true,
                        "change": 2
                    }
                }
            })),
            error: None,
        });
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            let mut write = h.server_write;
            write_response(&mut write, &response).await;
        });
        h.server.initialize().await.unwrap();
        assert!(h.server.initialized);
        assert!(h.server.supports_completion());
        assert!(h.server.supports_hover());
        assert!(h.server.capabilities.text_document_sync.is_some());
        let msg = h.client_recv.receive().await.unwrap();
        match msg {
            LspMessage::Request(req) => assert_eq!(req.method, "initialize"),
            _ => panic!("expected initialize request"),
        }
        let msg = h.client_recv.receive().await.unwrap();
        match msg {
            LspMessage::Notification(n) => assert_eq!(n.method, "initialized"),
            _ => panic!("expected initialized notification"),
        }
    }
}
