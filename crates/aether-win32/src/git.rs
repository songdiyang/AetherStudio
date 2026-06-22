use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Git 文件状态
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitFileStatus {
    Unmodified,     // 未修改
    Modified,       // 已修改
    Added,          // 已暂存
    Deleted,        // 已删除
    Renamed,        // 重命名
    Copied,         // 已复制
    Untracked,      // 未跟踪
    Ignored,        // 已忽略
    Conflict,       // 冲突
}

/// Git 仓库状态
#[derive(Clone, Debug, Default)]
pub struct GitRepository {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub file_status: HashMap<String, GitFileStatus>,
    pub staged_files: Vec<String>,
    pub unstaged_files: Vec<String>,
    pub untracked_files: Vec<String>,
    pub has_changes: bool,
}

impl GitRepository {
    pub fn new() -> Self {
        Self::default()
    }

    /// 检测指定路径是否为 Git 仓库
    pub fn detect(path: &Path) -> Self {
        let mut repo = Self::new();
        
        // 检查 .git 目录是否存在
        let git_dir = path.join(".git");
        if git_dir.exists() {
            repo.is_repo = true;
            repo.branch = Self::get_branch(path);
            let (status_map, staged, unstaged, untracked) = Self::get_status(path);
            repo.file_status = status_map;
            repo.staged_files = staged;
            repo.unstaged_files = unstaged;
            repo.untracked_files = untracked;
            repo.has_changes = !repo.staged_files.is_empty() 
                || !repo.unstaged_files.is_empty() 
                || !repo.untracked_files.is_empty();
        }
        
        repo
    }

    /// 获取当前分支名
    fn get_branch(path: &Path) -> Option<String> {
        let head_path = path.join(".git").join("HEAD");
        if let Ok(content) = std::fs::read_to_string(&head_path) {
            let content = content.trim();
            if content.starts_with("ref: refs/heads/") {
                return Some(content[16..].to_string());
            }
            // 分离 HEAD（detached HEAD）
            return Some(content[..7].to_string());
        }
        None
    }

    /// 获取文件状态（通过 git status --porcelain 解析）
    fn get_status(path: &Path) -> (HashMap<String, GitFileStatus>, Vec<String>, Vec<String>, Vec<String>) {
        let mut status_map = HashMap::new();
        let mut staged = Vec::new();
        let mut unstaged = Vec::new();
        let mut untracked = Vec::new();
        
        if let Ok(output) = Command::new("git")
            .args(&["status", "--porcelain", "-u"])
            .current_dir(path)
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if line.len() >= 3 {
                        let index_status = line.chars().nth(0).unwrap_or(' ');
                        let worktree_status = line.chars().nth(1).unwrap_or(' ');
                        let file_path = &line[3..];
                        
                        let status = match (index_status, worktree_status) {
                            ('A', ' ') | ('A', 'M') | ('A', 'D') => GitFileStatus::Added,
                            ('M', ' ') | ('M', 'M') => GitFileStatus::Modified,
                            ('D', ' ') | ('D', 'D') | (' ', 'D') => GitFileStatus::Deleted,
                            ('R', _) => GitFileStatus::Renamed,
                            ('C', _) => GitFileStatus::Copied,
                            ('?', '?') => GitFileStatus::Untracked,
                            ('U', _) | (_, 'U') => GitFileStatus::Conflict,
                            _ => GitFileStatus::Unmodified,
                        };
                        
                        status_map.insert(file_path.to_string(), status);
                        
                        if index_status != ' ' && index_status != '?' {
                            staged.push(file_path.to_string());
                        }
                        if worktree_status != ' ' && worktree_status != '?' {
                            unstaged.push(file_path.to_string());
                        }
                        if index_status == '?' && worktree_status == '?' {
                            untracked.push(file_path.to_string());
                        }
                    }
                }
            }
        }
        
        (status_map, staged, unstaged, untracked)
    }

    /// 刷新状态
    pub fn refresh(&mut self, path: &Path) {
        *self = Self::detect(path);
    }

    /// 获取文件状态
    pub fn file_status(&self, file: &str) -> GitFileStatus {
        self.file_status.get(file).copied().unwrap_or(GitFileStatus::Unmodified)
    }

    /// 获取状态图标
    pub fn status_icon(status: GitFileStatus) -> &'static str {
        match status {
            GitFileStatus::Modified => "M",
            GitFileStatus::Added => "A",
            GitFileStatus::Deleted => "D",
            GitFileStatus::Untracked => "U",
            GitFileStatus::Conflict => "C",
            GitFileStatus::Renamed => "R",
            GitFileStatus::Copied => "C",
            GitFileStatus::Ignored => "I",
            GitFileStatus::Unmodified => "",
        }
    }

    /// 获取状态颜色（用于UI渲染）
    pub fn status_color(status: GitFileStatus) -> (f32, f32, f32) {
        match status {
            GitFileStatus::Modified => (0.9, 0.7, 0.2),  // 黄色
            GitFileStatus::Added => (0.2, 0.8, 0.3),     // 绿色
            GitFileStatus::Deleted => (0.9, 0.2, 0.2),   // 红色
            GitFileStatus::Untracked => (0.5, 0.5, 0.5), // 灰色
            GitFileStatus::Conflict => (0.9, 0.2, 0.9),  // 紫色
            GitFileStatus::Renamed => (0.2, 0.6, 0.9),   // 蓝色
            GitFileStatus::Copied => (0.2, 0.9, 0.9),    // 青色
            GitFileStatus::Ignored => (0.4, 0.4, 0.4),    // 深灰
            GitFileStatus::Unmodified => (0.0, 0.0, 0.0),  // 黑色（不显示）
        }
    }
}

/// Git 命令执行器
pub struct GitCommand;

impl GitCommand {
    /// 执行 git 命令，返回 (stdout, stderr, success)
    pub fn exec(path: &Path, args: &[&str]) -> (String, String, bool) {
        let output = match Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
        {
            Ok(o) => o,
            Err(e) => return (String::new(), format!("执行 git 命令失败: {}", e), false),
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let success = output.status.success();
        (stdout, stderr, success)
    }

    /// git add <file>
    pub fn add(path: &Path, file: &str) -> Result<String, String> {
        let (stdout, stderr, success) = Self::exec(path, &["add", file]);
        if success {
            Ok(stdout)
        } else {
            Err(stderr)
        }
    }

    /// git add .
    pub fn add_all(path: &Path) -> Result<String, String> {
        let (stdout, stderr, success) = Self::exec(path, &["add", "."]);
        if success {
            Ok(stdout)
        } else {
            Err(stderr)
        }
    }

    /// git reset HEAD <file>
    pub fn unstage(path: &Path, file: &str) -> Result<String, String> {
        let (stdout, stderr, success) = Self::exec(path, &["reset", "HEAD", file]);
        if success {
            Ok(stdout)
        } else {
            Err(stderr)
        }
    }

    /// git commit -m <message>
    pub fn commit(path: &Path, message: &str) -> Result<String, String> {
        let (stdout, stderr, success) = Self::exec(path, &["commit", "-m", message]);
        if success {
            Ok(stdout)
        } else {
            Err(stderr)
        }
    }

    /// git push
    pub fn push(path: &Path) -> Result<String, String> {
        let (stdout, stderr, success) = Self::exec(path, &["push"]);
        if success {
            Ok(stdout)
        } else {
            Err(stderr)
        }
    }

    /// git pull
    pub fn pull(path: &Path) -> Result<String, String> {
        let (stdout, stderr, success) = Self::exec(path, &["pull"]);
        if success {
            Ok(stdout)
        } else {
            Err(stderr)
        }
    }

    /// git fetch
    pub fn fetch(path: &Path) -> Result<String, String> {
        let (stdout, stderr, success) = Self::exec(path, &["fetch"]);
        if success {
            Ok(stdout)
        } else {
            Err(stderr)
        }
    }

    /// git checkout -b <branch>
    pub fn create_branch(path: &Path, branch: &str) -> Result<String, String> {
        let (stdout, stderr, success) = Self::exec(path, &["checkout", "-b", branch]);
        if success {
            Ok(stdout)
        } else {
            Err(stderr)
        }
    }

    /// git checkout <branch>
    pub fn switch_branch(path: &Path, branch: &str) -> Result<String, String> {
        let (stdout, stderr, success) = Self::exec(path, &["checkout", branch]);
        if success {
            Ok(stdout)
        } else {
            Err(stderr)
        }
    }

    /// git branch
    pub fn list_branches(path: &Path) -> Vec<String> {
        let (stdout, _, success) = Self::exec(path, &["branch", "--format=%(refname:short)"]);
        if !success {
            return Vec::new();
        }
        stdout.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    }

    /// git log --oneline -n <count>
    pub fn log(path: &Path, count: usize) -> Vec<String> {
        let count_str = count.to_string();
        let (stdout, _, success) = Self::exec(path, &["log", "--oneline", "-n", &count_str]);
        if !success {
            return Vec::new();
        }
        stdout.lines().map(|s| s.to_string()).collect()
    }

    /// git clone <url> <path>
    pub fn clone(url: &str, path: &Path) -> Result<String, String> {
        let (stdout, stderr, success) = Self::exec(Path::new("."), &["clone", url, path.to_str().unwrap_or(".")]);
        if success {
            Ok(stdout)
        } else {
            Err(stderr)
        }
    }
}

/// Git 集成管理器
pub struct GitIntegration {
    pub repo: GitRepository,
    pub enabled: bool,
    pub current_folder: Option<std::path::PathBuf>,
    /// 上次操作结果
    pub last_result: Option<Result<String, String>>,
    /// 提交消息输入
    pub commit_message: String,
    /// 选中的 Git 文件（用于 diff）
    pub selected_file: Option<String>,
    /// Git 面板滚动偏移
    pub scroll_y: f32,
    /// 鼠标悬停的 Git 文件
    pub hover_file: Option<String>,
    /// 是否显示 diff 视图
    pub show_diff: bool,
    /// diff 内容缓存
    pub diff_content: Option<String>,
    /// Git 面板按钮悬停状态 ("commit", "refresh", "stage_all", "unstage_all")
    pub hover_button: Option<String>,
}

impl GitIntegration {
    pub fn new() -> Self {
        Self {
            repo: GitRepository::new(),
            enabled: true,
            current_folder: None,
            last_result: None,
            commit_message: String::new(),
            selected_file: None,
            scroll_y: 0.0,
            hover_file: None,
            show_diff: false,
            diff_content: None,
            hover_button: None,
        }
    }

    /// 获取当前分支名
    pub fn current_branch_name(&self) -> Option<String> {
        self.repo.branch.clone()
    }

    /// 获取已暂存文件列表（带状态）
    pub fn staged_files(&self) -> Vec<(String, GitFileStatus)> {
        self.repo.staged_files.iter()
            .filter_map(|f| self.repo.file_status.get(f).map(|s| (f.clone(), *s)))
            .collect()
    }

    /// 获取未暂存修改文件列表（带状态）
    pub fn unstaged_files(&self) -> Vec<(String, GitFileStatus)> {
        self.repo.unstaged_files.iter()
            .filter_map(|f| self.repo.file_status.get(f).map(|s| (f.clone(), *s)))
            .collect()
    }

    /// 获取未跟踪文件列表
    pub fn untracked_files(&self) -> Vec<String> {
        self.repo.untracked_files.clone()
    }

    /// 获取指定文件的状态
    pub fn file_status_str(&self, file: &str) -> GitFileStatus {
        self.repo.file_status.get(file).copied().unwrap_or(GitFileStatus::Unmodified)
    }

    /// 检测并初始化 Git 仓库
    pub fn detect(&mut self, path: &Path) {
        self.current_folder = Some(path.to_path_buf());
        self.repo = GitRepository::detect(path);
        self.commit_message.clear();
        self.selected_file = None;
        self.show_diff = false;
        self.diff_content = None;
    }

    /// 刷新状态
    pub fn refresh(&mut self) {
        if let Some(path) = &self.current_folder {
            self.repo = GitRepository::detect(path);
        }
    }

    /// 是否启用了 Git
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 是否在 Git 仓库中
    pub fn is_repo(&self) -> bool {
        self.repo.is_repo
    }

    /// 暂存文件
    pub fn stage_file(&mut self, file: &str) -> Result<String, String> {
        if let Some(path) = &self.current_folder {
            let result = GitCommand::add(path, file);
            self.refresh();
            self.last_result = Some(result.clone());
            result
        } else {
            Err("未打开文件夹".to_string())
        }
    }

    /// 暂存所有文件
    pub fn stage_all(&mut self) -> Result<String, String> {
        if let Some(path) = &self.current_folder {
            let result = GitCommand::add_all(path);
            self.refresh();
            self.last_result = Some(result.clone());
            result
        } else {
            Err("未打开文件夹".to_string())
        }
    }

    /// 取消暂存
    pub fn unstage_file(&mut self, file: &str) -> Result<String, String> {
        if let Some(path) = &self.current_folder {
            let result = GitCommand::unstage(path, file);
            self.refresh();
            self.last_result = Some(result.clone());
            result
        } else {
            Err("未打开文件夹".to_string())
        }
    }

    /// 提交更改
    pub fn commit(&mut self, message: &str) -> Result<String, String> {
        if let Some(path) = &self.current_folder {
            let result = GitCommand::commit(path, message);
            self.refresh();
            self.last_result = Some(result.clone());
            result
        } else {
            Err("未打开文件夹".to_string())
        }
    }

    /// 推送
    pub fn push(&mut self) -> Result<String, String> {
        if let Some(path) = &self.current_folder {
            let result = GitCommand::push(path);
            self.refresh();
            self.last_result = Some(result.clone());
            result
        } else {
            Err("未打开文件夹".to_string())
        }
    }

    /// 拉取
    pub fn pull(&mut self) -> Result<String, String> {
        if let Some(path) = &self.current_folder {
            let result = GitCommand::pull(path);
            self.refresh();
            self.last_result = Some(result.clone());
            result
        } else {
            Err("未打开文件夹".to_string())
        }
    }

    /// 获取分支列表
    pub fn branches(&self) -> Vec<String> {
        if let Some(path) = &self.current_folder {
            GitCommand::list_branches(path)
        } else {
            Vec::new()
        }
    }

    /// 切换分支
    pub fn switch_branch(&mut self, branch: &str) -> Result<String, String> {
        if let Some(path) = &self.current_folder {
            let result = GitCommand::switch_branch(path, branch);
            self.refresh();
            self.last_result = Some(result.clone());
            result
        } else {
            Err("未打开文件夹".to_string())
        }
    }

    /// 创建分支
    pub fn create_branch(&mut self, branch: &str) -> Result<String, String> {
        if let Some(path) = &self.current_folder {
            let result = GitCommand::create_branch(path, branch);
            self.refresh();
            self.last_result = Some(result.clone());
            result
        } else {
            Err("未打开文件夹".to_string())
        }
    }

    /// 克隆仓库
    pub fn clone_repo(url: &str, path: &Path) -> Result<String, String> {
        GitCommand::clone(url, path)
    }
}

impl Default for GitIntegration {
    fn default() -> Self {
        Self::new()
    }
}
