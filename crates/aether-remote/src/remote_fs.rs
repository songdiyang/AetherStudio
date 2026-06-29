use std::path::PathBuf;
use std::sync::mpsc;
use std::time::SystemTime;

/// 远程目录条目
#[derive(Clone, Debug)]
pub struct RemoteDirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// 文件系统事件
#[derive(Clone, Debug)]
pub enum FsEvent {
    Created { path: String },
    Modified { path: String },
    Deleted { path: String },
    Renamed { from: String, to: String },
}

/// 远程文件系统结果类型
pub type Result<T> = std::result::Result<T, String>;

/// 远程文件系统抽象 trait
/// 统一SSH、容器等远程环境的文件访问接口
pub trait RemoteFs: Send + Sync {
    /// 读取文件内容
    fn read_file(&self, path: &str) -> Result<Vec<u8>>;

    /// 写入文件内容
    fn write_file(&self, path: &str, content: &[u8]) -> Result<()>;

    /// 列出目录内容
    fn list_dir(&self, path: &str) -> Result<Vec<RemoteDirEntry>>;

    /// 监听文件变更（如果后端支持）
    fn watch(&self, path: &str) -> Result<mpsc::Receiver<FsEvent>>;

    /// 在远程执行命令（默认实现，要求后端覆盖以提供审计和限制）
    fn exec(&self, _command: &str) -> Result<(String, String)> {
        Err("exec 未在此后端实现".to_string())
    }

    /// 执行受限命令（白名单机制）
    /// SEC-R01: 严格命令白名单 + shell 元字符过滤，防止远程命令注入
    /// 注意：仅靠前缀匹配是不安全的（如 "git ; rm -rf /" 会绕过），因此
    ///   1) 提取命令名（首个 token）做精确匹配；
    ///   2) 拒绝任何 shell 元字符以阻断命令串联/替换/重定向。
    fn exec_restricted(&self, command: &str) -> Result<(String, String)> {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return Err("命令为空".to_string());
        }

        // SEC-R02: 拒绝 shell 元字符，防止命令注入
        // 这些字符允许攻击者串联任意命令、做命令替换或重定向输入输出
        const SHELL_METACHARS: &[char] = &[';', '|', '&', '`', '$', '>', '<', '\n', '\r'];
        if trimmed.chars().any(|c| SHELL_METACHARS.contains(&c)) {
            return Err(format!("命令包含禁止的 shell 元字符: {}", command));
        }

        // SEC-R03: 最小化命令白名单
        // 移除所有 shell（bash/sh/zsh 等）、可执行任意代码的解释器（python/node 等）、
        // 容器/编排工具（docker/kubectl 等）、网络工具（curl/wget/ssh 等）、
        // 以及所有 C 库函数名（fork/execve/system/popen 等，它们并非真实 shell 命令）。
        const ALLOWED_COMMANDS: &[&str] = &[
            // 只读文件系统操作
            "ls", "cat", "pwd", "echo", "find", "grep", "head", "tail", "wc", "stat", "file",
            "which", "diff", "sort", "uniq", "tr", "less", "more",
            // 系统信息（只读）
            "uname", "whoami", "id", "ps", "df", "du",
            // 文件操作（必要但需审计）
            "mkdir", "touch", "cp", "mv", "rm", "chmod", "chown",
            // Git 读取操作（专用入口 git_exec 已存在，此处保留基础访问）
            "git", // 压缩/归档
            "tar", "gzip", "gunzip",
        ];

        // 提取命令名（第一个空白分隔的 token），严格匹配白名单
        let cmd_name = trimmed.split_whitespace().next().unwrap_or("");
        if !ALLOWED_COMMANDS.iter().any(|&allowed| allowed == cmd_name) {
            return Err(format!("命令被拒绝（不在白名单中）: {}", command));
        }

        // 记录审计日志
        eprintln!("[AUDIT] exec_restricted: {}", command);
        self.exec(command)
    }

    /// 检查路径是否存在
    fn exists(&self, path: &str) -> Result<bool> {
        // H-41: 使用 list_dir 检查路径存在性，避免读取整个文件
        match self.list_dir(path) {
            Ok(_) => Ok(true),
            Err(_) => {
                // 如果是文件，尝试 list_dir 其父目录
                if let Some(parent) = std::path::Path::new(path).parent() {
                    if let Ok(entries) = self.list_dir(parent.to_str().unwrap_or(".")) {
                        let file_name = std::path::Path::new(path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(path);
                        return Ok(entries.iter().any(|e| e.name == file_name));
                    }
                }
                Ok(false)
            }
        }
    }

    /// 检查路径是否是 Git 仓库
    fn is_git_repo(&self, path: &str) -> Result<bool> {
        // 使用 SFTP 检查 .git 目录是否存在，避免 shell 注入
        let git_dir = format!("{}/.git", path.trim_end_matches('/'));
        match self.list_dir(&git_dir) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// 获取远程 Git 仓库信息
    fn get_git_info(&self, path: &str) -> Result<GitRemoteInfo> {
        // 使用 shell_escape 转义路径，避免命令注入
        let escaped_path = shell_escape::unix::escape(std::borrow::Cow::Borrowed(path));
        // 获取远程 URL
        let (stdout, _) = self.exec(&format!("cd {} && git remote -v", escaped_path))?;
        let mut remote_url = String::new();
        for line in stdout.lines() {
            if line.contains("origin") && line.contains("(fetch)") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    remote_url = parts[1].to_string();
                }
                break;
            }
        }

        // 获取当前分支
        let (stdout, _) =
            self.exec(&format!("cd {} && git branch --show-current", escaped_path))?;
        let branch = stdout.trim().to_string();

        // 检查状态
        let (stdout, _) = self.exec(&format!("cd {} && git status --porcelain", escaped_path))?;
        let has_changes = !stdout.trim().is_empty();

        Ok(GitRemoteInfo {
            remote_url,
            current_branch: branch,
            has_uncommitted_changes: has_changes,
        })
    }

    /// 执行 Git 命令
    fn git_exec(&self, path: &str, git_args: &[&str]) -> Result<(String, String)> {
        let escaped_path = shell_escape::unix::escape(std::borrow::Cow::Borrowed(path));
        let escaped_args: Vec<String> = git_args
            .iter()
            .map(|arg| shell_escape::unix::escape(std::borrow::Cow::Borrowed(arg)).into_owned())
            .collect();
        let cmd = format!("cd {} && git {}", escaped_path, escaped_args.join(" "));
        self.exec(&cmd)
    }
}

/// Git 远程仓库信息
#[derive(Clone, Debug)]
pub struct GitRemoteInfo {
    pub remote_url: String,
    pub current_branch: String,
    pub has_uncommitted_changes: bool,
}

/// 通过 SSH 访问的 Git 仓库
#[derive(Clone, Debug)]
pub struct GitSshRepo {
    pub repo_path: PathBuf,
    pub remote_url: String,
    pub ssh_host: String,
    pub ssh_port: u16,
}

impl GitSshRepo {
    pub fn new(repo_path: PathBuf, remote_url: String, ssh_host: String, ssh_port: u16) -> Self {
        Self {
            repo_path,
            remote_url,
            ssh_host,
            ssh_port,
        }
    }

    /// 解析 Git SSH URL 获取主机信息
    pub fn from_url(url: &str, repo_path: PathBuf) -> Result<Self> {
        // 支持 git@host:repo.git 格式
        if let Some(rest) = url.strip_prefix("git@") {
            if let Some((host, _repo)) = rest.split_once(':') {
                let host_parts: Vec<&str> = host.split(':').collect();
                let ssh_host = host_parts[0].to_string();
                let ssh_port = 22; // 默认端口
                return Ok(Self::new(repo_path, url.to_string(), ssh_host, ssh_port));
            }
        }

        // 支持 ssh://user@host:port/repo.git 格式
        if let Some(rest) = url.strip_prefix("ssh://") {
            let mut parts = rest.split('/');
            let user_host = parts.next().unwrap_or("");
            let repo = parts.next().unwrap_or("");

            let (user, host_port) = user_host.split_once('@').unwrap_or(("", user_host));
            let (host, port) = host_port.split_once(':').unwrap_or((host_port, "22"));

            let ssh_host = host.to_string();
            let ssh_port = port.parse().unwrap_or(22);
            let full_url = format!("ssh://{}@{}/{}", user, host_port, repo);

            return Ok(Self::new(repo_path, full_url, ssh_host, ssh_port));
        }

        Err("无法解析 Git SSH URL".to_string())
    }
}
