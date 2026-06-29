//! SSH 远程文件系统(通过 shell out 调用系统 `ssh` 二进制)
//!
//! 设计取舍:面向 Windows 开发者,Windows 10+ 自带 OpenSSH client
//! (`C:\Windows\System32\OpenSSH\ssh.exe`),无需自带 SSH 库。
//! 编译期零依赖(无 russh/ssh2/libssh2),运行期依赖系统 ssh。
//!
//! 限制:
//! - 密码认证不支持(shell 无 tty 无法交互输入密码),请配置密钥认证
//! - known_hosts 由 ssh.exe 自动校验,默认严格模式
//! - 调用方应先调 [`ssh_available`] 校验,缺失时引导访问 [`SSH_DOWNLOAD_URL`]

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc;

use crate::remote_fs::{FsEvent, RemoteDirEntry, RemoteFs, Result};

/// OpenSSH 下载页(用户缺失 ssh 时引导跳转)
pub const SSH_DOWNLOAD_URL: &str = "https://learn.microsoft.com/openssh";

/// P1-3: 单引号包裹路径用于 shell 命令，转义内部单引号避免注入
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// 校验系统是否安装 ssh 并在 PATH 中可调用
///
/// UI 层应在初始化时调用此函数,返回 false 时提示用户安装 OpenSSH
/// 并跳转 [`SSH_DOWNLOAD_URL`]。
pub fn ssh_available() -> bool {
    Command::new("ssh")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// SSH 认证方式
#[derive(Clone)]
pub enum SshAuth {
    Password(String),
    Key {
        path: String,
        passphrase: Option<String>,
    },
    Agent,
}

impl std::fmt::Debug for SshAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SshAuth::Password(_) => f.debug_tuple("Password").field(&"[REDACTED]").finish(),
            SshAuth::Key { path, passphrase } => {
                let mut dbg = f.debug_struct("Key");
                dbg.field("path", path);
                if passphrase.is_some() {
                    dbg.field("passphrase", &"[REDACTED]");
                }
                dbg.finish()
            }
            SshAuth::Agent => f.debug_struct("Agent").finish(),
        }
    }
}

/// SSH 连接配置
#[derive(Clone)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,
}

impl std::fmt::Debug for SshConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("auth", &self.auth)
            .finish()
    }
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

/// SSH 远程文件系统实现(通过系统 ssh 二进制)
pub struct SshRemoteFs {
    config: SshConfig,
    /// 连接软状态(connect() 后设为 true)
    connected: bool,
}

impl SshRemoteFs {
    /// 创建新的 SSH 远程文件系统
    pub fn new(config: SshConfig) -> Self {
        Self {
            config,
            connected: false,
        }
    }

    /// 创建已标记为连接状态的实例（用于后台线程，跳过重复 connect 测试）
    ///
    /// 由于 shell out 模式下每次操作都独立调用 ssh（无持久连接），
    /// `connected` 仅是软状态标志。当主线程已通过 `connect()` 验证连接后，
    /// 后台线程可据此构造已连接实例直接调用 `list_dir` 等方法，避免重复探测。
    /// 调用方需确保配置已通过 `connect()` 验证。
    pub fn new_connected(config: SshConfig) -> Self {
        Self {
            config,
            connected: true,
        }
    }

    /// 建立 SSH 连接(测试连接可用性)
    ///
    /// 用 `ssh -o BatchMode=yes -o ConnectTimeout=5 ... exit 0` 测试。
    /// BatchMode=yes 禁用交互式密码提示,确保非阻塞。
    pub fn connect(&mut self) -> Result<()> {
        // 密码认证在 shell out 模式下不支持(无 tty)
        if matches!(self.config.auth, SshAuth::Password(_)) {
            return Err(
                "shell out 模式不支持密码认证,请配置密钥认证(SshAuth::Key 或 SshAuth::Agent)"
                    .to_string(),
            );
        }

        let (stdout, stderr, ok) =
            self.ssh(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "exit", "0"]);
        if !ok {
            return Err(format!(
                "SSH 连接失败:\n{}\n{}",
                String::from_utf8_lossy(&stderr),
                String::from_utf8_lossy(&stdout)
            ));
        }
        self.connected = true;
        Ok(())
    }

    /// 检查连接是否活跃(软状态)
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// 断开 SSH 连接(shell out 模式无持久连接,仅重置状态)
    pub fn disconnect(&mut self) {
        self.connected = false;
    }

    /// 构造 ssh 命令的基础参数(用户名、端口、密钥)
    fn base_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
        ];

        if self.config.port != 22 {
            args.push("-p".to_string());
            args.push(self.config.port.to_string());
        }

        // 密钥文件(Password 在 connect() 已拒绝,这里只处理 Key/Agent)
        if let SshAuth::Key { path, .. } = &self.config.auth {
            args.push("-i".to_string());
            args.push(path.clone());
        }

        // user@host
        let target = if self.config.username.is_empty() {
            self.config.host.clone()
        } else {
            format!("{}@{}", self.config.username, self.config.host)
        };
        args.push(target);
        args
    }

    /// 执行 ssh 命令(无 stdin),返回 (stdout, stderr, success)
    fn ssh(&self, remote_args: &[&str]) -> (Vec<u8>, Vec<u8>, bool) {
        let base = self.base_args();
        let mut cmd = Command::new("ssh");
        for a in &base {
            cmd.arg(a);
        }
        for a in remote_args {
            cmd.arg(a);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        match cmd.output() {
            Ok(o) => (o.stdout, o.stderr, o.status.success()),
            Err(e) => (
                Vec::new(),
                format!("执行 ssh 命令失败: {}", e).into_bytes(),
                false,
            ),
        }
    }

    /// 执行 ssh 命令(带 stdin),返回 (stdout, stderr, success)
    fn ssh_with_stdin(&self, remote_args: &[&str], input: &[u8]) -> (Vec<u8>, Vec<u8>, bool) {
        let base = self.base_args();
        let mut cmd = Command::new("ssh");
        for a in &base {
            cmd.arg(a);
        }
        for a in remote_args {
            cmd.arg(a);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return (
                    Vec::new(),
                    format!("执行 ssh 命令失败: {}", e).into_bytes(),
                    false,
                )
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input);
        }

        match child.wait_with_output() {
            Ok(o) => (o.stdout, o.stderr, o.status.success()),
            Err(e) => (
                Vec::new(),
                format!("等待 ssh 命令失败: {}", e).into_bytes(),
                false,
            ),
        }
    }
}

impl RemoteFs for SshRemoteFs {
    /// 读取远程文件内容
    ///
    /// `ssh user@host cat path`(二进制安全)
    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        if !self.connected {
            return Err("SSH 未连接,请先调用 connect()".to_string());
        }
        let (stdout, stderr, ok) = self.ssh(&["cat", "--", path]);
        if !ok {
            return Err(format!(
                "读取文件失败:\n{}\n{}",
                String::from_utf8_lossy(&stderr),
                String::from_utf8_lossy(&stdout)
            ));
        }
        Ok(stdout)
    }

    /// 写入文件到远程(通过 stdin 管道,避免 shell 命令注入)
    ///
    /// P1-3: 原子写入——先写到临时文件再 mv，避免写入中途断连损坏远程文件。
    /// `ssh user@host "cat > 'path.tmp' && mv 'path.tmp' 'path'"` + stdin 写入内容。
    /// 临时文件与目标在同一目录（同文件系统），mv 是原子操作。
    fn write_file(&self, path: &str, content: &[u8]) -> Result<()> {
        if !self.connected {
            return Err("SSH 未连接,请先调用 connect()".to_string());
        }
        // 构造同目录临时文件名：用 PID + 纳秒时间戳避免并发冲突
        let tmp_suffix = format!(
            ".tmp.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let tmp_full = format!("{}{}", path, tmp_suffix);
        let tmp_q = shell_quote(&tmp_full);
        let path_q = shell_quote(path);
        // cat > tmp 接收 stdin（路径用单引号包裹避免注入），写成功后 mv 原子替换
        let remote_cmd = format!("cat > {} && mv {} {}", tmp_q, tmp_q, path_q);
        let (stdout, stderr, ok) = self.ssh_with_stdin(&[&remote_cmd], content);
        if !ok {
            // 写入失败时尝试清理临时文件（best-effort）
            let _ = self.ssh(&[&format!("rm -f {}", tmp_q)]);
            return Err(format!(
                "写入远程文件失败:\n{}\n{}",
                String::from_utf8_lossy(&stderr),
                String::from_utf8_lossy(&stdout)
            ));
        }
        Ok(())
    }

    /// 列出远程目录内容
    ///
    /// 用 `stat -c '%n\t%s\t%Y\t%F' path/*` 一次性获取所有文件属性。
    /// 空目录时 stat 报错,返回空列表。
    fn list_dir(&self, path: &str) -> Result<Vec<RemoteDirEntry>> {
        if !self.connected {
            return Err("SSH 未连接,请先调用 connect()".to_string());
        }
        // stat 格式:文件名\t大小\tmtime\t类型
        // 用 find 避免空目录的 glob 不展开问题
        let remote_cmd = format!(
            "find '{}' -maxdepth 1 -mindepth 1 -exec stat -c '%n\t%s\t%Y\t%F' {{}} +",
            path.replace('\'', "'\\''")
        );
        let (stdout, stderr, ok) = self.ssh(&[&remote_cmd]);
        if !ok {
            // 空目录或目录不存在
            let err = String::from_utf8_lossy(&stderr);
            if err.contains("No such file or directory") {
                return Err(format!("目录不存在: {}", path));
            }
            // 空目录 find 无输出,exit 0,走下面解析
            if !stdout.is_empty() {
                return Err(format!("列出目录失败:\n{}", err));
            }
        }

        let stdout = String::from_utf8_lossy(&stdout);
        let mut entries = Vec::new();
        for line in stdout.lines() {
            let fields: Vec<&str> = line.splitn(4, '\t').collect();
            if fields.len() < 4 {
                continue;
            }
            let full_path = fields[0];
            let name = std::path::Path::new(full_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let size: u64 = fields[1].parse().unwrap_or(0);
            let mtime: u64 = fields[2].parse().unwrap_or(0);
            let is_dir = fields[3] == "directory";

            entries.push(RemoteDirEntry {
                name,
                is_dir,
                size,
                modified: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(mtime)),
            });
        }
        Ok(entries)
    }

    /// 监听文件变更(SSH 场景下不支持)
    fn watch(&self, _path: &str) -> Result<mpsc::Receiver<FsEvent>> {
        Err("SSH 后端不支持文件监视".to_string())
    }

    /// 在远程执行命令
    ///
    /// `ssh user@host command`,ssh 自动把远程 stdout/stderr 转发到本地
    fn exec(&self, command: &str) -> Result<(String, String)> {
        if !self.connected {
            return Err("SSH 未连接,请先调用 connect()".to_string());
        }
        eprintln!("[AUDIT] ssh exec: {}", command);
        let (stdout, stderr, _ok) = self.ssh(&[command]);
        Ok((
            String::from_utf8_lossy(&stdout).into_owned(),
            String::from_utf8_lossy(&stderr).into_owned(),
        ))
    }
}
