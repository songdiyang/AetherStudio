use std::sync::mpsc;

use crate::remote_fs::{FsEvent, RemoteDirEntry, RemoteFs, Result};

/// 容器运行时后端
#[derive(Clone, Debug)]
pub enum ContainerBackend {
    Docker,
    Podman,
}

/// 容器连接配置
#[derive(Clone, Debug)]
pub struct ContainerConfig {
    pub backend: ContainerBackend,
    pub container_name: String,
    pub image: String,
    pub workspace_mount: String,
}

/// 容器远程文件系统实现（占位符）
/// 通过 docker exec 和 docker cp 操作容器内文件
pub struct ContainerRemoteFs {
    _config: ContainerConfig,
}

impl ContainerRemoteFs {
    pub fn new(config: ContainerConfig) -> Self {
        Self { _config: config }
    }

    fn backend_cmd(&self) -> &'static str {
        match self._config.backend {
            ContainerBackend::Docker => "docker",
            ContainerBackend::Podman => "podman",
        }
    }
}

impl RemoteFs for ContainerRemoteFs {
    fn read_file(&self, _path: &str) -> Result<Vec<u8>> {
        Err("Container read_file not yet implemented".to_string())
    }

    fn write_file(&self, _path: &str, _content: &[u8]) -> Result<()> {
        Err("Container write_file not yet implemented".to_string())
    }

    fn list_dir(&self, _path: &str) -> Result<Vec<RemoteDirEntry>> {
        Err("Container list_dir not yet implemented".to_string())
    }

    fn watch(&self, _path: &str) -> Result<mpsc::Receiver<FsEvent>> {
        // H-40: 返回错误说明不支持，而非返回永远无事件的 receiver
        Err("Container 后端不支持文件监视".to_string())
    }

    fn exec(&self, command: &str) -> Result<(String, String)> {
        // SEC-R02: 记录审计日志 + 命令白名单校验
        eprintln!("[AUDIT] container exec: {}", command);

        let trimmed = command.trim();
        if trimmed.is_empty() {
            return Err("命令为空".to_string());
        }

        // SEC-R02: 拒绝 shell 元字符，防止命令注入
        // 注意：即便 docker/podman exec 使用参数列表传递，命令最终仍由容器内
        // 的 `sh -c` 解释，因此必须过滤元字符。
        const SHELL_METACHARS: &[char] = &[';', '|', '&', '`', '$', '>', '<', '\n', '\r'];
        if trimmed.chars().any(|c| SHELL_METACHARS.contains(&c)) {
            return Err(format!("命令包含禁止的 shell 元字符: {}", command));
        }

        // SEC-R03: 最小化命令白名单（与 RemoteFs::exec_restricted 保持一致）
        const ALLOWED_COMMANDS: &[&str] = &[
            "ls", "cat", "pwd", "echo", "find", "grep", "head", "tail", "wc", "stat", "file",
            "which", "diff", "sort", "uniq", "tr", "less", "more", "uname", "whoami", "id", "ps",
            "df", "du", "mkdir", "touch", "cp", "mv", "rm", "chmod", "chown", "git", "tar", "gzip",
            "gunzip",
        ];
        let cmd_name = trimmed.split_whitespace().next().unwrap_or("");
        if !ALLOWED_COMMANDS.iter().any(|&allowed| allowed == cmd_name) {
            return Err(format!("命令被拒绝（不在白名单中）: {}", command));
        }

        // 校验容器名仅含字母数字、连字符和下划线（H-13: 防止注入 Docker 标志）
        let valid_name = self
            ._config
            .container_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
        if !valid_name {
            return Err(format!("非法容器名: {}", self._config.container_name));
        }

        // 使用参数列表而非 shell 拼接，避免命令注入
        let output = std::process::Command::new(self.backend_cmd())
            .arg("exec")
            .arg(&self._config.container_name)
            .arg("sh")
            .arg("-c")
            .arg(command)
            .output()
            .map_err(|e| e.to_string())?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok((stdout, stderr))
    }
}

/// 解析 .devcontainer.json 配置
pub fn parse_devcontainer_json(_content: &str) -> Result<ContainerConfig> {
    Err("devcontainer.json parsing not yet implemented".to_string())
}
