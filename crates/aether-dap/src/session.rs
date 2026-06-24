use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::transport::{spawn_adapter, DapTransport};
use crate::types::*;

/// 调试会话
/// 管理单个调试适配器的完整生命周期
pub struct DebugSession {
    _process: tokio::process::Child,
    transport: DapTransport,
    state: DebugSessionState,
    #[allow(dead_code)]
    breakpoints: HashMap<String, Vec<Breakpoint>>,
    event_tx: mpsc::UnboundedSender<DapEventUi>,
    seq_generator: RequestIdGenerator,
}

impl DebugSession {
    /// 启动并初始化调试会话
    pub async fn start(
        config: AdapterConfig,
        event_tx: mpsc::UnboundedSender<DapEventUi>,
    ) -> std::io::Result<Self> {
        let mut process = spawn_adapter(&config).await?;
        let stdin = process.stdin.take().unwrap();
        let stdout = process.stdout.take().unwrap();
        let transport = DapTransport::new(stdin, stdout);

        let mut session = Self {
            _process: process,
            transport,
            state: DebugSessionState::Initializing,
            breakpoints: HashMap::new(),
            event_tx,
            seq_generator: RequestIdGenerator::new(),
        };

        // 发送 initialize 请求
        session.initialize().await?;

        Ok(session)
    }

    /// 发送 initialize 请求
    async fn initialize(&mut self) -> std::io::Result<()> {
        let seq = self.seq_generator.next();
        let args = serde_json::json!({
            "clientID": "aether",
            "clientName": "Aether Editor",
            "adapterID": "aether-debug",
            "linesStartAt1": true,
            "columnsStartAt1": true,
            "supportsVariableType": true,
            "supportsVariablePaging": false,
            "supportsRunInTerminalRequest": false,
            "locale": "zh-CN",
        });

        let request = DapMessage::Request(DapRequest {
            seq,
            message_type: "request".to_string(),
            command: "initialize".to_string(),
            arguments: Some(args),
        });

        self.transport.send(&request).await?;

        // 等待 initialize 响应
        loop {
            let message = self.transport.receive().await?;
            match message {
                DapMessage::Response(resp) if resp.command == "initialize" && resp.success => {
                    break;
                }
                DapMessage::Event(evt) => {
                    self.handle_event(evt).await?;
                }
                _ => {}
            }
        }

        self.state = DebugSessionState::Running;
        Ok(())
    }

    /// 启动调试（launch）
    pub async fn launch(
        &mut self,
        program: &str,
        args: Vec<String>,
        cwd: Option<&str>,
    ) -> std::io::Result<()> {
        let seq = self.seq_generator.next();
        let mut launch_args = serde_json::json!({
            "program": program,
            "args": args,
        });

        if let Some(cwd) = cwd {
            launch_args["cwd"] = serde_json::Value::String(cwd.to_string());
        }

        let request = DapMessage::Request(DapRequest {
            seq,
            message_type: "request".to_string(),
            command: "launch".to_string(),
            arguments: Some(launch_args),
        });

        self.transport.send(&request).await?;

        // 等待响应
        loop {
            let message = self.transport.receive().await?;
            match message {
                DapMessage::Response(resp) if resp.command == "launch" => {
                    if resp.success {
                        self.state = DebugSessionState::Running;
                    }
                    break;
                }
                DapMessage::Event(evt) => {
                    self.handle_event(evt).await?;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// 设置断点
    pub async fn set_breakpoints(
        &mut self,
        source_path: &str,
        lines: Vec<u32>,
    ) -> std::io::Result<Vec<Breakpoint>> {
        let seq = self.seq_generator.next();
        let breakpoints: Vec<serde_json::Value> = lines
            .iter()
            .map(|&line| serde_json::json!({"line": line}))
            .collect();

        let args = serde_json::json!({
            "source": {"path": source_path},
            "breakpoints": breakpoints,
        });

        let request = DapMessage::Request(DapRequest {
            seq,
            message_type: "request".to_string(),
            command: "setBreakpoints".to_string(),
            arguments: Some(args),
        });

        self.transport.send(&request).await?;

        // 等待响应
        loop {
            let message = self.transport.receive().await?;
            match message {
                DapMessage::Response(resp) if resp.command == "setBreakpoints" && resp.success => {
                    let body = resp.body.unwrap_or(serde_json::json!({"breakpoints": []}));
                    let breakpoints: Vec<Breakpoint> = serde_json::from_value(
                        body.get("breakpoints")
                            .cloned()
                            .unwrap_or(serde_json::Value::Array(vec![])),
                    )
                    .unwrap_or_default();
                    return Ok(breakpoints);
                }
                DapMessage::Event(evt) => {
                    self.handle_event(evt).await?;
                }
                _ => {}
            }
        }
    }

    /// 继续执行
    pub async fn continue_execution(&mut self, thread_id: i64) -> std::io::Result<()> {
        self.send_simple_request("continue", serde_json::json!({"threadId": thread_id}))
            .await
    }

    /// 单步跳过（Next）
    pub async fn next(&mut self, thread_id: i64) -> std::io::Result<()> {
        self.send_simple_request("next", serde_json::json!({"threadId": thread_id}))
            .await
    }

    /// 单步进入（Step In）
    pub async fn step_in(&mut self, thread_id: i64) -> std::io::Result<()> {
        self.send_simple_request("stepIn", serde_json::json!({"threadId": thread_id}))
            .await
    }

    /// 单步跳出（Step Out）
    pub async fn step_out(&mut self, thread_id: i64) -> std::io::Result<()> {
        self.send_simple_request("stepOut", serde_json::json!({"threadId": thread_id}))
            .await
    }

    /// 暂停
    pub async fn pause(&mut self, thread_id: i64) -> std::io::Result<()> {
        self.send_simple_request("pause", serde_json::json!({"threadId": thread_id}))
            .await
    }

    /// 获取调用栈
    pub async fn stack_trace(&mut self, thread_id: i64) -> std::io::Result<Vec<StackFrame>> {
        let seq = self.seq_generator.next();
        let args = serde_json::json!({"threadId": thread_id});

        let request = DapMessage::Request(DapRequest {
            seq,
            message_type: "request".to_string(),
            command: "stackTrace".to_string(),
            arguments: Some(args),
        });

        self.transport.send(&request).await?;

        loop {
            let message = self.transport.receive().await?;
            match message {
                DapMessage::Response(resp) if resp.command == "stackTrace" && resp.success => {
                    let body = resp.body.unwrap_or(serde_json::json!({"stackFrames": []}));
                    let frames: Vec<StackFrame> = serde_json::from_value(
                        body.get("stackFrames")
                            .cloned()
                            .unwrap_or(serde_json::Value::Array(vec![])),
                    )
                    .unwrap_or_default();
                    return Ok(frames);
                }
                DapMessage::Event(evt) => {
                    self.handle_event(evt).await?;
                }
                _ => {}
            }
        }
    }

    /// 获取作用域
    pub async fn scopes(&mut self, frame_id: i64) -> std::io::Result<Vec<Scope>> {
        let seq = self.seq_generator.next();
        let args = serde_json::json!({"frameId": frame_id});

        let request = DapMessage::Request(DapRequest {
            seq,
            message_type: "request".to_string(),
            command: "scopes".to_string(),
            arguments: Some(args),
        });

        self.transport.send(&request).await?;

        loop {
            let message = self.transport.receive().await?;
            match message {
                DapMessage::Response(resp) if resp.command == "scopes" && resp.success => {
                    let body = resp.body.unwrap_or(serde_json::json!({"scopes": []}));
                    let scopes: Vec<Scope> = serde_json::from_value(
                        body.get("scopes")
                            .cloned()
                            .unwrap_or(serde_json::Value::Array(vec![])),
                    )
                    .unwrap_or_default();
                    return Ok(scopes);
                }
                DapMessage::Event(evt) => {
                    self.handle_event(evt).await?;
                }
                _ => {}
            }
        }
    }

    /// 获取变量
    pub async fn variables(&mut self, variables_reference: i64) -> std::io::Result<Vec<Variable>> {
        let seq = self.seq_generator.next();
        let args = serde_json::json!({"variablesReference": variables_reference});

        let request = DapMessage::Request(DapRequest {
            seq,
            message_type: "request".to_string(),
            command: "variables".to_string(),
            arguments: Some(args),
        });

        self.transport.send(&request).await?;

        loop {
            let message = self.transport.receive().await?;
            match message {
                DapMessage::Response(resp) if resp.command == "variables" && resp.success => {
                    let body = resp.body.unwrap_or(serde_json::json!({"variables": []}));
                    let variables: Vec<Variable> = serde_json::from_value(
                        body.get("variables")
                            .cloned()
                            .unwrap_or(serde_json::Value::Array(vec![])),
                    )
                    .unwrap_or_default();
                    return Ok(variables);
                }
                DapMessage::Event(evt) => {
                    self.handle_event(evt).await?;
                }
                _ => {}
            }
        }
    }

    /// 评估表达式
    pub async fn evaluate(
        &mut self,
        expression: &str,
        frame_id: Option<i64>,
    ) -> std::io::Result<String> {
        let seq = self.seq_generator.next();
        let mut args = serde_json::json!({
            "expression": expression,
            "context": "repl",
        });

        if let Some(fid) = frame_id {
            args["frameId"] = serde_json::Value::Number(serde_json::Number::from(fid));
        }

        let request = DapMessage::Request(DapRequest {
            seq,
            message_type: "request".to_string(),
            command: "evaluate".to_string(),
            arguments: Some(args),
        });

        self.transport.send(&request).await?;

        loop {
            let message = self.transport.receive().await?;
            match message {
                DapMessage::Response(resp) if resp.command == "evaluate" && resp.success => {
                    let body = resp.body.unwrap_or(serde_json::json!({"result": ""}));
                    let result = body
                        .get("result")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    return Ok(result);
                }
                DapMessage::Event(evt) => {
                    self.handle_event(evt).await?;
                }
                _ => {}
            }
        }
    }

    /// 断开连接
    pub async fn disconnect(&mut self) -> std::io::Result<()> {
        self.send_simple_request("disconnect", serde_json::json!({"terminateDebuggee": true}))
            .await?;
        self.state = DebugSessionState::Terminated;
        Ok(())
    }

    /// 获取当前状态
    pub fn state(&self) -> &DebugSessionState {
        &self.state
    }

    /// 发送简单请求（不需要解析响应体）
    async fn send_simple_request(
        &mut self,
        command: &str,
        arguments: serde_json::Value,
    ) -> std::io::Result<()> {
        let seq = self.seq_generator.next();
        let request = DapMessage::Request(DapRequest {
            seq,
            message_type: "request".to_string(),
            command: command.to_string(),
            arguments: Some(arguments),
        });

        self.transport.send(&request).await?;

        // 等待响应
        loop {
            let message = self.transport.receive().await?;
            match message {
                DapMessage::Response(resp) if resp.command == command => {
                    break;
                }
                DapMessage::Event(evt) => {
                    self.handle_event(evt).await?;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// 处理 DAP 事件
    async fn handle_event(&mut self, event: DapEvent) -> std::io::Result<()> {
        match event.event.as_str() {
            "stopped" => {
                self.state = DebugSessionState::Paused;
                let reason = event
                    .body
                    .as_ref()
                    .and_then(|b| b.get("reason"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let thread_id = event
                    .body
                    .as_ref()
                    .and_then(|b| b.get("threadId"))
                    .and_then(|v| v.as_i64());
                let _ = self
                    .event_tx
                    .send(DapEventUi::Stopped { reason, thread_id });
            }
            "continued" => {
                self.state = DebugSessionState::Running;
                let thread_id = event
                    .body
                    .as_ref()
                    .and_then(|b| b.get("threadId"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let _ = self.event_tx.send(DapEventUi::Continued { thread_id });
            }
            "exited" => {
                let exit_code = event
                    .body
                    .as_ref()
                    .and_then(|b| b.get("exitCode"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let _ = self.event_tx.send(DapEventUi::Exited { exit_code });
            }
            "terminated" => {
                self.state = DebugSessionState::Terminated;
                let _ = self.event_tx.send(DapEventUi::Terminated);
            }
            "output" => {
                let category = event
                    .body
                    .as_ref()
                    .and_then(|b| b.get("category"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("console")
                    .to_string();
                let output = event
                    .body
                    .as_ref()
                    .and_then(|b| b.get("output"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let _ = self.event_tx.send(DapEventUi::Output { category, output });
            }
            "breakpoint" => {
                if let Some(body) = &event.body {
                    if let Ok(bp) = serde_json::from_value::<Breakpoint>(body.clone()) {
                        let _ = self
                            .event_tx
                            .send(DapEventUi::BreakpointValidated { breakpoint: bp });
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
