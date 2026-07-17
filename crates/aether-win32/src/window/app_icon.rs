//! 应用图标加载
//!
//! 编译期通过 `include_bytes!` 嵌入多尺寸 ICO（16/32/48/64/128/256），
//! 运行期写到 exe 同目录 `resources/app_icons/aether.ico` 后用
//! `LoadImageW(LR_LOADFROMFILE)` 加载，最后作为窗口类的大图标（任务栏/Alt+Tab）
//! 和小图标（标题栏）使用。
//!
//! DPI 自适应：调用 `GetDpiForSystem()` 获取系统 DPI，按比例选择请求尺寸，
//! 让 Windows 直接从 ICO 中挑选最接近的位图（不再无谓放大低分辨率位图）。
//!
//! 之所以不直接编译到 .exe 资源段：避免引入 `winres`/`embed-resource` 等
//! 编译期资源依赖，所有产物集中在 .ico 文件 + `include_bytes!` 中，可移植性最好。

use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{LoadImageW, HICON, IMAGE_ICON, LR_LOADFROMFILE};

/// 嵌入的多尺寸 ICO 数据
const APP_ICO_BYTES: &[u8] = include_bytes!("../../resources/app_icons/aether.ico");

/// ICO 中可用的位图尺寸（按升序排列，用于 DPI 向上取最近匹配）
const ICO_AVAILABLE_SIZES: &[u32] = &[16, 32, 48, 64, 128, 256];

/// 根据请求的尺寸，从 ICO 可用尺寸中挑选最接近的一个（向上优先）
fn pick_nearest_ico_size(requested: u32) -> u32 {
    for &s in ICO_AVAILABLE_SIZES {
        if s >= requested {
            return s;
        }
    }
    *ICO_AVAILABLE_SIZES.last().unwrap()
}

/// 按系统 DPI 计算大图标请求尺寸（基线 32px @ 96 DPI）
fn big_icon_size_for_dpi() -> u32 {
    let dpi = unsafe { GetDpiForSystem() };
    let scaled = 32 * dpi / 96;
    pick_nearest_ico_size(scaled.max(32))
}

/// 按系统 DPI 计算小图标请求尺寸（基线 16px @ 96 DPI）
fn small_icon_size_for_dpi() -> u32 {
    let dpi = unsafe { GetDpiForSystem() };
    let scaled = 16 * dpi / 96;
    pick_nearest_ico_size(scaled.max(16))
}

/// 把嵌入的 ICO 字节写到磁盘临时位置，返回 HICON 列表（大图标 + 小图标）
///
/// 第二次及以后调用会复用已写出的文件。失败时返回 `(None, None)`，
/// 调用方应回退到默认图标（不显式设置 hIcon 即可）。
pub(crate) fn load_app_icons() -> (Option<HICON>, Option<HICON>) {
    let ico_path = match ensure_ico_on_disk() {
        Some(p) => p,
        None => return (None, None),
    };

    let path_w: Vec<u16> = ico_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // DPI 感知的请求尺寸：让 Windows 从 ICO 中挑最接近的高分辨率位图
    let big_size = big_icon_size_for_dpi() as i32;
    let small_size = small_icon_size_for_dpi() as i32;

    unsafe {
        // 大图标：任务栏 / Alt+Tab（DPI 缩放后取最近 ICO 尺寸）
        let hicon_big = LoadImageW(
            HINSTANCE(std::ptr::null_mut()),
            PCWSTR(path_w.as_ptr()),
            IMAGE_ICON,
            big_size,
            big_size,
            LR_LOADFROMFILE,
        )
        .ok()
        .map(|h| HICON(h.0));

        // 小图标：标题栏（DPI 缩放后取最近 ICO 尺寸）
        let hicon_small = LoadImageW(
            HINSTANCE(std::ptr::null_mut()),
            PCWSTR(path_w.as_ptr()),
            IMAGE_ICON,
            small_size,
            small_size,
            LR_LOADFROMFILE,
        )
        .ok()
        .map(|h| HICON(h.0));

        (hicon_big, hicon_small)
    }
}

fn ensure_ico_on_disk() -> Option<PathBuf> {
    // 优先放在 exe 同目录（开发期），不存在则回退到 %TEMP%\Aether\
    let target_dir = if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            parent.join("resources").join("app_icons")
        } else {
            temp_aether_dir()
        }
    } else {
        temp_aether_dir()
    };

    if let Err(e) = std::fs::create_dir_all(&target_dir) {
        eprintln!("[app_icon] 创建图标目录失败: {e}");
        return None;
    }

    let ico_path = target_dir.join("aether.ico");
    if !ico_path.exists() {
        if let Err(e) = std::fs::write(&ico_path, APP_ICO_BYTES) {
            eprintln!("[app_icon] 写出图标失败: {e}");
            return None;
        }
    }
    Some(ico_path)
}

fn temp_aether_dir() -> PathBuf {
    std::env::temp_dir().join("Aether")
}
