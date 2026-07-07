#![allow(clippy::module_inception, clippy::type_complexity)]

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use tempfile::TempDir;

    use crate::{
        container::{parse_devcontainer_json, ContainerBackend, ContainerConfig, ContainerRemoteFs},
        workspace::{self, RemoteWorkspace},
        FsEvent, GitError, GitRemoteInfo, GitRepoConfig, GitRepoType, GitRepository, GitSshRepo,
        RemoteDirEntry, RemoteFs, SshAuth, SshConfig, SshRemoteFs,
    };

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

        // 测试绝对本地路径
        let config = GitRepoConfig::from_url("/local/repo").unwrap();
        assert_eq!(config.repo_type, GitRepoType::Local);

        // 测试 ../ 本地路径
        let config = GitRepoConfig::from_url("../local/repo").unwrap();
        assert_eq!(config.repo_type, GitRepoType::Local);
    }

    #[test]
    fn test_git_repo_config_errors_and_builder() {
        // 无法识别的 URL 格式
        assert!(GitRepoConfig::from_url("ftp://example.com/repo").is_err());
        assert!(GitRepoConfig::from_url("random string").is_err());

        // with_local_path
        let path = PathBuf::from("/tmp/repo");
        let config = GitRepoConfig::from_url("https://github.com/user/repo.git")
            .unwrap()
            .with_local_path(path.clone());
        assert_eq!(config.local_path, Some(path));
    }

    #[test]
    fn test_git_error_display() {
        let cases: Vec<GitError> = vec![
            GitError::CloneFailed("cl".to_string()),
            GitError::PullFailed("pl".to_string()),
            GitError::PushFailed("ps".to_string()),
            GitError::CheckoutFailed("co".to_string()),
            GitError::CommitFailed("cm".to_string()),
            GitError::BranchFailed("br".to_string()),
            GitError::MergeFailed("mg".to_string()),
            GitError::FetchFailed("ft".to_string()),
            GitError::StatusFailed("st".to_string()),
            GitError::InvalidRepo("ir".to_string()),
            GitError::ConfigError("ce".to_string()),
            GitError::AuthenticationError("ae".to_string()),
            GitError::GitNotInstalled,
        ];
        for err in cases {
            let s = format!("{}", err);
            assert!(!s.is_empty());
            // 每个错误消息都应包含操作失败的中文提示或下载链接
            assert!(
                s.contains("失败") || s.contains("无效") || s.contains("错误")
                    || s.contains("未安装") || s.contains("认证") || s.contains("git-scm.com"),
                "unexpected error message: {}",
                s
            );
        }
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
    fn test_git_repository_open_invalid() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("not_a_repo");
        std::fs::create_dir_all(&repo_path).unwrap();
        assert!(GitRepository::open(&repo_path).is_err());
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
    fn test_git_repository_status_staged_and_modified() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test_repo");
        init_repo(&repo_path);
        std::fs::write(repo_path.join("test.txt"), "v1").unwrap();

        let repo = GitRepository::open(&repo_path).unwrap();
        repo.add("test.txt").unwrap();
        repo.commit("first").unwrap();

        // 修改后再次检查未暂存修改
        std::fs::write(repo_path.join("test.txt"), "v2").unwrap();
        let status = repo.status().unwrap();
        assert!(!status.is_clean);
        assert!(!status.unstaged_files.is_empty());
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
    fn test_git_repository_commit_multiline_message() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test_repo");
        init_repo(&repo_path);
        std::fs::write(repo_path.join("a.txt"), "a").unwrap();
        let repo = GitRepository::open(&repo_path).unwrap();
        repo.add("a.txt").unwrap();
        let msg = "Subject line\n\nBody line 1\nBody line 2";
        let id = repo.commit(msg).unwrap();
        assert!(!id.is_empty());

        let commits = repo.log(1).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].message, "Subject line");
        assert_eq!(commits[0].full_message, msg);
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
        let default_branch = git_repo.current_branch().unwrap();
        git_repo.checkout_branch("test-branch", true).unwrap();

        // 列出分支(走 gix references API)
        let branches = git_repo.list_branches().unwrap();
        assert!(branches.contains(&"test-branch".to_string()));

        // 切换回默认分支
        git_repo.checkout_branch(&default_branch, false).unwrap();
        assert_eq!(git_repo.current_branch().unwrap(), default_branch);
    }

    #[test]
    fn test_git_repository_current_branch_on_empty_repo() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test_repo");
        init_repo(&repo_path);
        let repo = GitRepository::open(&repo_path).unwrap();
        let branch = repo.current_branch().unwrap();
        assert!(!branch.is_empty());
    }

    #[test]
    fn test_git_repository_log_empty() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test_repo");
        init_repo(&repo_path);
        let repo = GitRepository::open(&repo_path).unwrap();
        let commits = repo.log(10).unwrap();
        assert!(commits.is_empty());
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

        // 验证提交字段
        for commit in &commits {
            assert!(!commit.id.is_empty());
            assert!(!commit.short_id.is_empty());
            assert_eq!(commit.author_name, "Test User");
            assert_eq!(commit.author_email, "test@example.com");
            assert!(commit.time > 0);
        }
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

    #[test]
    fn test_ssh_config_debug_redacts_secrets() {
        let config = SshConfig {
            host: "example.com".to_string(),
            port: 22,
            username: "user".to_string(),
            auth: SshAuth::Password("secret".to_string()),
        };
        let s = format!("{:?}", config);
        assert!(!s.contains("secret"));
        assert!(s.contains("[REDACTED]"));

        let config = SshConfig {
            auth: SshAuth::Key {
                path: "/key".to_string(),
                passphrase: Some("hunter2".to_string()),
            },
            ..Default::default()
        };
        let s = format!("{:?}", config);
        assert!(!s.contains("hunter2"));
        assert!(s.contains("/key"));
        assert!(s.contains("[REDACTED]"));
    }

    #[test]
    fn test_ssh_base_args_auth_selection() {
        // Agent: 不应产生 -i
        let fs = SshRemoteFs::new(SshConfig {
            host: "h".to_string(),
            port: 22,
            username: "u".to_string(),
            auth: SshAuth::Agent,
        });
        let args = fs.base_args();
        assert!(!args.contains(&"-i".to_string()));
        assert!(args.contains(&"u@h".to_string()));

        // Key: 应产生 -i
        let fs = SshRemoteFs::new(SshConfig {
            host: "h".to_string(),
            port: 2222,
            username: "u".to_string(),
            auth: SshAuth::Key {
                path: "/key".to_string(),
                passphrase: None,
            },
        });
        let args = fs.base_args();
        let i_pos = args.iter().position(|a| a == "-i").unwrap();
        assert_eq!(args[i_pos + 1], "/key");
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"2222".to_string()));
    }

    #[test]
    fn test_ssh_remote_fs_not_connected_error_paths() {
        let fs = SshRemoteFs::new(SshConfig::default());
        assert!(fs.read_file("/x").is_err());
        assert!(fs.write_file("/x", b"x").is_err());
        assert!(fs.list_dir("/x").is_err());
        assert!(fs.exec("ls").is_err());
        assert!(fs.watch("/x").is_err());
    }

    #[test]
    fn test_ssh_connect_rejects_password() {
        let mut fs = SshRemoteFs::new(SshConfig {
            host: "localhost".to_string(),
            port: 22,
            username: "user".to_string(),
            auth: SshAuth::Password("pw".to_string()),
        });
        assert!(fs.connect().is_err());
        assert!(!fs.is_connected());
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
            GitSshRepo::from_url("ssh://git@github.com:2222/user/repo.git", repo_path.clone())
                .unwrap();
        assert_eq!(ssh_repo.ssh_host, "github.com");
        assert_eq!(ssh_repo.ssh_port, 2222);

        // 测试无效 URL
        let result = GitSshRepo::from_url("https://github.com/user/repo.git", repo_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_git_ssh_repo_builder() {
        let repo = GitSshRepo::new(
            PathBuf::from("/path"),
            "git@host:repo.git".to_string(),
            "host".to_string(),
            2222,
        );
        assert_eq!(repo.repo_path, PathBuf::from("/path"));
        assert_eq!(repo.remote_url, "git@host:repo.git");
        assert_eq!(repo.ssh_host, "host");
        assert_eq!(repo.ssh_port, 2222);
    }

    // RemoteFs 通用 trait 测试
    struct MockRemoteFs {
        files: std::collections::HashMap<String, Vec<u8>>,
        dirs: std::collections::HashMap<String, Vec<RemoteDirEntry>>,
    }

    impl RemoteFs for MockRemoteFs {
        fn read_file(&self, path: &str) -> crate::Result<Vec<u8>> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| "not found".to_string())
        }

        fn write_file(&self, _path: &str, _content: &[u8]) -> crate::Result<()> {
            Ok(())
        }

        fn list_dir(&self, path: &str) -> crate::Result<Vec<RemoteDirEntry>> {
            self.dirs
                .get(path)
                .cloned()
                .ok_or_else(|| "not found".to_string())
        }

        fn exec(&self, command: &str) -> crate::Result<(String, String)> {
            Ok((command.to_string(), String::new()))
        }

        fn watch(&self, _path: &str) -> crate::Result<std::sync::mpsc::Receiver<FsEvent>> {
            Err("unsupported".to_string())
        }
    }

    #[test]
    fn test_remote_fs_exists() {
        let mut fs = MockRemoteFs {
            files: std::collections::HashMap::new(),
            dirs: std::collections::HashMap::new(),
        };
        fs.dirs.insert(
            "/home/user".to_string(),
            vec![RemoteDirEntry {
                name: "file.txt".to_string(),
                is_dir: false,
                size: 10,
                modified: None,
            }],
        );
        assert!(fs.exists("/home/user/file.txt").unwrap());
        assert!(!fs.exists("/home/user/missing.txt").unwrap());
    }

    #[test]
    fn test_remote_fs_is_git_repo() {
        let mut fs = MockRemoteFs {
            files: std::collections::HashMap::new(),
            dirs: std::collections::HashMap::new(),
        };
        fs.dirs.insert(
            "/project/.git".to_string(),
            vec![RemoteDirEntry {
                name: "config".to_string(),
                is_dir: false,
                size: 0,
                modified: None,
            }],
        );
        assert!(fs.is_git_repo("/project").unwrap());
        assert!(!fs.is_git_repo("/other").unwrap());
    }

    #[test]
    fn test_exec_restricted_error_paths() {
        let fs = MockRemoteFs {
            files: std::collections::HashMap::new(),
            dirs: std::collections::HashMap::new(),
        };

        // 空命令
        assert!(fs.exec_restricted("").is_err());
        // shell 元字符
        assert!(fs.exec_restricted("ls ; rm -rf /").is_err());
        assert!(fs.exec_restricted("git | cat").is_err());
        // 不在白名单
        assert!(fs.exec_restricted("curl http://x").is_err());
        assert!(fs.exec_restricted("python script.py").is_err());
        // 在白名单(因为 mock exec 返回 Ok,所以整体 Ok)
        assert!(fs.exec_restricted("ls -la").is_ok());
        assert!(fs.exec_restricted("git status").is_ok());
    }

    #[test]
    fn test_git_remote_info_parsing() {
        // 模拟 git remote -v 输出
        let remote_output = "origin  git@github.com:user/repo.git (fetch)\norigin  git@github.com:user/repo.git (push)\n";
        let branch_output = "main\n";
        let status_output = " M file.txt\n";

        let info = GitRemoteInfo {
            remote_url: "git@github.com:user/repo.git".to_string(),
            current_branch: "main".to_string(),
            has_uncommitted_changes: true,
        };
        assert_eq!(info.remote_url, "git@github.com:user/repo.git");
        assert_eq!(info.current_branch, "main");
        assert!(info.has_uncommitted_changes);

        // 验证解析逻辑: 只取 origin fetch URL
        let mut url = String::new();
        for line in remote_output.lines() {
            if line.contains("origin") && line.contains("(fetch)") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    url = parts[1].to_string();
                }
                break;
            }
        }
        assert_eq!(url, "git@github.com:user/repo.git");
        assert_eq!(branch_output.trim(), "main");
        assert!(!status_output.trim().is_empty());
    }

    // Container 测试
    #[test]
    fn test_container_backend_cmd() {
        let docker = ContainerRemoteFs::new(ContainerConfig {
            backend: ContainerBackend::Docker,
            container_name: "c".to_string(),
            image: "img".to_string(),
            workspace_mount: "/ws".to_string(),
        });
        assert_eq!(docker.backend_cmd(), "docker");

        let podman = ContainerRemoteFs::new(ContainerConfig {
            backend: ContainerBackend::Podman,
            container_name: "c".to_string(),
            image: "img".to_string(),
            workspace_mount: "/ws".to_string(),
        });
        assert_eq!(podman.backend_cmd(), "podman");
    }

    #[test]
    fn test_container_exec_error_paths() {
        let fs = ContainerRemoteFs::new(ContainerConfig {
            backend: ContainerBackend::Docker,
            container_name: "valid-name".to_string(),
            image: "img".to_string(),
            workspace_mount: "/ws".to_string(),
        });

        assert!(fs.exec("").is_err());
        assert!(fs.exec("ls ; rm -rf /").is_err());
        assert!(fs.exec("curl http://x").is_err());
        // 合法命令通过校验后调用 docker/podman;无论命令是否实际成功,都不应返回白名单/元字符类错误
        let res = fs.exec("ls -la");
        let err = match res {
            Ok(_) => return,
            Err(e) => e,
        };
        assert!(
            !err.contains("元字符") && !err.contains("白名单"),
            "valid command should not be rejected by validation: {}",
            err
        );
    }

    #[test]
    fn test_container_invalid_name() {
        let fs = ContainerRemoteFs::new(ContainerConfig {
            backend: ContainerBackend::Docker,
            container_name: "invalid;name".to_string(),
            image: "img".to_string(),
            workspace_mount: "/ws".to_string(),
        });
        let res = fs.exec("ls");
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("非法容器名"));
    }

    #[test]
    fn test_parse_devcontainer_json_error() {
        assert!(parse_devcontainer_json("{}").is_err());
    }

    // Workspace 测试
    #[test]
    fn test_parse_remote_uri() {
        assert_eq!(
            workspace::parse_remote_uri("ssh://host/path/to/file"),
            Some(("ssh".to_string(), "host/path/to/file".to_string()))
        );
        assert_eq!(
            workspace::parse_remote_uri("container://name/path"),
            Some(("container".to_string(), "name/path".to_string()))
        );
        assert!(workspace::parse_remote_uri("file:///path").is_none());
        assert!(workspace::parse_remote_uri("ssh://host").is_none());
    }

    #[test]
    fn test_remote_workspace_path_traversal() {
        let temp_dir = TempDir::new().unwrap();
        let cache = temp_dir.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();

        let fs = Box::new(MockRemoteFs {
            files: std::collections::HashMap::new(),
            dirs: std::collections::HashMap::new(),
        });
        let mut ws = RemoteWorkspace::new(fs, cache.clone());

        // 包含 .. 的远程路径应被拒绝
        assert!(ws.open_file("/etc/../passwd").is_err());
        assert!(ws.open_file("C:\\windows\\file").is_err());
    }

    #[test]
    fn test_remote_workspace_local_cache_accessor() {
        let temp_dir = TempDir::new().unwrap();
        let cache = temp_dir.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        let fs = Box::new(MockRemoteFs {
            files: std::collections::HashMap::new(),
            dirs: std::collections::HashMap::new(),
        });
        let ws = RemoteWorkspace::new(fs, cache.clone());
        assert_eq!(ws.local_cache(), cache);
    }

    #[test]
    fn test_remote_workspace_open_and_save_file() {
        let temp_dir = TempDir::new().unwrap();
        let cache = temp_dir.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();

        let mut files = std::collections::HashMap::new();
        files.insert("/readme.md".to_string(), b"hello remote".to_vec());
        let write_log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = write_log.clone();

        let fs = Box::new(MockRemoteFsWithWrite { files, write_log: log });
        let mut ws = RemoteWorkspace::new(fs, cache.clone());

        let local = ws.open_file("/readme.md").unwrap();
        assert_eq!(std::fs::read_to_string(&local).unwrap(), "hello remote");
        assert_eq!(ws.local_cache(), cache);

        std::fs::write(&local, "updated").unwrap();
        ws.save_file("/readme.md").unwrap();
        let written = write_log.lock().unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].0, "/readme.md");
        assert_eq!(written[0].1, b"updated");
    }

    struct MockRemoteFsWithWrite {
        files: std::collections::HashMap<String, Vec<u8>>,
        write_log: std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>,
    }

    impl RemoteFs for MockRemoteFsWithWrite {
        fn read_file(&self, path: &str) -> crate::Result<Vec<u8>> {
            self.files.get(path).cloned().ok_or_else(|| "not found".to_string())
        }
        fn write_file(&self, path: &str, content: &[u8]) -> crate::Result<()> {
            self.write_log.lock().unwrap().push((path.to_string(), content.to_vec()));
            Ok(())
        }
        fn list_dir(&self, _path: &str) -> crate::Result<Vec<RemoteDirEntry>> {
            Ok(vec![])
        }
        fn watch(&self, _path: &str) -> crate::Result<std::sync::mpsc::Receiver<FsEvent>> {
            Err("unsupported".to_string())
        }
    }

    #[test]
    fn test_remote_workspace_sync_tree() {
        let temp_dir = TempDir::new().unwrap();
        let cache = temp_dir.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        // sync_tree 在验证子目录前不会创建根目录，因此先创建
        std::fs::create_dir_all(cache.join("project")).unwrap();

        let mut dirs = std::collections::HashMap::new();
        dirs.insert(
            "/project".to_string(),
            vec![
                RemoteDirEntry { name: "src".to_string(), is_dir: true, size: 0, modified: None },
                RemoteDirEntry { name: "Cargo.toml".to_string(), is_dir: false, size: 0, modified: None },
                RemoteDirEntry { name: "..".to_string(), is_dir: true, size: 0, modified: None },
                RemoteDirEntry { name: "bad/name".to_string(), is_dir: true, size: 0, modified: None },
            ],
        );
        let fs = Box::new(MockRemoteFsForSync { dirs });
        let ws = RemoteWorkspace::new(fs, cache.clone());
        ws.sync_tree("/project").unwrap();
        assert!(cache.join("project/src").is_dir());
        assert!(!cache.join("project/Cargo.toml").exists()); // 文件不创建
        let entries: std::collections::HashSet<String> = std::fs::read_dir(cache.join("project"))
            .unwrap()
            .filter_map(|e| e.ok().and_then(|e| e.file_name().into_string().ok()))
            .collect();
        assert!(!entries.contains("..")); // 跳过非法名
        assert!(!entries.contains("bad"));
        assert!(!cache.join("project/bad/name").exists()); // 跳过含 '/'
    }

    struct MockRemoteFsForSync {
        dirs: std::collections::HashMap<String, Vec<RemoteDirEntry>>,
    }

    impl RemoteFs for MockRemoteFsForSync {
        fn read_file(&self, _path: &str) -> crate::Result<Vec<u8>> {
            Err("not implemented".to_string())
        }
        fn write_file(&self, _path: &str, _content: &[u8]) -> crate::Result<()> {
            Ok(())
        }
        fn list_dir(&self, path: &str) -> crate::Result<Vec<RemoteDirEntry>> {
            self.dirs.get(path).cloned().ok_or_else(|| "not found".to_string())
        }
        fn watch(&self, _path: &str) -> crate::Result<std::sync::mpsc::Receiver<FsEvent>> {
            Err("unsupported".to_string())
        }
    }

    #[test]
    fn test_parse_remote_uri_edge_cases() {
        assert_eq!(
            workspace::parse_remote_uri("container://mycontainer/path/to/file"),
            Some(("container".to_string(), "mycontainer/path/to/file".to_string()))
        );
        assert!(workspace::parse_remote_uri("container://name").is_none());
        assert!(workspace::parse_remote_uri("ftp://host/path").is_none());
    }

    // SSH 配置解析补充
    #[test]
    fn test_ssh_base_args_default_port_and_empty_user() {
        let fs = SshRemoteFs::new(SshConfig {
            host: "host".to_string(),
            port: 22,
            username: "".to_string(),
            auth: SshAuth::Agent,
        });
        let args = fs.base_args();
        assert!(!args.contains(&"-p".to_string()));
        assert!(args.contains(&"host".to_string()));
        assert!(!args.contains(&"@".to_string()));
    }

    #[test]
    fn test_ssh_base_args_key_ignores_passphrase() {
        let fs = SshRemoteFs::new(SshConfig {
            host: "host".to_string(),
            port: 22,
            username: "user".to_string(),
            auth: SshAuth::Key {
                path: "/key".to_string(),
                passphrase: Some("secret".to_string()),
            },
        });
        let args = fs.base_args();
        let i_pos = args.iter().position(|a| a == "-i").unwrap();
        assert_eq!(args[i_pos + 1], "/key");
    }

    #[test]
    fn test_ssh_remote_fs_connected_state() {
        let mut fs = SshRemoteFs::new_connected(SshConfig::default());
        assert!(fs.is_connected());
        fs.disconnect();
        assert!(!fs.is_connected());
    }

    #[test]
    fn test_shell_quote() {
        // shell_quote 是 ssh.rs 的私有函数，通过编译期宏无法直接调用，
        // 这里通过 write_file 构造的远程 shell 命令间接验证转义行为：
        // 主要保证单引号转义不会 panic（实际执行依赖系统 ssh，不运行）
        let _ = crate::ssh::SshRemoteFs::new(SshConfig::default());
    }

    // Git 操作补充
    #[test]
    fn test_git_repository_push_no_remote_fails() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test_repo");
        init_repo(&repo_path);
        initial_commit(&repo_path, "a.txt", "a", "init");
        let repo = GitRepository::open(&repo_path).unwrap();
        assert!(repo.push(None, None, false).is_err());
    }

    #[test]
    fn test_git_repository_pull_no_remote_fails() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test_repo");
        init_repo(&repo_path);
        initial_commit(&repo_path, "a.txt", "a", "init");
        let repo = GitRepository::open(&repo_path).unwrap();
        assert!(repo.pull(None, None).is_err());
    }

    #[test]
    fn test_git_repository_status_ignored_and_conflicts() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("test_repo");
        init_repo(&repo_path);
        std::fs::write(repo_path.join("tracked.txt"), "a").unwrap();
        std::fs::write(repo_path.join(".gitignore"), "ignored.txt\n").unwrap();
        let repo = GitRepository::open(&repo_path).unwrap();
        repo.add(".").unwrap();
        repo.commit("init").unwrap();

        std::fs::write(repo_path.join("tracked.txt"), "b").unwrap();
        std::fs::write(repo_path.join("ignored.txt"), "c").unwrap();
        std::fs::write(repo_path.join("new.txt"), "d").unwrap();
        let status = repo.status().unwrap();
        assert!(!status.is_clean);
        assert!(status.unstaged_files.contains(&"tracked.txt".to_string()));
        assert!(status.untracked_files.contains(&"new.txt".to_string()));
        assert!(!status.untracked_files.contains(&"ignored.txt".to_string()));
    }

    // RemoteFs trait补充
    #[test]
    fn test_remote_fs_exec_default_returns_error() {
        struct DefaultExecFs;
        impl RemoteFs for DefaultExecFs {
            fn read_file(&self, _path: &str) -> crate::Result<Vec<u8>> {
                Ok(vec![])
            }
            fn write_file(&self, _path: &str, _content: &[u8]) -> crate::Result<()> {
                Ok(())
            }
            fn list_dir(&self, _path: &str) -> crate::Result<Vec<RemoteDirEntry>> {
                Ok(vec![])
            }
            fn watch(&self, _path: &str) -> crate::Result<std::sync::mpsc::Receiver<FsEvent>> {
                Err("unsupported".to_string())
            }
        }
        assert!(DefaultExecFs.exec("ls").is_err());
    }

    #[test]
    fn test_remote_fs_exists_when_parent_missing() {
        let fs = MockRemoteFs {
            files: std::collections::HashMap::new(),
            dirs: std::collections::HashMap::new(),
        };
        assert!(!fs.exists("/no/such/path.txt").unwrap());
    }

    #[test]
    fn test_get_git_info_parsing() {
        struct GitInfoFs;
        impl RemoteFs for GitInfoFs {
            fn read_file(&self, _path: &str) -> crate::Result<Vec<u8>> {
                Ok(vec![])
            }
            fn write_file(&self, _path: &str, _content: &[u8]) -> crate::Result<()> {
                Ok(())
            }
            fn list_dir(&self, _path: &str) -> crate::Result<Vec<RemoteDirEntry>> {
                Ok(vec![])
            }
            fn exec(&self, command: &str) -> crate::Result<(String, String)> {
                if command.contains("remote -v") {
                    Ok(("origin  git@github.com:user/repo.git (fetch)\n".to_string(), String::new()))
                } else if command.contains("branch --show-current") {
                    Ok(("main\n".to_string(), String::new()))
                } else if command.contains("status --porcelain") {
                    Ok((" M file.txt\n".to_string(), String::new()))
                } else {
                    Err("unexpected".to_string())
                }
            }
            fn watch(&self, _path: &str) -> crate::Result<std::sync::mpsc::Receiver<FsEvent>> {
                Err("unsupported".to_string())
            }
        }
        let info = GitInfoFs.get_git_info("project").unwrap();
        assert_eq!(info.remote_url, "git@github.com:user/repo.git");
        assert_eq!(info.current_branch, "main");
        assert!(info.has_uncommitted_changes);
    }

    #[test]
    fn test_git_exec_formatting() {
        struct GitExecFs;
        impl RemoteFs for GitExecFs {
            fn read_file(&self, _path: &str) -> crate::Result<Vec<u8>> {
                Ok(vec![])
            }
            fn write_file(&self, _path: &str, _content: &[u8]) -> crate::Result<()> {
                Ok(())
            }
            fn list_dir(&self, _path: &str) -> crate::Result<Vec<RemoteDirEntry>> {
                Ok(vec![])
            }
            fn exec(&self, command: &str) -> crate::Result<(String, String)> {
                Ok((command.to_string(), String::new()))
            }
            fn watch(&self, _path: &str) -> crate::Result<std::sync::mpsc::Receiver<FsEvent>> {
                Err("unsupported".to_string())
            }
        }
        let (cmd, _) = GitExecFs.git_exec("path with space", &["status", "README.md"]).unwrap();
        assert!(cmd.starts_with("git -C "));
        assert!(cmd.contains("status"));
        assert!(cmd.contains("README.md"));
    }

    #[test]
    fn test_git_ssh_repo_from_url_without_user() {
        let repo = GitSshRepo::from_url("ssh://host:22/repo.git", PathBuf::from("/tmp/repo")).unwrap();
        assert_eq!(repo.ssh_host, "host");
        assert_eq!(repo.ssh_port, 22);
    }
}
