use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// DAP 消息枚举（JSON-RPC 2.0 风格）
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DapMessage {
    Request(DapRequest),
    Response(DapResponse),
    Event(DapEvent),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DapRequest {
    #[serde(rename = "seq")]
    pub seq: i64,
    #[serde(rename = "type")]
    pub message_type: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DapResponse {
    #[serde(rename = "seq")]
    pub seq: i64,
    #[serde(rename = "type")]
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
    #[serde(rename = "type")]
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
