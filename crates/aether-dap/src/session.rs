#![allow(clippy::items_after_test_module)]

use std::collections::HashMap;
use std::time::Duration;
use tokio::process::Child;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::transport::{spawn_adapter, spawn_stderr_drain, DapTransport};
use crate::types::*;

/// 默认请求超时（秒）。
///
/// DAP 请求-响应通常是即时确认（如 continue/next 的响应），实际停止事件
/// 通过后续的 "stopped" 通知异步推送。30 秒足以覆盖慢速适配器初始化。
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// H-04: 发送 disconnect 后等待适配器自行退出的超时时间。
///
/// 超过此时间适配器仍未退出则强制 kill，防止僵尸进程。
const DISCONNECT_KILL_TIMEOUT: Duration = Duration::from_secs(5);

/// 调试会话
/// 管理单个调试适配器的完整生命周期
pub struct DebugSession {
    transport: DapTransport,
    state: DebugSessionState,
    #[allow(dead_code)]
    breakpoints: HashMap<String, Vec<Breakpoint>>,
    event_tx: mpsc::UnboundedSender<DapEventUi>,
    seq_generator: RequestIdGenerator,
    /// H-04: 子进程句柄，disconnect 后超时未退出则强制 kill
    child: Option<Child>,
    /// H-04: stderr 读取任务句柄，finalize 时主动 abort 避免任务残留
    stderr_drain: Option<JoinHandle<()>>,
}

impl DebugSession {
    /// 启动并初始化调试会话
    pub async fn start(
        config: AdapterConfig,
        event_tx: mpsc::UnboundedSender<DapEventUi>,
    ) -> std::io::Result<Self> {
        let mut process = spawn_adapter(&config).await?;
        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("Failed to capture adapter stdin"))?;
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("Failed to capture adapter stdout"))?;
        // H-04: 单独取出 stderr，保留 Child 句柄以便后续 kill
        let stderr = process
            .stderr
            .take()
            .ok_or_else(|| std::io::Error::other("Failed to capture adapter stderr"))?;
        let transport = DapTransport::new(stdin, stdout);

        // 启动后台 stderr 读取任务，避免适配器 stderr 缓冲区满后阻塞
        let stderr_drain = spawn_stderr_drain(stderr);

        let mut session = Self {
            transport,
            state: DebugSessionState::Initializing,
            breakpoints: HashMap::new(),
            event_tx,
            seq_generator: RequestIdGenerator::new(),
            child: Some(process),
            stderr_drain: Some(stderr_drain),
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
            message_type: "request".into(),
            command: "initialize".to_string(),
            arguments: Some(args),
        });

        self.transport.send(&request).await?;

        // 等待 initialize 响应（初始化可能较慢，给予更长超时）
        let fut = async {
            loop {
                let message = self.transport.receive().await?;
                match message {
                    DapMessage::Response(resp) if resp.command == "initialize" => {
                        if resp.success {
                            break Ok(());
                        } else {
                            return Err(std::io::Error::other(format!(
                                "initialize failed: {}",
                                resp.message.unwrap_or_default()
                            )));
                        }
                    }
                    DapMessage::Event(evt) => {
                        self.handle_event(evt).await?;
                    }
                    _ => {}
                }
            }
        };

        timeout(Duration::from_secs(60), fut).await.map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "DAP initialize timed out")
        })??;

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
        // H-11: 状态机校验 — 已终止的会话不能重新 launch
        if self.state == DebugSessionState::Terminated {
            return Err(std::io::Error::other(
                "Cannot launch: session already terminated",
            ));
        }

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
            message_type: "request".into(),
            command: "launch".to_string(),
            arguments: Some(launch_args),
        });

        self.transport.send(&request).await?;

        // 等待响应（launch 可能耗时较长，如编译）
        let fut = async {
            loop {
                let message = self.transport.receive().await?;
                match message {
                    DapMessage::Response(resp) if resp.command == "launch" => {
                        if resp.success {
                            self.state = DebugSessionState::Running;
                            break;
                        } else {
                            return Err(std::io::Error::other(format!(
                                "launch failed: {}",
                                resp.message.unwrap_or_default()
                            )));
                        }
                    }
                    DapMessage::Event(evt) => {
                        self.handle_event(evt).await?;
                    }
                    _ => {}
                }
            }
            Ok(())
        };

        timeout(Duration::from_secs(120), fut).await.map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "DAP launch timed out")
        })??;

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
            message_type: "request".into(),
            command: "setBreakpoints".to_string(),
            arguments: Some(args),
        });

        self.transport.send(&request).await?;

        let fut = async {
            loop {
                let message = self.transport.receive().await?;
                match message {
                    DapMessage::Response(resp) if resp.command == "setBreakpoints" => {
                        if !resp.success {
                            return Err(std::io::Error::other(format!(
                                "setBreakpoints failed: {}",
                                resp.message.unwrap_or_default()
                            )));
                        }
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
        };

        timeout(DEFAULT_REQUEST_TIMEOUT, fut).await.map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "DAP setBreakpoints timed out")
        })?
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
            message_type: "request".into(),
            command: "stackTrace".to_string(),
            arguments: Some(args),
        });

        self.transport.send(&request).await?;

        let fut = async {
            loop {
                let message = self.transport.receive().await?;
                match message {
                    DapMessage::Response(resp) if resp.command == "stackTrace" => {
                        if !resp.success {
                            return Err(std::io::Error::other(format!(
                                "stackTrace failed: {}",
                                resp.message.unwrap_or_default()
                            )));
                        }
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
        };

        timeout(DEFAULT_REQUEST_TIMEOUT, fut).await.map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "DAP stackTrace timed out")
        })?
    }

    /// 获取作用域
    pub async fn scopes(&mut self, frame_id: i64) -> std::io::Result<Vec<Scope>> {
        let seq = self.seq_generator.next();
        let args = serde_json::json!({"frameId": frame_id});

        let request = DapMessage::Request(DapRequest {
            seq,
            message_type: "request".into(),
            command: "scopes".to_string(),
            arguments: Some(args),
        });

        self.transport.send(&request).await?;

        let fut = async {
            loop {
                let message = self.transport.receive().await?;
                match message {
                    DapMessage::Response(resp) if resp.command == "scopes" => {
                        if !resp.success {
                            return Err(std::io::Error::other(format!(
                                "scopes failed: {}",
                                resp.message.unwrap_or_default()
                            )));
                        }
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
        };

        timeout(DEFAULT_REQUEST_TIMEOUT, fut).await.map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "DAP scopes timed out")
        })?
    }

    /// 获取变量
    pub async fn variables(&mut self, variables_reference: i64) -> std::io::Result<Vec<Variable>> {
        let seq = self.seq_generator.next();
        let args = serde_json::json!({"variablesReference": variables_reference});

        let request = DapMessage::Request(DapRequest {
            seq,
            message_type: "request".into(),
            command: "variables".to_string(),
            arguments: Some(args),
        });

        self.transport.send(&request).await?;

        let fut = async {
            loop {
                let message = self.transport.receive().await?;
                match message {
                    DapMessage::Response(resp) if resp.command == "variables" => {
                        if !resp.success {
                            return Err(std::io::Error::other(format!(
                                "variables failed: {}",
                                resp.message.unwrap_or_default()
                            )));
                        }
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
        };

        timeout(DEFAULT_REQUEST_TIMEOUT, fut).await.map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "DAP variables timed out")
        })?
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
            message_type: "request".into(),
            command: "evaluate".to_string(),
            arguments: Some(args),
        });

        self.transport.send(&request).await?;

        let fut = async {
            loop {
                let message = self.transport.receive().await?;
                match message {
                    DapMessage::Response(resp) if resp.command == "evaluate" => {
                        if !resp.success {
                            return Err(std::io::Error::other(format!(
                                "evaluate failed: {}",
                                resp.message.unwrap_or_default()
                            )));
                        }
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
        };

        timeout(DEFAULT_REQUEST_TIMEOUT, fut).await.map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "DAP evaluate timed out")
        })?
    }

    /// 断开调试连接并终止被调试进程。
    ///
    /// 此方法会发送 `terminateDebuggee: true`，被调试的进程将被强制终止。
    /// 若希望在断开后让被调试进程继续运行，请使用 [`detach`]。
    pub async fn disconnect(&mut self) -> std::io::Result<()> {
        // H-04: 即使发送请求失败，也要确保子进程被回收
        let _ = self
            .send_simple_request("disconnect", serde_json::json!({"terminateDebuggee": true}))
            .await;
        self.finalize_child().await;
        self.state = DebugSessionState::Terminated;
        Ok(())
    }

    /// 分离调试器但保留被调试进程运行。
    ///
    /// 与 [`disconnect`] 不同，此方法发送 `terminateDebuggee: false`，
    /// 适用于「附加到运行进程」场景下，希望断开调试器但不杀掉进程。
    pub async fn detach(&mut self) -> std::io::Result<()> {
        let _ = self
            .send_simple_request(
                "disconnect",
                serde_json::json!({"terminateDebuggee": false}),
            )
            .await;
        // H-04: detach 不杀被调试进程，但适配器进程本身仍需回收
        self.finalize_child().await;
        self.state = DebugSessionState::Terminated;
        Ok(())
    }

    /// H-04: 等待适配器子进程在 DISCONNECT_KILL_TIMEOUT 内自行退出，
    /// 超时则强制 kill，防止僵尸进程驻留。
    ///
    /// 多次调用安全：第二次取 child 会得到 None，直接返回。
    async fn finalize_child(&mut self) {
        // 先中止 stderr drain 任务，避免 child 退出/被 kill 后任务仍短暂持有句柄
        if let Some(drain) = self.stderr_drain.take() {
            drain.abort();
        }
        if let Some(mut child) = self.child.take() {
            match timeout(DISCONNECT_KILL_TIMEOUT, child.wait()).await {
                Ok(_) => {
                    // 适配器已正常退出
                }
                Err(_) => {
                    // 超时未退出，强制 kill
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }
            }
        }
    }

    /// 获取当前状态
    pub fn state(&self) -> &DebugSessionState {
        &self.state
    }

    /// 发送简单请求（不需要解析响应体），验证 success 字段。
    ///
    /// 修复点：原实现 `resp.command == command` 即 break，不检查 `resp.success`，
    /// 导致失败的请求被当作成功处理。
    async fn send_simple_request(
        &mut self,
        command: &str,
        arguments: serde_json::Value,
    ) -> std::io::Result<()> {
        let seq = self.seq_generator.next();
        let request = DapMessage::Request(DapRequest {
            seq,
            message_type: "request".into(),
            command: command.to_string(),
            arguments: Some(arguments),
        });

        self.transport.send(&request).await?;

        let fut = async {
            loop {
                let message = self.transport.receive().await?;
                match message {
                    DapMessage::Response(resp) if resp.command == command => {
                        if resp.success {
                            break;
                        } else {
                            return Err(std::io::Error::other(format!(
                                "{} failed: {}",
                                command,
                                resp.message.unwrap_or_default()
                            )));
                        }
                    }
                    DapMessage::Event(evt) => {
                        self.handle_event(evt).await?;
                    }
                    _ => {}
                }
            }
            Ok(())
        };

        timeout(DEFAULT_REQUEST_TIMEOUT, fut).await.map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("DAP {} timed out", command),
            )
        })??;

        Ok(())
    }

    /// 处理 DAP 事件
    async fn handle_event(&mut self, event: DapEvent) -> std::io::Result<()> {
        // H-11: 状态机校验 — 已终止的会话忽略所有延迟事件，防止"复活"
        // 仅允许 "terminated" 和 "exited" 事件通过（它们确认终止，不会改变状态）
        if self.state == DebugSessionState::Terminated
            && event.event != "terminated"
            && event.event != "exited"
        {
            return Ok(());
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::DapTransport;
    use tokio::io::duplex;

    fn setup() -> (
        DebugSession,
        DapTransport,
        mpsc::UnboundedReceiver<DapEventUi>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (client_in, adapter_in) = duplex(64 * 1024);
        let (adapter_out, client_out) = duplex(64 * 1024);
        let transport = DapTransport::new_from_parts(client_in, client_out);
        let session = DebugSession::with_transport(transport, tx);
        let fake = DapTransport::new_from_parts(adapter_out, adapter_in);
        (session, fake, rx)
    }

    fn success_response(command: &str, body: Option<serde_json::Value>) -> DapMessage {
        DapMessage::Response(DapResponse {
            seq: 100,
            message_type: "response".into(),
            request_seq: 1,
            success: true,
            command: command.into(),
            body,
            message: None,
        })
    }

    fn failure_response(command: &str, message: &str) -> DapMessage {
        DapMessage::Response(DapResponse {
            seq: 100,
            message_type: "response".into(),
            request_seq: 1,
            success: false,
            command: command.into(),
            body: None,
            message: Some(message.into()),
        })
    }

    fn event_message(event: &str, body: serde_json::Value) -> DapMessage {
        DapMessage::Event(DapEvent {
            seq: 200,
            message_type: "event".into(),
            event: event.into(),
            body: Some(body),
        })
    }

    #[tokio::test]
    async fn start_fails_without_command() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let config = AdapterConfig::default();
        match DebugSession::start(config, tx).await {
            Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput),
            Ok(_) => panic!("expected start to fail"),
        }
    }

    #[tokio::test]
    async fn initialize_success_sets_running() {
        let (mut session, mut fake, _rx) = setup();
        fake.send(&success_response("initialize", None))
            .await
            .unwrap();
        session.initialize().await.unwrap();
        assert_eq!(*session.state(), DebugSessionState::Running);
    }

    #[tokio::test]
    async fn initialize_failure_returns_error() {
        let (mut session, mut fake, _rx) = setup();
        fake.send(&failure_response("initialize", "unsupported"))
            .await
            .unwrap();
        let err = session.initialize().await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
    }

    #[tokio::test]
    async fn initialize_routes_events_before_response() {
        let (mut session, mut fake, mut rx) = setup();
        fake.send(&event_message(
            "stopped",
            serde_json::json!({"reason": "breakpoint", "threadId": 7}),
        ))
        .await
        .unwrap();
        fake.send(&success_response("initialize", None))
            .await
            .unwrap();

        session.initialize().await.unwrap();
        assert_eq!(*session.state(), DebugSessionState::Running);
        assert!(matches!(
            rx.try_recv(),
            Ok(DapEventUi::Stopped {
                reason,
                thread_id: Some(7)
            }) if reason == "breakpoint"
        ));
    }

    #[tokio::test]
    async fn launch_success() {
        let (mut session, mut fake, _rx) = setup();
        fake.send(&success_response("launch", None)).await.unwrap();
        session.launch("/bin/app", vec![], None).await.unwrap();
        assert_eq!(*session.state(), DebugSessionState::Running);
    }

    #[tokio::test]
    async fn launch_failure_returns_error() {
        let (mut session, mut fake, _rx) = setup();
        fake.send(&failure_response("launch", "program not found"))
            .await
            .unwrap();
        let err = session.launch("/bin/app", vec![], None).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
    }

    #[tokio::test]
    async fn set_breakpoints_success() {
        let (mut session, mut fake, _rx) = setup();
        let body = serde_json::json!({
            "breakpoints": [
                {"id": 1, "verified": true, "line": 10},
                {"id": 2, "verified": false, "line": 20},
            ]
        });
        fake.send(&success_response("setBreakpoints", Some(body)))
            .await
            .unwrap();
        let bps = session
            .set_breakpoints("/src/main.rs", vec![10, 20])
            .await
            .unwrap();
        assert_eq!(bps.len(), 2);
        assert_eq!(bps[0].id, Some(1));
    }

    #[tokio::test]
    async fn set_breakpoints_failure_returns_error() {
        let (mut session, mut fake, _rx) = setup();
        fake.send(&failure_response("setBreakpoints", "invalid source"))
            .await
            .unwrap();
        let err = session
            .set_breakpoints("/src/main.rs", vec![10])
            .await
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
    }

    #[tokio::test]
    async fn continue_execution_success() {
        let (mut session, mut fake, _rx) = setup();
        fake.send(&success_response("continue", None))
            .await
            .unwrap();
        session.continue_execution(1).await.unwrap();
    }

    #[tokio::test]
    async fn next_failure_returns_error() {
        let (mut session, mut fake, _rx) = setup();
        fake.send(&failure_response("next", "not paused"))
            .await
            .unwrap();
        let err = session.next(1).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
    }

    #[tokio::test]
    async fn step_in_success() {
        let (mut session, mut fake, _rx) = setup();
        fake.send(&success_response("stepIn", None)).await.unwrap();
        session.step_in(1).await.unwrap();
    }

    #[tokio::test]
    async fn step_out_failure_returns_error() {
        let (mut session, mut fake, _rx) = setup();
        fake.send(&failure_response("stepOut", "not paused"))
            .await
            .unwrap();
        let err = session.step_out(1).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
    }

    #[tokio::test]
    async fn pause_success() {
        let (mut session, mut fake, _rx) = setup();
        fake.send(&success_response("pause", None)).await.unwrap();
        session.pause(1).await.unwrap();
    }

    #[tokio::test]
    async fn stack_trace_success() {
        let (mut session, mut fake, _rx) = setup();
        let body = serde_json::json!({
            "stackFrames": [
                {"id": 1, "name": "main", "line": 10, "column": 1},
                {"id": 2, "name": "foo", "line": 20, "column": 1},
            ]
        });
        fake.send(&success_response("stackTrace", Some(body)))
            .await
            .unwrap();
        let frames = session.stack_trace(1).await.unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].name, "main");
    }

    #[tokio::test]
    async fn scopes_success() {
        let (mut session, mut fake, _rx) = setup();
        let body = serde_json::json!({
            "scopes": [
                {"name": "Locals", "variables_reference": 100, "expensive": false},
            ]
        });
        fake.send(&success_response("scopes", Some(body)))
            .await
            .unwrap();
        let scopes = session.scopes(1).await.unwrap();
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].name, "Locals");
    }

    #[tokio::test]
    async fn variables_success() {
        let (mut session, mut fake, _rx) = setup();
        let body = serde_json::json!({
            "variables": [
                {"name": "x", "value": "42", "variablesReference": 0},
            ]
        });
        fake.send(&success_response("variables", Some(body)))
            .await
            .unwrap();
        let vars = session.variables(100).await.unwrap();
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].value, "42");
    }

    #[tokio::test]
    async fn evaluate_success() {
        let (mut session, mut fake, _rx) = setup();
        let body = serde_json::json!({"result": "hello"});
        fake.send(&success_response("evaluate", Some(body)))
            .await
            .unwrap();
        let result = session.evaluate("x", Some(1)).await.unwrap();
        assert_eq!(result, "hello");
    }

    #[tokio::test]
    async fn evaluate_failure_returns_error() {
        let (mut session, mut fake, _rx) = setup();
        fake.send(&failure_response("evaluate", "cannot evaluate"))
            .await
            .unwrap();
        let err = session.evaluate("x", None).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
    }

    #[tokio::test]
    async fn disconnect_terminates_state() {
        let (mut session, mut fake, _rx) = setup();
        fake.send(&success_response("disconnect", None))
            .await
            .unwrap();
        session.disconnect().await.unwrap();
        assert_eq!(*session.state(), DebugSessionState::Terminated);
    }

    #[tokio::test]
    async fn detach_terminates_state() {
        let (mut session, mut fake, _rx) = setup();
        fake.send(&success_response("disconnect", None))
            .await
            .unwrap();
        session.detach().await.unwrap();
        assert_eq!(*session.state(), DebugSessionState::Terminated);
    }

    #[tokio::test]
    async fn handle_event_stopped_sets_paused_and_sends_ui_event() {
        let (mut session, _fake, mut rx) = setup();
        let event = DapEvent {
            seq: 1,
            message_type: "event".into(),
            event: "stopped".into(),
            body: Some(serde_json::json!({"reason": "step", "threadId": 5})),
        };
        session.handle_event(event).await.unwrap();
        assert_eq!(*session.state(), DebugSessionState::Paused);
        assert!(matches!(
            rx.try_recv(),
            Ok(DapEventUi::Stopped {
                reason,
                thread_id: Some(5)
            }) if reason == "step"
        ));
    }

    #[tokio::test]
    async fn handle_event_continued_sets_running() {
        let (mut session, _fake, mut rx) = setup();
        session.state = DebugSessionState::Paused;
        let event = DapEvent {
            seq: 1,
            message_type: "event".into(),
            event: "continued".into(),
            body: Some(serde_json::json!({"threadId": 5})),
        };
        session.handle_event(event).await.unwrap();
        assert_eq!(*session.state(), DebugSessionState::Running);
        assert!(matches!(
            rx.try_recv(),
            Ok(DapEventUi::Continued { thread_id: 5 })
        ));
    }

    #[tokio::test]
    async fn handle_event_terminated_sets_terminated() {
        let (mut session, _fake, mut rx) = setup();
        let event = DapEvent {
            seq: 1,
            message_type: "event".into(),
            event: "terminated".into(),
            body: None,
        };
        session.handle_event(event).await.unwrap();
        assert_eq!(*session.state(), DebugSessionState::Terminated);
        assert!(matches!(rx.try_recv(), Ok(DapEventUi::Terminated)));
    }

    #[tokio::test]
    async fn handle_event_exited_sends_ui_event() {
        let (mut session, _fake, mut rx) = setup();
        let event = DapEvent {
            seq: 1,
            message_type: "event".into(),
            event: "exited".into(),
            body: Some(serde_json::json!({"exitCode": 42})),
        };
        session.handle_event(event).await.unwrap();
        assert!(matches!(
            rx.try_recv(),
            Ok(DapEventUi::Exited { exit_code: 42 })
        ));
    }

    #[tokio::test]
    async fn handle_event_output_sends_ui_event() {
        let (mut session, _fake, mut rx) = setup();
        let event = DapEvent {
            seq: 1,
            message_type: "event".into(),
            event: "output".into(),
            body: Some(serde_json::json!({"category": "stdout", "output": "hello"})),
        };
        session.handle_event(event).await.unwrap();
        assert!(matches!(
            rx.try_recv(),
            Ok(DapEventUi::Output { category, output }) if category == "stdout" && output == "hello"
        ));
    }

    #[tokio::test]
    async fn handle_event_breakpoint_validated_sends_ui_event() {
        let (mut session, _fake, mut rx) = setup();
        let event = DapEvent {
            seq: 1,
            message_type: "event".into(),
            event: "breakpoint".into(),
            body: Some(serde_json::json!({"id": 9, "verified": true, "line": 15})),
        };
        session.handle_event(event).await.unwrap();
        assert!(matches!(
            rx.try_recv(),
            Ok(DapEventUi::BreakpointValidated { breakpoint }) if breakpoint.id == Some(9)
        ));
    }

    #[tokio::test]
    async fn handle_event_unknown_is_noop() {
        let (mut session, _fake, mut rx) = setup();
        let event = DapEvent {
            seq: 1,
            message_type: "event".into(),
            event: "custom".into(),
            body: None,
        };
        session.handle_event(event).await.unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn state_returns_current_state() {
        let (session, _fake, _rx) = setup();
        assert_eq!(*session.state(), DebugSessionState::Initializing);
    }
}

#[cfg(test)]
impl DebugSession {
    /// 使用已有传输层构造会话，仅用于测试。
    pub fn with_transport(
        transport: DapTransport,
        event_tx: mpsc::UnboundedSender<DapEventUi>,
    ) -> Self {
        Self {
            transport,
            state: DebugSessionState::Initializing,
            breakpoints: HashMap::new(),
            event_tx,
            seq_generator: RequestIdGenerator::new(),
            child: None,
            stderr_drain: None,
        }
    }
}

/// H-04: 防止 DebugSession 异常路径下未调用 disconnect 导致僵尸进程。
///
/// tokio::process::Child 默认不会在 drop 时 kill 子进程，
/// 必须显式处理，否则适配器进程会一直驻留。
impl Drop for DebugSession {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // 尝试同步 kill，忽略错误
            let _ = child.start_kill();
            // 在 drop 中不能 await，让进程在后台被回收
            // tokio 会在 runtime 中处理孤儿进程的 reaping
        }
    }
}
