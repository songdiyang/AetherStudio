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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_id_generator() {
        let mut gen = RequestIdGenerator::new();
        assert_eq!(gen.next(), serde_json::Value::Number(1i64.into()));
        assert_eq!(gen.next(), serde_json::Value::Number(2i64.into()));
    }

    #[test]
    fn test_request_id_generator_default() {
        let mut gen = RequestIdGenerator::default();
        assert_eq!(gen.next(), serde_json::Value::Number(1i64.into()));
    }

    #[test]
    fn test_lsp_message_serde_request() {
        let msg = LspMessage::Request(LspRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::Number(1i64.into()),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({"key": "value"})),
        });
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"method\":\"initialize\""));

        let parsed: LspMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            LspMessage::Request(req) => {
                assert_eq!(req.id, serde_json::Value::Number(1i64.into()));
                assert_eq!(req.method, "initialize");
            }
            _ => panic!("expected request"),
        }
    }

    #[test]
    fn test_lsp_message_serde_response() {
        let msg = LspMessage::Response(LspResponse {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::Number(1i64.into()),
            result: Some(serde_json::json!({"items": []})),
            error: None,
        });
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: LspMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            LspMessage::Response(resp) => {
                assert_eq!(resp.id, serde_json::Value::Number(1i64.into()));
                assert!(resp.result.is_some());
            }
            _ => panic!("expected response"),
        }
    }

    #[test]
    fn test_lsp_message_serde_response_error() {
        let msg = LspMessage::Response(LspResponse {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::Number(1i64.into()),
            result: None,
            error: Some(LspError {
                code: -32601,
                message: "Method not found".to_string(),
                data: None,
            }),
        });
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: LspMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            LspMessage::Response(resp) => {
                assert!(resp.error.is_some());
                let err = resp.error.unwrap();
                assert_eq!(err.code, -32601);
                assert_eq!(err.message, "Method not found");
            }
            _ => panic!("expected response"),
        }
    }

    #[test]
    fn test_lsp_message_serde_notification() {
        let msg = LspMessage::Notification(LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "textDocument/didOpen".to_string(),
            params: Some(serde_json::json!({"textDocument": {"uri": "file:///test.rs"}})),
        });
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("\"id\""));
        let parsed: LspMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            LspMessage::Notification(notif) => {
                assert_eq!(notif.method, "textDocument/didOpen");
                assert!(notif.params.is_some());
            }
            _ => panic!("expected notification"),
        }
    }

    #[test]
    fn test_server_config_default() {
        let cfg = ServerConfig::default();
        assert!(cfg.command.is_none());
        assert!(cfg.args.is_empty());
        assert!(cfg.env.is_empty());
        assert!(cfg.root_uri.is_none());
        assert!(cfg.initialization_options.is_none());
    }

    #[test]
    fn test_document_state_clone() {
        let uri = Url::parse("file:///test.rs").unwrap();
        let state = DocumentState {
            uri: uri.clone(),
            version: 1,
            language_id: "rust".to_string(),
            text: "fn main() {}".to_string(),
        };
        let cloned = state.clone();
        assert_eq!(cloned.uri, uri);
        assert_eq!(cloned.version, 1);
        assert_eq!(cloned.language_id, "rust");
    }

    #[test]
    fn test_diagnostic_collection_default() {
        let coll = DiagnosticCollection::default();
        assert!(coll.by_uri.is_empty());
    }

    #[test]
    fn test_server_capabilities_cache_default() {
        let caps = ServerCapabilitiesCache::default();
        assert!(caps.completion_provider.is_none());
        assert!(caps.hover_provider.is_none());
        assert!(caps.semantic_tokens_provider.is_none());
    }
}
