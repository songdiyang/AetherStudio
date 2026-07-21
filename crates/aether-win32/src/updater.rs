//! 自动更新
//!
//! 流程：后台线程查询版本清单比对版本 → 有新版则下载 aether-setup.exe
//! 到 %TEMP%（校验 SHA256）→ PostMessage(WM_UPDATE_CHECK_DONE) 通知
//! UI 线程 → 用户确认后静默运行安装包（setup /S）并退出本进程，
//! 安装器负责杀进程、覆盖安装并重启应用。
//!
//! 更新源：优先走镜像服务器（MIRROR_MANIFEST_URL，国内下载快），
//! 服务器不可达或清单异常时自动回退 GitHub Releases API。
//!
//! 版本号注入：CI（release-main.yml）在构建时设置环境变量 AETHER_VERSION
//! （如 "v2026.07.21-1"）；本地开发构建回退到 Cargo.toml 的 workspace 版本。

use std::io::Read;
use std::path::PathBuf;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// 应用版本（编译期注入）
pub const APP_VERSION: &str = match option_env!("AETHER_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// 是否为 CI 发布构建（开发构建不在启动时自动检查更新，避免每次启动都弹框）
pub const IS_RELEASE_BUILD: bool = option_env!("AETHER_VERSION").is_some();

/// 更新检查完成消息（后台线程 → UI 线程，lparam 为 Box<UpdateCheckMessage> 指针）
pub const WM_UPDATE_CHECK_DONE: u32 = WM_APP + 8;

const GITHUB_REPO: &str = "songdiyang/AetherStudio";
const SETUP_ASSET_NAME: &str = "aether-setup.exe";
/// 镜像服务器版本清单地址（优先级高于 GitHub）。
/// 服务器侧规格：Nginx 静态目录 /aether/ 下提供 latest.json + aether-setup.exe。
const MIRROR_MANIFEST_URL: &str = "https://aetherstudio.cn/aether/latest.json";
/// 下载上限，防止异常响应撑爆磁盘
const MAX_SETUP_BYTES: u64 = 300 * 1024 * 1024;

/// 更新检查结果
pub enum UpdateCheckResult {
    /// 已是最新
    UpToDate,
    /// 有新版，安装包已下载到本地
    Available { version: String, setup_path: PathBuf },
    /// 检查或下载失败
    Error(String),
}

/// WM_UPDATE_CHECK_DONE 携带的消息体
pub struct UpdateCheckMessage {
    /// 是否为用户手动触发（手动：任何结果都弹窗；自动：仅发现新版时弹窗）
    pub manual: bool,
    pub result: UpdateCheckResult,
}

/// HWND 的 Send 包装（PostMessageW 线程安全）
#[derive(Clone, Copy)]
struct SendHwnd(usize);
unsafe impl Send for SendHwnd {}

/// 在后台线程启动更新检查，完成后投递 WM_UPDATE_CHECK_DONE
pub fn start_check(hwnd: HWND, manual: bool) {
    let hwnd = SendHwnd(hwnd.0 as usize);
    std::thread::spawn(move || {
        let msg = Box::new(UpdateCheckMessage {
            manual,
            result: check_and_download(),
        });
        let raw = Box::into_raw(msg);
        let hwnd = HWND(hwnd.0 as *mut _);
        unsafe {
            // 失败时回收 Box 防止泄漏
            if PostMessageW(
                hwnd,
                WM_UPDATE_CHECK_DONE,
                WPARAM(0),
                LPARAM(raw as isize),
            )
            .is_err()
            {
                let _ = Box::from_raw(raw);
            }
        }
    });
}

/// 用户确认更新：启动静默安装并退出本进程
pub fn run_setup_and_exit(setup_path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new(setup_path)
        .arg("/S")
        .spawn()
        .map_err(|e| format!("启动安装程序失败: {e}"))?;
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
    }
    Ok(())
}

/// 检查更新；有新版则下载安装包（阻塞，须在后台线程调用）
fn check_and_download() -> UpdateCheckResult {
    match fetch_latest_release() {
        Ok(release) => {
            if !is_newer(&release.tag, APP_VERSION) {
                return UpdateCheckResult::UpToDate;
            }
            match download_setup(&release) {
                Ok(path) => UpdateCheckResult::Available {
                    version: release.tag,
                    setup_path: path,
                },
                Err(e) => UpdateCheckResult::Error(e),
            }
        }
        Err(e) => UpdateCheckResult::Error(e),
    }
}

struct LatestRelease {
    tag: String,
    setup_url: String,
    /// 安装包 SHA256（十六进制小写）；镜像清单提供，GitHub 源无此字段则不校验
    sha256: Option<String>,
}

/// 获取最新版本信息：镜像服务器优先，失败自动回退 GitHub
fn fetch_latest_release() -> Result<LatestRelease, String> {
    match fetch_from_mirror() {
        Ok(release) => Ok(release),
        Err(mirror_err) => {
            eprintln!("[updater] 镜像服务器不可用，回退 GitHub: {mirror_err}");
            fetch_from_github()
                .map_err(|github_err| format!("镜像: {mirror_err}；GitHub: {github_err}"))
        }
    }
}

/// 镜像服务器清单格式（latest.json）：
/// {
///   "version": "v2026.07.21-2",
///   "setup_url": "https://your-server-domain.com/aether/aether-setup.exe",
///   "sha256": "64位十六进制小写，可选"
/// }
fn fetch_from_mirror() -> Result<LatestRelease, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    let body: serde_json::Value = agent
        .get(MIRROR_MANIFEST_URL)
        .set("User-Agent", "aether-updater")
        .call()
        .map_err(|e| format!("请求清单失败: {e}"))?
        .into_json()
        .map_err(|e| format!("解析清单失败: {e}"))?;
    parse_manifest(&body)
}

fn parse_manifest(body: &serde_json::Value) -> Result<LatestRelease, String> {
    let tag = body
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "清单缺少 version 字段".to_string())?
        .to_string();
    let setup_url = body
        .get("setup_url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "清单缺少 setup_url 字段".to_string())?
        .to_string();
    let sha256 = body
        .get("sha256")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());
    Ok(LatestRelease {
        tag,
        setup_url,
        sha256,
    })
}

fn fetch_from_github() -> Result<LatestRelease, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(15))
        .build();
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let body: serde_json::Value = agent
        .get(&url)
        .set("User-Agent", "aether-updater")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("查询最新版本失败: {e}"))?
        .into_json()
        .map_err(|e| format!("解析版本信息失败: {e}"))?;

    let tag = body
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Release 响应缺少 tag_name".to_string())?
        .to_string();

    let setup_url = body
        .get("assets")
        .and_then(|v| v.as_array())
        .and_then(|assets| {
            assets.iter().find(|a| {
                a.get("name").and_then(|n| n.as_str()) == Some(SETUP_ASSET_NAME)
            })
        })
        .and_then(|a| a.get("browser_download_url"))
        .and_then(|u| u.as_str())
        .ok_or_else(|| format!("Release {tag} 中没有 {SETUP_ASSET_NAME}"))?
        .to_string();

    Ok(LatestRelease {
        tag,
        setup_url,
        sha256: None,
    })
}

fn download_setup(release: &LatestRelease) -> Result<PathBuf, String> {
    // 下载需要允许重定向（GitHub 资产会 302 到 CDN）
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(600))
        .build();
    let response = agent
        .get(&release.setup_url)
        .set("User-Agent", "aether-updater")
        .call()
        .map_err(|e| format!("下载安装包失败: {e}"))?;

    let path = std::env::temp_dir().join(format!("aether-setup-{}.exe", release.tag));
    let mut file = std::fs::File::create(&path)
        .map_err(|e| format!("创建临时文件失败: {e}"))?;
    let mut reader = response.into_reader().take(MAX_SETUP_BYTES);
    std::io::copy(&mut reader, &mut file).map_err(|e| format!("写入安装包失败: {e}"))?;
    drop(file);

    if let Some(expected) = &release.sha256 {
        if let Err(e) = verify_sha256(&path, expected) {
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
    }
    Ok(path)
}

/// 校验文件 SHA256（expected 为十六进制字符串，大小写不敏感）
fn verify_sha256(path: &std::path::Path, expected: &str) -> Result<(), String> {
    use sha2::Digest;
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("读取安装包失败: {e}"))?;
    let mut hasher = sha2::Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| format!("计算校验和失败: {e}"))?;
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected.to_lowercase() {
        return Err(format!("安装包校验和不匹配（期望 {expected}，实际 {actual}）"));
    }
    Ok(())
}

/// 把 "v2026.07.21-1" 之类的版本串拆成数字段 [2026, 7, 21, 1]
fn parse_version(v: &str) -> Vec<u64> {
    v.trim_start_matches('v')
        .split(['.', '-'])
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

/// remote 是否比 local 新（逐段数值比较，缺段按 0 补齐）
fn is_newer(remote: &str, local: &str) -> bool {
    let r = parse_version(remote);
    let l = parse_version(local);
    for i in 0..r.len().max(l.len()) {
        let rv = r.get(i).copied().unwrap_or(0);
        let lv = l.get(i).copied().unwrap_or(0);
        if rv != lv {
            return rv > lv;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer() {
        assert!(is_newer("v2026.07.21-1", "0.1.0"));
        assert!(is_newer("v2026.07.21-2", "v2026.07.21-1"));
        assert!(is_newer("2026.07.22-1", "v2026.07.21-9"));
        assert!(!is_newer("v2026.07.21-1", "v2026.07.21-1"));
        assert!(!is_newer("v2026.07.21-1", "v2026.07.21-2"));
        assert!(!is_newer("0.1.0", "v2026.07.21-1"));
    }

    #[test]
    fn test_parse_manifest() {
        let body = serde_json::json!({
            "version": "v2026.07.21-2",
            "setup_url": "https://example.com/aether/aether-setup.exe",
            "sha256": "ABC123"
        });
        let r = parse_manifest(&body).unwrap();
        assert_eq!(r.tag, "v2026.07.21-2");
        assert_eq!(r.setup_url, "https://example.com/aether/aether-setup.exe");
        assert_eq!(r.sha256.as_deref(), Some("abc123"));

        // sha256 可选
        let body = serde_json::json!({
            "version": "v1.0.0",
            "setup_url": "https://example.com/s.exe"
        });
        assert!(parse_manifest(&body).unwrap().sha256.is_none());

        // 缺字段报错
        assert!(parse_manifest(&serde_json::json!({"setup_url": "x"})).is_err());
        assert!(parse_manifest(&serde_json::json!({"version": "v1"})).is_err());
    }
}
