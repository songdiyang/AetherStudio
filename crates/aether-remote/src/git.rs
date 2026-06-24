use git2::{Repository, Signature, Time};
use std::path::{Path, PathBuf};

use crate::remote_fs::Result;
use crate::ssh::SshConfig;

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
    pub ssh_config: Option<SshConfig>,
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
            ssh_config: None,
            local_path: None,
        })
    }

    /// 设置 SSH 配置
    pub fn with_ssh_config(mut self, config: SshConfig) -> Self {
        self.ssh_config = Some(config);
        self
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
        }
    }
}

impl std::error::Error for GitError {}

/// Git 仓库管理器
pub struct GitRepository {
    repo: Repository,
    /// 仓库配置（预留字段，当前未使用）
    #[allow(dead_code)]
    config: GitRepoConfig,
}

impl GitRepository {
    /// 克隆远程仓库
    pub fn clone(url: &str, path: &Path) -> Result<Self> {
        let config = GitRepoConfig::from_url(url)?;

        let repo =
            Repository::clone(url, path).map_err(|e| GitError::CloneFailed(e.to_string()))?;

        Ok(Self { repo, config })
    }

    /// 打开现有仓库
    pub fn open(path: &Path) -> Result<Self> {
        let repo = Repository::open(path).map_err(|e| GitError::InvalidRepo(e.to_string()))?;

        let url = repo
            .remotes()
            .ok()
            .and_then(|remotes| {
                remotes
                    .iter()
                    .nth(0)
                    .and_then(|name| name)
                    .and_then(|name| repo.find_remote(name).ok())
                    .and_then(|remote| Some(remote.url().unwrap_or("").to_string()))
            })
            .unwrap_or_default();

        let config = GitRepoConfig::from_url(&url).unwrap_or_else(|_| GitRepoConfig {
            url,
            repo_type: GitRepoType::Local,
            ssh_config: None,
            local_path: Some(path.to_path_buf()),
        });

        Ok(Self { repo, config })
    }

    /// 获取当前分支名称
    pub fn current_branch(&self) -> Result<String> {
        let head = self.repo.head();

        match head {
            Ok(head) => {
                let shorthand = head.shorthand().ok_or("HEAD 未指向任何分支")?;
                Ok(shorthand.to_string())
            }
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch => {
                // 仓库刚初始化，没有提交，默认返回 master/main
                Ok("master".to_string())
            }
            Err(e) => Err(GitError::StatusFailed(e.to_string()).into()),
        }
    }

    /// 获取仓库状态
    pub fn status(&self) -> Result<GitStatus> {
        let statuses = self
            .repo
            .statuses(None)
            .map_err(|e| GitError::StatusFailed(e.to_string()))?;

        let mut status = GitStatus {
            is_clean: true,
            staged_files: Vec::new(),
            unstaged_files: Vec::new(),
            untracked_files: Vec::new(),
            conflicts: Vec::new(),
            branch: self.current_branch()?,
            ahead_behind: None,
        };

        for entry in statuses.iter() {
            let path = entry.path().unwrap_or("").to_string();
            let flags = entry.status();

            status.is_clean = false;

            if flags.is_index_new() || flags.is_index_modified() || flags.is_index_deleted() {
                status.staged_files.push(path.clone());
            }

            if flags.is_wt_new() {
                status.untracked_files.push(path.clone());
            } else if flags.is_wt_modified() || flags.is_wt_deleted() {
                status.unstaged_files.push(path.clone());
            }

            if flags.is_conflicted() {
                status.conflicts.push(path);
            }
        }

        // 获取领先/落后信息
        if let Ok(local_branch) = self.repo.head() {
            if let Some(local_branch_name) = local_branch.shorthand() {
                if let Ok(local_ref) = self
                    .repo
                    .resolve_reference_from_short_name(local_branch_name)
                {
                    if let Some(target_oid) = local_ref.target() {
                        if let Ok(upstream) = self.repo.branch_upstream_name(local_branch_name) {
                            let upstream_str = std::str::from_utf8(&upstream).map_err(|e| {
                                GitError::StatusFailed(format!("无法解析分支名: {}", e))
                            })?;
                            if let Ok(remote_ref) = self.repo.find_reference(upstream_str) {
                                if let Some(remote_oid) = remote_ref.target() {
                                    if let Ok((ahead, behind)) =
                                        self.repo.graph_ahead_behind(target_oid, remote_oid)
                                    {
                                        status.ahead_behind = Some((ahead, behind));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(status)
    }

    /// 拉取远程更改
    pub fn pull(&self, remote_name: Option<&str>, branch_name: Option<&str>) -> Result<()> {
        let remote = remote_name.unwrap_or("origin");
        let binding = self.current_branch()?;
        let branch = branch_name.unwrap_or(&binding);

        let mut remote_obj = self
            .repo
            .find_remote(remote)
            .map_err(|e| GitError::PullFailed(format!("未找到远程: {}", e)))?;

        remote_obj
            .fetch(&[branch], None, None)
            .map_err(|e| GitError::FetchFailed(e.to_string()))?;

        let fetch_head = self
            .repo
            .find_reference("FETCH_HEAD")
            .map_err(|e| GitError::FetchFailed(e.to_string()))?;

        let fetch_commit = self
            .repo
            .reference_to_annotated_commit(&fetch_head)
            .map_err(|e| GitError::FetchFailed(e.to_string()))?;

        let head = self
            .repo
            .head()
            .map_err(|e| GitError::PullFailed(e.to_string()))?;

        let _head_commit = self
            .repo
            .reference_to_annotated_commit(&head)
            .map_err(|e| GitError::PullFailed(e.to_string()))?;

        let analysis = self
            .repo
            .merge_analysis(&[&fetch_commit])
            .map_err(|e| GitError::PullFailed(e.to_string()))?;

        if analysis.0.is_up_to_date() {
            return Ok(());
        }

        if analysis.0.is_fast_forward() {
            let fetch_commit_obj = self
                .repo
                .find_commit(fetch_commit.id())
                .map_err(|e| GitError::CheckoutFailed(e.to_string()))?;
            let tree = fetch_commit_obj
                .tree()
                .map_err(|e| GitError::CheckoutFailed(e.to_string()))?;

            let tree_obj = tree.as_object();
            self.repo
                .checkout_tree(tree_obj, None)
                .map_err(|e| GitError::CheckoutFailed(e.to_string()))?;

            self.repo
                .head()
                .unwrap()
                .set_target(fetch_commit.id(), "Fast-forward pull")
                .map_err(|e| GitError::PullFailed(e.to_string()))?;
        } else if analysis.0.is_normal() {
            // 执行合并
            let _merge_result = self
                .repo
                .merge(&[&fetch_commit], None, None)
                .map_err(|e| GitError::MergeFailed(e.to_string()))?;

            // 需要提交合并结果
            let signature = self.create_signature()?;
            let head_commit_obj = self.repo.head().unwrap().peel_to_commit().unwrap();
            let fetch_commit_obj = self
                .repo
                .find_commit(fetch_commit.id())
                .map_err(|e| GitError::MergeFailed(e.to_string()))?;

            let mut index = self
                .repo
                .merge_commits(&head_commit_obj, &fetch_commit_obj, None)
                .map_err(|e| GitError::MergeFailed(e.to_string()))?;

            if index.has_conflicts() {
                return Err(GitError::MergeFailed("存在合并冲突，需要手动解决".to_string()).into());
            }

            let tree_oid = index
                .write_tree_to(&self.repo)
                .map_err(|e| GitError::CommitFailed(e.to_string()))?;

            let tree = self
                .repo
                .find_tree(tree_oid)
                .map_err(|e| GitError::CommitFailed(e.to_string()))?;

            self.repo
                .commit(
                    Some("HEAD"),
                    &signature,
                    &signature,
                    &format!("Merge branch '{}' of {}", branch, remote),
                    &tree,
                    &[&head_commit_obj, &fetch_commit_obj],
                )
                .map_err(|e| GitError::CommitFailed(e.to_string()))?;

            self.repo
                .cleanup_state()
                .map_err(|e| GitError::PullFailed(e.to_string()))?;
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
        let remote = remote_name.unwrap_or("origin");
        let binding = self.current_branch()?;
        let branch = branch_name.unwrap_or(&binding);

        let mut remote_obj = self
            .repo
            .find_remote(remote)
            .map_err(|e| GitError::PushFailed(format!("未找到远程: {}", e)))?;

        let refspec = format!(
            "{}refs/heads/{}:refs/heads/{}",
            if force { "+" } else { "" },
            branch,
            branch
        );

        remote_obj
            .push(&[&refspec], None)
            .map_err(|e| GitError::PushFailed(e.to_string()))?;

        Ok(())
    }

    /// 添加文件到暂存区
    pub fn add(&self, pathspec: &str) -> Result<()> {
        let mut index = self
            .repo
            .index()
            .map_err(|e| GitError::StatusFailed(e.to_string()))?;

        index
            .add_path(Path::new(pathspec))
            .map_err(|e| GitError::StatusFailed(e.to_string()))?;

        index
            .write()
            .map_err(|e| GitError::StatusFailed(e.to_string()))?;

        Ok(())
    }

    /// 提交更改
    pub fn commit(&self, message: &str) -> Result<String> {
        let signature = self.create_signature()?;
        let mut index = self
            .repo
            .index()
            .map_err(|e| GitError::CommitFailed(e.to_string()))?;

        let tree_oid = index
            .write_tree()
            .map_err(|e| GitError::CommitFailed(e.to_string()))?;

        let tree = self
            .repo
            .find_tree(tree_oid)
            .map_err(|e| GitError::CommitFailed(e.to_string()))?;

        let parent_commit = self.repo.head().and_then(|head| head.peel_to_commit()).ok();

        let commit_id = if let Some(parent) = parent_commit {
            self.repo
                .commit(
                    Some("HEAD"),
                    &signature,
                    &signature,
                    message,
                    &tree,
                    &[&parent],
                )
                .map_err(|e| GitError::CommitFailed(e.to_string()))?
        } else {
            self.repo
                .commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
                .map_err(|e| GitError::CommitFailed(e.to_string()))?
        };

        Ok(commit_id.to_string())
    }

    /// 创建并切换分支
    pub fn checkout_branch(&self, branch_name: &str, create: bool) -> Result<()> {
        if create {
            let commit = self
                .repo
                .head()
                .and_then(|head| head.peel_to_commit())
                .map_err(|e| GitError::BranchFailed(e.to_string()))?;

            self.repo
                .branch(branch_name, &commit, false)
                .map_err(|e| GitError::BranchFailed(e.to_string()))?;
        }

        let obj = self
            .repo
            .revparse_single(&format!("refs/heads/{}", branch_name))
            .map_err(|e| GitError::CheckoutFailed(e.to_string()))?;

        self.repo
            .checkout_tree(&obj, None)
            .map_err(|e| GitError::CheckoutFailed(e.to_string()))?;

        self.repo
            .set_head(&format!("refs/heads/{}", branch_name))
            .map_err(|e| GitError::CheckoutFailed(e.to_string()))?;

        Ok(())
    }

    /// 列出所有分支
    pub fn list_branches(&self) -> Result<Vec<String>> {
        let mut branches = Vec::new();

        let branches_iter = self
            .repo
            .branches(None)
            .map_err(|e| GitError::StatusFailed(e.to_string()))?;

        for branch in branches_iter {
            if let Ok((branch, _)) = branch {
                if let Some(name) = branch.name().ok().flatten() {
                    branches.push(name.to_string());
                }
            }
        }

        Ok(branches)
    }

    /// 获取提交历史
    pub fn log(&self, max_count: usize) -> Result<Vec<GitCommit>> {
        let mut revwalk = self
            .repo
            .revwalk()
            .map_err(|e| GitError::StatusFailed(e.to_string()))?;

        revwalk
            .push_head()
            .map_err(|e| GitError::StatusFailed(e.to_string()))?;

        let commits: Vec<GitCommit> = revwalk
            .take(max_count)
            .filter_map(|id| id.ok())
            .filter_map(|id| self.repo.find_commit(id).ok())
            .map(|commit| {
                let author = commit.author();
                let message = commit.message().unwrap_or("");
                let summary = message.lines().next().unwrap_or("");

                GitCommit {
                    id: commit.id().to_string(),
                    short_id: commit.id().to_string()[..7].to_string(),
                    message: summary.to_string(),
                    full_message: message.to_string(),
                    author_name: author.name().unwrap_or("").to_string(),
                    author_email: author.email().unwrap_or("").to_string(),
                    time: author.when(),
                }
            })
            .collect();

        Ok(commits)
    }

    /// 创建签名
    fn create_signature(&self) -> Result<Signature<'static>> {
        // 尝试从 Git 配置获取用户信息
        let config = self
            .repo
            .config()
            .map_err(|e| GitError::ConfigError(e.to_string()))?;

        let name = config
            .get_string("user.name")
            .unwrap_or_else(|_| "Unknown".to_string());
        let email = config
            .get_string("user.email")
            .unwrap_or_else(|_| "unknown@example.com".to_string());

        Ok(Signature::now(&name, &email).map_err(|e| GitError::ConfigError(e.to_string()))?)
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
    pub ahead_behind: Option<(usize, usize)>, // (ahead, behind)
}

/// Git 提交信息
#[derive(Clone, Debug)]
pub struct GitCommit {
    pub id: String,
    pub short_id: String,
    pub message: String,
    pub full_message: String,
    pub author_name: String,
    pub author_email: String,
    pub time: Time,
}

impl From<GitError> for String {
    fn from(err: GitError) -> Self {
        err.to_string()
    }
}

/// SSH Git 凭证助手
pub fn setup_ssh_credentials(config: &SshConfig) -> Result<git2::Cred> {
    use git2::Cred;

    match &config.auth {
        crate::ssh::SshAuth::Password(password) => {
            Cred::userpass_plaintext(&config.username, password)
                .map_err(|e| GitError::AuthenticationError(e.to_string()).into())
        }
        crate::ssh::SshAuth::Key { path, passphrase } => Cred::ssh_key_from_agent(&config.username)
            .or_else(|_| {
                Cred::ssh_key(
                    &config.username,
                    None,
                    Path::new(path),
                    passphrase.as_deref(),
                )
            })
            .map_err(|e| GitError::AuthenticationError(e.to_string()).into()),
        crate::ssh::SshAuth::Agent => Cred::ssh_key_from_agent(&config.username)
            .map_err(|e| GitError::AuthenticationError(e.to_string()).into()),
    }
}
