#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use tempfile::TempDir;

    use crate::{GitRepoConfig, GitRepoType, GitRepository, GitSshRepo, SshAuth, SshConfig};

    /// 在指定路径执行 git 命令(测试 fixture 用,不依赖 git2)
    fn git(repo_path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(repo_path)
            .args(args)
            .output()
            .expect("git 命令执行失败");
        if !output.status.success() {
            panic!(
                "git {} 失败: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    /// 初始化仓库 + 配置 user.name/email(避免 commit 时报错)
    fn init_repo(repo_path: &Path) {
        // current_dir 要求路径存在,先创建目录
        std::fs::create_dir_all(repo_path).unwrap();
        git(repo_path, &["init"]);
        git(repo_path, &["config", "user.name", "Test User"]);
        git(repo_path, &["config", "user.email", "test@example.com"]);
    }

    /// 创建初始提交(fixture 用)
    fn initial_commit(repo_path: &Path, filename: &str, content: &str, message: &str) {
        std::fs::write(repo_path.join(filename), content).unwrap();
        git(repo_path, &["add", filename]);
        git(repo_path, &["commit", "-m", message]);
    }

    // Git 仓库测试
    #[test]
    fn test_git_repo_config_from_url() {
        // 测试 HTTPS URL
        let config = GitRepoConfig::from_url("https://github.com/user/repo.git").unwrap();
        assert_eq!(config.repo_type, GitRepoType::Https);
        assert_eq!(config.url, "https://github.com/user/repo.git");

        // 测试 SSH URL
        let config = GitRepoConfig::from_url("ssh://git@github.com:user/repo.git").unwrap();
        assert_eq!(config.repo_type, GitRepoType::Ssh);

        // 测试 Git SSH URL
        let config = GitRepoConfig::from_url("git@github.com:user/repo.git").unwrap();
        assert_eq!(config.repo_type, GitRepoType::Ssh);

        // 测试本地路径
        let config = GitRepoConfig::from_url("./local/repo").unwrap();
        assert_eq!(config.repo_type, GitRepoType::Local);
    }

    #[test]
    fn test_git_repository_creation() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test_repo");

        // 初始化一个 Git 仓库(用 git CLI,不依赖 git2)
        init_repo(&repo_path);

        // 打开仓库
        let repo = GitRepository::open(&repo_path);
        assert!(repo.is_ok());
    }

    #[test]
    fn test_git_repository_status() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test_repo");

        // 初始化 Git 仓库
        init_repo(&repo_path);

        // 创建一个文件(未跟踪)
        let test_file = repo_path.join("test.txt");
        std::fs::write(&test_file, "test content").unwrap();

        // 打开仓库并检查状态
        let repo = GitRepository::open(&repo_path).unwrap();
        let status = repo.status().unwrap();

        assert!(!status.is_clean);
        assert!(!status.untracked_files.is_empty());
    }

    #[test]
    fn test_git_repository_add_and_commit() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test_repo");

        // 初始化 Git 仓库(含 user.name/email 配置)
        init_repo(&repo_path);

        // 创建并添加文件
        let test_file = repo_path.join("test.txt");
        std::fs::write(&test_file, "test content").unwrap();

        let git_repo = GitRepository::open(&repo_path).unwrap();
        git_repo.add("test.txt").unwrap();

        // 提交(走 gix commit API)
        let commit_id = git_repo.commit("Initial commit").unwrap();
        assert!(!commit_id.is_empty());

        // 检查状态是否干净
        let status = git_repo.status().unwrap();
        assert!(status.is_clean);
    }

    #[test]
    fn test_git_repository_branches() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test_repo");

        // 初始化 + 创建初始提交(fixture)
        init_repo(&repo_path);
        initial_commit(&repo_path, "test.txt", "test content", "Initial commit");

        // 切换并创建新分支(走我们的 API)
        let git_repo = GitRepository::open(&repo_path).unwrap();
        git_repo.checkout_branch("test-branch", true).unwrap();

        // 列出分支(走 gix references API)
        let branches = git_repo.list_branches().unwrap();
        assert!(branches.contains(&"test-branch".to_string()));
    }

    // SSH 配置测试
    #[test]
    fn test_ssh_config_default() {
        let config = SshConfig::default();
        assert_eq!(config.port, 22);
        assert!(config.host.is_empty());
        assert!(config.username.is_empty());
        assert!(matches!(config.auth, SshAuth::Agent));
    }

    #[test]
    fn test_ssh_config_with_password() {
        let config = SshConfig {
            host: "example.com".to_string(),
            port: 22,
            username: "user".to_string(),
            auth: SshAuth::Password("password".to_string()),
        };

        assert_eq!(config.host, "example.com");
        assert_eq!(config.port, 22);
        assert_eq!(config.username, "user");
    }

    // Git URL 解析测试
    #[test]
    fn test_git_ssh_url_parsing() {
        let repo_path = PathBuf::from("/tmp/test_repo");

        // 测试 git@host:repo.git 格式
        let ssh_repo =
            GitSshRepo::from_url("git@github.com:user/repo.git", repo_path.clone()).unwrap();
        assert_eq!(ssh_repo.ssh_host, "github.com");
        assert_eq!(ssh_repo.ssh_port, 22);

        // 测试 ssh://user@host:port/repo.git 格式
        let ssh_repo =
            GitSshRepo::from_url("ssh://git@github.com:22/user/repo.git", repo_path.clone())
                .unwrap();
        assert_eq!(ssh_repo.ssh_host, "github.com");
        assert_eq!(ssh_repo.ssh_port, 22);

        // 测试无效 URL
        let result = GitSshRepo::from_url("https://github.com/user/repo.git", repo_path);
        assert!(result.is_err());
    }

    // Git 提交历史测试
    #[test]
    fn test_git_log() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test_repo");

        // 初始化 + 创建两个提交(fixture)
        init_repo(&repo_path);
        initial_commit(&repo_path, "test1.txt", "content 1", "First commit");
        initial_commit(&repo_path, "test2.txt", "content 2", "Second commit");

        // 获取提交历史(走 gix rev_walk API)
        let git_repo = GitRepository::open(&repo_path).unwrap();
        let commits = git_repo.log(10).unwrap();

        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].message, "Second commit");
        assert_eq!(commits[1].message, "First commit");
    }
}
