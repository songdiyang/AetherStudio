//! Git 仓库管理(通过 shell out 调用系统 `git` 二进制)
//!
//! 设计取舍:面向 Windows 开发者,默认已安装 Git for Windows。
//! 不依赖 libgit2/git2/gix 等库,编译期零 C 依赖,运行期依赖系统 git。
//! 调用方应先调 [`git_available`] 校验,缺失时引导用户访问 [`GIT_DOWNLOAD_URL`]。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::remote_fs::Result;

/// Git 官方下载页(用户缺失 git 时引导跳转)
pub const GIT_DOWNLOAD_URL: &str = "https://git-scm.com/downloads";

/// 校验系统是否安装 git 并在 PATH 中可调用
///
/// UI 层应在初始化时调用此函数,返回 false 时提示用户安装 git
/// 并跳转 [`GIT_DOWNLOAD_URL`]。
pub fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Git 仓库类型
#[derive(Clone, Debug, PartialEq)]
pub enum GitRepoType {
    Local,
    Ssh,
    Https,
}

/// Git 仓库配置
#[derive(Clone, Debug)]
pub struct GitRepoConfig {
    pub url: String,
    pub repo_type: GitRepoType,
    pub local_path: Option<PathBuf>,
}

impl GitRepoConfig {
    /// 从 URL 解析仓库配置
    pub fn from_url(url: &str) -> Result<Self> {
        let repo_type = if url.starts_with("ssh://") || url.contains("git@") {
            GitRepoType::Ssh
        } else if url.starts_with("https://") {
            GitRepoType::Https
        } else if url.starts_with("/") || url.starts_with("./") || url.starts_with("../") {
            GitRepoType::Local
        } else {
            return Err("无法识别的 Git 仓库 URL 格式".to_string());
        };

        Ok(Self {
            url: url.to_string(),
            repo_type,
            local_path: None,
        })
    }

    /// 设置本地路径
    pub fn with_local_path(mut self, path: PathBuf) -> Self {
        self.local_path = Some(path);
        self
    }
}

/// Git 操作错误
#[derive(Debug)]
pub enum GitError {
    CloneFailed(String),
    PullFailed(String),
    PushFailed(String),
    CheckoutFailed(String),
    CommitFailed(String),
    BranchFailed(String),
    MergeFailed(String),
    FetchFailed(String),
    StatusFailed(String),
    InvalidRepo(String),
    ConfigError(String),
    AuthenticationError(String),
    /// 系统 git 未安装或不在 PATH
    GitNotInstalled,
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::CloneFailed(msg) => write!(f, "克隆失败: {}", msg),
            GitError::PullFailed(msg) => write!(f, "拉取失败: {}", msg),
            GitError::PushFailed(msg) => write!(f, "推送失败: {}", msg),
            GitError::CheckoutFailed(msg) => write!(f, "检出失败: {}", msg),
            GitError::CommitFailed(msg) => write!(f, "提交失败: {}", msg),
            GitError::BranchFailed(msg) => write!(f, "分支操作失败: {}", msg),
            GitError::MergeFailed(msg) => write!(f, "合并失败: {}", msg),
            GitError::FetchFailed(msg) => write!(f, "获取失败: {}", msg),
            GitError::StatusFailed(msg) => write!(f, "状态查询失败: {}", msg),
            GitError::InvalidRepo(msg) => write!(f, "无效仓库: {}", msg),
            GitError::ConfigError(msg) => write!(f, "配置错误: {}", msg),
            GitError::AuthenticationError(msg) => write!(f, "认证失败: {}", msg),
            GitError::GitNotInstalled => write!(
                f,
                "系统未安装 git 或不在 PATH 中,请访问 {} 下载安装",
                GIT_DOWNLOAD_URL
            ),
        }
    }
}

impl std::error::Error for GitError {}

/// Git 仓库管理器(通过系统 git 二进制实现)
pub struct GitRepository {
    /// 工作区路径
    workdir: PathBuf,
    /// 仓库配置
    config: GitRepoConfig,
}

impl GitRepository {
    /// 克隆远程仓库
    ///
    /// SSH 走系统 `ssh` 二进制,自动用 `~/.ssh/config` 和 ssh-agent。
    pub fn clone(url: &str, path: &Path) -> Result<Self> {
        // 校验 URL 格式(from_url 内部完成),config 由 open 重新构造
        let _config = GitRepoConfig::from_url(url)?;

        let (stdout, stderr, ok) = Self::run(
            Path::new("."),
            &["clone", "--", url, path.to_str().unwrap_or(".")],
        );
        if !ok {
            return Err(GitError::CloneFailed(format!(
                "{}\n{}",
                String::from_utf8_lossy(&stderr),
                String::from_utf8_lossy(&stdout)
            ))
            .to_string());
        }

        // 重新打开以构造 GitRepository
        Self::open(path)
    }

    /// 打开现有仓库
    pub fn open(path: &Path) -> Result<Self> {
        // 校验是否为 git 仓库
        let git_dir = path.join(".git");
        if !git_dir.exists() {
            return Err(
                GitError::InvalidRepo(format!("{} 下未找到 .git 目录", path.display())).to_string(),
            );
        }

        // 读取 origin 远程 URL(用于推断 repo_type)
        let (stdout, _, ok) = Self::run(path, &["remote", "get-url", "origin"]);
        let url = if ok {
            String::from_utf8_lossy(&stdout).trim().to_string()
        } else {
            String::new()
        };

        let config = if url.is_empty() {
            GitRepoConfig {
                url: String::new(),
                repo_type: GitRepoType::Local,
                local_path: Some(path.to_path_buf()),
            }
        } else {
            GitRepoConfig::from_url(&url).unwrap_or_else(|_| GitRepoConfig {
                url,
                repo_type: GitRepoType::Local,
                local_path: Some(path.to_path_buf()),
            })
        };

        Ok(Self {
            workdir: path.to_path_buf(),
            config,
        })
    }

    /// 获取当前分支名称
    ///
    /// 用 `git symbolic-ref --short HEAD`,在 unborn HEAD(空仓库)时也能返回当前分支名。
    /// detached HEAD 时 symbolic-ref 失败,返回 "detached"。
    pub fn current_branch(&self) -> Result<String> {
        let (stdout, _, ok) = Self::run(&self.workdir, &["symbolic-ref", "--short", "HEAD"]);
        if ok {
            return Ok(String::from_utf8_lossy(&stdout).trim().to_string());
        }
        // symbolic-ref 失败 = detached HEAD
        Ok("detached".to_string())
    }

    /// 获取仓库状态
    pub fn status(&self) -> Result<GitStatus> {
        let branch = self.current_branch()?;
        let mut status = GitStatus {
            is_clean: true,
            staged_files: Vec::new(),
            unstaged_files: Vec::new(),
            untracked_files: Vec::new(),
            conflicts: Vec::new(),
            branch,
            ahead_behind: None,
        };

        // porcelain v1 -z 格式:每条记录以 NUL 分隔,格式 "XY path"
        let (stdout, stderr, ok) = Self::run(&self.workdir, &["status", "--porcelain=v1", "-z"]);
        if !ok {
            return Err(GitError::StatusFailed(format!(
                "{}\n{}",
                String::from_utf8_lossy(&stderr),
                String::from_utf8_lossy(&stdout)
            ))
            .to_string());
        }

        let stdout = String::from_utf8_lossy(&stdout);
        for entry in stdout.split('\0') {
            if entry.is_empty() || entry.len() < 3 {
                continue;
            }

            let x = entry.as_bytes()[0] as char;
            let y = entry.as_bytes()[1] as char;
            let path = entry[3..].to_string();

            status.is_clean = false;

            if x == '?' && y == '?' {
                status.untracked_files.push(path);
            } else if x == '!' && y == '!' {
                // ignored, skip
            } else {
                let is_conflict = x == 'U'
                    || y == 'U'
                    || (x == 'A' && y == 'A')
                    || (x == 'D' && y == 'D')
                    || (x == 'U' && y == 'U');

                if is_conflict {
                    status.conflicts.push(path);
                } else {
                    if x != ' ' && x != '?' {
                        status.staged_files.push(path.clone());
                    }
                    if y != ' ' && y != '?' {
                        status.unstaged_files.push(path);
                    }
                }
            }
        }

        Ok(status)
    }

    /// 添加文件到暂存区
    pub fn add(&self, pathspec: &str) -> Result<()> {
        let (stdout, stderr, ok) = Self::run(&self.workdir, &["add", "--", pathspec]);
        if !ok {
            return Err(GitError::StatusFailed(format!(
                "{}\n{}",
                String::from_utf8_lossy(&stderr),
                String::from_utf8_lossy(&stdout)
            ))
            .to_string());
        }
        Ok(())
    }

    /// 提交更改,返回新 commit 的 full hash
    pub fn commit(&self, message: &str) -> Result<String> {
        let (stdout, stderr, ok) = Self::run(&self.workdir, &["commit", "-m", message]);
        if !ok {
            return Err(GitError::CommitFailed(format!(
                "{}\n{}",
                String::from_utf8_lossy(&stderr),
                String::from_utf8_lossy(&stdout)
            ))
            .to_string());
        }

        // 取新 commit 的 full hash
        let (stdout, stderr, ok) = Self::run(&self.workdir, &["rev-parse", "HEAD"]);
        if !ok {
            return Err(GitError::CommitFailed(format!(
                "rev-parse HEAD 失败: {}\n{}",
                String::from_utf8_lossy(&stderr),
                String::from_utf8_lossy(&stdout)
            ))
            .to_string());
        }
        Ok(String::from_utf8_lossy(&stdout).trim().to_string())
    }

    /// 创建并切换分支(create=true)或切换已有分支(create=false)
    pub fn checkout_branch(&self, branch_name: &str, create: bool) -> Result<()> {
        let args: Vec<&str> = if create {
            vec!["checkout", "-b", branch_name]
        } else {
            vec!["checkout", branch_name]
        };
        let (stdout, stderr, ok) = Self::run(&self.workdir, &args);
        if !ok {
            return Err(GitError::CheckoutFailed(format!(
                "{}\n{}",
                String::from_utf8_lossy(&stderr),
                String::from_utf8_lossy(&stdout)
            ))
            .to_string());
        }
        Ok(())
    }

    /// 列出所有本地分支
    pub fn list_branches(&self) -> Result<Vec<String>> {
        let (stdout, stderr, ok) =
            Self::run(&self.workdir, &["branch", "--format=%(refname:short)"]);
        if !ok {
            return Err(GitError::BranchFailed(format!(
                "{}\n{}",
                String::from_utf8_lossy(&stderr),
                String::from_utf8_lossy(&stdout)
            ))
            .to_string());
        }
        let branches = String::from_utf8_lossy(&stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Ok(branches)
    }

    /// 获取提交历史
    ///
    /// 格式:`%H%x00%h%x00%an%x00%ae%x00%at%x00%s%x00%B%x1e`
    /// - %H = full hash, %h = short hash
    /// - %an = author name, %ae = author email
    /// - %at = author unix timestamp
    /// - %s = subject (summary)
    /// - %B = raw body (full message)
    /// - %x00 = NUL 字段分隔, %x1e = RS 记录分隔
    pub fn log(&self, max_count: usize) -> Result<Vec<GitCommit>> {
        let max_str = max_count.to_string();
        let format = "%H%x00%h%x00%an%x00%ae%x00%at%x00%s%x00%B%x1e";
        let (stdout, stderr, ok) = Self::run(
            &self.workdir,
            &["log", "-n", &max_str, &format!("--format={}", format)],
        );
        if !ok {
            // 空仓库(无 HEAD 提交)返回空列表,不算错误
            let err = String::from_utf8_lossy(&stderr);
            if err.contains("does not have any commits") || err.contains("unknown revision") {
                return Ok(Vec::new());
            }
            return Err(GitError::StatusFailed(format!(
                "{}\n{}",
                err,
                String::from_utf8_lossy(&stdout)
            ))
            .to_string());
        }

        let stdout = String::from_utf8_lossy(&stdout);
        let mut commits = Vec::new();
        for record in stdout.split('\x1e') {
            let record = record.trim_matches('\n');
            if record.is_empty() {
                continue;
            }
            let fields: Vec<&str> = record.splitn(7, '\0').collect();
            if fields.len() < 7 {
                continue;
            }
            commits.push(GitCommit {
                id: fields[0].to_string(),
                short_id: fields[1].to_string(),
                author_name: fields[2].to_string(),
                author_email: fields[3].to_string(),
                time: fields[4].parse::<i64>().unwrap_or(0),
                message: fields[5].to_string(),
                full_message: fields[6].trim_end().to_string(),
            });
        }
        Ok(commits)
    }

    /// 拉取远程更改(fast-forward only)
    ///
    /// 非 fast-forward 时返回错误,符合"normal merge should return error for manual handling"。
    pub fn pull(&self, remote_name: Option<&str>, branch_name: Option<&str>) -> Result<()> {
        let mut args = vec!["pull", "--ff-only"];
        if let Some(r) = remote_name {
            args.push(r);
        }
        if let Some(b) = branch_name {
            args.push(b);
        }
        let (stdout, stderr, ok) = Self::run(&self.workdir, &args);
        if !ok {
            return Err(GitError::PullFailed(format!(
                "{}\n{}",
                String::from_utf8_lossy(&stderr),
                String::from_utf8_lossy(&stdout)
            ))
            .to_string());
        }
        Ok(())
    }

    /// 推送本地更改
    pub fn push(
        &self,
        remote_name: Option<&str>,
        branch_name: Option<&str>,
        force: bool,
    ) -> Result<()> {
        let mut args = vec!["push"];
        if force {
            args.push("--force");
        }
        if let Some(r) = remote_name {
            args.push(r);
        }
        if let Some(b) = branch_name {
            args.push(b);
        }
        let (stdout, stderr, ok) = Self::run(&self.workdir, &args);
        if !ok {
            return Err(GitError::PushFailed(format!(
                "{}\n{}",
                String::from_utf8_lossy(&stderr),
                String::from_utf8_lossy(&stdout)
            ))
            .to_string());
        }
        Ok(())
    }

    /// 在 workdir 下执行 git 命令,返回 (stdout, stderr, success)
    fn run(workdir: &Path, args: &[&str]) -> (Vec<u8>, Vec<u8>, bool) {
        match Command::new("git").current_dir(workdir).args(args).output() {
            Ok(o) => (o.stdout, o.stderr, o.status.success()),
            Err(e) => (
                Vec::new(),
                format!("执行 git 命令失败: {}", e).into_bytes(),
                false,
            ),
        }
    }

    /// 获取工作区路径
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// 获取配置
    pub fn config(&self) -> &GitRepoConfig {
        &self.config
    }
}

/// Git 仓库状态
#[derive(Clone, Debug)]
pub struct GitStatus {
    pub is_clean: bool,
    pub staged_files: Vec<String>,
    pub unstaged_files: Vec<String>,
    pub untracked_files: Vec<String>,
    pub conflicts: Vec<String>,
    pub branch: String,
    pub ahead_behind: Option<(u32, u32)>,
}

/// Git 提交记录
#[derive(Clone, Debug)]
pub struct GitCommit {
    pub id: String,
    pub short_id: String,
    pub message: String,
    pub full_message: String,
    pub author_name: String,
    pub author_email: String,
    /// Unix 时间戳(秒)
    pub time: i64,
}
