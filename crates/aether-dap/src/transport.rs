use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout};

use crate::types::DapMessage;

/// DAP 传输层最大消息大小（64MB，与 LSP 一致）
const MAX_CONTENT_LENGTH: usize = 64 * 1024 * 1024;

/// DAP 传输层
/// 负责与调试适配器进程的 JSON-RPC 通信
pub struct DapTransport {
    stdin: Box<dyn AsyncWrite + Unpin + Send>,
    stdout: Box<dyn AsyncRead + Unpin + Send>,
    seq_counter: i64,
}

impl DapTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            stdin: Box::new(stdin),
            stdout: Box::new(stdout),
            seq_counter: 1,
        }
    }

    /// 发送 DAP 消息
    pub async fn send(&mut self, message: &DapMessage) -> std::io::Result<()> {
        let json = serde_json::to_string(message)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let content = json.into_bytes();
        let header = format!("Content-Length: {}\r\n\r\n", content.len());

        self.stdin.write_all(header.as_bytes()).await?;
        self.stdin.write_all(&content).await?;
        self.stdin.flush().await?;

        Ok(())
    }

    /// 接收 DAP 消息
    pub async fn receive(&mut self) -> std::io::Result<DapMessage> {
        // 读取 Content-Length 头
        // C-06: 添加最大头部长度限制，防止恶意服务器发送无界数据导致内存耗尽
        const MAX_HEADER_LEN: usize = 8 * 1024; // 8KB
        let mut header = Vec::new();
        let mut byte = [0u8; 1];

        loop {
            self.stdout.read_exact(&mut byte).await?;
            header.push(byte[0]);
            if header.ends_with(b"\r\n\r\n") {
                break;
            }
            if header.len() > MAX_HEADER_LEN {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "DAP header exceeds 8KB limit",
                ));
            }
        }

        let header_str = String::from_utf8_lossy(&header);
        let content_length = header_str
            .lines()
            .find(|line| line.starts_with("Content-Length:"))
            .and_then(|line| line[15..].trim().parse::<usize>().ok())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Missing Content-Length header",
                )
            })?;

        // SEC-H02: 拒绝超大消息，防止恶意 DAP 服务器 OOM 崩溃编辑器
        if content_length > MAX_CONTENT_LENGTH {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Content-Length {} 超过最大限制 {}",
                    content_length, MAX_CONTENT_LENGTH
                ),
            ));
        }

        // 读取消息体
        let mut buffer = vec![0u8; content_length];
        self.stdout.read_exact(&mut buffer).await?;

        let message: DapMessage = serde_json::from_slice(&buffer)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        Ok(message)
    }

    pub fn next_seq(&mut self) -> i64 {
        let seq = self.seq_counter;
        self.seq_counter += 1;
        seq
    }
}

#[cfg(test)]
impl DapTransport {
    /// 使用任意实现了 [`AsyncRead`]/[`AsyncWrite`] 的内存流构造传输层，仅用于测试。
    pub fn new_from_parts<W, R>(stdin: W, stdout: R) -> Self
    where
        W: AsyncWrite + Unpin + Send + 'static,
        R: AsyncRead + Unpin + Send + 'static,
    {
        Self {
            stdin: Box::new(stdin),
            stdout: Box::new(stdout),
            seq_counter: 1,
        }
    }
}

/// 启动调试适配器进程
pub async fn spawn_adapter(config: &crate::types::AdapterConfig) -> std::io::Result<Child> {
    let command = config.command.as_ref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "No adapter command specified",
        )
    })?;

    let mut cmd = tokio::process::Command::new(command);
    cmd.args(&config.args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    for (key, value) in &config.env {
        cmd.env(key, value);
    }

    if let Some(cwd) = &config.cwd {
        cmd.current_dir(cwd);
    }

    cmd.spawn()
}

/// 后台持续读取子进程 stderr，避免管道缓冲区满导致调试适配器阻塞。
///
/// 与 LSP 相同，DAP 适配器也会向 stderr 输出日志，若不读取会导致
/// 64KB 缓冲区满后适配器进程完全阻塞，进而导致调试请求卡死。
pub fn spawn_stderr_drain(mut stderr: tokio::process::ChildStderr) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut buf = [0u8; 4096];
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    // 持续读取并丢弃。生产环境可接入日志系统。
                }
                Err(_) => break,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};

    fn make_transport_and_peer() -> (
        DapTransport,
        tokio::io::DuplexStream,
        tokio::io::DuplexStream,
    ) {
        // client_in: session writes -> adapter reads
        // client_out: adapter writes -> session reads
        let (client_in, adapter_in) = duplex(64 * 1024);
        let (adapter_out, client_out) = duplex(64 * 1024);
        let transport = DapTransport::new_from_parts(client_in, client_out);
        (transport, adapter_in, adapter_out)
    }

    #[tokio::test]
    async fn send_produces_valid_dap_frame() {
        let (mut transport, mut adapter_in, _adapter_out) = make_transport_and_peer();
        let msg = DapMessage::Request(DapRequest {
            seq: 1,
            message_type: "request".into(),
            command: "initialize".into(),
            arguments: Some(serde_json::json!({"clientID": "aether"})),
        });

        transport.send(&msg).await.unwrap();

        let mut raw_header = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            adapter_in.read_exact(&mut byte).await.unwrap();
            raw_header.push(byte[0]);
            if raw_header.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let header = String::from_utf8_lossy(&raw_header);
        let content_length = header
            .lines()
            .find(|l| l.starts_with("Content-Length:"))
            .and_then(|l| l[15..].trim().parse::<usize>().ok())
            .unwrap();

        let mut body = vec![0u8; content_length];
        adapter_in.read_exact(&mut body).await.unwrap();
        let received: DapMessage = serde_json::from_slice(&body).unwrap();
        match received {
            DapMessage::Request(r) => assert_eq!(r.command, "initialize"),
            _ => panic!("expected request"),
        }
    }

    #[tokio::test]
    async fn receive_decodes_framed_message() {
        let (mut transport, _adapter_in, mut adapter_out) = make_transport_and_peer();
        let msg = DapMessage::Response(DapResponse {
            seq: 2,
            message_type: "response".into(),
            request_seq: 1,
            success: true,
            command: "initialize".into(),
            body: None,
            message: None,
        });
        let json = serde_json::to_string(&msg).unwrap();
        let frame = format!("Content-Length: {}\r\n\r\n{}", json.len(), json);
        adapter_out.write_all(frame.as_bytes()).await.unwrap();

        let received = transport.receive().await.unwrap();
        match received {
            DapMessage::Response(r) => {
                assert_eq!(r.command, "initialize");
                assert!(r.success);
            }
            _ => panic!("expected response"),
        }
    }

    #[tokio::test]
    async fn receive_missing_content_length_returns_invalid_data() {
        let (mut transport, _adapter_in, mut adapter_out) = make_transport_and_peer();
        adapter_out
            .write_all(b"No-Length-Header\r\n\r\n{}")
            .await
            .unwrap();
        let err = transport.receive().await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn receive_malformed_json_returns_invalid_data() {
        let (mut transport, _adapter_in, mut adapter_out) = make_transport_and_peer();
        let body = b"not json";
        let frame = format!(
            "Content-Length: {}\r\n\r\n{}",
            body.len(),
            String::from_utf8_lossy(body)
        );
        adapter_out.write_all(frame.as_bytes()).await.unwrap();
        let err = transport.receive().await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn receive_oversized_content_length_returns_invalid_data() {
        let (mut transport, _adapter_in, mut adapter_out) = make_transport_and_peer();
        let frame = format!("Content-Length: {}\r\n\r\n", 64 * 1024 * 1024 + 1);
        adapter_out.write_all(frame.as_bytes()).await.unwrap();
        let err = transport.receive().await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn next_seq_increments() {
        let (mut transport, _adapter_in, _adapter_out) = make_transport_and_peer();
        assert_eq!(transport.next_seq(), 1);
        assert_eq!(transport.next_seq(), 2);
        assert_eq!(transport.next_seq(), 3);
    }
}
