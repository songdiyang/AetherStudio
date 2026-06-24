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
