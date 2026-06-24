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
            if let Some((content_length, header_end)) = self.parse_header() {
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
    fn parse_header(&self) -> Option<(usize, usize)> {
        let buf = &self.read_buffer;
        let header_str = std::str::from_utf8(buf).ok()?;

        // 查找 header 结束标记 \r\n\r\n
        let header_end = header_str.find("\r\n\r\n")?;
        let header_part = &header_str[..header_end];

        // 解析 Content-Length
        for line in header_part.lines() {
            if let Some(val) = line.strip_prefix("Content-Length: ") {
                return Some((val.parse().ok()?, header_end + 4));
            }
        }

        None
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
