use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct AppSettings {
    pub ai: AiSettings,
    pub ui: UiSettings,
    pub remote: RemoteSettings,
}

impl std::fmt::Debug for AppSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppSettings")
            .field("ai", &self.ai)
            .field("ui", &self.ui)
            .field("remote", &self.remote)
            .finish()
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct AiSettings {
    pub provider: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
}

impl std::fmt::Debug for AiSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiSettings")
            .field("provider", &self.provider)
            .field("api_key", &"[REDACTED]")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .finish()
    }
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct UiSettings {
    pub theme: String,
    pub font_size: u32,
    pub sidebar_visible: bool,
    /// 活动栏图标顺序（持久化键列表，空表示使用默认顺序）
    #[serde(default)]
    pub activity_bar_order: Vec<String>,
    /// 菜单栏顶部项顺序（持久化键列表，空表示使用默认顺序）
    #[serde(default)]
    pub menu_bar_order: Vec<String>,
    /// 主窗口左上角 X 坐标（屏幕坐标），None 表示使用默认位置
    #[serde(default)]
    pub window_x: Option<i32>,
    /// 主窗口左上角 Y 坐标（屏幕坐标），None 表示使用默认位置
    #[serde(default)]
    pub window_y: Option<i32>,
    /// 主窗口宽度（像素），None 表示使用默认尺寸
    #[serde(default)]
    pub window_width: Option<u32>,
    /// 主窗口高度（像素），None 表示使用默认尺寸
    #[serde(default)]
    pub window_height: Option<u32>,
    /// 主窗口是否最大化
    #[serde(default)]
    pub window_maximized: bool,
    /// 上次打开的工作区路径，None 表示未打开任何工作区
    #[serde(default)]
    pub last_workspace: Option<PathBuf>,
}

/// SSH 服务器配置（持久化到 settings.json）
/// 密码/passphrase 不持久化（安全考虑），连接时由用户输入
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct SshServerConfig {
    /// 服务器显示名称
    pub name: String,
    /// 主机地址（IP 或域名）
    pub host: String,
    /// SSH 端口
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// 登录用户名
    pub username: String,
    /// 认证方式: "password" | "key" | "agent"
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    /// 密钥文件路径（auth_type == "key" 时使用）
    #[serde(default)]
    pub key_path: String,
}

fn default_ssh_port() -> u16 {
    22
}

fn default_auth_type() -> String {
    "agent".to_string()
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct RemoteSettings {
    /// 已保存的 SSH 服务器配置列表
    #[serde(default)]
    pub ssh_servers: Vec<SshServerConfig>,
    /// 兼容旧版本字段（已废弃，读取时忽略）
    #[serde(default, skip_serializing)]
    pub ssh_hosts: Vec<String>,
}

impl AppSettings {
    pub fn settings_path() -> PathBuf {
        let config_dir = dirs::config_dir().unwrap_or_else(|| std::env::temp_dir());
        let aether_dir = config_dir.join("Aether");
        if let Err(e) = std::fs::create_dir_all(&aether_dir) {
            eprintln!("警告: 无法创建配置目录 {}: {}", aether_dir.display(), e);
        }
        aether_dir.join("settings.json")
    }

    pub fn load() -> Self {
        let path = Self::settings_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(mut settings) = serde_json::from_str::<AppSettings>(&content) {
                // P1-2: 密码认证禁用——加载时扫描并中和遗留的 password 配置。
                // 将 auth_type == "password" 的服务器迁移为 "agent"，确保旧版本
                // 保存的配置在加载后不会生成 Password 认证（纵深防御 + 实时同步）。
                let migrated = settings
                    .remote
                    .ssh_servers
                    .iter_mut()
                    .filter(|s| s.auth_type == "password")
                    .map(|s| {
                        s.auth_type = "agent".to_string();
                        s.name.clone()
                    })
                    .collect::<Vec<_>>();
                if !migrated.is_empty() {
                    eprintln!(
                        "[P1-2] 已禁用 {} 个服务器的密码认证并迁移为 Agent: {:?}",
                        migrated.len(),
                        migrated
                    );
                    // 持久化迁移结果（best-effort，失败不影响加载）
                    let _ = settings.save();
                }
                return settings;
            }
        }
        Self::default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::settings_path();
        // AI-H03: 不再清除 api_key 写入文件。
        // settings.json 位于用户私有目录，受文件系统权限保护。
        // 若需更高安全性，未来可迁移至 Windows Credential Manager。
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // P1-3: 原子写入——临时文件 + fsync + rename
        // 写入过程中崩溃只会留下临时文件，不会损坏 settings.json。
        // rename 在同卷上是原子操作（Windows MoveFileEx / POSIX rename）。
        let mut tmp_path = path.clone();
        let mut suffix = std::ffi::OsString::from(".tmp.");
        suffix.push(std::process::id().to_string());
        suffix.push(".");
        suffix.push(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos().to_string())
                .unwrap_or_else(|_| "0".to_string()),
        );
        tmp_path.set_extension(suffix);

        #[cfg(windows)]
        {
            use std::io::Write;
            use std::os::windows::fs::OpenOptionsExt;
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .share_mode(0)
                .open(&tmp_path)?;
            let mut writer = std::io::BufWriter::new(file);
            writer.write_all(content.as_bytes())?;
            writer.flush()?;
            // fsync 确保数据落盘后再 rename
            writer.get_ref().sync_all()?;
        }
        #[cfg(not(windows))]
        {
            std::fs::write(&tmp_path, &content)?;
            let _ = std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600));
            // fsync 确保数据落盘后再 rename
            std::fs::File::open(&tmp_path)?.sync_all()?;
        }

        // 原子 rename：旧文件被整体替换，要么成功要么原文件不变
        std::fs::rename(&tmp_path, &path).map_err(|e| {
            // rename 失败时清理临时文件，避免残留
            let _ = std::fs::remove_file(&tmp_path);
            e
        })?;
        Ok(())
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ai: AiSettings {
                provider: "openai".to_string(),
                api_key: String::new(),
                base_url: None,
                model: "gpt-4".to_string(),
            },
            ui: UiSettings::default(),
            remote: RemoteSettings::default(),
        }
    }
}
