use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
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
    // C-10: api_key 不再序列化到 settings.json，改为 DPAPI 加密单独存储
    #[serde(skip_serializing, default)]
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub system_prompt: Option<String>,
}

impl std::fmt::Debug for AiSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiSettings")
            .field("provider", &self.provider)
            .field("api_key", &"[REDACTED]")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .field(
                "system_prompt",
                &self.system_prompt.as_deref().map(|_| "[PRESENT]"),
            )
            .finish()
    }
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
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
        let config_dir = dirs::config_dir().unwrap_or_else(std::env::temp_dir);
        let aether_dir = config_dir.join("Aether");
        if let Err(e) = std::fs::create_dir_all(&aether_dir) {
            eprintln!("警告: 无法创建配置目录 {}: {}", aether_dir.display(), e);
        }
        aether_dir.join("settings.json")
    }

    /// C-10: 加密后的 API 密钥存储路径
    pub fn api_key_path() -> PathBuf {
        let config_dir = dirs::config_dir().unwrap_or_else(std::env::temp_dir);
        let aether_dir = config_dir.join("Aether");
        let _ = std::fs::create_dir_all(&aether_dir);
        aether_dir.join("api_key.enc")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::settings_path(), &Self::api_key_path())
    }

    fn load_from(settings_path: &std::path::Path, api_key_path: &std::path::Path) -> Self {
        if let Ok(content) = std::fs::read_to_string(settings_path) {
            match serde_json::from_str::<AppSettings>(&content) {
                Ok(mut settings) => {
                    // C-10: 从单独加密文件加载 API 密钥
                    if let Ok(encrypted) = std::fs::read(api_key_path) {
                        if let Ok(api_key) = decrypt_api_key(&encrypted) {
                            settings.ai.api_key = api_key;
                        }
                    }

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
                        let _ = settings.save_to(settings_path, api_key_path);
                    }
                    return settings;
                }
                Err(e) => {
                    // M-13: JSON 损坏时记录警告并备份原文件，避免用户在不知情下丢失设置
                    eprintln!("[M-13] 警告: settings.json 解析失败，回退到默认设置: {}", e);
                    let backup = settings_path.with_extension("json.corrupt");
                    if std::fs::rename(settings_path, &backup).is_ok() {
                        eprintln!("[M-13] 已将损坏的配置备份到 {}", backup.display());
                    }
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::settings_path(), &Self::api_key_path())
    }

    fn save_to(
        &self,
        path: &std::path::Path,
        api_key_path: &std::path::Path,
    ) -> std::io::Result<()> {
        // C-10: settings.json 不再写入明文 api_key；改为单独 DPAPI 加密存储
        let mut settings_for_save = self.clone();
        settings_for_save.ai.api_key.clear();
        let content = serde_json::to_string_pretty(&settings_for_save)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // P1-3: 原子写入——临时文件 + fsync + rename
        // 写入过程中崩溃只会留下临时文件，不会损坏 settings.json。
        // rename 在同卷上是原子操作（Windows MoveFileEx / POSIX rename）。
        let mut tmp_path = path.to_path_buf();
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
        std::fs::rename(&tmp_path, path).inspect_err(|_e| {
            // rename 失败时清理临时文件，避免残留
            let _ = std::fs::remove_file(&tmp_path);
        })?;

        // C-10: settings.json 不再包含明文 api_key，单独加密保存
        if !self.ai.api_key.is_empty() {
            if let Ok(encrypted) = encrypt_api_key(&self.ai.api_key) {
                let _ = std::fs::write(api_key_path, encrypted);
            }
        } else {
            let _ = std::fs::remove_file(api_key_path);
        }

        Ok(())
    }
}

/// C-10: 使用 Windows DPAPI 加密 API 密钥
#[cfg(windows)]
fn encrypt_api_key(api_key: &str) -> std::io::Result<Vec<u8>> {
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_LOCAL_MACHINE,
    };

    let bytes = api_key.as_bytes();
    let data_in = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut data_out = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptProtectData(
            &data_in,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_LOCAL_MACHINE,
            &mut data_out,
        )
        .map_err(|e| std::io::Error::other(e.to_string()))?;

        let slice = std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize);
        let result = slice.to_vec();
        windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(data_out.pbData as *mut _));
        Ok(result)
    }
}

/// C-10: 非 Windows 平台回退为 UTF-8 字节（项目主要面向 Windows，此处仅保证编译）
#[cfg(not(windows))]
fn encrypt_api_key(api_key: &str) -> std::io::Result<Vec<u8>> {
    Ok(api_key.as_bytes().to_vec())
}

/// C-10: 使用 Windows DPAPI 解密 API 密钥
#[cfg(windows)]
fn decrypt_api_key(data: &[u8]) -> std::io::Result<String> {
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_LOCAL_MACHINE,
    };

    let data_in = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut data_out = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptUnprotectData(
            &data_in,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_LOCAL_MACHINE,
            &mut data_out,
        )
        .map_err(|e| std::io::Error::other(e.to_string()))?;

        let slice = std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize);
        let result = String::from_utf8(slice.to_vec())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(data_out.pbData as *mut _));
        Ok(result)
    }
}

#[cfg(not(windows))]
fn decrypt_api_key(data: &[u8]) -> std::io::Result<String> {
    String::from_utf8(data.to_vec())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ai: AiSettings {
                provider: "openai".to_string(),
                api_key: String::new(),
                base_url: None,
                model: "gpt-4".to_string(),
                temperature: Some(0.7),
                max_tokens: Some(2048),
                system_prompt: None,
            },
            ui: UiSettings::default(),
            remote: RemoteSettings::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(prefix: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos().to_string())
            .unwrap_or_else(|_| "0".to_string());
        dir.push(format!("{}-{}-{}", prefix, std::process::id(), stamp));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn test_api_key_encryption_roundtrip() {
        // C-10: 验证 DPAPI 加密/解密往返正确
        let key = "sk-test-12345";
        let encrypted = encrypt_api_key(key).expect("加密失败");
        assert_ne!(encrypted, key.as_bytes());
        let decrypted = decrypt_api_key(&encrypted).expect("解密失败");
        assert_eq!(decrypted, key);
    }

    #[test]
    fn test_decrypt_invalid_data_fails() {
        // 非 Windows 平台：解密失败路径表现为非法 UTF-8
        #[cfg(not(windows))]
        {
            let invalid = vec![0xFF, 0xFE, 0xFD];
            assert!(decrypt_api_key(&invalid).is_err());
        }
        // Windows 平台：随机字节无法被 DPAPI 解密
        #[cfg(windows)]
        {
            let invalid = vec![0xDE, 0xAD, 0xBE, 0xEF];
            assert!(decrypt_api_key(&invalid).is_err());
        }
    }

    #[test]
    fn test_settings_save_does_not_include_plaintext_api_key() {
        // C-10: 验证 settings.json 序列化中不包含明文 api_key
        let mut settings = AppSettings::default();
        settings.ai.api_key = "secret-key".to_string();
        let json = serde_json::to_string(&settings).expect("序列化失败");
        assert!(!json.contains("secret-key"), "api_key 不应以明文出现在 JSON 中");
    }

    #[test]
    fn test_default_settings_values() {
        let s = AppSettings::default();
        assert_eq!(s.ai.provider, "openai");
        assert_eq!(s.ai.model, "gpt-4");
        assert_eq!(s.ai.base_url, None);
        assert_eq!(s.ai.temperature, Some(0.7));
        assert_eq!(s.ai.max_tokens, Some(2048));
        assert!(s.ai.api_key.is_empty());
        assert_eq!(s.ui.theme, String::new());
        assert_eq!(s.ui.font_size, 0);
        assert!(!s.ui.sidebar_visible);
        assert!(s.remote.ssh_servers.is_empty());
        assert!(s.remote.ssh_hosts.is_empty());
    }

    #[test]
    fn test_settings_deserialize_defaults() {
        let json = r#"{}"#;
        let s: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.ai.provider, "openai");
        assert_eq!(s.ui.theme, String::new());
        assert!(!s.ui.sidebar_visible);
    }

    #[test]
    fn test_settings_deserialize_different_provider() {
        let json = r#"{
            "ai": {
                "provider": "anthropic",
                "model": "claude-3-opus",
                "base_url": "https://api.anthropic.com",
                "temperature": 0.5,
                "max_tokens": 4096
            }
        }"#;
        let s: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.ai.provider, "anthropic");
        assert_eq!(s.ai.model, "claude-3-opus");
        assert_eq!(s.ai.base_url.as_deref(), Some("https://api.anthropic.com"));
        assert_eq!(s.ai.temperature, Some(0.5));
        assert_eq!(s.ai.max_tokens, Some(4096));
    }

    #[test]
    fn test_ssh_server_config_defaults() {
        let cfg: SshServerConfig = serde_json::from_str(r#"{"name":"x","host":"h","username":"u"}"#).unwrap();
        assert_eq!(cfg.port, 22);
        assert_eq!(cfg.auth_type, "agent");
        assert!(cfg.key_path.is_empty());
    }

    #[test]
    fn test_settings_save_and_load_roundtrip() {
        let dir = temp_test_dir("aether-settings-rt");
        let settings_path = dir.join("settings.json");
        let api_key_path = dir.join("api_key.enc");

        let mut s = AppSettings::default();
        s.ai.provider = "custom".to_string();
        s.ai.model = "custom-model".to_string();
        s.ai.api_key = "sk-roundtrip".to_string();
        s.ui.font_size = 16;
        s.ui.theme = "dark".to_string();
        s.remote.ssh_servers.push(SshServerConfig {
            name: "home".to_string(),
            host: "192.168.1.1".to_string(),
            port: 2222,
            username: "u".to_string(),
            auth_type: "key".to_string(),
            key_path: "C:\\key".to_string(),
        });

        s.save_to(&settings_path, &api_key_path).unwrap();

        // settings.json 中不应包含明文 api_key
        let json = std::fs::read_to_string(&settings_path).unwrap();
        assert!(!json.contains("sk-roundtrip"));
        assert!(json.contains("custom-model"));
        assert!(json.contains("192.168.1.1"));

        // 加密文件应存在且可解密
        assert!(api_key_path.exists());
        let loaded = AppSettings::load_from(&settings_path, &api_key_path);
        assert_eq!(loaded.ai.api_key, "sk-roundtrip");
        assert_eq!(loaded.ai.provider, "custom");
        assert_eq!(loaded.ui.font_size, 16);
        assert_eq!(loaded.remote.ssh_servers.len(), 1);
        assert_eq!(loaded.remote.ssh_servers[0].auth_type, "key");

        // 清理
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_settings_load_missing_returns_default() {
        let dir = temp_test_dir("aether-settings-missing");
        let settings_path = dir.join("nope.json");
        let api_key_path = dir.join("nope.enc");
        let loaded = AppSettings::load_from(&settings_path, &api_key_path);
        assert_eq!(loaded.ai.provider, "openai");
        assert!(loaded.ai.api_key.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_settings_load_corrupted_returns_default() {
        let dir = temp_test_dir("aether-settings-corrupt");
        let settings_path = dir.join("settings.json");
        let api_key_path = dir.join("api_key.enc");
        std::fs::write(&settings_path, "this is not json").unwrap();

        let loaded = AppSettings::load_from(&settings_path, &api_key_path);
        assert_eq!(loaded.ai.provider, "openai");
        assert!(loaded.ai.api_key.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_settings_password_auth_migration() {
        let dir = temp_test_dir("aether-settings-migrate");
        let settings_path = dir.join("settings.json");
        let api_key_path = dir.join("api_key.enc");

        let mut s = AppSettings::default();
        s.remote.ssh_servers.push(SshServerConfig {
            name: "legacy".to_string(),
            host: "h".to_string(),
            port: 22,
            username: "u".to_string(),
            auth_type: "password".to_string(),
            key_path: String::new(),
        });
        s.save_to(&settings_path, &api_key_path).unwrap();

        let loaded = AppSettings::load_from(&settings_path, &api_key_path);
        assert_eq!(loaded.remote.ssh_servers[0].auth_type, "agent");

        // 迁移后持久化的文件也应是 agent
        let migrated_json = std::fs::read_to_string(&settings_path).unwrap();
        assert!(migrated_json.contains("agent"));
        assert!(!migrated_json.contains("password"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_settings_empty_api_key_removes_encrypted_file() {
        let dir = temp_test_dir("aether-settings-nokey");
        let settings_path = dir.join("settings.json");
        let api_key_path = dir.join("api_key.enc");

        let mut s = AppSettings::default();
        s.ai.api_key = "temp-key".to_string();
        s.save_to(&settings_path, &api_key_path).unwrap();
        assert!(api_key_path.exists());

        s.ai.api_key.clear();
        s.save_to(&settings_path, &api_key_path).unwrap();
        assert!(!api_key_path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_settings_debug_redacts_api_key() {
        let mut s = AppSettings::default();
        s.ai.api_key = "super-secret".to_string();
        let debug = format!("{:?}", s);
        assert!(!debug.contains("super-secret"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn test_settings_ui_window_fields_default() {
        let ui: UiSettings = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(ui.window_x, None);
        assert_eq!(ui.window_y, None);
        assert_eq!(ui.window_width, None);
        assert_eq!(ui.window_height, None);
        assert!(!ui.window_maximized);
        assert!(ui.activity_bar_order.is_empty());
        assert!(ui.menu_bar_order.is_empty());
    }

    #[test]
    fn test_settings_ui_window_fields_roundtrip() {
        let json = r#"{
            "theme": "light",
            "font_size": 14,
            "sidebar_visible": false,
            "window_x": 100,
            "window_y": 200,
            "window_width": 1280,
            "window_height": 720,
            "window_maximized": true,
            "activity_bar_order": ["files", "search"],
            "menu_bar_order": ["file", "edit"],
            "last_workspace": "C:\\\\proj"
        }"#;
        let ui: UiSettings = serde_json::from_str(json).unwrap();
        assert_eq!(ui.theme, "light");
        assert_eq!(ui.font_size, 14);
        assert!(!ui.sidebar_visible);
        assert_eq!(ui.window_x, Some(100));
        assert_eq!(ui.window_y, Some(200));
        assert_eq!(ui.window_width, Some(1280));
        assert_eq!(ui.window_height, Some(720));
        assert!(ui.window_maximized);
        assert_eq!(ui.activity_bar_order, vec!["files", "search"]);
        assert_eq!(ui.menu_bar_order, vec!["file", "edit"]);
        assert_eq!(ui.last_workspace, Some(PathBuf::from("C:\\proj")));
    }
}
