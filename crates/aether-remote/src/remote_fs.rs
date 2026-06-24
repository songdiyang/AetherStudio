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

    /// 在远程执行命令
    fn exec(&self, command: &str) -> Result<(String, String)>;

    /// 检查路径是否存在
    fn exists(&self, path: &str) -> Result<bool> {
        match self.read_file(path) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// 检查路径是否是 Git 仓库
    fn is_git_repo(&self, path: &str) -> Result<bool> {
        // 通过检查 .git 目录或文件来判断
        match self.exec(&format!("test -d {}/.git", path)) {
            Ok((_, _)) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// 获取远程 Git 仓库信息
    fn get_git_info(&self, path: &str) -> Result<GitRemoteInfo> {
        // 获取远程 URL
        let (stdout, _) = self.exec(&format!("cd {} && git remote -v", path))?;
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
        let (stdout, _) = self.exec(&format!("cd {} && git branch --show-current", path))?;
        let branch = stdout.trim().to_string();

        // 检查状态
        let (stdout, _) = self.exec(&format!("cd {} && git status --porcelain", path))?;
        let has_changes = !stdout.trim().is_empty();

        Ok(GitRemoteInfo {
            remote_url,
            current_branch: branch,
            has_uncommitted_changes: has_changes,
        })
    }

    /// 执行 Git 命令
    fn git_exec(&self, path: &str, git_args: &[&str]) -> Result<(String, String)> {
        let cmd = format!("cd {} && git {}", path, git_args.join(" "));
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
