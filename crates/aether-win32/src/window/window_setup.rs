//! 窗口创建与设置相关函数。
//!
//! 从 `window.rs` 拆分而来，保持原有逻辑不变。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;

use crate::editor::EditorState;
use crate::launch::{copydata_result, parse_copydata_lparam, LaunchArgs};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{get_and_set_state, invalidate_window, EDITOR_STATE, WINDOW_COUNT};

/// 设置 DPI 感知模式
pub(crate) fn set_dpi_awareness() {
    unsafe {
        // 尝试设置 Per-Monitor V2 DPI 感知（Windows 10 1607+）
        use windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext;
        use windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;
        use windows::Win32::UI::HiDpi::{SetProcessDpiAwareness, PROCESS_PER_MONITOR_DPI_AWARE};

        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_err() {
            // V2 失败时回退到 Per-Monitor DPI 感知（Windows 8.1+）
            let _ = SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
        }
    }
}

/// 启用 DWM Acrylic / Mica  backdrop 效果
pub(crate) fn enable_dwm_acrylic(hwnd: HWND) {
    unsafe {
        // DWM 属性常量
        const DWBT_MAINWINDOW: u32 = 0;

        // 启用沉浸式暗色模式
        let dark_mode: windows::Win32::Foundation::BOOL = true.into();
        let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            hwnd,
            windows::Win32::Graphics::Dwm::DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_mode as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<windows::Win32::Foundation::BOOL>() as u32,
        );

        // Windows 11: 使用主机 backdrop brush (Acrylic/Mica)
        let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            hwnd,
            windows::Win32::Graphics::Dwm::DWMWA_USE_HOSTBACKDROPBRUSH,
            &DWBT_MAINWINDOW as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );

        // Windows 11 22H2+: DWMWA_SYSTEMBACKDROP_TYPE (38)
        // 1 = Auto, 2 = Mica, 3 = Acrylic, 4 = Mica Alt
        // 使用 Mica Alt (4) 让标题栏 + 客户区都能透出 backdrop
        let backdrop_type: u32 = 4;
        let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            hwnd,
            windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE(38),
            &backdrop_type as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );

        // Windows 11 备选：Mica 效果（旧 attribute 1029，兼容更早版本）
        let mica_enabled: windows::Win32::Foundation::BOOL = true.into();
        let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            hwnd,
            windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE(1029i32),
            &mica_enabled as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<windows::Win32::Foundation::BOOL>() as u32,
        );

        // 启用客户区穿透：让 DWM backdrop 能在透明 clear 区域透出
        // DWMWA_ALLOW_WINDOW_CLIENT_AREA_TO_TRANSPARENT_BLUR (unused constant, value 35)
        // 但更可靠的是 DWMWA_NCRENDERING_POLICY = DWMNCRP_DISABLED
        let nc_policy: u32 = 2; // DWMNCRP_DISABLED
        let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            hwnd,
            windows::Win32::Graphics::Dwm::DWMWA_NCRENDERING_POLICY,
            &nc_policy as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

/// 获取 DPI 缩放比例和缩放后的窗口大小
pub(crate) fn get_dpi_scaled_size(base_width: i32, base_height: i32) -> (f32, i32, i32) {
    unsafe {
        use windows::Win32::UI::HiDpi::GetDpiForSystem;

        let dpi = GetDpiForSystem();
        let scale = dpi as f32 / 96.0;
        let scaled_width = (base_width as f32 * scale) as i32;
        let scaled_height = (base_height as f32 * scale) as i32;
        (scale, scaled_width, scaled_height)
    }
}

/// P0.2b: 计算从持久化 settings 恢复的窗口矩形。
///
/// 返回 `(x, y, width, height, maximized)`。
/// - 若 `window_maximized` 为 true,矩形使用默认值(最大化由后续 ShowWindow 处理)
/// - 若 x/y/width/height 全部存在且构造的矩形至少部分位于某显示器内,使用持久化值
/// - 否则回退到 `CW_USEDEFAULT` 与 DPI 缩放后的默认尺寸
///
/// 显示器边界校验使用 `MonitorFromRect` + `MONITOR_DEFAULTTONULL`,
/// 仅当矩形与某显示器有交集时才认为有效,避免窗口出现在已拔出的显示器上。
pub(crate) fn compute_restored_window_rect(
    ui: &aether_shared::settings::UiSettings,
    default_width: i32,
    default_height: i32,
) -> (i32, i32, i32, i32, bool) {
    // 最大化优先:矩形值交给系统,创建后通过 SW_MAXIMIZE 恢复
    if ui.window_maximized {
        return (
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            default_width,
            default_height,
            true,
        );
    }

    let (Some(&x), Some(&y), Some(&w), Some(&h)) = (
        ui.window_x.as_ref(),
        ui.window_y.as_ref(),
        ui.window_width.as_ref(),
        ui.window_height.as_ref(),
    ) else {
        return (
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            default_width,
            default_height,
            false,
        );
    };

    // 拒绝明显异常的尺寸(过小或负值)
    if w < 200 || h < 150 {
        return (
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            default_width,
            default_height,
            false,
        );
    }

    let w_i = w as i32;
    let h_i = h as i32;
    let rect = RECT {
        left: x,
        top: y,
        right: x.saturating_add(w_i),
        bottom: y.saturating_add(h_i),
    };

    // 校验矩形是否落在某个已连接显示器内(MONITOR_DEFAULTTONULL: 无交集返回 NULL)
    unsafe {
        use windows::Win32::Graphics::Gdi::{MonitorFromRect, MONITOR_DEFAULTTONULL};
        let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONULL);
        if monitor.is_invalid() {
            // 矩形不在任何显示器上(可能显示器已断开),回退默认
            return (
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                default_width,
                default_height,
                false,
            );
        }
    }

    (x, y, w_i, h_i, false)
}

/// P0.2c: 主窗口退出前持久化窗口状态到 settings.json。
///
/// 持久化内容:
/// - `window_maximized`: 当前是否最大化
/// - `window_x/y/width/height`: 仅在非最大化时更新(最大化时保留上次的正常矩形)
/// - `last_workspace`: 当前打开的工作区路径(若有)
///
/// 保存失败仅打印警告,不阻塞退出流程。
pub(crate) fn persist_window_state(state: &EditorState, hwnd: HWND) {
    let mut settings = state.app_settings.clone();

    // 获取窗口矩形(屏幕坐标,包含非客户区)
    let mut rect = RECT::default();
    let rect_ok =
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect).is_ok() };
    if rect_ok {
        // 最大化时只记录最大化标志,不覆盖正常矩形(下次启动用 SW_MAXIMIZE 恢复)
        if !state.is_maximized {
            let w = (rect.right - rect.left).max(0) as u32;
            let h = (rect.bottom - rect.top).max(0) as u32;
            // 拒绝异常尺寸(例如最小化后的退化矩形)
            if w >= 200 && h >= 150 {
                settings.ui.window_x = Some(rect.left);
                settings.ui.window_y = Some(rect.top);
                settings.ui.window_width = Some(w);
                settings.ui.window_height = Some(h);
            }
        }
        settings.ui.window_maximized = state.is_maximized;
    }

    // 持久化当前工作区(若已打开文件夹)
    settings.ui.last_workspace = state.current_folder.clone();

    if let Err(e) = settings.save() {
        eprintln!("警告: 持久化窗口状态失败: {}", e);
    }
    // AI 对话历史由温数据层（SQLite）实时归档，退出时 WarmDataStore Drop 自动 flush，无需额外处理
}

/// 应用 CLI 传入的启动参数到指定编辑器状态
pub(crate) fn apply_launch_args(state: &mut EditorState, args: &LaunchArgs) {
    let mut loaded_file_for_goto: Option<std::path::PathBuf> = None;

    for path in &args.paths {
        if path.is_dir() {
            state.open_folder(path.clone());
        } else if path.is_file() {
            // 文件：先打开所在文件夹作为工作区，再加载文件到标签页
            if let Some(parent) = path.parent() {
                state.open_folder(parent.to_path_buf());
            }
            state.load_file(path.clone());
            if args.goto.is_some() && loaded_file_for_goto.is_none() {
                loaded_file_for_goto = Some(path.clone());
            }
        }
    }

    // 如果提供了 goto，且第一个路径就是文件，则直接跳转
    // 如果 goto 指向的文件不是第一个路径，需要在后续扩展中支持按文件名匹配
    if let Some(goto) = args.goto {
        let target_file = loaded_file_for_goto.or_else(|| args.paths.first().cloned());
        if target_file.map(|p| p.is_file()).unwrap_or(false) {
            state.goto_position(goto.line, goto.column);
        }
    }

    // REQ-P1-07: 不直接调用 render()，标记全窗口脏区域，由调用方触发 WM_PAINT
    state.dirty_tracker.mark_full_window();
}

/// WM_COPYDATA：接收来自第二个实例或 CLI 的启动参数
pub(crate) unsafe fn on_copydata(
    hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if let Some(args) = parse_copydata_lparam(lparam) {
        if let Some(state) = get_and_set_state(hwnd) {
            apply_launch_args(&mut state.borrow_mut(), &args);
            // REQ-P1-07: 触发重绘（apply_launch_args 已标记脏区域）
            invalidate_window(hwnd);
            return copydata_result(true);
        }
    }
    copydata_result(false)
}

/// WM_DESTROY
pub(crate) unsafe fn on_destroy(
    hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    // 卸载低层键盘钩子（仅当主窗口销毁时，引用计数为 0 时真正卸载）
    crate::keyboard_hook::uninstall();
    // 释放窗口关联的编辑器状态
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RefCell<EditorState>;
    if !ptr.is_null() {
        // P0.2c: 主窗口退出前持久化窗口状态(矩形/最大化/工作区)。
        // 用 Rc::from_raw 取回所有权,在 drop 之前完成持久化。
        let rc = Rc::from_raw(ptr);
        // 清理当前线程的活跃状态引用，避免已释放 Rc 残留在 thread_local
        let clear_active = EDITOR_STATE.with(|s| {
            s.borrow()
                .as_ref()
                .map(|active| Rc::ptr_eq(active, &rc))
                .unwrap_or(false)
        });
        if clear_active {
            EDITOR_STATE.with(|s| *s.borrow_mut() = None);
        }
        {
            let state = rc.borrow();
            if state.is_main_window {
                persist_window_state(&state, hwnd);
            }
        }
        // 显式 drop,减少引用计数(与原 let _ = Rc::from_raw(ptr) 等价)
        drop(rc);
        let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
    }
    // UI-C02: 仅当所有窗口都关闭时才退出应用程序
    // L-01: 使用 compare_exchange 防止 fetch_sub 下溢回绕到 usize::MAX，导致 PostQuitMessage 永不触发
    loop {
        let current = WINDOW_COUNT.load(Ordering::SeqCst);
        if current == 0 {
            // 计数器已为 0，异常状态，不再减避免下溢
            break;
        }
        if WINDOW_COUNT
            .compare_exchange(current, current - 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            if current == 1 {
                PostQuitMessage(0);
            }
            break;
        }
        // CAS 失败，重试
    }
    LRESULT(0)
}
