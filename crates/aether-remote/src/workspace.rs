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

/// REMOTE-M01: 本地缓存最大大小（500MB），防止无限增长
const MAX_CACHE_SIZE: u64 = 500 * 1024 * 1024;
/// 超过最大缓存大小时，清理到目标的 80%
const CACHE_CLEANUP_RATIO: f64 = 0.8;

impl RemoteWorkspace {
    pub fn new(connection: Box<dyn RemoteFs>, local_cache: PathBuf) -> Self {
        Self {
            connection,
            local_cache,
            file_versions: HashMap::new(),
        }
    }

    /// 验证本地路径仍在缓存目录内，防止路径遍历攻击
    fn validate_local_path(&self, local_path: &Path) -> Result<PathBuf> {
        let canonical_local = match local_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                // 文件可能还不存在，先检查父目录
                if let Some(parent) = local_path.parent() {
                    let canonical_parent = parent.canonicalize().map_err(|e| e.to_string())?;
                    let file_name = local_path.file_name().ok_or("无效路径: 缺少文件名")?;
                    canonical_parent.join(file_name)
                } else {
                    return Err("无效路径: 无父目录".to_string());
                }
            }
        };
        let canonical_cache = self
            .local_cache
            .canonicalize()
            .unwrap_or_else(|_| self.local_cache.clone());
        if !canonical_local.starts_with(&canonical_cache) {
            return Err(format!(
                "路径遍历检测: 路径 {} 超出缓存目录",
                local_path.display()
            ));
        }
        Ok(local_path.to_path_buf())
    }

    /// 打开远程文件（按需下载到本地缓存）
    pub fn open_file(&mut self, remote_path: &str) -> Result<PathBuf> {
        // 校验远程路径不含路径遍历组件
        if remote_path.contains("..") || remote_path.contains("\\") {
            return Err(format!("非法远程路径: {}", remote_path));
        }

        let content = self.connection.read_file(remote_path)?;
        let local_path = self.local_cache.join(remote_path.trim_start_matches('/'));
        let local_path = self.validate_local_path(&local_path)?;

        // 确保父目录存在
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        // REMOTE-M01: 写入前检查缓存大小，超过限制时自动清理旧文件
        let _ = self.check_cache_size();

        std::fs::write(&local_path, &content).map_err(|e| e.to_string())?;

        // 更新版本
        self.file_versions.insert(remote_path.to_string(), 1);

        Ok(local_path)
    }

    /// 保存本地修改到远程
    pub fn save_file(&mut self, remote_path: &str) -> Result<()> {
        // 校验远程路径不含路径遍历组件
        if remote_path.contains("..") || remote_path.contains("\\") {
            return Err(format!("非法远程路径: {}", remote_path));
        }

        let local_path = self.local_cache.join(remote_path.trim_start_matches('/'));
        let local_path = self.validate_local_path(&local_path)?;
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
            // 校验目录条目名不含路径遍历
            if entry.name.contains("..") || entry.name.contains('/') || entry.name.contains('\\') {
                continue;
            }

            let remote_full_path = format!("{}/{}", remote_path.trim_end_matches('/'), entry.name);
            let local_path = self
                .local_cache
                .join(remote_full_path.trim_start_matches('/'));
            let local_path = self.validate_local_path(&local_path)?;

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

    /// REMOTE-M01: 检查并清理缓存目录大小
    fn check_cache_size(&self) -> Result<()> {
        let total_size = self.calc_cache_size()?;
        if total_size > MAX_CACHE_SIZE {
            let target_size = (MAX_CACHE_SIZE as f64 * CACHE_CLEANUP_RATIO) as u64;
            self.trim_cache(target_size)?;
        }
        Ok(())
    }

    /// 递归计算缓存目录总大小
    fn calc_cache_size(&self) -> Result<u64> {
        let mut total: u64 = 0;
        if self.local_cache.exists() {
            Self::walk_dir_size(&self.local_cache, &mut total)?;
        }
        Ok(total)
    }

    fn walk_dir_size(path: &Path, total: &mut u64) -> Result<()> {
        for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                Self::walk_dir_size(&path, total)?;
            } else if path.is_file() {
                *total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
        Ok(())
    }

    /// 简单清理策略：删除最旧的文件直到大小降至目标值以下
    fn trim_cache(&self, target_size: u64) -> Result<()> {
        let mut files: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
        Self::collect_files_by_age(&self.local_cache, &mut files)?;
        // 按修改时间升序（最旧的在前）
        files.sort_by_key(|(_, t)| *t);

        let mut current_size = self.calc_cache_size()?;
        for (path, _) in &files {
            if current_size <= target_size {
                break;
            }
            if let Ok(meta) = std::fs::metadata(path) {
                let size = meta.len();
                if std::fs::remove_file(path).is_ok() {
                    current_size = current_size.saturating_sub(size);
                }
            }
        }
        Ok(())
    }

    fn collect_files_by_age(
        dir: &Path,
        files: &mut Vec<(PathBuf, std::time::SystemTime)>,
    ) -> Result<()> {
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                Self::collect_files_by_age(&path, files)?;
            } else if path.is_file() {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        files.push((path, modified));
                    }
                }
            }
        }
        Ok(())
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
