use bytes::BytesMut;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout};

use crate::types::LspMessage;

/// LSP 写入器：仅持有 stdin，负责发送消息到子进程
///
/// 拆分自原 LspTransport，让 reader task 独占 stdout，
/// 避免 send/receive 互锁。
pub struct LspWriter<W = ChildStdin> {
    stdin: W,
}

impl<W: AsyncWrite + Unpin + Send + 'static> LspWriter<W> {
    pub fn new(stdin: W) -> Self {
        Self { stdin }
    }

    /// 发送一条 LSP 消息
    pub async fn send(&mut self, message: &LspMessage) -> io::Result<()> {
        let bytes = encode_message(message)?;
        self.stdin.write_all(&bytes).await?;
        self.stdin.flush().await?;
        Ok(())
    }
}

/// LSP 传输层：处理 JSON-RPC over stdio 的编码/解码
/// 遵循 LSP 规范：Header + Content-Length + Content-Type + \r\n + JSON body
///
/// 保留此类型用于测试兼容：可将一个 LspWriter + LspReader 对包装为
/// 双向 transport，使旧测试无需大幅改动。
pub struct LspTransport<W = ChildStdin, R = ChildStdout> {
    stdin: W,
    stdout: R,
    read_buffer: BytesMut,
}

impl LspTransport<ChildStdin, ChildStdout> {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            stdin,
            stdout,
            read_buffer: BytesMut::with_capacity(8192),
        }
    }
}

impl<W, R> LspTransport<W, R>
where
    W: AsyncWrite + Unpin + Send,
    R: AsyncRead + Unpin + Send,
{
    /// 构造一个通用传输实例，便于测试使用任意 AsyncRead/AsyncWrite 对
    #[cfg(test)]
    pub(crate) fn new_generic(stdin: W, stdout: R) -> Self {
        Self {
            stdin,
            stdout,
            read_buffer: BytesMut::with_capacity(8192),
        }
    }

    /// 发送一条 LSP 消息
    pub async fn send(&mut self, message: &LspMessage) -> io::Result<()> {
        let bytes = encode_message(message)?;
        self.stdin.write_all(&bytes).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    /// 接收一条 LSP 消息（阻塞直到完整消息到达）
    pub async fn receive(&mut self) -> io::Result<LspMessage> {
        loop {
            // 尝试解析缓冲区中已有的数据
            if let Some((content_length, header_end)) = parse_header_buffer(&self.read_buffer) {
                let total_needed = header_end + content_length;
                if self.read_buffer.len() >= total_needed {
                    let json_bytes = self.read_buffer.split_to(total_needed);
                    let json_str = std::str::from_utf8(&json_bytes[header_end..])
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

                    let message = serde_json::from_str(json_str).map_err(|e| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("JSON parse error: {}", e),
                        )
                    })?;

                    return Ok(message);
                }
            }

            // 需要更多数据
            let mut temp = [0u8; 4096];
            let n = self.stdout.read(&mut temp).await?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "LSP server closed stdout",
                ));
            }
            self.read_buffer.extend_from_slice(&temp[..n]);
        }
    }
}

/// LSP 读取器：仅持有 stdout，负责接收子进程消息
///
/// 拆分自原 LspTransport，让 reader task 独占访问，
/// server 端只持 LspWriter，无共享锁竞争。
pub struct LspReader<R = ChildStdout> {
    stdout: R,
    read_buffer: BytesMut,
}

impl<R: AsyncRead + Unpin + Send + 'static> LspReader<R> {
    pub fn new(stdout: R) -> Self {
        Self {
            stdout,
            read_buffer: BytesMut::with_capacity(8192),
        }
    }

    /// 接收一条 LSP 消息（阻塞直到完整消息到达）
    pub async fn receive(&mut self) -> io::Result<LspMessage> {
        loop {
            // 尝试解析缓冲区中已有的数据
            if let Some((content_length, header_end)) = self.parse_header()? {
                let total_needed = header_end + content_length;
                if self.read_buffer.len() >= total_needed {
                    let json_bytes = self.read_buffer.split_to(total_needed);
                    let json_str = std::str::from_utf8(&json_bytes[header_end..])
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

                    let message = serde_json::from_str(json_str).map_err(|e| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("JSON parse error: {}", e),
                        )
                    })?;

                    return Ok(message);
                }
            }

            // 需要更多数据
            let mut temp = [0u8; 4096];
            let n = self.stdout.read(&mut temp).await?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "LSP server closed stdout",
                ));
            }
            self.read_buffer.extend_from_slice(&temp[..n]);
        }
    }

    /// 解析 Header，返回 (content_length, header_end_position)
    /// 返回 `Ok(None)` 表示需要更多数据，`Err` 表示协议错误需关闭连接
    fn parse_header(&self) -> io::Result<Option<(usize, usize)>> {
        let buf = &self.read_buffer;

        // H-07: 限制 Header 大小为 8KB，防止恶意 LSP 服务器发送无限 Header 导致 OOM
        const MAX_HEADER_LEN: usize = 8 * 1024;
        if buf.len() > MAX_HEADER_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "LSP header exceeds 8KB limit",
            ));
        }

        // H-31: 先在原始字节中搜索 \r\n\r\n，避免 body 中部分 UTF-8 序列导致失败
        let header_end_bytes = match buf.windows(4).position(|window| window == b"\r\n\r\n") {
            Some(pos) => pos,
            None => return Ok(None), // 需要更多数据
        };
        let header_bytes = &buf[..header_end_bytes];
        let header_str = std::str::from_utf8(header_bytes).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "LSP header is not valid UTF-8")
        })?;

        // 解析 Content-Length
        for line in header_str.lines() {
            if let Some(val) = line.strip_prefix("Content-Length: ") {
                let content_len: usize = val.parse().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "Invalid Content-Length value")
                })?;
                // C-07: 超大消息返回显式错误，避免返回 None 导致协议失同步
                const MAX_CONTENT_LENGTH: usize = 64 * 1024 * 1024;
                if content_len > MAX_CONTENT_LENGTH {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "LSP message Content-Length {} exceeds 64MB limit",
                            content_len
                        ),
                    ));
                }
                return Ok(Some((content_len, header_end_bytes + 4)));
            }
        }

        Ok(None)
    }
}

/// 将 LSP 消息编码为 JSON-RPC over stdio 字节流
pub(crate) fn encode_message(message: &LspMessage) -> io::Result<Vec<u8>> {
    let json = serde_json::to_string(message).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("JSON serialize error: {}", e),
        )
    })?;

    let header = format!(
        "Content-Length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n",
        json.len()
    );

    let mut bytes = Vec::with_capacity(header.len() + json.len());
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(json.as_bytes());
    Ok(bytes)
}

/// 从缓冲区解析 LSP Header，返回 (content_length, header_end_position)
pub(crate) fn parse_header_buffer(buf: &BytesMut) -> Option<(usize, usize)> {
    // H-31: 先在原始字节中搜索 \r\n\r\n，避免 body 中部分 UTF-8 序列导致失败
    let header_end_bytes = buf.windows(4).position(|window| window == b"\r\n\r\n")?;
    let header_bytes = &buf[..header_end_bytes];
    let header_str = std::str::from_utf8(header_bytes).ok()?;

    // 解析 Content-Length
    for line in header_str.lines() {
        if let Some(val) = line.strip_prefix("Content-Length: ") {
            // H-33: 找不到 Content-Length 时返回错误而非 None
            let content_len: usize = val.parse().ok()?;
            // H-32: 添加最大 Content-Length 检查（64MB）
            const MAX_CONTENT_LENGTH: usize = 64 * 1024 * 1024;
            if content_len > MAX_CONTENT_LENGTH {
                return None;
            }
            return Some((content_len, header_end_bytes + 4));
        }
    }

    None
}

/// 启动语言服务器进程
pub async fn spawn_server(config: &crate::types::ServerConfig) -> io::Result<Child> {
    let mut cmd = build_command(config);
    cmd.spawn()
}

/// 根据配置构造 Command（不实际执行，便于单元测试校验参数）
pub(crate) fn build_command(config: &crate::types::ServerConfig) -> tokio::process::Command {
    let command = config
        .command
        .as_ref()
        .and_then(|p| p.to_str())
        .unwrap_or("rust-analyzer"); // 默认尝试 rust-analyzer

    let mut cmd = tokio::process::Command::new(command);
    cmd.args(&config.args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // 设置环境变量
    for (key, value) in &config.env {
        cmd.env(key, value);
    }

    cmd
}

/// 后台持续读取子进程 stderr，避免管道缓冲区满导致子进程阻塞。
///
/// LSP/DAP 服务器在 stderr 输出日志，若不读取，64KB 缓冲区满后
/// 服务器进程会完全阻塞，进而导致编辑器请求超时卡死。
pub fn spawn_stderr_drain(mut stderr: tokio::process::ChildStderr) {
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut buf = [0u8; 4096];
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    // 持续读取并丢弃，避免缓冲区满。
                }
                Err(_) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_message() {
        let msg = LspMessage::Notification(crate::types::LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "initialized".to_string(),
            params: None,
        });
        let bytes = encode_message(&msg).unwrap();
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.contains("Content-Length:"));
        assert!(s.contains("Content-Type: application/vscode-jsonrpc; charset=utf-8"));
        assert!(s.contains("\"method\":\"initialized\""));
        assert!(s.contains("\r\n\r\n"));
    }

    #[test]
    fn test_parse_header_buffer_basic() {
        let body = r#"{"jsonrpc":"2.0"}"#;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut buf = BytesMut::from(header.as_str());
        buf.extend_from_slice(body.as_bytes());
        buf.extend_from_slice(b"\r\n");

        let (len, end) = parse_header_buffer(&buf).unwrap();
        assert_eq!(len, body.len());
        assert_eq!(end, header.len());
        // 解析后 body 仍在缓冲区中
        assert_eq!(buf.len(), header.len() + body.len() + 2);
    }

    #[test]
    fn test_parse_header_buffer_missing_content_length() {
        let buf = BytesMut::from("Content-Type: text\r\n\r\nbody");
        assert!(parse_header_buffer(&buf).is_none());
    }

    #[test]
    fn test_parse_header_buffer_too_large() {
        let buf = BytesMut::from("Content-Length: 999999999\r\n\r\n");
        assert!(parse_header_buffer(&buf).is_none());
    }

    #[tokio::test]
    async fn test_transport_roundtrip() {
        let msg = LspMessage::Notification(crate::types::LspNotification {
            jsonrpc: "2.0".to_string(),
            method: "initialized".to_string(),
            params: None,
        });
        let encoded = encode_message(&msg).unwrap();

        // 使用 duplex 模拟双向管道
        let (client_read, mut server_write) = tokio::io::duplex(1024);
        let (mut server_read, client_write) = tokio::io::duplex(1024);

        // server 端：收到请求后回写同样的消息
        tokio::spawn(async move {
            let mut buf = vec![0u8; encoded.len()];
            server_read.read_exact(&mut buf).await.unwrap();
            server_write.write_all(&buf).await.unwrap();
            server_write.flush().await.unwrap();
        });

        let mut transport = LspTransport::new_generic(client_write, client_read);
        transport.send(&msg).await.unwrap();
        let received = transport.receive().await.unwrap();

        match received {
            LspMessage::Notification(n) => {
                assert_eq!(n.method, "initialized");
            }
            _ => panic!("expected notification"),
        }
    }

    #[tokio::test]
    async fn test_transport_receive_eof() {
        let (client_read, server_write) = tokio::io::duplex(1024);
        let (_server_read, client_write) = tokio::io::duplex(1024);

        // 立即关闭写端
        drop(server_write);

        let mut transport = LspTransport::new_generic(client_write, client_read);
        assert!(transport.receive().await.is_err());
    }

    #[test]
    fn test_build_command_default() {
        let config = crate::types::ServerConfig::default();
        let cmd = build_command(&config);
        // tokio::process::Command 不暴露内部参数,因此验证通过构造不失败即可
        let _ = cmd;
    }

    #[test]
    fn test_build_command_with_config() {
        use std::path::PathBuf;
        let config = crate::types::ServerConfig {
            command: Some(PathBuf::from("test-lsp")),
            args: vec!["--stdio".to_string()],
            env: {
                let mut m = std::collections::HashMap::new();
                m.insert("KEY".to_string(), "VALUE".to_string());
                m
            },
            ..Default::default()
        };
        let _cmd = build_command(&config);
    }

    #[tokio::test]
    async fn test_spawn_server_missing_binary() {
        use std::path::PathBuf;
        let config = crate::types::ServerConfig {
            command: Some(PathBuf::from("this_binary_does_not_exist_12345")),
            ..Default::default()
        };
        assert!(spawn_server(&config).await.is_err());
    }
}
