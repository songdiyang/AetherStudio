use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout};

use crate::types::DapMessage;

/// DAP 传输层最大消息大小（64MB，与 LSP 一致）
const MAX_CONTENT_LENGTH: usize = 64 * 1024 * 1024;

/// DAP 传输层
/// 负责与调试适配器进程的 JSON-RPC 通信
pub struct DapTransport {
    stdin: ChildStdin,
    stdout: ChildStdout,
    seq_counter: i64,
}

impl DapTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            stdin,
            stdout,
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
        let mut header = Vec::new();
        let mut byte = [0u8; 1];

        loop {
            self.stdout.read_exact(&mut byte).await?;
            header.push(byte[0]);
            if header.ends_with(b"\r\n\r\n") {
                break;
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
pub fn spawn_stderr_drain(mut child: tokio::process::Child) {
    if let Some(mut stderr) = child.stderr.take() {
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
            // 等待子进程退出，避免僵尸进程
            let _ = child.wait().await;
        });
    }
}
