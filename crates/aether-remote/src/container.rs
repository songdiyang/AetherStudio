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
        let (_tx, rx) = mpsc::channel();
        Ok(rx)
    }

    fn exec(&self, command: &str) -> Result<(String, String)> {
        // 使用 std::process 在容器内执行命令
        let output = std::process::Command::new(self.backend_cmd())
            .args(["exec", &self._config.container_name, "sh", "-c", command])
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
