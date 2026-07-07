use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// DAP 消息枚举（JSON-RPC 2.0 风格）
///
/// C-02: 使用 `tag = "type"` 内部标签替代 `untagged`，确保 serde 根据
/// `type` 字段（"request"/"response"/"event"）正确分派变体，
/// 避免 Response 因字段兼容被误判为 Request。
#[derive(Clone, Debug, Serialize, Deserialize)]
// C-08: DAP 消息必须按 type 字段分发，untagged 会将响应误解析为请求
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DapMessage {
    Request(DapRequest),
    Response(DapResponse),
    Event(DapEvent),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DapRequest {
    #[serde(rename = "seq")]
    pub seq: i64,
    #[serde(skip)]
    pub message_type: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DapResponse {
    #[serde(rename = "seq")]
    pub seq: i64,
    #[serde(skip)]
    pub message_type: String,
    pub request_seq: i64,
    pub success: bool,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DapEvent {
    #[serde(rename = "seq")]
    pub seq: i64,
    #[serde(skip)]
    pub message_type: String,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

/// 调试适配器配置
#[derive(Clone, Debug, Default)]
pub struct AdapterConfig {
    pub command: Option<std::path::PathBuf>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub program: Option<String>,
    pub cwd: Option<std::path::PathBuf>,
}

/// 断点
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Breakpoint {
    pub id: Option<i64>,
    pub verified: bool,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub source: Option<Source>,
    pub message: Option<String>,
}

/// 源代码文件信息
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Source {
    pub name: Option<String>,
    pub path: Option<String>,
    #[serde(rename = "sourceReference")]
    pub source_reference: Option<i64>,
}

/// 堆栈帧
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StackFrame {
    pub id: i64,
    pub name: String,
    pub source: Option<Source>,
    pub line: u32,
    pub column: u32,
}

/// 作用域
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Scope {
    pub name: String,
    #[serde(rename = "presentationHint")]
    pub presentation_hint: Option<String>,
    pub variables_reference: i64,
    pub expensive: bool,
}

/// 变量
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub value: String,
    #[serde(rename = "type")]
    pub var_type: Option<String>,
    #[serde(rename = "variablesReference")]
    pub variables_reference: i64,
}

/// 调试会话状态
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebugSessionState {
    Initializing,
    Running,
    Paused,
    Stopped,
    Terminated,
}

/// DAP 事件（推送到UI层）
#[derive(Clone, Debug)]
pub enum DapEventUi {
    Stopped {
        reason: String,
        thread_id: Option<i64>,
    },
    Continued {
        thread_id: i64,
    },
    Exited {
        exit_code: i64,
    },
    Terminated,
    Output {
        category: String,
        output: String,
    },
    BreakpointValidated {
        breakpoint: Breakpoint,
    },
    ThreadStarted {
        thread_id: i64,
    },
    ThreadExited {
        thread_id: i64,
    },
}

/// 请求ID生成器
pub struct RequestIdGenerator {
    next_seq: i64,
}

impl RequestIdGenerator {
    pub fn new() -> Self {
        Self { next_seq: 1 }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> i64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
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
    fn request_serde_roundtrip() {
        let req = DapRequest {
            seq: 1,
            message_type: "request".into(),
            command: "initialize".into(),
            arguments: Some(serde_json::json!({"clientID": "aether"})),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"seq\":1"));
        assert!(json.contains("\"command\":\"initialize\""));
        // `message_type` 由外层 DapMessage 的 tag 提供，内部字段跳过序列化。
        assert!(!json.contains("\"type\":\"request\""));

        let parsed: DapRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.seq, 1);
        assert_eq!(parsed.command, "initialize");
    }

    #[test]
    fn response_serde_roundtrip() {
        let resp = DapResponse {
            seq: 2,
            message_type: "response".into(),
            request_seq: 1,
            success: true,
            command: "initialize".into(),
            body: Some(serde_json::json!({"supportsConfigurationDoneRequest": true})),
            message: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(!json.contains("\"type\":\"response\""));
        let parsed: DapResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.seq, 2);
        assert_eq!(parsed.request_seq, 1);
        assert!(parsed.success);
        assert_eq!(parsed.command, "initialize");
    }

    #[test]
    fn event_serde_roundtrip() {
        let evt = DapEvent {
            seq: 3,
            message_type: "event".into(),
            event: "stopped".into(),
            body: Some(serde_json::json!({"reason": "breakpoint", "threadId": 42})),
        };
        let json = serde_json::to_string(&evt).unwrap();
        assert!(!json.contains("\"type\":\"event\""));
        let parsed: DapEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.seq, 3);
        assert_eq!(parsed.event, "stopped");
    }

    #[test]
    fn message_request_roundtrip() {
        let msg = DapMessage::Request(DapRequest {
            seq: 1,
            message_type: "request".into(),
            command: "next".into(),
            arguments: Some(serde_json::json!({"threadId": 7})),
        });
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: DapMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            DapMessage::Request(r) => {
                assert_eq!(r.command, "next");
            }
            _ => panic!("expected request"),
        }
    }

    #[test]
    fn message_response_roundtrip() {
        let msg = DapMessage::Response(DapResponse {
            seq: 2,
            message_type: "response".into(),
            request_seq: 1,
            success: true,
            command: "launch".into(),
            body: None,
            message: None,
        });
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: DapMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            DapMessage::Response(r) => assert!(r.success),
            _ => panic!("expected response"),
        }
    }

    #[test]
    fn message_event_roundtrip() {
        let msg = DapMessage::Event(DapEvent {
            seq: 5,
            message_type: "event".into(),
            event: "output".into(),
            body: Some(serde_json::json!({"output": "hello"})),
        });
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: DapMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            DapMessage::Event(e) => assert_eq!(e.event, "output"),
            _ => panic!("expected event"),
        }
    }

    #[test]
    fn message_deserialize_missing_type_fails() {
        let json = r#"{"seq":1,"command":"initialize"}"#;
        assert!(serde_json::from_str::<DapMessage>(json).is_err());
    }

    #[test]
    fn message_deserialize_unknown_type_fails() {
        let json = r#"{"type":"notify","seq":1}"#;
        assert!(serde_json::from_str::<DapMessage>(json).is_err());
    }

    #[test]
    fn request_id_generator_increments() {
        let mut gen = RequestIdGenerator::new();
        assert_eq!(gen.next(), 1);
        assert_eq!(gen.next(), 2);
        assert_eq!(gen.next(), 3);
    }

    #[test]
    fn request_id_generator_default() {
        let mut gen = RequestIdGenerator::default();
        let mut gen2 = RequestIdGenerator::new();
        assert_eq!(gen2.next(), gen.next());
    }

    #[test]
    fn debug_session_state_equality() {
        assert_eq!(
            DebugSessionState::Initializing,
            DebugSessionState::Initializing
        );
        assert_ne!(DebugSessionState::Initializing, DebugSessionState::Running);
    }

    #[test]
    fn adapter_config_default() {
        let cfg = AdapterConfig::default();
        assert!(cfg.command.is_none());
        assert!(cfg.args.is_empty());
        assert!(cfg.env.is_empty());
        assert!(cfg.program.is_none());
        assert!(cfg.cwd.is_none());
    }

    #[test]
    fn breakpoint_serde_roundtrip() {
        let bp = Breakpoint {
            id: Some(10),
            verified: true,
            line: Some(20),
            column: Some(5),
            source: Some(Source {
                name: Some("main.rs".into()),
                path: Some("/src/main.rs".into()),
                source_reference: None,
            }),
            message: Some("ok".into()),
        };
        let json = serde_json::to_string(&bp).unwrap();
        let parsed: Breakpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, bp.id);
        assert!(parsed.verified);
    }

    #[test]
    fn source_serde_roundtrip() {
        let src = Source {
            name: Some("lib.rs".into()),
            path: Some("/src/lib.rs".into()),
            source_reference: Some(7),
        };
        let parsed: Source = serde_json::from_str(&serde_json::to_string(&src).unwrap()).unwrap();
        assert_eq!(parsed.name, src.name);
        assert_eq!(parsed.source_reference, Some(7));
    }

    #[test]
    fn stack_frame_serde_roundtrip() {
        let frame = StackFrame {
            id: 1,
            name: "main".into(),
            source: None,
            line: 10,
            column: 1,
        };
        let parsed: StackFrame =
            serde_json::from_str(&serde_json::to_string(&frame).unwrap()).unwrap();
        assert_eq!(parsed.id, 1);
        assert_eq!(parsed.line, 10);
    }

    #[test]
    fn scope_serde_roundtrip() {
        let scope = Scope {
            name: "Locals".into(),
            presentation_hint: Some("locals".into()),
            variables_reference: 100,
            expensive: false,
        };
        let parsed: Scope = serde_json::from_str(&serde_json::to_string(&scope).unwrap()).unwrap();
        assert_eq!(parsed.name, "Locals");
        assert_eq!(parsed.variables_reference, 100);
    }

    #[test]
    fn variable_serde_roundtrip() {
        let var = Variable {
            name: "x".into(),
            value: "42".into(),
            var_type: Some("i32".into()),
            variables_reference: 0,
        };
        let parsed: Variable = serde_json::from_str(&serde_json::to_string(&var).unwrap()).unwrap();
        assert_eq!(parsed.value, "42");
        assert_eq!(parsed.var_type.as_deref(), Some("i32"));
    }
}
