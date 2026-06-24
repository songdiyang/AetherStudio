#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    use crate::{GitRepoConfig, GitRepoType, GitRepository, GitSshRepo, SshAuth, SshConfig};

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

        // 初始化一个 Git 仓库
        git2::Repository::init(&repo_path).unwrap();

        // 打开仓库
        let repo = GitRepository::open(&repo_path);
        assert!(repo.is_ok());
    }

    #[test]
    fn test_git_repository_status() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test_repo");

        // 初始化 Git 仓库
        git2::Repository::init(&repo_path).unwrap();

        // 创建一个文件
        let test_file = repo_path.join("test.txt");
        std::fs::write(&test_file, "test content").unwrap();

        // 打开仓库并检查状态
        let repo = GitRepository::open(&repo_path).unwrap();
        let status = repo.status().unwrap();

        assert!(!status.is_clean);
        assert!(status.untracked_files.len() > 0);
    }

    #[test]
    fn test_git_repository_add_and_commit() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test_repo");

        // 初始化 Git 仓库
        git2::Repository::init(&repo_path).unwrap();

        // 配置用户信息
        let repo = git2::Repository::open(&repo_path).unwrap();
        repo.config()
            .unwrap()
            .set_str("user.name", "Test User")
            .unwrap();
        repo.config()
            .unwrap()
            .set_str("user.email", "test@example.com")
            .unwrap();

        // 创建并添加文件
        let test_file = repo_path.join("test.txt");
        std::fs::write(&test_file, "test content").unwrap();

        let git_repo = GitRepository::open(&repo_path).unwrap();
        git_repo.add("test.txt").unwrap();

        // 提交
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

        // 初始化 Git 仓库
        let repo = git2::Repository::init(&repo_path).unwrap();

        // 配置用户信息
        repo.config()
            .unwrap()
            .set_str("user.name", "Test User")
            .unwrap();
        repo.config()
            .unwrap()
            .set_str("user.email", "test@example.com")
            .unwrap();

        // 创建初始提交
        let test_file = repo_path.join("test.txt");
        std::fs::write(&test_file, "test content").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("test.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();

        let signature = git2::Signature::now("Test User", "test@example.com").unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Initial commit",
            &tree,
            &[],
        )
        .unwrap();

        // 切换并创建新分支
        let git_repo = GitRepository::open(&repo_path).unwrap();
        git_repo.checkout_branch("test-branch", true).unwrap();

        // 列出分支
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

        // 初始化 Git 仓库
        let repo = git2::Repository::init(&repo_path).unwrap();

        // 配置用户信息
        repo.config()
            .unwrap()
            .set_str("user.name", "Test User")
            .unwrap();
        repo.config()
            .unwrap()
            .set_str("user.email", "test@example.com")
            .unwrap();

        // 创建第一个提交
        let test_file1 = repo_path.join("test1.txt");
        std::fs::write(&test_file1, "content 1").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("test1.txt")).unwrap();
        index.write().unwrap();
        let tree_id1 = index.write_tree().unwrap();
        let tree1 = repo.find_tree(tree_id1).unwrap();

        let signature = git2::Signature::now("Test User", "test@example.com").unwrap();
        let commit1 = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "First commit",
                &tree1,
                &[],
            )
            .unwrap();

        // 创建第二个提交
        let test_file2 = repo_path.join("test2.txt");
        std::fs::write(&test_file2, "content 2").unwrap();

        index.add_path(Path::new("test2.txt")).unwrap();
        index.write().unwrap();
        let tree_id2 = index.write_tree().unwrap();
        let tree2 = repo.find_tree(tree_id2).unwrap();

        let commit1_obj = repo.find_commit(commit1).unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Second commit",
            &tree2,
            &[&commit1_obj],
        )
        .unwrap();

        // 获取提交历史
        let git_repo = GitRepository::open(&repo_path).unwrap();
        let commits = git_repo.log(10).unwrap();

        assert!(commits.len() == 2);
        assert_eq!(commits[0].message, "Second commit");
        assert_eq!(commits[1].message, "First commit");
    }
}
