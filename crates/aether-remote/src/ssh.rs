use std::net::TcpStream;
use std::path::Path;
use std::sync::mpsc;
use std::io::Read;

use crate::remote_fs::{RemoteFs, RemoteDirEntry, FsEvent, Result};

/// SSH 认证方式
#[derive(Clone, Debug)]
pub enum SshAuth {
    Password(String),
    Key { path: String, passphrase: Option<String> },
    Agent,
}

/// SSH 连接配置
#[derive(Clone, Debug)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 22,
            username: String::new(),
            auth: SshAuth::Agent,
        }
    }
}

/// SSH 远程文件系统实现
pub struct SshRemoteFs {
    config: SshConfig,
    session: Option<ssh2::Session>,
}

impl SshRemoteFs {
    /// 创建新的 SSH 远程文件系统
    pub fn new(config: SshConfig) -> Self {
        Self {
            config,
            session: None,
        }
    }

    /// 建立 SSH 连接
    pub fn connect(&mut self) -> Result<()> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        let tcp = TcpStream::connect(&addr)
            .map_err(|e| format!("TCP 连接失败: {}", e))?;
        
        let mut session = ssh2::Session::new()
            .map_err(|e| format!("SSH 会话创建失败: {}", e))?;
        
        session.set_tcp_stream(tcp);
        session.handshake()
            .map_err(|e| format!("SSH 握手失败: {}", e))?;
        
        // 认证
        match &self.config.auth {
            SshAuth::Password(password) => {
                session.userauth_password(&self.config.username, password)
                    .map_err(|e| format!("密码认证失败: {}", e))?;
            }
            SshAuth::Key { path, passphrase } => {
                session.userauth_pubkey_file(
                    &self.config.username,
                    None,
                    Path::new(path),
                    passphrase.as_deref()
                ).map_err(|e| format!("密钥认证失败: {}", e))?;
            }
            SshAuth::Agent => {
                session.userauth_agent(&self.config.username)
                    .map_err(|e| format!("SSH Agent 认证失败: {}", e))?;
            }
        }
        
        self.session = Some(session);
        Ok(())
    }

    /// 检查连接是否活跃
    pub fn is_connected(&self) -> bool {
        self.session.is_some()
    }

    /// 断开 SSH 连接
    pub fn disconnect(&mut self) {
        self.session = None;
    }
}

impl RemoteFs for SshRemoteFs {
    /// 读取远程文件内容
    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let session = self.session.as_ref()
            .ok_or("SSH 未连接，请先调用 connect()")?;
        
        let sftp = session.sftp()
            .map_err(|e| format!("SFTP 初始化失败: {}", e))?;
        
        let mut file = sftp.open(Path::new(path))
            .map_err(|e| format!("打开文件失败: {}", e))?;
        let mut content = Vec::new();
        file.read_to_end(&mut content)
            .map_err(|e| format!("读取文件失败: {}", e))?;
        
        Ok(content)
    }

    /// 写入文件到远程
    fn write_file(&self, path: &str, content: &[u8]) -> Result<()> {
        let session = self.session.as_ref()
            .ok_or("SSH 未连接，请先调用 connect()")?;

        // 使用 base64 编码文件内容来安全传输
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(content);
        let cmd = format!("printf '%s' '{}' | base64 -d > '{}'", encoded, path);
        
        let mut channel = session.channel_session()
            .map_err(|e| format!("创建通道失败: {}", e))?;
        channel.exec(&cmd)
            .map_err(|e| format!("执行命令失败: {}", e))?;
        
        let mut stdout = String::new();
        let mut stderr = String::new();
        channel.read_to_string(&mut stdout)
            .map_err(|e| format!("读取 stdout 失败: {}", e))?;
        channel.stderr().read_to_string(&mut stderr)
            .map_err(|e| format!("读取 stderr 失败: {}", e))?;
        let _ = channel.wait_close();
        
        if !stderr.is_empty() {
            return Err(format!("写入远程文件失败: {}", stderr));
        }
        
        Ok(())
    }

    /// 列出远程目录内容
    fn list_dir(&self, path: &str) -> Result<Vec<RemoteDirEntry>> {
        let session = self.session.as_ref()
            .ok_or("SSH 未连接，请先调用 connect()")?;

        let sftp = session.sftp()
            .map_err(|e| format!("SFTP 初始化失败: {}", e))?;
        
        let entries = sftp.readdir(Path::new(path))
            .map_err(|e| format!("列出目录失败: {}", e))?;
        
        let mut result = Vec::new();
        for (path_buf, stat) in entries {
            let name = path_buf.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            result.push(RemoteDirEntry {
                name,
                is_dir: stat.is_dir(),
                size: stat.size.unwrap_or(0),
                modified: stat.mtime.map(|t| {
                    std::time::UNIX_EPOCH + std::time::Duration::from_secs(t)
                }),
            });
        }
        
        Ok(result)
    }

    /// 监听文件变更（SSH 场景下可能不适用）
    fn watch(&self, _path: &str) -> Result<mpsc::Receiver<FsEvent>> {
        let (_tx, rx) = mpsc::channel();
        Ok(rx)
    }

    /// 在远程执行命令
    fn exec(&self, command: &str) -> Result<(String, String)> {
        let session = self.session.as_ref()
            .ok_or("SSH 未连接，请先调用 connect()")?;
        
        let mut channel = session.channel_session()
            .map_err(|e| format!("创建通道失败: {}", e))?;
        channel.exec(command)
            .map_err(|e| format!("执行命令失败: {}", e))?;
        
        let mut stdout = String::new();
        let mut stderr = String::new();
        channel.read_to_string(&mut stdout)
            .map_err(|e| format!("读取 stdout 失败: {}", e))?;
        channel.stderr().read_to_string(&mut stderr)
            .map_err(|e| format!("读取 stderr 失败: {}", e))?;
        let _ = channel.wait_close();
        
        Ok((stdout, stderr))
    }
}
