use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::editor::EditorState;
use crate::launch::LaunchArgs;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::WindowsAndMessaging::*;

// ===== 子模块声明 =====
mod app_icon;
mod ime_handler;
mod keyboard_handler;
mod mouse_handler;
mod window_messages;
mod window_setup;

// ===== 从子模块导入处理函数 =====
use app_icon::load_app_icons;
use ime_handler::*;
use keyboard_handler::*;
use mouse_handler::*;
use window_messages::*;
use window_setup::{
    apply_launch_args, compute_restored_window_rect, enable_dwm_acrylic, get_dpi_scaled_size,
    on_copydata, on_destroy, set_dpi_awareness,
};

// ===== 常量 =====
pub(crate) const CLASS_NAME: &str = "AetherEditor";
pub(crate) const WINDOW_TITLE: &str = "Aether";

/// 长按检测定时器 ID（用于 SetTimer/KillTimer/WM_TIMER）
pub(crate) const LP_TIMER_ID: usize = 0xA001;
/// 终端刷新定时器 ID（终端运行时周期性触发重绘以显示异步输出）
pub(crate) const TERM_TIMER_ID: usize = 0xA002;
/// P3.4: Hover tooltip 防抖定时器 ID
pub(crate) const HOVER_TIMER_ID: usize = 0xA003;
/// 新建项目对话框输入框光标闪烁定时器 ID
pub const CARET_TIMER_ID: usize = 0xA004;
/// AI 后台刷新定时器 ID（流式生成 / 测试连接期间周期性重绘，完成后自动停止）
pub(crate) const AI_TIMER_ID: usize = 0xA005;
/// 语法高亮刷新定时器 ID（打开文件后周期性重绘，直到后台高亮结果到达并着色，随后自动停止）
pub(crate) const HIGHLIGHT_TIMER_ID: usize = 0xA006;
/// AI 对话温数据归档定时器 ID（周期检查空闲会话，归档进 MemoryStore）
pub(crate) const AI_ARCHIVE_TIMER_ID: usize = 0xA007;
/// AI 归档检查间隔（毫秒）：每 5 秒检查一次是否满足「空闲 30 秒」归档条件
pub(crate) const AI_ARCHIVE_MS: u32 = 5000;
/// 长按阈值（毫秒）
pub(crate) const LP_THRESHOLD_MS: u32 = 500;
/// 终端刷新间隔（毫秒），约 20fps 足以实时显示 shell 输出
pub(crate) const TERM_REFRESH_MS: u32 = 50;
/// AI 后台刷新间隔（毫秒），用于流式生成与测试连接期间的平滑重绘
pub(crate) const AI_REFRESH_MS: u32 = 80;
/// 语法高亮刷新间隔（毫秒），约 30fps，让后台高亮结果尽快着色显示
pub(crate) const HIGHLIGHT_REFRESH_MS: u32 = 33;
/// P3.4: Hover tooltip 触发延迟（毫秒）
pub(crate) const HOVER_DELAY_MS: u32 = 500;
/// 长按期间允许的鼠标移动容差（逻辑像素，超过则取消长按检测）
pub(crate) const LP_MOVE_TOLERANCE: f32 = 4.0;
/// P3.4: Hover tooltip 防抖鼠标移动容差（逻辑像素，超过则重新计时）
pub(crate) const HOVER_MOVE_TOLERANCE: f32 = 4.0;

// ===== 共享状态 =====

thread_local! {
    pub(crate) static EDITOR_STATE: RefCell<Option<Rc<RefCell<EditorState>>>> = const { RefCell::new(None) };
    // P2-9: 暂存 WM_CHAR 收到的高代理，等待配对的低代理组合为完整码点
    pub(crate) static PENDING_HIGH_SURROGATE: RefCell<Option<u16>> = const { RefCell::new(None) };
}

/// UI-C02: 全局窗口计数器，防止多窗口时关闭任一窗口就退出整个应用
pub(crate) static WINDOW_COUNT: AtomicUsize = AtomicUsize::new(0);

/// REQ-P1-07: 标记窗口客户区为失效，触发下一次 WM_PAINT 重绘。
///
/// 替代事件处理中的直接 `render()` 调用。Windows 会自动合并多次
/// `InvalidateRect` 为一次 WM_PAINT，天然消除双重渲染。
/// 事件处理只需修改状态 + 调用本函数，实际渲染统一由 WM_PAINT 驱动。
pub(crate) fn invalidate_window(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(hwnd, None, false);
    }
}

/// 设置当前活跃窗口的编辑器状态
pub(crate) fn set_active_state(state: Rc<RefCell<EditorState>>) {
    EDITOR_STATE.with(|s| {
        *s.borrow_mut() = Some(state);
    });
}

/// 从窗口的 GWLP_USERDATA 获取状态，并设为当前活跃状态
/// 使用 ManuallyDrop 防止 panic 时触发 drop 导致 use-after-free
unsafe fn get_window_state(hwnd: HWND) -> Option<Rc<RefCell<EditorState>>> {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RefCell<EditorState>;
    if ptr.is_null() {
        return None;
    }
    // ManuallyDrop 确保即使后续 panic 也不会触发 drop
    let rc = ManuallyDrop::new(Rc::from_raw(ptr));
    let cloned = (*rc).clone();
    // 不调用 ManuallyDrop::into_inner，保持原始引用计数不变
    // 同时设为当前活跃状态
    set_active_state(cloned.clone());
    Some(cloned)
}

/// 从窗口获取状态并设为当前活跃状态（用于消息处理函数）
pub(crate) fn get_and_set_state(hwnd: HWND) -> Option<Rc<RefCell<EditorState>>> {
    unsafe {
        let state = get_window_state(hwnd);
        // 同步 thread_local 到当前窗口状态
        if let Some(ref s) = state {
            set_active_state(s.clone());
        }
        state
    }
}

// ===== 入口函数 =====

pub fn run(args: LaunchArgs) {
    // 初始化日志系统（失败不阻塞启动）
    match crate::logging::init_logging() {
        Ok(_) => {
            tracing::info!("Aether Studio 启动（run 函数入口）");
        }
        Err(e) => {
            eprintln!("警告: 日志初始化失败: {}", e);
            // 即使日志初始化失败，也尝试写入一个临时文件以便调试
            let temp = std::env::temp_dir().join(format!(
                "aether_init_logging_error_{}.txt",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            ));
            let _ = std::fs::write(&temp, format!("{}", e));
        }
    }

    unsafe {
        // 设置 DPI 感知模式（Per-Monitor V2）
        set_dpi_awareness();

        let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();

        let class_name: Vec<u16> = CLASS_NAME.encode_utf16().chain(Some(0)).collect();
        // 加载应用图标（ICO），失败时 hIcon 留 null（使用系统默认）
        let (hicon_big, _hicon_small) = load_app_icons();
        let wc = WNDCLASSW {
            // P2-5: 启用 CS_DBLCLKS 才能收到 WM_LBUTTONDBLCLK 双击消息
            style: CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(std::ptr::null_mut()),
            hIcon: hicon_big.unwrap_or(HICON(std::ptr::null_mut())),
            ..Default::default()
        };

        RegisterClassW(&wc);

        // 创建第一个窗口
        let hwnd = create_editor_window(instance.into(), None);

        // 应用启动参数：打开传入的文件夹或文件
        if let Some(state) = get_and_set_state(hwnd) {
            apply_launch_args(&mut state.borrow_mut(), &args);
            // REQ-P1-07: 触发重绘（apply_launch_args 已标记脏区域）
            invalidate_window(hwnd);
        }

        // 发布构建启动时静默检查更新（仅发现新版时才弹窗；开发构建跳过）
        if crate::updater::IS_RELEASE_BUILD {
            crate::updater::start_check(hwnd, false);
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// 创建一个新的编辑器窗口
///
/// `instance`: 模块实例句柄
/// `owner`: 可选的父窗口句柄
pub(crate) unsafe fn create_editor_window(
    instance: windows::Win32::Foundation::HINSTANCE,
    owner: Option<HWND>,
) -> HWND {
    // P0.2b: 主窗口(无 owner)从持久化 settings 恢复窗口矩形;
    //        子窗口(有 owner)使用默认尺寸,避免多窗口相互覆盖。
    let is_main_window = owner.is_none();
    let settings = if is_main_window {
        aether_shared::settings::AppSettings::load()
    } else {
        aether_shared::settings::AppSettings::default()
    };

    // 获取主显示器 DPI 并计算缩放后的窗口大小（作为回退值）
    let (_dpi_scale, scaled_width, scaled_height) = get_dpi_scaled_size(1280, 800);

    // 计算恢复的窗口矩形（仅在持久化值有效且可见时使用）
    let (restore_x, restore_y, restore_w, restore_h, restore_maximized) =
        compute_restored_window_rect(&settings.ui, scaled_width, scaled_height);

    let class_name: Vec<u16> = CLASS_NAME.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = WINDOW_TITLE.encode_utf16().chain(Some(0)).collect();

    let ex_style = if owner.is_some() {
        WS_EX_APPWINDOW | WS_EX_WINDOWEDGE
    } else {
        WS_EX_APPWINDOW
    };

    let hwnd = CreateWindowExW(
        ex_style,
        windows::core::PCWSTR(class_name.as_ptr()),
        windows::core::PCWSTR(title.as_ptr()),
        WS_POPUP | WS_VISIBLE | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX,
        restore_x,
        restore_y,
        restore_w,
        restore_h,
        // UI-C03: 始终传 NULL 作为 hWndParent。
        // WS_POPUP 样式下，非 NULL 的 hWndParent 会建立真正的 parent-child 关系，
        // 导致关闭父窗口时 Windows 自动销毁所有子窗口（连锁关闭 bug）。
        // owner 参数仅用于 is_main_window 判断，不作为窗口 parent。
        HWND(std::ptr::null_mut()),
        None,
        instance,
        None,
    )
    .unwrap();

    // P0.2b: 若上次退出时处于最大化状态,创建后立即最大化
    if restore_maximized {
        let _ = ShowWindow(hwnd, SW_MAXIMIZE);
    }

    // 启用 DWM Acrylic / Mica 效果
    enable_dwm_acrylic(hwnd);

    // 启用拖拽文件/文件夹到窗口
    unsafe {
        use windows::Win32::UI::Shell::DragAcceptFiles;
        DragAcceptFiles(hwnd, true);
    }

    // 初始化编辑器状态并关联到窗口
    init_editor_state(hwnd, is_main_window);

    hwnd
}

/// 初始化编辑器状态：DPI 缩放、客户区尺寸、渲染目标、GWLP_USERDATA 存储
unsafe fn init_editor_state(hwnd: HWND, is_main_window: bool) {
    let state = EditorState::new(hwnd, is_main_window).unwrap();
    let state_rc = Rc::new(RefCell::new(state));

    // 获取窗口实际 DPI 并计算缩放因子
    {
        use windows::Win32::UI::HiDpi::GetDpiForWindow;
        let dpi = GetDpiForWindow(hwnd);
        let scale = dpi as f32 / 96.0;
        let mut state = state_rc.borrow_mut();
        state.dpi_scale = scale;
        // REQ-P2-07: 布局常量按 DPI 缩放
        state.layout.apply_dpi_scale(scale);
        // REQ-P2-04: IME 候选/合成窗口尺寸按 DPI 缩放
        state.ime.set_dpi_scale(scale);
    }

    // 获取实际客户区物理像素尺寸
    let mut client_rect = RECT::default();
    if GetClientRect(hwnd, &mut client_rect).is_ok() {
        let w = (client_rect.right - client_rect.left) as u32;
        let h = (client_rect.bottom - client_rect.top) as u32;
        if w > 0 && h > 0 {
            state_rc.borrow_mut().resize(w, h);
        }
    }

    // 初始化渲染目标并首次渲染
    {
        let _ = state_rc.borrow_mut().init_render_target();
        state_rc.borrow_mut().render();
    }

    // 设为当前活跃状态
    set_active_state(state_rc.clone());

    // P0-3: 安装低层键盘钩子，确保 Backspace/Delete/方向键不被任意 IME 系统级拦截
    if !crate::keyboard_hook::install(hwnd) {
        tracing::error!("[P0-3] 键盘钩子安装失败！终端里的汉字将无法用 Backspace 删除");
    }

    // 将状态存储到窗口的用户数据区，以便窗口过程可以访问
    // 使用 GWLP_USERDATA 来存储 Rc<RefCell<EditorState>> 的指针
    let state_ptr = Rc::into_raw(state_rc) as *mut RefCell<EditorState> as isize;
    let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr);

    // UI-C02: 窗口成功创建，递增全局计数器
    WINDOW_COUNT.fetch_add(1, Ordering::SeqCst);
}

// ===== 窗口过程 =====

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // C-01: WndProc 是 FFI 边界（extern "system"），任何 panic 穿越此边界均为未定义行为。
    // 使用 catch_unwind 包裹整个函数体，panic 时回退到 DefWindowProcW 以保证进程稳定。
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        // UI-M06: 从窗口 GWLP_USERDATA 获取状态，同步到 thread_local，
        // 防止多窗口消息交错时键盘输入路由到错误窗口
        match msg {
            WM_LBUTTONDOWN => on_l_button_down(hwnd, msg, wparam, lparam),
            WM_MBUTTONDOWN => on_m_button_down(hwnd, msg, wparam, lparam),
            WM_MOUSEMOVE => on_mouse_move(hwnd, msg, wparam, lparam),
            WM_LBUTTONUP => on_l_button_up(hwnd, msg, wparam, lparam),
            WM_LBUTTONDBLCLK => on_l_button_dblclk(hwnd, msg, wparam, lparam),
            WM_RBUTTONDOWN => on_r_button_down(hwnd, msg, wparam, lparam),
            WM_RBUTTONUP => on_r_button_up(hwnd, msg, wparam, lparam),
            WM_TIMER => on_timer(hwnd, msg, wparam, lparam),
            WM_DESTROY => on_destroy(hwnd, msg, wparam, lparam),
            msg if msg == WM_APP + 2 => on_wm_app_2(hwnd, msg, wparam, lparam),
            msg if msg == WM_APP + 3 => on_wm_app_3(hwnd, msg, wparam, lparam),
            msg if msg == WM_APP + 4 => on_wm_app_4(hwnd, msg, wparam, lparam),
            msg if msg == WM_APP + 5 => on_wm_app_5(hwnd, msg, wparam, lparam),
            msg if msg == WM_APP + 6 => on_wm_app_6(hwnd, msg, wparam, lparam),
            msg if msg == WM_APP + 7 => on_wm_app_7(hwnd, msg, wparam, lparam),
            msg if msg == crate::updater::WM_UPDATE_CHECK_DONE => {
                on_wm_app_8(hwnd, msg, wparam, lparam)
            }
            // P0-3: 低层键盘钩子投递给主窗口的自定义消息 - 终端直接接收编辑键
            msg if msg == crate::keyboard_hook::WM_TERMINAL_BACKSPACE => {
                crate::keyboard_hook::handle_backspace_msg(hwnd)
            }
            msg if msg == crate::keyboard_hook::WM_TERMINAL_DELETE => {
                crate::keyboard_hook::handle_delete_msg(hwnd)
            }
            msg if msg == crate::keyboard_hook::WM_TERMINAL_ARROW => {
                crate::keyboard_hook::handle_arrow_msg(hwnd, wparam.0 as u32)
            }
            WM_DROPFILES => on_dropfiles(hwnd, msg, wparam, lparam),
            WM_COPYDATA => on_copydata(hwnd, msg, wparam, lparam),
            WM_SIZE => on_size(hwnd, msg, wparam, lparam),
            WM_DPICHANGED => on_dpichanged(hwnd, msg, wparam, lparam),
            WM_NCACTIVATE => on_ncactivate(hwnd, msg, wparam, lparam),
            WM_NCCALCSIZE => on_nccalcsize(hwnd, msg, wparam, lparam),
            WM_NCHITTEST => on_nchittest(hwnd, msg, wparam, lparam),
            WM_ERASEBKGND => on_erasebkgnd(hwnd, msg, wparam, lparam),
            WM_PAINT => on_paint(hwnd, msg, wparam, lparam),
            WM_IME_STARTCOMPOSITION => on_ime_startcomposition(hwnd, msg, wparam, lparam),
            WM_IME_COMPOSITION => on_ime_composition(hwnd, msg, wparam, lparam),
            WM_IME_ENDCOMPOSITION => on_ime_endcomposition(hwnd, msg, wparam, lparam),
            WM_IME_CHAR => on_ime_char(hwnd, msg, wparam, lparam),
            WM_CHAR => on_char(hwnd, msg, wparam, lparam),
            WM_KEYDOWN => on_key_down(hwnd, msg, wparam, lparam),
            WM_MOUSEWHEEL => on_mouse_wheel(hwnd, msg, wparam, lparam),
            WM_MOUSEHWHEEL => on_mouse_hwheel(hwnd, msg, wparam, lparam),
            WM_SETCURSOR => on_setcursor(hwnd, msg, wparam, lparam),
            WM_SETFOCUS => on_set_focus(hwnd, msg, wparam, lparam),
            WM_KILLFOCUS => on_kill_focus(hwnd, msg, wparam, lparam),
            _ => on_default(hwnd, msg, wparam, lparam),
        }
    }));
    match result {
        Ok(lr) => lr,
        Err(panic_payload) => {
            // 记录诊断信息后回退到默认处理，避免 panic 穿越 FFI 边界
            let msg_str = panic_payload
                .downcast_ref::<&'static str>()
                .copied()
                .or_else(|| panic_payload.downcast_ref::<String>().map(|s| s.as_str()))
                .unwrap_or("<non-string panic>");
            eprintln!(
                "[C-01] window_proc panic recovered (msg={}): {}",
                msg, msg_str
            );
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}
