use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::remote_fs::{FsEvent, RemoteFs, Result};

/// 远程工作区
/// 管理远程文件系统的本地缓存和同步
pub struct RemoteWorkspace {
    connection: Box<dyn RemoteFs>,
    local_cache: PathBuf,
    file_versions: HashMap<String, u64>,
}

impl RemoteWorkspace {
    pub fn new(connection: Box<dyn RemoteFs>, local_cache: PathBuf) -> Self {
        Self {
            connection,
            local_cache,
            file_versions: HashMap::new(),
        }
    }

    /// 打开远程文件（按需下载到本地缓存）
    pub fn open_file(&mut self, remote_path: &str) -> Result<PathBuf> {
        let content = self.connection.read_file(remote_path)?;
        let local_path = self.local_cache.join(remote_path.trim_start_matches('/'));

        // 确保父目录存在
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        std::fs::write(&local_path, &content).map_err(|e| e.to_string())?;

        // 更新版本
        self.file_versions.insert(remote_path.to_string(), 1);

        Ok(local_path)
    }

    /// 保存本地修改到远程
    pub fn save_file(&mut self, remote_path: &str) -> Result<()> {
        let local_path = self.local_cache.join(remote_path.trim_start_matches('/'));
        let content = std::fs::read(&local_path).map_err(|e| e.to_string())?;

        self.connection.write_file(remote_path, &content)?;

        // 递增版本
        let version = self
            .file_versions
            .entry(remote_path.to_string())
            .or_insert(0);
        *version += 1;

        Ok(())
    }

    /// 同步远程目录结构到本地缓存
    pub fn sync_tree(&self, remote_path: &str) -> Result<()> {
        let entries = self.connection.list_dir(remote_path)?;

        for entry in entries {
            let remote_full_path = format!("{}/{}", remote_path.trim_end_matches('/'), entry.name);
            let local_path = self
                .local_cache
                .join(remote_full_path.trim_start_matches('/'));

            if entry.is_dir {
                std::fs::create_dir_all(&local_path).map_err(|e| e.to_string())?;
            }
        }

        Ok(())
    }

    /// 获取本地缓存路径
    pub fn local_cache(&self) -> &Path {
        &self.local_cache
    }

    /// 监听远程文件变更
    pub fn watch(&self, remote_path: &str) -> Result<std::sync::mpsc::Receiver<FsEvent>> {
        self.connection.watch(remote_path)
    }
}

/// 远程 URI 解析
/// 支持 ssh://host/path 和 container://name/path 格式
pub fn parse_remote_uri(uri: &str) -> Option<(String, String)> {
    if let Some(rest) = uri.strip_prefix("ssh://") {
        if let Some((host, path)) = rest.split_once('/') {
            return Some(("ssh".to_string(), format!("{}/{}", host, path)));
        }
    }

    if let Some(rest) = uri.strip_prefix("container://") {
        if let Some((name, path)) = rest.split_once('/') {
            return Some(("container".to_string(), format!("{}/{}", name, path)));
        }
    }

    None
}
