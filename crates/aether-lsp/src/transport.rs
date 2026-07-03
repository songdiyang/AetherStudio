use bytes::BytesMut;
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout};

use crate::types::LspMessage;

/// LSP 传输层：处理 JSON-RPC over stdio 的编码/解码
/// 遵循 LSP 规范：Header + Content-Length + Content-Type + \r\n + JSON body
pub struct LspTransport {
    stdin: ChildStdin,
    stdout: ChildStdout,
    read_buffer: BytesMut,
}

impl LspTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            stdin,
            stdout,
            read_buffer: BytesMut::with_capacity(8192),
        }
    }

    /// 发送一条 LSP 消息
    pub async fn send(&mut self, message: &LspMessage) -> io::Result<()> {
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

        self.stdin.write_all(header.as_bytes()).await?;
        self.stdin.write_all(json.as_bytes()).await?;
        self.stdin.flush().await?;

        Ok(())
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
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid Content-Length value",
                    )
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

/// 启动语言服务器进程
pub async fn spawn_server(config: &crate::types::ServerConfig) -> io::Result<Child> {
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

    cmd.spawn()
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
