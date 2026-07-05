use lsp_types::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// LSP 消息枚举（JSON-RPC 2.0）
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LspMessage {
    Request(LspRequest),
    Response(LspResponse),
    Notification(LspNotification),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LspRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LspResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<LspError>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LspError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LspNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// 语言服务器配置
#[derive(Clone, Debug, Default)]
pub struct ServerConfig {
    /// 服务器可执行文件路径（可选，默认从PATH查找）
    pub command: Option<PathBuf>,
    /// 传递给服务器的额外参数
    pub args: Vec<String>,
    /// 环境变量覆盖
    pub env: std::collections::HashMap<String, String>,
    /// 工作区根目录
    pub root_uri: Option<Url>,
    /// 初始化选项
    pub initialization_options: Option<serde_json::Value>,
}

/// 文档同步状态
#[derive(Clone, Debug)]
pub struct DocumentState {
    pub uri: Url,
    pub version: i32,
    pub language_id: String,
    pub text: String,
}

/// 诊断集合
#[derive(Clone, Debug, Default)]
pub struct DiagnosticCollection {
    pub by_uri: std::collections::HashMap<Url, Vec<Diagnostic>>,
}

/// 补全项包装
#[derive(Clone, Debug)]
pub struct CompletionItemEx {
    pub item: CompletionItem,
    pub source: String, // 来自哪个语言服务器
}

/// 服务器能力缓存
#[derive(Clone, Debug, Default)]
pub struct ServerCapabilitiesCache {
    pub completion_provider: Option<CompletionOptions>,
    pub hover_provider: Option<HoverProviderCapability>,
    pub definition_provider: Option<OneOf<bool, DefinitionOptions>>,
    pub references_provider: Option<OneOf<bool, ReferencesOptions>>,
    pub rename_provider: Option<OneOf<bool, RenameOptions>>,
    pub code_action_provider: Option<CodeActionProviderCapability>,
    pub document_formatting_provider: Option<OneOf<bool, DocumentFormattingOptions>>,
    pub diagnostic_provider: Option<DiagnosticServerCapabilities>,
    pub text_document_sync: Option<TextDocumentSyncOptions>,
    pub semantic_tokens_provider: Option<SemanticTokensServerCapabilities>,
    pub inlay_hint_provider: Option<OneOf<bool, InlayHintServerCapabilities>>,
}

/// 请求ID生成器
pub struct RequestIdGenerator {
    next_id: i64,
}

impl RequestIdGenerator {
    pub fn new() -> Self {
        Self { next_id: 1 }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> serde_json::Value {
        let id = self.next_id;
        self.next_id += 1;
        serde_json::Value::Number(id.into())
    }
}

impl Default for RequestIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}
