use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::session::DebugSession;
use crate::types::*;

/// DAP 客户端管理器
/// 管理多个调试会话
pub struct DapClient {
    sessions: HashMap<String, DebugSession>,
    event_tx: mpsc::UnboundedSender<DapEventUi>,
}

impl DapClient {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<DapEventUi>) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let client = Self {
            sessions: HashMap::new(),
            event_tx,
        };
        (client, event_rx)
    }

    /// 启动调试会话
    pub async fn start_session(&mut self, id: &str, config: AdapterConfig) -> std::io::Result<()> {
        let session = DebugSession::start(config, self.event_tx.clone()).await?;
        self.sessions.insert(id.to_string(), session);
        Ok(())
    }

    /// 获取会话
    pub fn get_session(&mut self, id: &str) -> Option<&mut DebugSession> {
        self.sessions.get_mut(id)
    }

    /// 停止所有会话
    pub async fn stop_all(&mut self) {
        for (_, session) in self.sessions.iter_mut() {
            let _ = session.disconnect().await;
        }
        self.sessions.clear();
    }
}

/// 默认适配器配置发现
pub fn default_adapter_config(language_id: &str) -> Option<AdapterConfig> {
    match language_id {
        "rust" | "c" | "cpp" => Some(AdapterConfig {
            command: Some(std::path::PathBuf::from("lldb-dap")),
            args: vec![],
            env: HashMap::new(),
            program: None,
            cwd: None,
        }),
        "python" => Some(AdapterConfig {
            command: Some(std::path::PathBuf::from("debugpy.adapter")),
            args: vec![],
            env: HashMap::new(),
            program: None,
            cwd: None,
        }),
        "javascript" | "typescript" => Some(AdapterConfig {
            command: Some(std::path::PathBuf::from("node")),
            args: vec!["node_modules/.bin/node-debug2-adapter".to_string()],
            env: HashMap::new(),
            program: None,
            cwd: None,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::DebugSession;
    use crate::transport::DapTransport;
    use tokio::io::duplex;

    fn make_session_with_fake() -> (DebugSession, DapTransport) {
        let (tx, _rx) = mpsc::unbounded_channel();
        let (client_in, adapter_in) = duplex(64 * 1024);
        let (adapter_out, client_out) = duplex(64 * 1024);
        let transport = DapTransport::new_from_parts(client_in, client_out);
        let session = DebugSession::with_transport(transport, tx);
        let fake = DapTransport::new_from_parts(adapter_out, adapter_in);
        (session, fake)
    }

    fn success_response(command: &str) -> DapMessage {
        DapMessage::Response(DapResponse {
            seq: 1,
            message_type: "response".into(),
            request_seq: 1,
            success: true,
            command: command.into(),
            body: None,
            message: None,
        })
    }

    #[test]
    fn new_creates_channel_and_empty_sessions() {
        let (client, mut rx) = DapClient::new();
        assert!(client.sessions.is_empty());
        // 验证事件通道可用
        client.event_tx.send(DapEventUi::Terminated).unwrap();
        assert!(matches!(rx.try_recv(), Ok(DapEventUi::Terminated)));
    }

    #[test]
    fn default_adapter_config_rust() {
        let cfg = default_adapter_config("rust").unwrap();
        assert_eq!(cfg.command, Some(std::path::PathBuf::from("lldb-dap")));
    }

    #[test]
    fn default_adapter_config_c_and_cpp() {
        for lang in ["c", "cpp"] {
            let cfg = default_adapter_config(lang).unwrap();
            assert_eq!(cfg.command, Some(std::path::PathBuf::from("lldb-dap")));
        }
    }

    #[test]
    fn default_adapter_config_python() {
        let cfg = default_adapter_config("python").unwrap();
        assert_eq!(
            cfg.command,
            Some(std::path::PathBuf::from("debugpy.adapter"))
        );
    }

    #[test]
    fn default_adapter_config_javascript_and_typescript() {
        for lang in ["javascript", "typescript"] {
            let cfg = default_adapter_config(lang).unwrap();
            assert_eq!(cfg.command, Some(std::path::PathBuf::from("node")));
            assert_eq!(cfg.args, vec!["node_modules/.bin/node-debug2-adapter"]);
        }
    }

    #[test]
    fn default_adapter_config_unknown_returns_none() {
        assert!(default_adapter_config("go").is_none());
        assert!(default_adapter_config("").is_none());
    }

    #[tokio::test]
    async fn get_session_returns_some_and_none() {
        let (mut client, _rx) = DapClient::new();
        let (session, _fake) = make_session_with_fake();
        client.sessions.insert("s1".into(), session);
        assert!(client.get_session("s1").is_some());
        assert!(client.get_session("missing").is_none());
    }

    #[tokio::test]
    async fn start_session_fails_without_command() {
        let (mut client, _rx) = DapClient::new();
        let config = AdapterConfig::default();
        let result = client.start_session("s1", config).await;
        assert!(result.is_err());
        assert!(client.sessions.is_empty());
    }

    #[tokio::test]
    async fn stop_all_disconnects_sessions_and_clears_map() {
        let (mut client, _rx) = DapClient::new();
        let (session, mut fake) = make_session_with_fake();
        fake.send(&success_response("disconnect")).await.unwrap();
        client.sessions.insert("s1".into(), session);

        client.stop_all().await;
        assert!(client.sessions.is_empty());
    }
}
