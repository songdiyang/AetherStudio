use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::dialogs::Dialogs;
use crate::editor::EditorState;

const CLASS_NAME: &str = "AetherEditor";
const WINDOW_TITLE: &str = "Aether";

/// 长按检测定时器 ID（用于 SetTimer/KillTimer/WM_TIMER）
const LP_TIMER_ID: usize = 0xA001;
/// 终端刷新定时器 ID（终端运行时周期性触发重绘以显示异步输出）
const TERM_TIMER_ID: usize = 0xA002;
/// 长按阈值（毫秒）
const LP_THRESHOLD_MS: u32 = 500;
/// 终端刷新间隔（毫秒），约 20fps 足以实时显示 shell 输出
const TERM_REFRESH_MS: u32 = 50;
/// 长按期间允许的鼠标移动容差（逻辑像素，超过则取消长按检测）
const LP_MOVE_TOLERANCE: f32 = 4.0;

/// 设置 DPI 感知模式
fn set_dpi_awareness() {
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
fn enable_dwm_acrylic(hwnd: HWND) {
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
fn get_dpi_scaled_size(base_width: i32, base_height: i32) -> (f32, i32, i32) {
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
fn compute_restored_window_rect(
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
fn persist_window_state(state: &EditorState, hwnd: HWND) {
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
}

thread_local! {
    static EDITOR_STATE: RefCell<Option<Rc<RefCell<EditorState>>>> = RefCell::new(None);
    // P2-9: 暂存 WM_CHAR 收到的高代理，等待配对的低代理组合为完整码点
    static PENDING_HIGH_SURROGATE: RefCell<Option<u16>> = RefCell::new(None);
}

/// UI-C02: 全局窗口计数器，防止多窗口时关闭任一窗口就退出整个应用
static WINDOW_COUNT: AtomicUsize = AtomicUsize::new(0);

/// 设置当前活跃窗口的编辑器状态
fn set_active_state(state: Rc<RefCell<EditorState>>) {
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

pub fn run() {
    unsafe {
        // 设置 DPI 感知模式（Per-Monitor V2）
        set_dpi_awareness();

        let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();

        let class_name: Vec<u16> = CLASS_NAME.encode_utf16().chain(Some(0)).collect();
        let wc = WNDCLASSW {
            // P2-5: 启用 CS_DBLCLKS 才能收到 WM_LBUTTONDBLCLK 双击消息
            style: CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(std::ptr::null_mut()),
            ..Default::default()
        };

        RegisterClassW(&wc);

        // 创建第一个窗口
        create_editor_window(instance.into(), None);

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
unsafe fn create_editor_window(
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

    let state = EditorState::new(hwnd, is_main_window).unwrap();
    let state_rc = Rc::new(RefCell::new(state));

    // 将状态存储到窗口的用户数据区，以便窗口过程可以访问
    // 使用 GWL_USERDATA 来存储 Rc<RefCell<EditorState>> 的指针
    let state_ptr = Rc::into_raw(state_rc) as *mut RefCell<EditorState> as isize;
    let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr);

    // 获取窗口实际 DPI 并计算缩放因子
    {
        use windows::Win32::UI::HiDpi::GetDpiForWindow;
        let dpi = GetDpiForWindow(hwnd);
        let scale = dpi as f32 / 96.0;
        // UI-H01: 使用 ManuallyDrop 保护，防止 borrow_mut panic 时 Rc 被释放导致 Use-after-free
        let state_ref = ManuallyDrop::new(Rc::from_raw(state_ptr as *mut RefCell<EditorState>));
        state_ref.borrow_mut().dpi_scale = scale;
        let _ = Rc::into_raw(ManuallyDrop::into_inner(state_ref));
    }

    // 获取实际客户区物理像素尺寸
    let mut client_rect = RECT::default();
    if GetClientRect(hwnd, &mut client_rect).is_ok() {
        let w = (client_rect.right - client_rect.left) as u32;
        let h = (client_rect.bottom - client_rect.top) as u32;
        if w > 0 && h > 0 {
            // UI-H01: 使用 ManuallyDrop 保护
            let state_ref = ManuallyDrop::new(Rc::from_raw(state_ptr as *mut RefCell<EditorState>));
            state_ref.borrow_mut().resize(w, h);
            let _ = Rc::into_raw(ManuallyDrop::into_inner(state_ref));
        }
    }

    // 初始化渲染目标并首次渲染
    {
        let state_ref = ManuallyDrop::new(Rc::from_raw(state_ptr as *mut RefCell<EditorState>));
        let _ = state_ref.borrow_mut().init_render_target();
        state_ref.borrow_mut().render();
        // 设为当前活跃状态
        let state_ref = ManuallyDrop::new(Rc::from_raw(state_ptr as *mut RefCell<EditorState>));
        set_active_state((*state_ref).clone());
        // 不调用 ManuallyDrop::into_inner，保持原始引用计数不变
    }

    // UI-C02: 窗口成功创建，递增全局计数器
    WINDOW_COUNT.fetch_add(1, Ordering::SeqCst);

    hwnd
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        // UI-M06: 从窗口 GWLP_USERDATA 获取状态，同步到 thread_local，
        // 防止多窗口消息交错时键盘输入路由到错误窗口
        let get_state = || -> Option<Rc<RefCell<EditorState>>> {
            let state = get_window_state(hwnd);
            // 同步 thread_local 到当前窗口状态
            if let Some(ref s) = state {
                set_active_state(s.clone());
            }
            state
        };

        match msg {
            WM_LBUTTONDOWN => {
                let raw_x = (lparam.0 & 0xFFFF) as i16 as f32;
                let raw_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                if let Some(state) = get_state() {
                    let mut st = state.borrow_mut();
                    // 默认取消终端焦点，只有点击底部面板时才聚焦
                    st.terminal_panel.focused = false;
                    // 标记鼠标左键按下（用于 WM_TIMER 长按判定）
                    st.lbutton_down = true;
                    // 将物理像素转换为逻辑像素(DIP)
                    let mouse_x = raw_x / st.dpi_scale;
                    let mouse_y = raw_y / st.dpi_scale;
                    let layout = st.layout.clone();
                    // 自定义模式下，点击所属区域之外 → 退出自定义模式
                    let _activity_region_lp = layout.activity_bar_region();
                    let _titlebar_region_lp = layout.title_bar_region();
                    if st.activity_bar.customize_mode
                        && !_activity_region_lp.contains(mouse_x, mouse_y)
                    {
                        st.activity_bar.exit_customize();
                    }
                    if st.menu_bar.customize_mode && !_titlebar_region_lp.contains(mouse_x, mouse_y)
                    {
                        st.menu_bar.exit_customize();
                    }

                    // 对话框优先拦截点击
                    if st.ssh_dialog.visible {
                        if let Some(action) = st.handle_ssh_dialog_click(mouse_x, mouse_y) {
                            match action {
                                crate::ssh::DialogAction::Connect => {
                                    // C-09: SSH 连接移至后台线程，避免阻塞 UI
                                    if st.ssh_connecting {
                                        // 正在连接中，忽略重复点击
                                    } else if let Some(config) = st.ssh_dialog.to_config() {
                                        st.start_ssh_connect(config);
                                    } else {
                                        st.ssh_dialog.error_message =
                                            Some("请填写主机和用户名".to_string());
                                    }
                                }
                                crate::ssh::DialogAction::Cancel => {
                                    st.ssh_dialog.visible = false;
                                }
                                crate::ssh::DialogAction::None => {}
                            }
                        }
                        drop(st);
                        state.borrow_mut().render();
                        return LRESULT(0);
                    }

                    if st.clone_dialog.visible {
                        if let Some(action) = st.handle_clone_dialog_click(mouse_x, mouse_y) {
                            match action {
                                crate::ssh::DialogAction::Connect => {
                                    if st.clone_dialog.url.is_empty() {
                                        st.clone_dialog.error_message =
                                            Some("请输入仓库 URL".to_string());
                                    } else if st.git_cloning {
                                        // C-09: 正在克隆中，忽略重复点击
                                    } else {
                                        // 打开文件夹选择对话框
                                        drop(st);
                                        if let Some(target_path) =
                                            crate::dialogs::Dialogs::open_folder_dialog(
                                                hwnd,
                                                "选择克隆目标文件夹",
                                            )
                                        {
                                            // C-09: Git 克隆移至后台线程，避免阻塞 UI
                                            let mut st = state.borrow_mut();
                                            let url = st.clone_dialog.url.clone();
                                            st.start_git_clone(url, target_path);
                                            drop(st);
                                            state.borrow_mut().render();
                                            return LRESULT(0);
                                        }
                                        // 文件夹对话框取消
                                        state.borrow_mut().render();
                                        return LRESULT(0);
                                    }
                                }
                                crate::ssh::DialogAction::Cancel => {
                                    st.clone_dialog.visible = false;
                                }
                                crate::ssh::DialogAction::None => {}
                            }
                        }
                        drop(st);
                        state.borrow_mut().render();
                        return LRESULT(0);
                    }

                    // 0. 检测标题栏区域点击（包含菜单项和窗口控制按钮）
                    let titlebar_region = layout.title_bar_region();
                    if titlebar_region.contains(mouse_x, mouse_y) {
                        let btn_width = 40.0;
                        let close_x = titlebar_region.x + titlebar_region.width - btn_width;
                        let maximize_x = close_x - btn_width;
                        let minimize_x = maximize_x - btn_width;

                        // 先检测是否点击了窗口控制按钮区域
                        let panel_btn_width = 28.0;
                        let right_panel_btn_x = minimize_x - panel_btn_width;
                        let bottom_panel_btn_x = right_panel_btn_x - panel_btn_width;
                        let left_sidebar_btn_x = bottom_panel_btn_x - panel_btn_width;
                        // 用户按钮
                        let user_btn_size = 26.0;
                        let user_btn_x = minimize_x - 28.0 * 3.0 - user_btn_size - 4.0;
                        let user_btn_y =
                            titlebar_region.y + (titlebar_region.height - user_btn_size) / 2.0;

                        if mouse_x >= user_btn_x
                            && mouse_x < user_btn_x + user_btn_size
                            && mouse_y >= user_btn_y
                            && mouse_y < user_btn_y + user_btn_size
                        {
                            // 点击用户头像按钮，切换菜单
                            st.user_menu.toggle();
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }

                        // 关闭用户菜单（如果打开）
                        if st.user_menu.is_open {
                            st.user_menu.close();
                        }

                        if mouse_x >= minimize_x {
                            if mouse_x >= close_x {
                                // 关闭窗口
                                drop(st);
                                let _ = DestroyWindow(hwnd);
                                return LRESULT(0);
                            } else if mouse_x >= maximize_x {
                                // 最大化/还原
                                let is_max = st.is_maximized;
                                drop(st);
                                if is_max {
                                    let _ = ShowWindow(hwnd, SW_RESTORE);
                                } else {
                                    let _ = ShowWindow(hwnd, SW_MAXIMIZE);
                                }
                                return LRESULT(0);
                            } else {
                                // 最小化
                                drop(st);
                                let _ = ShowWindow(hwnd, SW_MINIMIZE);
                                return LRESULT(0);
                            }
                        } else if mouse_x >= right_panel_btn_x {
                            // 切换右侧 AI 面板可见性
                            st.layout.toggle_right_panel();
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        } else if mouse_x >= bottom_panel_btn_x {
                            // 切换底部终端面板可见性
                            st.layout.toggle_terminal_panel();
                            if st.layout.bottom_panel_visible {
                                st.terminal_panel.focused = true;
                                if !st.terminal_panel.running {
                                    let _ = st.terminal_panel.start();
                                }
                                let _ = SetTimer(hwnd, TERM_TIMER_ID, TERM_REFRESH_MS, None);
                            } else {
                                st.terminal_panel.focused = false;
                                let _ = KillTimer(hwnd, TERM_TIMER_ID);
                            }
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        } else if mouse_x >= left_sidebar_btn_x {
                            // 切换左侧侧边栏可见性
                            st.layout.toggle_sidebar();
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }

                        // 检测是否点击了菜单项
                        if let Some(idx) = st.menu_bar.hit_test(
                            mouse_x,
                            mouse_y - titlebar_region.y,
                            titlebar_region.height,
                        ) {
                            // 长按检测：记录按下信息并启动定时器
                            st.lpress_start = Some(std::time::Instant::now());
                            st.lpress_x = mouse_x;
                            st.lpress_y = mouse_y;
                            st.lpress_target = Some(crate::input::PressTarget::MenuBar);
                            st.lpress_index = idx;
                            let _ = SetTimer(hwnd, LP_TIMER_ID, LP_THRESHOLD_MS, None);

                            // 自定义模式下：不展开子菜单，而是开始拖拽
                            if st.menu_bar.customize_mode {
                                st.menu_bar.begin_drag(idx);
                                drop(st);
                                state.borrow_mut().render();
                                return LRESULT(0);
                            }

                            let was_active = st.menu_bar.active_index == Some(idx);
                            st.menu_bar.close_all();
                            if !was_active {
                                st.menu_bar.expand(idx);
                            }
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }

                        // 标题栏拖动开始（点击了标题栏但非按钮/菜单区域）
                        st.menu_bar.close_all();
                        drop(st);
                        let _ = ReleaseCapture();
                        let _ = SendMessageW(
                            hwnd,
                            WM_NCLBUTTONDOWN,
                            WPARAM(HTCAPTION as usize),
                            LPARAM(0),
                        );
                        return LRESULT(0);
                    }

                    // 检测用户菜单项点击
                    if st.user_menu.is_open {
                        if let Some(idx) = st.user_menu.hit_test_menu(mouse_x, mouse_y) {
                            let item = st.user_menu.items[idx].clone();
                            match item {
                                crate::user_menu::UserMenuItem::EditorSettings => {
                                    st.user_menu.close();
                                    st.sidebar_content =
                                        crate::layout::SidebarContent::RemoteManagerPanel;
                                    drop(st);
                                    state.borrow_mut().render();
                                    return LRESULT(0);
                                }
                                crate::user_menu::UserMenuItem::AetherSettings => {
                                    st.user_menu.close();
                                    st.status_message = "Aether 设置（待实现）".to_string();
                                    drop(st);
                                    state.borrow_mut().render();
                                    return LRESULT(0);
                                }
                                crate::user_menu::UserMenuItem::HelpDocs => {
                                    st.user_menu.close();
                                    st.status_message = "帮助文档（待实现）".to_string();
                                    drop(st);
                                    state.borrow_mut().render();
                                    return LRESULT(0);
                                }
                                crate::user_menu::UserMenuItem::FeatureRequest => {
                                    st.user_menu.close();
                                    st.status_message = "提交功能建议（待实现）".to_string();
                                    drop(st);
                                    state.borrow_mut().render();
                                    return LRESULT(0);
                                }
                                crate::user_menu::UserMenuItem::BugReport => {
                                    st.user_menu.close();
                                    st.status_message = "问题反馈（待实现）".to_string();
                                    drop(st);
                                    state.borrow_mut().render();
                                    return LRESULT(0);
                                }
                                crate::user_menu::UserMenuItem::Logout => {
                                    st.user_menu.close();
                                    st.status_message = "退出登录（待实现）".to_string();
                                    drop(st);
                                    state.borrow_mut().render();
                                    return LRESULT(0);
                                }
                                _ => {}
                            }
                        } else {
                            // 点击菜单外部，关闭菜单
                            st.user_menu.close();
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }
                    }

                    // 1. 检测子菜单点击（子菜单在标题栏下方弹出）
                    if let Some(active_idx) = st.menu_bar.active_index {
                        if let Some(&submenu_x) = st.menu_bar.item_x_positions.get(active_idx) {
                            let submenu_y = titlebar_region.y + titlebar_region.height;
                            if let Some(sub_idx) = st.menu_bar.hit_test_submenu(
                                active_idx, mouse_x, mouse_y, submenu_x, submenu_y,
                            ) {
                                if let Some(item) = st.menu_bar.items.get(active_idx) {
                                    if let Some(menu_item) = item.items.get(sub_idx) {
                                        if menu_item.enabled
                                            && menu_item.command_id
                                                != crate::menu_bar::CommandId::None
                                        {
                                            let cmd = menu_item.command_id;
                                            st.menu_bar.close_all();
                                            drop(st);
                                            state.borrow_mut().execute_command(cmd, hwnd);
                                            state.borrow_mut().render();
                                            return LRESULT(0);
                                        }
                                    }
                                }
                            }
                        }
                        st.menu_bar.close_all();
                        drop(st);
                        state.borrow_mut().render();
                        return LRESULT(0);
                    }

                    // 2. 检测活动栏点击
                    let activity_region = layout.activity_bar_region();
                    if activity_region.contains(mouse_x, mouse_y) {
                        if let Some(idx) =
                            st.activity_bar
                                .hit_test(mouse_x, mouse_y, activity_region.y)
                        {
                            // 长按检测：记录按下信息并启动定时器
                            st.lpress_start = Some(std::time::Instant::now());
                            st.lpress_x = mouse_x;
                            st.lpress_y = mouse_y;
                            st.lpress_target = Some(crate::input::PressTarget::ActivityBar);
                            st.lpress_index = idx;
                            let _ = SetTimer(hwnd, LP_TIMER_ID, LP_THRESHOLD_MS, None);

                            // 自定义模式下：不切换活动，而是开始拖拽
                            if st.activity_bar.customize_mode {
                                st.activity_bar.begin_drag(idx);
                                drop(st);
                                state.borrow_mut().render();
                                return LRESULT(0);
                            }

                            let view = st.activity_bar.items[idx].view;
                            if view == crate::layout::ActivityBarView::AiAssistant {
                                // AI 图标切换右侧面板（不再占用左侧栏）
                                st.layout.right_panel_visible = !st.layout.right_panel_visible;
                                if st.layout.right_panel_visible
                                    && st.layout.right_panel_width < 1.0
                                {
                                    st.layout.right_panel_width = 320.0;
                                }
                                // C-10: 关闭面板时取消输入框聚焦，避免键盘残留路由
                                if !st.layout.right_panel_visible {
                                    st.ai_panel.input_focused = false;
                                }
                                st.activity_bar.switch_to(idx);
                                st.activity_view = view;
                                st.status_message = if st.layout.right_panel_visible {
                                    "AI 面板已打开".to_string()
                                } else {
                                    "AI 面板已关闭".to_string()
                                };
                            } else {
                                st.activity_bar.switch_to(idx);
                                st.activity_view = view;
                                st.layout.sidebar_visible = true;
                                st.sidebar_content = crate::layout::SidebarContent::from_view(view);
                            }
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }
                    }

                    // 3. 检测拖拽边框点击（在侧边栏之前）
                    let editor_region = layout.editor_region();
                    let right_panel_resize_zone = layout.right_panel_visible
                        && (mouse_x >= editor_region.right() - 4.0
                            && mouse_x <= editor_region.right() + 4.0)
                        && mouse_y >= editor_region.y
                        && mouse_y < editor_region.y + editor_region.height;
                    let bottom_panel_resize_zone = layout.bottom_panel_visible
                        && (mouse_y >= editor_region.bottom() - 4.0
                            && mouse_y <= editor_region.bottom() + 4.0)
                        && mouse_x >= editor_region.x
                        && mouse_x < editor_region.x + editor_region.width;

                    if right_panel_resize_zone {
                        st.layout.right_panel_resizing = true;
                        drop(st);
                        state.borrow_mut().render();
                        return LRESULT(0);
                    }
                    if bottom_panel_resize_zone {
                        st.layout.bottom_panel_resizing = true;
                        drop(st);
                        state.borrow_mut().render();
                        return LRESULT(0);
                    }

                    // 3. 检测侧边栏点击
                    let sidebar_region = layout.sidebar_region();
                    if sidebar_region.contains(mouse_x, mouse_y) {
                        let _sidebar_rel_x = mouse_x - sidebar_region.x;
                        let _sidebar_rel_y = mouse_y - sidebar_region.y;

                        if st.sidebar_content == crate::layout::SidebarContent::RemoteManagerPanel {
                            // SSH 管理面板：检测按钮点击
                            let panel = &st.ssh_manager_panel;
                            // 检测操作按钮点击（连接/编辑/删除）
                            let mut clicked_btn = None;
                            for &(idx, action, ref rect) in &panel.item_btn_rects {
                                if rect.contains(mouse_x, mouse_y) {
                                    clicked_btn = Some((idx, action));
                                    break;
                                }
                            }
                            if let Some((idx, action)) = clicked_btn {
                                if idx < 997 {
                                    // 服务器条目按钮
                                    match action {
                                        0 => {
                                            // 连接/断开
                                            if st.is_ssh_connected(idx) {
                                                st.disconnect_ssh();
                                            } else {
                                                st.connect_ssh_server(idx);
                                            }
                                        }
                                        1 => {
                                            // 编辑
                                            if let Some(config) = st.ssh_servers().get(idx).cloned()
                                            {
                                                st.ssh_manager_panel.start_edit(idx, &config);
                                            }
                                        }
                                        2 => {
                                            // 删除
                                            st.delete_ssh_server(idx);
                                        }
                                        _ => {}
                                    }
                                } else if idx == 997 {
                                    // 添加按钮（列表模式）
                                    st.ssh_manager_panel.start_add();
                                } else if idx == 998 {
                                    match action {
                                        0 => {
                                            // 保存
                                            match st.save_ssh_server_from_form() {
                                                Ok(()) => {
                                                    st.status_message =
                                                        "服务器配置已保存".to_string();
                                                }
                                                Err(e) => {
                                                    st.ssh_manager_panel.error_message = Some(e);
                                                }
                                            }
                                        }
                                        1 => {
                                            // 取消
                                            st.ssh_manager_panel.cancel_edit();
                                        }
                                        _ => {}
                                    }
                                } else if idx == 999 {
                                    // 认证方式切换
                                    st.ssh_manager_panel.cycle_auth_type();
                                }
                                drop(st);
                                state.borrow_mut().render();
                                return LRESULT(0);
                            }
                            // 检测添加按钮
                            if let Some(ref rect) = panel.add_btn_rect {
                                if rect.contains(mouse_x, mouse_y) {
                                    st.ssh_manager_panel.start_add();
                                    drop(st);
                                    state.borrow_mut().render();
                                    return LRESULT(0);
                                }
                            }
                            // 检测保存/取消按钮（编辑模式）
                            if panel.editing {
                                if let Some(ref rect) = panel.save_btn_rect {
                                    if rect.contains(mouse_x, mouse_y) {
                                        match st.save_ssh_server_from_form() {
                                            Ok(()) => {
                                                st.status_message = "服务器配置已保存".to_string();
                                            }
                                            Err(e) => {
                                                st.ssh_manager_panel.error_message = Some(e);
                                            }
                                        }
                                        drop(st);
                                        state.borrow_mut().render();
                                        return LRESULT(0);
                                    }
                                }
                                if let Some(ref rect) = panel.cancel_btn_rect {
                                    if rect.contains(mouse_x, mouse_y) {
                                        st.ssh_manager_panel.cancel_edit();
                                        drop(st);
                                        state.borrow_mut().render();
                                        return LRESULT(0);
                                    }
                                }
                            }
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }
                        let sidebar_rel_x = mouse_x - sidebar_region.x;
                        let sidebar_rel_y = mouse_y - sidebar_region.y;
                        if st.handle_sidebar_click(sidebar_rel_x, sidebar_rel_y) {
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }
                    }

                    // 3.5 检测右侧面板内容点击（AI 助手）
                    let right_panel_region = layout.right_panel_region();
                    if layout.right_panel_visible && right_panel_region.contains(mouse_x, mouse_y) {
                        let rp_rel_x = mouse_x - right_panel_region.x;
                        let rp_rel_y = mouse_y - right_panel_region.y;
                        // C-10: 默认点击 AI 面板非输入框区域时取消输入框聚焦
                        st.ai_panel.input_focused = false;
                        let actions = crate::ai_panel::AiPanel::quick_actions();
                        let margin = 10.0;
                        let btn_w = (right_panel_region.width - margin * 2.0 - 8.0) / 2.0;
                        let btn_h = 28.0;
                        let btn_gap = 8.0;
                        let action_start_y = 52.0; // 标题 + 分隔线 + 边距
                        let action_rows = (actions.len() + 1) / 2;
                        let action_end_y =
                            action_start_y + action_rows as f32 * (btn_h + 6.0) + 8.0;

                        // 检测快捷操作按钮点击
                        if rp_rel_y >= action_start_y && rp_rel_y < action_end_y {
                            for (i, action) in actions.iter().enumerate() {
                                let col = i % 2;
                                let row = i / 2;
                                let bx = margin + col as f32 * (btn_w + btn_gap);
                                let by = action_start_y + row as f32 * (btn_h + 6.0);
                                if rp_rel_x >= bx
                                    && rp_rel_x < bx + btn_w
                                    && rp_rel_y >= by
                                    && rp_rel_y < by + btn_h
                                {
                                    let selected_code = if let Some(text) = st.get_selected_text() {
                                        text
                                    } else {
                                        st.buffer
                                            .get_all_text()
                                            .chars()
                                            .take(2000)
                                            .collect::<String>()
                                    };
                                    let settings = st.app_settings.ai.clone();
                                    let action_clone = *action;
                                    drop(st);
                                    let _ = state.borrow_mut().ai_panel.send_quick_action(
                                        action_clone,
                                        &selected_code,
                                        &settings,
                                    );
                                    state.borrow_mut().render();
                                    return LRESULT(0);
                                }
                            }
                        }

                        // 检测 Apply 按钮点击
                        let apply_y = right_panel_region.height - 76.0;
                        let apply_btn_w = 80.0;
                        let apply_btn_h = 24.0;
                        let apply_btn_x = right_panel_region.width - margin - apply_btn_w;
                        if rp_rel_x >= apply_btn_x
                            && rp_rel_x < apply_btn_x + apply_btn_w
                            && rp_rel_y >= apply_y
                            && rp_rel_y < apply_y + apply_btn_h
                        {
                            if let Some(code) = st.ai_panel.extract_last_code_block() {
                                st.apply_ai_code(&code);
                                st.status_message = "AI 代码已应用到编辑器".to_string();
                            }
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }

                        // 检测输入框点击（键盘输入由 WM_CHAR 处理）
                        let input_y = right_panel_region.height - 40.0;
                        if rp_rel_y >= input_y
                            && rp_rel_y < input_y + 32.0
                            && rp_rel_x >= margin
                            && rp_rel_x < right_panel_region.width - margin
                        {
                            // C-10: 点击输入框才聚焦，避免面板可见即劫持键盘
                            st.ai_panel.input_focused = true;
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }
                    }

                    // 4. 检测标签栏点击
                    let has_multiple_tabs = st.tab_count() > 1;
                    let tab_region = layout.tab_bar_region(has_multiple_tabs);
                    if tab_region.contains(mouse_x, mouse_y) {
                        if st.handle_tab_bar_click(mouse_x, mouse_y, tab_region.x) {
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }
                    }

                    // 4.5 检测查找替换面板点击
                    if st.find_visible {
                        let editor_region = layout.editor_content_region(has_multiple_tabs);
                        let panel_height = if st.replace_visible { 72.0 } else { 40.0 };
                        let panel_width = editor_region.width.min(600.0);
                        let panel_x = editor_region.x + editor_region.width - panel_width - 10.0;
                        let panel_y = editor_region.y;
                        if mouse_x >= panel_x
                            && mouse_x < panel_x + panel_width
                            && mouse_y >= panel_y
                            && mouse_y < panel_y + panel_height
                        {
                            let input_h = 24.0;
                            let input_w = panel_width - 120.0;
                            let find_y = panel_y + 8.0;
                            let find_input_x = panel_x + 50.0;
                            let find_input_w = input_w;
                            if mouse_x >= find_input_x
                                && mouse_x < find_input_x + find_input_w
                                && mouse_y >= find_y
                                && mouse_y < find_y + input_h
                            {
                                st.find_focus = crate::editor::FindReplaceFocus::FindQuery;
                            } else if st.replace_visible {
                                let replace_y = panel_y + 8.0 + input_h + 8.0;
                                let replace_input_x = panel_x + 50.0;
                                let replace_input_w = input_w;
                                if mouse_x >= replace_input_x
                                    && mouse_x < replace_input_x + replace_input_w
                                    && mouse_y >= replace_y
                                    && mouse_y < replace_y + input_h
                                {
                                    st.find_focus = crate::editor::FindReplaceFocus::ReplaceText;
                                }
                            }
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }
                    }

                    // 4.6 检测底部面板点击
                    let bottom_panel_region = layout.bottom_panel_region();
                    if bottom_panel_region.contains(mouse_x, mouse_y) {
                        st.terminal_panel.focused = true;
                        drop(st);
                        state.borrow_mut().render();
                        return LRESULT(0);
                    }

                    // 4.7 检测底部终端面板点击（已合并到底部面板区域）
                    // 底部面板点击已在 4.6 中处理，此处保留用于扩展

                    // 5. 欢迎页/编辑器区域点击
                    let mut welcome_x = if layout.activity_bar_visible {
                        layout.activity_bar_width
                    } else {
                        0.0
                    };
                    if layout.sidebar_visible {
                        welcome_x += layout.sidebar_width;
                    }
                    let welcome_width = st.window_width as f32 - welcome_x;
                    let welcome_y = layout.top_offset();
                    let welcome_height = st.window_height as f32
                        - welcome_y
                        - if layout.status_bar_visible {
                            layout.status_bar_height
                        } else {
                            0.0
                        };
                    let welcome_region = crate::layout::Region::new(
                        welcome_x,
                        welcome_y,
                        welcome_width,
                        welcome_height,
                    );

                    if welcome_region.contains(mouse_x, mouse_y) {
                        if st.show_welcome() {
                            let action = st.handle_welcome_click(
                                mouse_x,
                                mouse_y,
                                welcome_x,
                                welcome_y,
                                welcome_width,
                                welcome_height,
                            );
                            if let Some(action) = action {
                                drop(st);
                                match action {
                                    crate::welcome::WelcomeAction::OpenFolder => {
                                        if let Some(path) =
                                            Dialogs::open_folder_dialog(hwnd, "打开文件夹")
                                        {
                                            state.borrow_mut().open_folder(path);
                                            state.borrow_mut().render();
                                        }
                                    }
                                    crate::welcome::WelcomeAction::NewFile => {
                                        state.borrow_mut().new_file();
                                        state.borrow_mut().render();
                                    }
                                    crate::welcome::WelcomeAction::CloneRepo => {
                                        state.borrow_mut().clone_dialog.visible = true;
                                        state.borrow_mut().clone_dialog.reset();
                                        state.borrow_mut().render();
                                    }
                                    crate::welcome::WelcomeAction::OpenRemote => {
                                        state.borrow_mut().ssh_dialog.visible = true;
                                        state.borrow_mut().ssh_dialog.reset();
                                        state.borrow_mut().render();
                                    }
                                    crate::welcome::WelcomeAction::OpenRecentProject(path_str) => {
                                        let path = PathBuf::from(path_str);
                                        state.borrow_mut().open_folder(path);
                                        state.borrow_mut().render();
                                    }
                                    crate::welcome::WelcomeAction::MoreRecentProjects => {
                                        if let Some(path) =
                                            Dialogs::open_folder_dialog(hwnd, "打开文件夹")
                                        {
                                            state.borrow_mut().open_folder(path);
                                            state.borrow_mut().render();
                                        }
                                    }
                                }
                                return LRESULT(0);
                            }
                        } else {
                            let editor_content = layout.editor_content_region(has_multiple_tabs);
                            st.set_cursor_from_mouse(
                                mouse_x,
                                mouse_y,
                                editor_content.x,
                                editor_content.y,
                            );
                            st.clear_selection();
                            st.start_selection();
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }
                    }

                    // 6. 状态栏点击
                    let _status_region = layout.status_bar_region();
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let raw_x = (lparam.0 & 0xFFFF) as i16 as f32;
                let raw_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                let is_dragging = wparam.0 & 0x0001 != 0; // MK_LBUTTON

                if let Some(state) = get_state() {
                    let mut st = state.borrow_mut();
                    // 将物理像素转换为逻辑像素(DIP)
                    let mouse_x = raw_x / st.dpi_scale;
                    let mouse_y = raw_y / st.dpi_scale;
                    let layout = st.layout.clone();

                    // 对话框悬停处理
                    if st.ssh_dialog.visible {
                        let _ = st.handle_ssh_dialog_click(mouse_x, mouse_y);
                        drop(st);
                        state.borrow_mut().render();
                        return LRESULT(0);
                    }
                    if st.clone_dialog.visible {
                        let _ = st.handle_clone_dialog_click(mouse_x, mouse_y);
                        drop(st);
                        state.borrow_mut().render();
                        return LRESULT(0);
                    }

                    // 长按检测：移动超过容差则取消长按（视为普通拖拽/点击）
                    if is_dragging && st.lpress_target.is_some() {
                        let dx = mouse_x - st.lpress_x;
                        let dy = mouse_y - st.lpress_y;
                        if dx.abs() > LP_MOVE_TOLERANCE || dy.abs() > LP_MOVE_TOLERANCE {
                            let _ = KillTimer(hwnd, LP_TIMER_ID);
                            st.lpress_target = None;
                            st.lpress_start = None;
                        }
                    }
                    // 自定义模式下：跟随鼠标更新放置目标
                    let activity_dragging =
                        st.activity_bar.customize_mode && st.activity_bar.drag_index.is_some();
                    let menu_dragging =
                        st.menu_bar.customize_mode && st.menu_bar.drag_index.is_some();
                    if is_dragging && activity_dragging {
                        let bar_y = layout.activity_bar_region().y;
                        st.activity_bar.drop_index =
                            Some(st.activity_bar.drop_index_at(mouse_y, bar_y));
                        drop(st);
                        state.borrow_mut().render();
                        return LRESULT(0);
                    }
                    if is_dragging && menu_dragging {
                        // 与 menu_bar.hit_test 一致：使用绝对 mouse_x
                        st.menu_bar.drop_index = Some(st.menu_bar.drop_index_at(mouse_x));
                        drop(st);
                        state.borrow_mut().render();
                        return LRESULT(0);
                    }

                    // 更新标题栏区域悬停（包含菜单项和窗口控制按钮）
                    let old_titlebar_hover = st.titlebar_hover_button;
                    let titlebar_region = layout.title_bar_region();
                    if titlebar_region.contains(mouse_x, mouse_y) {
                        let btn_width = 40.0;
                        let close_x = titlebar_region.x + titlebar_region.width - btn_width;
                        let maximize_x = close_x - btn_width;
                        let minimize_x = maximize_x - btn_width;

                        // 检测窗口控制按钮悬停
                        let panel_btn_width = 28.0;
                        let right_panel_btn_x = minimize_x - panel_btn_width;
                        let bottom_panel_btn_x = right_panel_btn_x - panel_btn_width;
                        let left_sidebar_btn_x = bottom_panel_btn_x - panel_btn_width;
                        // 用户按钮
                        let user_btn_size = 26.0;
                        let user_btn_x = minimize_x - 28.0 * 3.0 - user_btn_size - 4.0;

                        if mouse_x >= minimize_x {
                            if mouse_x >= close_x {
                                st.titlebar_hover_button = Some(2);
                            } else if mouse_x >= maximize_x {
                                st.titlebar_hover_button = Some(1);
                            } else {
                                st.titlebar_hover_button = Some(0);
                            }
                        } else if mouse_x >= right_panel_btn_x {
                            st.titlebar_hover_button = Some(3);
                        } else if mouse_x >= bottom_panel_btn_x {
                            st.titlebar_hover_button = Some(4);
                        } else if mouse_x >= left_sidebar_btn_x {
                            st.titlebar_hover_button = Some(6);
                        } else if mouse_x >= user_btn_x && mouse_x < user_btn_x + user_btn_size {
                            st.titlebar_hover_button = Some(5);
                        } else {
                            st.titlebar_hover_button = None;
                        }
                    } else {
                        st.titlebar_hover_button = None;
                    }
                    let new_titlebar_hover = st.titlebar_hover_button;

                    // 更新菜单栏悬停（菜单项现在在标题栏内）
                    let old_menu_hover = st.menu_bar.hover_index;
                    if titlebar_region.contains(mouse_x, mouse_y) {
                        let btn_width = 40.0;
                        let minimize_x =
                            titlebar_region.x + titlebar_region.width - btn_width * 3.0;
                        // 只有在非按钮区域才检测菜单悬停
                        if mouse_x < minimize_x {
                            st.menu_bar.hover_index = st.menu_bar.hit_test(
                                mouse_x,
                                mouse_y - titlebar_region.y,
                                titlebar_region.height,
                            );
                        } else {
                            st.menu_bar.hover_index = None;
                        }
                    } else {
                        st.menu_bar.hover_index = None;
                    }
                    let new_menu_hover = st.menu_bar.hover_index;

                    // 更新活动栏悬停
                    let activity_region = layout.activity_bar_region();
                    st.activity_bar.hover_index =
                        st.activity_bar
                            .hit_test(mouse_x, mouse_y, activity_region.y);

                    // 更新标签栏悬停状态
                    let editor_content = layout.editor_content_region(st.tab_count() > 1);
                    let old_hover = st.hover_tab;
                    st.update_hover_tab(mouse_x, mouse_y, editor_content.x);
                    let new_hover = st.hover_tab;

                    // 更新文件树悬停状态
                    let sidebar_region = layout.sidebar_region();
                    let _old_tree_hover = st.hover_file_node;
                    let tree_hover_changed = if sidebar_region.contains(mouse_x, mouse_y) {
                        if st.sidebar_content == crate::layout::SidebarContent::RemoteManagerPanel {
                            // SSH 管理面板悬停检测
                            let old_hover = st.ssh_manager_panel.hover;
                            let old_action = st.ssh_manager_panel.hover_action;
                            // 检测悬停的操作按钮
                            let mut new_hover_action = None;
                            let btn_rects = st.ssh_manager_panel.item_btn_rects.clone();
                            for &(idx, action, ref rect) in &btn_rects {
                                if rect.contains(mouse_x, mouse_y) {
                                    new_hover_action = Some((idx, action));
                                    break;
                                }
                            }
                            st.ssh_manager_panel.hover_action = new_hover_action;
                            // 检测添加按钮悬停
                            if new_hover_action.is_none() {
                                if let Some(ref rect) = st.ssh_manager_panel.add_btn_rect {
                                    if rect.contains(mouse_x, mouse_y) {
                                        st.ssh_manager_panel.hover_action = Some((997, 0));
                                    }
                                }
                            }
                            // 检测保存/取消按钮悬停
                            if new_hover_action.is_none() && st.ssh_manager_panel.editing {
                                if let Some(ref rect) = st.ssh_manager_panel.save_btn_rect {
                                    if rect.contains(mouse_x, mouse_y) {
                                        st.ssh_manager_panel.hover_action = Some((998, 0));
                                    }
                                }
                                if st.ssh_manager_panel.hover_action.is_none() {
                                    if let Some(ref rect) = st.ssh_manager_panel.cancel_btn_rect {
                                        if rect.contains(mouse_x, mouse_y) {
                                            st.ssh_manager_panel.hover_action = Some((998, 1));
                                        }
                                    }
                                }
                            }
                            st.ssh_manager_panel.hover = None;
                            old_hover != st.ssh_manager_panel.hover
                                || old_action != st.ssh_manager_panel.hover_action
                        } else {
                            st.update_file_tree_hover(
                                mouse_x - sidebar_region.x,
                                mouse_y - sidebar_region.y,
                            )
                        }
                    } else {
                        let old = st.hover_file_node.take();
                        old.is_some()
                    };

                    // Update settings panel button hover
                    let settings_hover_changed = if sidebar_region.contains(mouse_x, mouse_y)
                        && st.sidebar_content == crate::layout::SidebarContent::RemoteManagerPanel
                    {
                        // SSH 管理面板已在上面处理悬停
                        false
                    } else {
                        let mut changed = false;
                        if st.settings_panel.hover_tab.is_some() {
                            st.settings_panel.hover_tab = None;
                            changed = true;
                        }
                        changed
                    };

                    // 更新 AI 面板快捷操作悬停（AI 面板位于右侧面板）
                    let right_panel_region = layout.right_panel_region();
                    let ai_hover_changed = if layout.right_panel_visible
                        && right_panel_region.contains(mouse_x, mouse_y)
                    {
                        let old_hover = st.ai_panel.hover_action;
                        let rel_x = mouse_x - right_panel_region.x;
                        let rel_y = mouse_y - right_panel_region.y;
                        let actions = crate::ai_panel::AiPanel::quick_actions();
                        let margin = 10.0;
                        let btn_w = (right_panel_region.width - margin * 2.0 - 8.0) / 2.0;
                        let btn_h = 28.0;
                        let btn_gap = 8.0;
                        let action_start_y = 52.0;
                        let mut new_hover = None;
                        for (i, action) in actions.iter().enumerate() {
                            let col = i % 2;
                            let row = i / 2;
                            let bx = margin + col as f32 * (btn_w + btn_gap);
                            let by = action_start_y + row as f32 * (btn_h + 6.0);
                            if rel_x >= bx
                                && rel_x < bx + btn_w
                                && rel_y >= by
                                && rel_y < by + btn_h
                            {
                                new_hover = Some(*action);
                                break;
                            }
                        }
                        st.ai_panel.hover_action = new_hover;
                        let apply_y = right_panel_region.height - 76.0;
                        let apply_btn_w = 80.0;
                        let apply_btn_h = 24.0;
                        let apply_btn_x = right_panel_region.width - margin - apply_btn_w;
                        let old_apply_hover = st.ai_panel.hover_apply_button;
                        st.ai_panel.hover_apply_button = rel_x >= apply_btn_x
                            && rel_x < apply_btn_x + apply_btn_w
                            && rel_y >= apply_y
                            && rel_y < apply_y + apply_btn_h;
                        let apply_hover_changed = old_apply_hover != st.ai_panel.hover_apply_button;
                        old_hover != new_hover || apply_hover_changed
                    } else {
                        let old = st.ai_panel.hover_apply_button;
                        st.ai_panel.hover_apply_button = false;
                        old
                    };

                    // 更新欢迎页悬停状态
                    let old_welcome_hover = st.welcome_hover_action.clone();
                    if st.show_welcome() {
                        let mut welcome_x = if layout.activity_bar_visible {
                            layout.activity_bar_width
                        } else {
                            0.0
                        };
                        if layout.sidebar_visible {
                            welcome_x += layout.sidebar_width;
                        }
                        let welcome_y = layout.top_offset();
                        let welcome_width = st.window_width as f32 - welcome_x;
                        let welcome_height = st.window_height as f32
                            - welcome_y
                            - if layout.status_bar_visible {
                                layout.status_bar_height
                            } else {
                                0.0
                            };
                        st.welcome_hover_action = st.hit_test_welcome_action(
                            mouse_x,
                            mouse_y,
                            welcome_x,
                            welcome_y,
                            welcome_width,
                            welcome_height,
                        );
                    } else {
                        st.welcome_hover_action = None;
                    }
                    let welcome_hover_changed = old_welcome_hover != st.welcome_hover_action;

                    // 检测右侧面板拖拽边框（编辑器右边缘）
                    let editor_region = layout.editor_region();
                    let right_panel_resize_zone = layout.right_panel_visible
                        && (mouse_x >= editor_region.right() - 4.0
                            && mouse_x <= editor_region.right() + 4.0)
                        && mouse_y >= editor_region.y
                        && mouse_y < editor_region.y + editor_region.height;

                    // 检测底部面板拖拽边框（编辑器底部边缘）
                    let bottom_panel_resize_zone = layout.bottom_panel_visible
                        && (mouse_y >= editor_region.bottom() - 4.0
                            && mouse_y <= editor_region.bottom() + 4.0)
                        && mouse_x >= editor_region.x
                        && mouse_x < editor_region.x + editor_region.width;

                    // 设置拖拽光标
                    if right_panel_resize_zone || st.layout.right_panel_resizing {
                        let hcursor = windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                            None,
                            windows::Win32::UI::WindowsAndMessaging::IDC_SIZEWE,
                        )
                        .unwrap_or_default();
                        let _ = windows::Win32::UI::WindowsAndMessaging::SetCursor(hcursor);
                    } else if bottom_panel_resize_zone || st.layout.bottom_panel_resizing {
                        let hcursor = windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                            None,
                            windows::Win32::UI::WindowsAndMessaging::IDC_SIZENS,
                        )
                        .unwrap_or_default();
                        let _ = windows::Win32::UI::WindowsAndMessaging::SetCursor(hcursor);
                    } else if st.welcome_hover_action.is_some() {
                        let hcursor = windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                            None,
                            windows::Win32::UI::WindowsAndMessaging::IDC_HAND,
                        )
                        .unwrap_or_default();
                        let _ = windows::Win32::UI::WindowsAndMessaging::SetCursor(hcursor);
                    }

                    // 处理拖拽调整
                    if is_dragging {
                        if st.layout.right_panel_resizing {
                            let delta = mouse_x - editor_region.right();
                            st.layout.resize_right_panel(-delta);
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        } else if st.layout.bottom_panel_resizing {
                            let delta = mouse_y - editor_region.bottom();
                            st.layout.resize_bottom_panel(-delta);
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }
                    }

                    if old_menu_hover != new_menu_hover
                        || old_hover != new_hover
                        || old_titlebar_hover != new_titlebar_hover
                        || tree_hover_changed
                        || settings_hover_changed
                        || ai_hover_changed
                        || welcome_hover_changed
                    {
                        drop(st);
                        state.borrow_mut().render();
                    } else if is_dragging {
                        st.set_cursor_from_mouse(
                            mouse_x,
                            mouse_y,
                            editor_content.x,
                            editor_content.y,
                        );
                        st.update_selection();
                        drop(st);
                        state.borrow_mut().render();
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = KillTimer(hwnd, LP_TIMER_ID);
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        let mut st = state.borrow_mut();
                        st.end_selection();
                        // 结束面板拖拽
                        st.layout.right_panel_resizing = false;
                        st.layout.bottom_panel_resizing = false;
                        // 长按检测状态清理
                        st.lbutton_down = false;
                        st.lpress_target = None;
                        st.lpress_start = None;
                        // 自定义模式下：完成拖拽重排 + 持久化
                        let persist_activity =
                            st.activity_bar.customize_mode && st.activity_bar.drag_index.is_some();
                        let persist_menu =
                            st.menu_bar.customize_mode && st.menu_bar.drag_index.is_some();
                        if persist_activity {
                            st.activity_bar.reorder();
                            st.app_settings.ui.activity_bar_order = st.activity_bar.order_keys();
                            let _ = st.app_settings.save();
                            st.status_message = "活动栏顺序已保存".to_string();
                        }
                        if persist_menu {
                            st.menu_bar.reorder();
                            st.app_settings.ui.menu_bar_order = st.menu_bar.order_keys();
                            let _ = st.app_settings.save();
                            st.status_message = "菜单栏顺序已保存".to_string();
                        }
                        // 仅在用户实际开始拖拽时才重绘
                        if persist_activity || persist_menu {
                            drop(st);
                            state.borrow_mut().render();
                            return;
                        }
                    }
                });
                LRESULT(0)
            }
            WM_LBUTTONDBLCLK => {
                // P2-5: 双击选词
                let raw_x = (lparam.0 & 0xFFFF) as i16 as f32;
                let raw_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                if let Some(state) = get_state() {
                    let mut st = state.borrow_mut();
                    // 仅在非对话框、非命令面板、非欢迎页时处理编辑器区域双击
                    // （settings_panel 在侧边栏，editor_region.contains 已排除）
                    if st.ssh_dialog.visible
                        || st.clone_dialog.visible
                        || st.command_palette.visible
                        || st.show_welcome()
                    {
                        return LRESULT(0);
                    }
                    let mouse_x = raw_x / st.dpi_scale;
                    let mouse_y = raw_y / st.dpi_scale;
                    let layout = st.layout.clone();
                    let has_multiple_tabs = st.tabs.len() > 1;
                    let editor_content = layout.editor_content_region(has_multiple_tabs);
                    let editor_region = crate::layout::Region::new(
                        editor_content.x,
                        editor_content.y,
                        editor_content.width,
                        editor_content.height,
                    );
                    if editor_region.contains(mouse_x, mouse_y) {
                        st.select_word_at_mouse(
                            mouse_x,
                            mouse_y,
                            editor_content.x,
                            editor_content.y,
                        );
                        drop(st);
                        state.borrow_mut().render();
                    }
                }
                LRESULT(0)
            }
            WM_TIMER => {
                if wparam.0 == TERM_TIMER_ID {
                    // 终端刷新：周期性重绘以显示异步到达的 shell 输出。
                    // render() 内部会调用 flush_output 拉取子进程输出。
                    // 底部终端面板不可见时自动停止定时器，避免空转。
                    let still_visible = EDITOR_STATE.with(|s| {
                        s.borrow()
                            .as_ref()
                            .map(|state| state.borrow().layout.bottom_panel_visible)
                            .unwrap_or(false)
                    });
                    if !still_visible {
                        let _ = KillTimer(hwnd, TERM_TIMER_ID);
                    } else if let Some(state) = get_state() {
                        state.borrow_mut().render();
                    }
                    return LRESULT(0);
                }
                if wparam.0 == LP_TIMER_ID {
                    let _ = KillTimer(hwnd, LP_TIMER_ID);
                    if let Some(state) = get_state() {
                        let mut st = state.borrow_mut();
                        if st.lbutton_down {
                            if let Some(target) = st.lpress_target {
                                // 检查按下时间是否达到长按阈值
                                if let Some(start) = st.lpress_start {
                                    if start.elapsed()
                                        >= std::time::Duration::from_millis(LP_THRESHOLD_MS as u64)
                                    {
                                        let idx = st.lpress_index;
                                        match target {
                                            crate::input::PressTarget::ActivityBar => {
                                                st.activity_bar.begin_drag(idx);
                                                st.status_message =
                                                    "活动栏自定义模式（拖拽排序，Esc 退出）"
                                                        .to_string();
                                            }
                                            crate::input::PressTarget::MenuBar => {
                                                st.menu_bar.begin_drag(idx);
                                                st.status_message =
                                                    "菜单栏自定义模式（拖拽排序，Esc 退出）"
                                                        .to_string();
                                            }
                                        }
                                        st.lpress_start = None;
                                        drop(st);
                                        state.borrow_mut().render();
                                        return LRESULT(0);
                                    }
                                }
                            }
                        }
                    }
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                // 释放窗口关联的编辑器状态
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RefCell<EditorState>;
                if !ptr.is_null() {
                    // P0.2c: 主窗口退出前持久化窗口状态(矩形/最大化/工作区)。
                    // 用 Rc::from_raw 取回所有权,在 drop 之前完成持久化。
                    let rc = Rc::from_raw(ptr);
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
                if WINDOW_COUNT.fetch_sub(1, Ordering::SeqCst) == 1 {
                    PostQuitMessage(0);
                }
                LRESULT(0)
            }
            msg if msg == WM_APP + 2 => {
                // 新建窗口请求
                let instance =
                    windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
                create_editor_window(instance.into(), Some(hwnd));
                LRESULT(0)
            }
            msg if msg == WM_APP + 3 => {
                // 文件夹异步扫描完成
                let raw = wparam.0;
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().on_folder_scan_complete(raw);
                        state.borrow_mut().render();
                    }
                });
                LRESULT(0)
            }
            msg if msg == WM_APP + 4 => {
                // C-09: SSH 异步连接完成
                let raw = wparam.0;
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().on_ssh_connect_complete(raw);
                        state.borrow_mut().render();
                    }
                });
                LRESULT(0)
            }
            msg if msg == WM_APP + 5 => {
                // C-09: Git 异步克隆完成
                let raw = wparam.0;
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().on_git_clone_complete(raw);
                        state.borrow_mut().render();
                    }
                });
                LRESULT(0)
            }
            msg if msg == WM_APP + 6 => {
                // P0-1: 远程子目录异步列目录完成
                let raw = wparam.0;
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().on_ssh_list_dir_complete(raw);
                        state.borrow_mut().render();
                    }
                });
                LRESULT(0)
            }
            WM_DROPFILES => {
                use windows::Win32::UI::Shell::{DragFinish, DragQueryFileW, HDROP};
                let hdrop = HDROP(wparam.0 as *mut std::ffi::c_void);

                let file_count = DragQueryFileW(hdrop, u32::MAX, None);
                for i in 0..file_count {
                    let path_len = DragQueryFileW(hdrop, i, None);
                    if path_len == 0 {
                        continue;
                    }
                    let mut path_buf = vec![0u16; (path_len + 1) as usize];
                    let _ = DragQueryFileW(hdrop, i, Some(&mut path_buf));
                    if let Ok(path_str) = String::from_utf16(&path_buf[..path_len as usize]) {
                        let path = PathBuf::from(path_str);
                        if path.is_dir() {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().open_folder(path);
                                    state.borrow_mut().render();
                                }
                            });
                            break;
                        } else {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().load_file(path);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                    }
                }
                DragFinish(hdrop);
                LRESULT(0)
            }
            WM_SIZE => {
                let mut client_rect = RECT::default();
                if GetClientRect(hwnd, &mut client_rect).is_ok() {
                    let width = (client_rect.right - client_rect.left) as u32;
                    let height = (client_rect.bottom - client_rect.top) as u32;
                    let is_max = wparam.0 == SIZE_MAXIMIZED as usize;
                    let is_min = wparam.0 == SIZE_MINIMIZED as usize;
                    EDITOR_STATE.with(|s| {
                        if let Some(state) = s.borrow().as_ref() {
                            let mut st = state.borrow_mut();
                            st.is_maximized = is_max;
                            if !is_min {
                                st.resize(width, height);
                            }
                            drop(st);
                            if !is_min {
                                state.borrow_mut().render();
                            }
                        }
                    });
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let new_dpi = (wparam.0 & 0xFFFF) as f32;
                let new_scale = new_dpi / 96.0;

                if lparam.0 != 0 {
                    let suggested_rect: *const RECT = lparam.0 as *const RECT;
                    let rect = &*suggested_rect;
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }

                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        let mut st = state.borrow_mut();
                        st.dpi_scale = new_scale;
                        st.render_ctx.set_dpi(new_dpi);
                        st.text_renderer.set_dpi_scale(new_scale);
                        st.status_message =
                            format!("DPI: {} ({}%)", new_dpi as u32, (new_scale * 100.0) as u32);
                        // UI-M09: DPI 切换后重建渲染目标，确保尺寸与新 DPI 匹配
                        let _ = st.init_render_target();
                        // P3-1: DPI 切换后必须重建 text_format_cache 与 brush_cache，
                        // 否则缓存的 IDWriteTextFormat 仍使用旧 font_size，导致渲染尺寸不一致
                        // Theme 已 derive(Copy)，按值拷贝避免借用冲突
                        let theme = st.theme;
                        let font_size = st.text_renderer.font_size();
                        st.render_ctx.init_common_resources(&theme, font_size);
                        drop(st);
                        state.borrow_mut().render();
                    }
                });
                LRESULT(0)
            }
            WM_NCACTIVATE => {
                // 阻止系统绘制非激活状态的边框（白色边框）
                // 返回 TRUE 表示已处理，不绘制系统默认的 NC 激活指示器
                LRESULT(1)
            }
            WM_NCCALCSIZE => {
                // 移除系统非客户区边框，避免白色边框线
                // 返回 0 表示客户区覆盖整个窗口，不绘制系统边框
                LRESULT(0)
            }
            WM_NCHITTEST => {
                // 自定义命中测试，实现无边框窗口的调整大小和拖动
                let x = ((lparam.0 & 0xFFFF) as i16) as i32;
                let y = (((lparam.0 >> 16) & 0xFFFF) as i16) as i32;
                let mut rect = RECT::default();
                if GetWindowRect(hwnd, &mut rect).is_ok() {
                    // UI-H05: 根据 DPI 缩放边框大小，确保高 DPI 下可点击区域足够大
                    use windows::Win32::UI::HiDpi::GetDpiForWindow;
                    let dpi = GetDpiForWindow(hwnd);
                    let scale = dpi as f32 / 96.0;
                    let border_size = (8.0 * scale) as i32;
                    let left = x - rect.left;
                    let top = y - rect.top;
                    let right = rect.right - x;
                    let bottom = rect.bottom - y;

                    let mut result = HTCLIENT;
                    if top < border_size {
                        if left < border_size {
                            result = HTTOPLEFT;
                        } else if right < border_size {
                            result = HTTOPRIGHT;
                        } else {
                            result = HTTOP;
                        }
                    } else if bottom < border_size {
                        if left < border_size {
                            result = HTBOTTOMLEFT;
                        } else if right < border_size {
                            result = HTBOTTOMRIGHT;
                        } else {
                            result = HTBOTTOM;
                        }
                    } else if left < border_size {
                        result = HTLEFT;
                    } else if right < border_size {
                        result = HTRIGHT;
                    } else {
                        // 标题栏区域全部返回 HTCLIENT，由 WM_LBUTTONDOWN 统一处理菜单/按钮点击和拖动
                        // 不返回 HTCAPTION/HTCLOSE 等系统码，因为 WS_POPUP 窗口系统不会正确处理它们
                    }
                    return LRESULT(result as isize);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_ERASEBKGND => {
                // 阻止系统擦除背景，避免白色闪烁
                LRESULT(1)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let _hdc = BeginPaint(hwnd, &mut ps);
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().render();
                    }
                });
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_IME_STARTCOMPOSITION => {
                // P0-2: IME 开始合成。仅做位置初始化，IME 候选/合成窗口位置由
                // 渲染时 set_candidate_window_position 同步。返回 0 表示已处理。
                LRESULT(0)
            }
            WM_IME_COMPOSITION => {
                // C-12: 键盘消息进入时先同步 thread_local 到当前窗口状态
                get_state();
                let lparam_flags = lparam.0 as u32;
                const GCS_COMPSTR: u32 = 0x0008;
                const GCS_RESULTSTR: u32 = 0x0800;

                // 优先处理结果串（已提交文本）：将合成串清空，并插入提交字符
                if lparam_flags & GCS_RESULTSTR != 0 {
                    let result = EDITOR_STATE.with(|s| {
                        s.borrow()
                            .as_ref()
                            .and_then(|state| state.borrow().ime.get_result_string())
                    });
                    if let Some(text) = result {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                state.borrow_mut().commit_composition(text);
                                state.borrow_mut().render();
                            }
                        });
                    }
                    // 结果串已包含完整提交，不再处理合成串
                    return LRESULT(0);
                }

                // 处理合成串（pre-edit text）：仅更新显示，不修改 buffer
                if lparam_flags & GCS_COMPSTR != 0 {
                    let comp = EDITOR_STATE.with(|s| {
                        s.borrow()
                            .as_ref()
                            .and_then(|state| state.borrow().ime.get_composition_string())
                    });
                    if let Some(text) = comp {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                state.borrow_mut().set_composition(text);
                                state.borrow_mut().render();
                            }
                        });
                    } else {
                        // 合成串为空：IME 已清除合成状态
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                state.borrow_mut().clear_composition();
                                state.borrow_mut().render();
                            }
                        });
                    }
                    return LRESULT(0);
                }

                // 无 GCS 标志：IME 取消当前合成
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().clear_composition();
                        state.borrow_mut().render();
                    }
                });
                LRESULT(0)
            }
            WM_IME_ENDCOMPOSITION => {
                // P0-2: IME 结束合成。清除合成串显示。
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().clear_composition();
                        state.borrow_mut().render();
                    }
                });
                LRESULT(0)
            }
            WM_IME_CHAR => {
                // P0-2: 阻止 TranslateMessage 从 WM_IME_CHAR 产生 WM_CHAR，
                // 避免中文输入字符被 WM_CHAR 重复插入。
                // 提交文本已通过 WM_IME_COMPOSITION + GCS_RESULTSTR 处理。
                LRESULT(0)
            }
            WM_CHAR => {
                // C-12: 键盘消息进入时先同步 thread_local 到当前窗口状态，
                // 防止 Alt+Tab / 任务栏切换焦点后键盘输入路由到错误窗口的 EditorState
                get_state();
                let ch = (wparam.0 & 0xFFFF) as u16;

                // P2-9: 处理 UTF-16 代理对以支持 BMP 外字符（emoji、CJK 扩展 B 等）
                // WM_CHAR 对 BMP 外字符发送两条消息：先高代理（0xD800-0xDBFF），后低代理（0xDC00-0xDFFF）
                if (0xD800..=0xDBFF).contains(&ch) {
                    // 高代理：暂存并跳过，等待配对的低代理
                    PENDING_HIGH_SURROGATE.with(|s| *s.borrow_mut() = Some(ch));
                    return LRESULT(0);
                }
                // 低代理：取出暂存的高代理，组合为完整码点
                let code_point: u32 = if (0xDC00..=0xDFFF).contains(&ch) {
                    let high = PENDING_HIGH_SURROGATE.with(|s| s.borrow_mut().take());
                    match high {
                        Some(h) => 0x10000 + ((h as u32 - 0xD800) << 10) + (ch as u32 - 0xDC00),
                        None => ch as u32, // 孤立低代理，char::from_u32 会返回 None 被丢弃
                    }
                } else {
                    // C-11: 输入非代理字符时，清除可能残留的高代理项，
                    // 避免上一次孤立高代理污染后续低代理输入产生错误字符
                    PENDING_HIGH_SURROGATE.with(|s| *s.borrow_mut() = None);
                    ch as u32
                };

                if ch >= 32 && ch != 127 {
                    if let Some(c) = char::from_u32(code_point) {
                        // Settings panel active field routing
                        let settings_field_active = EDITOR_STATE.with(|s| {
                            s.borrow()
                                .as_ref()
                                .map(|state| state.borrow().settings_panel.active_field.is_some())
                                .unwrap_or(false)
                        });
                        if settings_field_active {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().settings_panel.input_char(c);
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }

                        // 命令面板激活时，输入字符进入搜索框
                        let command_palette_active = EDITOR_STATE.with(|s| {
                            s.borrow()
                                .as_ref()
                                .map(|state| state.borrow().command_palette.visible)
                                .unwrap_or(false)
                        });
                        // 终端面板激活时，输入字符进入终端
                        let terminal_active = EDITOR_STATE.with(|s| {
                            s.borrow()
                                .as_ref()
                                .map(|state| state.borrow().terminal_panel.focused)
                                .unwrap_or(false)
                        });
                        let ssh_dialog_active = EDITOR_STATE.with(|s| {
                            s.borrow()
                                .as_ref()
                                .map(|state| state.borrow().ssh_dialog.visible)
                                .unwrap_or(false)
                        });
                        let clone_dialog_active = EDITOR_STATE.with(|s| {
                            s.borrow()
                                .as_ref()
                                .map(|state| state.borrow().clone_dialog.visible)
                                .unwrap_or(false)
                        });
                        if ssh_dialog_active {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().handle_ssh_dialog_key(c);
                                    state.borrow_mut().render();
                                }
                            });
                        } else if clone_dialog_active {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().handle_clone_dialog_key(c);
                                    state.borrow_mut().render();
                                }
                            });
                        } else if EDITOR_STATE.with(|s| {
                            s.borrow()
                                .as_ref()
                                .map(|state| {
                                    state.borrow().sidebar_content
                                        == crate::layout::SidebarContent::RemoteManagerPanel
                                        && state.borrow().ssh_manager_panel.editing
                                })
                                .unwrap_or(false)
                        }) {
                            // SSH 管理面板编辑模式：输入字符到当前焦点字段
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let mut st = state.borrow_mut();
                                    let field = st.ssh_manager_panel.focus_field;
                                    let field_str = match field {
                                        0 => &mut st.ssh_manager_panel.form_name,
                                        1 => &mut st.ssh_manager_panel.form_host,
                                        2 => &mut st.ssh_manager_panel.form_port,
                                        3 => &mut st.ssh_manager_panel.form_username,
                                        4 => &mut st.ssh_manager_panel.form_key_path,
                                        _ => &mut st.ssh_manager_panel.form_name,
                                    };
                                    field_str.push(c);
                                    drop(st);
                                    state.borrow_mut().render();
                                }
                            });
                        } else if command_palette_active {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().command_palette.append_query(c);
                                    state.borrow_mut().render();
                                }
                            });
                        } else if EDITOR_STATE.with(|s| {
                            s.borrow()
                                .as_ref()
                                .map(|state| {
                                    state.borrow().find_visible
                                        && state.borrow().find_focus
                                            != crate::editor::FindReplaceFocus::None
                                })
                                .unwrap_or(false)
                        }) {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let focus = state.borrow().find_focus;
                                    match focus {
                                        crate::editor::FindReplaceFocus::FindQuery => {
                                            state.borrow_mut().find_query.push(c);
                                            state.borrow_mut().find_all();
                                            state.borrow_mut().find_active_index = 0;
                                            if !state.borrow().find_results.is_empty() {
                                                let (line, col) = state.borrow().find_results[0];
                                                state.borrow_mut().cursor_line = line;
                                                state.borrow_mut().cursor_col = col;
                                                state.borrow_mut().selection_start =
                                                    Some((line, col));
                                                state.borrow_mut().selection_end = Some((
                                                    line,
                                                    col + state.borrow().find_query.len(),
                                                ));
                                            }
                                        }
                                        crate::editor::FindReplaceFocus::ReplaceText => {
                                            state.borrow_mut().replace_text.push(c);
                                        }
                                        _ => {}
                                    }
                                    state.borrow_mut().render();
                                }
                            });
                        } else if terminal_active {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().terminal_panel.input_line.push(c);
                                    state.borrow_mut().terminal_panel.cursor_pos += 1;
                                    state.borrow_mut().render();
                                }
                            });
                        } else if EDITOR_STATE.with(|s| {
                            s.borrow()
                                .as_ref()
                                .map(|state| state.borrow().ai_panel.input_focused)
                                .unwrap_or(false)
                        }) {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().ai_panel.input_char(c);
                                    state.borrow_mut().render();
                                }
                            });
                        } else {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    // P1-1: 多光标模式下广播到所有光标
                                    state.borrow_mut().broadcast_insert_char(c);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                    }
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                // C-12: 键盘消息进入时先同步 thread_local 到当前窗口状态
                get_state();
                let vk = VIRTUAL_KEY(wparam.0 as u16);
                let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
                let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;

                // 自定义模式下按 Escape 退出
                if vk == VK_ESCAPE {
                    let any_customize = EDITOR_STATE.with(|s| {
                        s.borrow()
                            .as_ref()
                            .map(|state| {
                                let st = state.borrow();
                                st.activity_bar.customize_mode || st.menu_bar.customize_mode
                            })
                            .unwrap_or(false)
                    });
                    if any_customize {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                let mut st = state.borrow_mut();
                                st.activity_bar.exit_customize();
                                st.menu_bar.exit_customize();
                                st.status_message = "已退出自定义排序模式".to_string();
                                drop(st);
                                state.borrow_mut().render();
                            }
                        });
                        return LRESULT(0);
                    }
                }

                // 欢迎页键盘导航：Tab/↓ next, Shift+Tab/↑ prev, Enter 触发
                let welcome_active = EDITOR_STATE.with(|s| {
                    s.borrow()
                        .as_ref()
                        .map(|state| state.borrow().show_welcome())
                        .unwrap_or(false)
                });
                if welcome_active && !ctrl {
                    let handled = match vk {
                        VK_TAB | VK_DOWN => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().welcome_focus_next();
                                    state.borrow_mut().render();
                                }
                            });
                            true
                        }
                        VK_UP => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().welcome_focus_prev();
                                    state.borrow_mut().render();
                                }
                            });
                            true
                        }
                        VK_RETURN => {
                            let action = EDITOR_STATE.with(|s| {
                                s.borrow()
                                    .as_ref()
                                    .and_then(|state| state.borrow().welcome_focus_action.clone())
                            });
                            if let Some(action) = action {
                                match action {
                                    crate::welcome::WelcomeAction::OpenFolder => {
                                        if let Some(path) =
                                            Dialogs::open_folder_dialog(hwnd, "打开文件夹")
                                        {
                                            EDITOR_STATE.with(|s| {
                                                if let Some(state) = s.borrow().as_ref() {
                                                    state.borrow_mut().open_folder(path);
                                                    state.borrow_mut().render();
                                                }
                                            });
                                        }
                                    }
                                    crate::welcome::WelcomeAction::OpenRecentProject(path_str) => {
                                        let path = PathBuf::from(path_str);
                                        EDITOR_STATE.with(|s| {
                                            if let Some(state) = s.borrow().as_ref() {
                                                state.borrow_mut().open_folder(path);
                                                state.borrow_mut().render();
                                            }
                                        });
                                    }
                                    crate::welcome::WelcomeAction::MoreRecentProjects => {
                                        if let Some(path) =
                                            Dialogs::open_folder_dialog(hwnd, "打开文件夹")
                                        {
                                            EDITOR_STATE.with(|s| {
                                                if let Some(state) = s.borrow().as_ref() {
                                                    state.borrow_mut().open_folder(path);
                                                    state.borrow_mut().render();
                                                }
                                            });
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            true
                        }
                        _ => false,
                    };
                    if handled {
                        return LRESULT(0);
                    }
                }

                // Settings field active - intercept keyboard input
                let settings_field_active = EDITOR_STATE.with(|s| {
                    s.borrow()
                        .as_ref()
                        .map(|state| state.borrow().settings_panel.active_field.is_some())
                        .unwrap_or(false)
                });
                if settings_field_active {
                    match vk {
                        VK_ESCAPE => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().settings_panel.active_field = None;
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_RETURN => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().settings_panel.active_field = None;
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_BACK => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().settings_panel.backspace();
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_DELETE => {
                            // UI-M05: Delete 键应清除字段而非执行 Backspace（删除末尾字符）
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().settings_panel.delete_forward();
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_TAB => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    if shift {
                                        state.borrow_mut().settings_panel.prev_field();
                                    } else {
                                        state.borrow_mut().settings_panel.next_field();
                                    }
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        _ => {
                            // Prevent editor from processing other keys while field is active
                            return LRESULT(0);
                        }
                    }
                }

                // 命令面板激活时优先处理键盘导航
                let command_palette_active = EDITOR_STATE.with(|s| {
                    s.borrow()
                        .as_ref()
                        .map(|state| state.borrow().command_palette.visible)
                        .unwrap_or(false)
                });

                // SSH 对话框激活时优先处理键盘
                let ssh_dialog_active = EDITOR_STATE.with(|s| {
                    s.borrow()
                        .as_ref()
                        .map(|state| state.borrow().ssh_dialog.visible)
                        .unwrap_or(false)
                });
                let clone_dialog_active = EDITOR_STATE.with(|s| {
                    s.borrow()
                        .as_ref()
                        .map(|state| state.borrow().clone_dialog.visible)
                        .unwrap_or(false)
                });

                if ssh_dialog_active {
                    match vk {
                        VK_ESCAPE => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().ssh_dialog.visible = false;
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_RETURN => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let mut st = state.borrow_mut();
                                    // C-09: SSH 连接移至后台线程，避免阻塞 UI
                                    if st.ssh_connecting {
                                        // 正在连接中，忽略
                                    } else if let Some(config) = st.ssh_dialog.to_config() {
                                        st.start_ssh_connect(config);
                                    } else {
                                        st.ssh_dialog.error_message =
                                            Some("请填写主机和用户名".to_string());
                                    }
                                    drop(st);
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_TAB => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().ssh_dialog.next_field();
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_BACK => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().handle_ssh_dialog_backspace();
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_V if ctrl => {
                            // P2-4: Ctrl+V 粘贴到当前 SSH 对话框字段
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().paste_into_ssh_dialog();
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        _ => {}
                    }
                    return LRESULT(0);
                }

                if clone_dialog_active {
                    match vk {
                        VK_ESCAPE => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().clone_dialog.visible = false;
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_RETURN => {
                            EDITOR_STATE.with(|s| -> LRESULT {
                                if let Some(state) = s.borrow().as_ref() {
                                    let mut st = state.borrow_mut();
                                    if st.clone_dialog.url.is_empty() {
                                        st.clone_dialog.error_message =
                                            Some("请输入仓库 URL".to_string());
                                        drop(st);
                                        state.borrow_mut().render();
                                    } else if st.git_cloning {
                                        // C-09: 正在克隆中，忽略
                                        drop(st);
                                    } else {
                                        let url = st.clone_dialog.url.clone();
                                        drop(st);
                                        if let Some(target_path) =
                                            crate::dialogs::Dialogs::open_folder_dialog(
                                                hwnd,
                                                "选择克隆目标文件夹",
                                            )
                                        {
                                            // C-09: Git 克隆移至后台线程，避免阻塞 UI
                                            let mut st = state.borrow_mut();
                                            st.start_git_clone(url, target_path);
                                            drop(st);
                                            state.borrow_mut().render();
                                            return LRESULT(0);
                                        }
                                        // 文件夹对话框取消
                                        state.borrow_mut().render();
                                    }
                                }
                                LRESULT(0)
                            });
                            return LRESULT(0);
                        }
                        VK_BACK => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().handle_clone_dialog_backspace();
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_V if ctrl => {
                            // P2-4: Ctrl+V 粘贴到克隆对话框 URL 字段
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().paste_into_clone_dialog();
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        _ => {}
                    }
                    return LRESULT(0);
                }

                // SSH 管理面板编辑模式键盘处理
                let ssh_mgr_editing = EDITOR_STATE.with(|s| {
                    s.borrow()
                        .as_ref()
                        .map(|state| {
                            state.borrow().sidebar_content
                                == crate::layout::SidebarContent::RemoteManagerPanel
                                && state.borrow().ssh_manager_panel.editing
                        })
                        .unwrap_or(false)
                });
                if ssh_mgr_editing {
                    match vk {
                        VK_ESCAPE => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().ssh_manager_panel.cancel_edit();
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_RETURN => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let mut st = state.borrow_mut();
                                    match st.save_ssh_server_from_form() {
                                        Ok(()) => {
                                            st.status_message = "服务器配置已保存".to_string();
                                        }
                                        Err(e) => {
                                            st.ssh_manager_panel.error_message = Some(e);
                                        }
                                    }
                                    drop(st);
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_TAB => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let mut st = state.borrow_mut();
                                    st.ssh_manager_panel.focus_field =
                                        (st.ssh_manager_panel.focus_field + 1) % 5;
                                    drop(st);
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_BACK => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let mut st = state.borrow_mut();
                                    let field = st.ssh_manager_panel.focus_field;
                                    let field_str = match field {
                                        0 => &mut st.ssh_manager_panel.form_name,
                                        1 => &mut st.ssh_manager_panel.form_host,
                                        2 => &mut st.ssh_manager_panel.form_port,
                                        3 => &mut st.ssh_manager_panel.form_username,
                                        4 => &mut st.ssh_manager_panel.form_key_path,
                                        _ => &mut st.ssh_manager_panel.form_name,
                                    };
                                    field_str.pop();
                                    drop(st);
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        _ => {}
                    }
                    return LRESULT(0);
                }

                if command_palette_active {
                    match vk {
                        VK_ESCAPE => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().command_palette.hide();
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_RETURN => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    if let Some(cmd) =
                                        state.borrow().command_palette.selected_command()
                                    {
                                        let hwnd = state.borrow().hwnd;
                                        state.borrow_mut().execute_command(cmd, hwnd);
                                    }
                                    state.borrow_mut().command_palette.hide();
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_UP => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().command_palette.select_prev();
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_DOWN => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().command_palette.select_next();
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        VK_BACK => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().command_palette.backspace_query();
                                    state.borrow_mut().render();
                                }
                            });
                            return LRESULT(0);
                        }
                        _ => {}
                    }
                }

                if ctrl {
                    match vk {
                        VK_O => {
                            if let Some(path) = Dialogs::open_file_dialog(hwnd, "打开文件", &[])
                            {
                                EDITOR_STATE.with(|s| {
                                    if let Some(state) = s.borrow().as_ref() {
                                        state.borrow_mut().load_file(path);
                                        state.borrow_mut().render();
                                    }
                                });
                            }
                        }
                        VK_K => {
                            if let Some(path) = Dialogs::open_folder_dialog(hwnd, "打开文件夹")
                            {
                                EDITOR_STATE.with(|s| {
                                    if let Some(state) = s.borrow().as_ref() {
                                        state.borrow_mut().open_folder(path);
                                        state.borrow_mut().render();
                                    }
                                });
                            }
                        }
                        VK_S => {
                            if shift {
                                if let Some(path) =
                                    Dialogs::save_file_dialog(hwnd, "另存为", "untitled.txt")
                                {
                                    EDITOR_STATE.with(|s| {
                                        if let Some(state) = s.borrow().as_ref() {
                                            state.borrow_mut().save_as(path);
                                            state.borrow_mut().render();
                                        }
                                    });
                                }
                            } else {
                                let need_dialog = EDITOR_STATE.with(|s| {
                                    s.borrow()
                                        .as_ref()
                                        .map(|state| state.borrow().file_path.is_none())
                                        .unwrap_or(true)
                                });
                                if need_dialog {
                                    if let Some(path) =
                                        Dialogs::save_file_dialog(hwnd, "保存文件", "untitled.txt")
                                    {
                                        EDITOR_STATE.with(|s| {
                                            if let Some(state) = s.borrow().as_ref() {
                                                state.borrow_mut().save_as(path);
                                                state.borrow_mut().render();
                                            }
                                        });
                                    }
                                } else {
                                    EDITOR_STATE.with(|s| {
                                        if let Some(state) = s.borrow().as_ref() {
                                            state.borrow_mut().save_file();
                                            state.borrow_mut().render();
                                        }
                                    });
                                }
                            }
                        }
                        VK_N => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().new_file();
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_B => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().layout.toggle_sidebar();
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_P => {
                            if shift {
                                EDITOR_STATE.with(|s| {
                                    if let Some(state) = s.borrow().as_ref() {
                                        state.borrow_mut().command_palette.toggle();
                                        state.borrow_mut().render();
                                    }
                                });
                            } else {
                                // P2-3: Ctrl+P 也打开命令面板（VS Code 中为 Quick Open；此处复用命令面板）
                                EDITOR_STATE.with(|s| {
                                    if let Some(state) = s.borrow().as_ref() {
                                        state.borrow_mut().command_palette.show();
                                        state.borrow_mut().render();
                                    }
                                });
                            }
                        }
                        VK_OEM_PLUS | VK_ADD => {
                            // P2-3: Ctrl+= 放大字体
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().zoom_font(Some(1.0));
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_OEM_MINUS | VK_SUBTRACT => {
                            // P2-3: Ctrl+- 缩小字体
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().zoom_font(Some(-1.0));
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_0 | VK_NUMPAD0 => {
                            // P2-3: Ctrl+0 重置字体大小
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().zoom_font(None);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_G => {
                            if shift {
                                EDITOR_STATE.with(|s| {
                                    if let Some(state) = s.borrow().as_ref() {
                                        state.borrow_mut().command_palette.show();
                                        state.borrow_mut().command_palette.update_query(">");
                                        state.borrow_mut().render();
                                    }
                                });
                            } else {
                                EDITOR_STATE.with(|s| {
                                    if let Some(state) = s.borrow().as_ref() {
                                        state.borrow_mut().command_palette.show();
                                        state.borrow_mut().command_palette.update_query(":");
                                        state.borrow_mut().render();
                                    }
                                });
                            }
                        }
                        VK_C => {
                            // 终端聚焦时 Ctrl+C 中断子进程；否则执行复制
                            let term_focused = EDITOR_STATE.with(|s| {
                                s.borrow()
                                    .as_ref()
                                    .map(|state| state.borrow().terminal_panel.focused)
                                    .unwrap_or(false)
                            });
                            if term_focused {
                                EDITOR_STATE.with(|s| {
                                    if let Some(state) = s.borrow().as_ref() {
                                        state.borrow_mut().terminal_panel.send_interrupt();
                                        state.borrow_mut().status_message =
                                            "终端已中断 (Ctrl+C)".to_string();
                                        state.borrow_mut().render();
                                    }
                                });
                            } else {
                                EDITOR_STATE.with(|s| {
                                    if let Some(state) = s.borrow().as_ref() {
                                        state.borrow_mut().copy();
                                        state.borrow_mut().render();
                                    }
                                });
                            }
                        }
                        VK_X => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().cut();
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_V => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().paste();
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_OEM_3 => {
                            // Ctrl+` 切换底部终端面板
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().layout.toggle_terminal_panel();
                                    if state.borrow().layout.bottom_panel_visible {
                                        // 打开时聚焦终端并按需启动 shell
                                        state.borrow_mut().terminal_panel.focused = true;
                                        if !state.borrow().terminal_panel.running {
                                            let _ = state.borrow_mut().terminal_panel.start();
                                        }
                                        // 启动周期刷新定时器以显示异步输出
                                        let _ =
                                            SetTimer(hwnd, TERM_TIMER_ID, TERM_REFRESH_MS, None);
                                    } else {
                                        state.borrow_mut().terminal_panel.focused = false;
                                        // 关闭时停止刷新定时器
                                        let _ = KillTimer(hwnd, TERM_TIMER_ID);
                                    }
                                    state.borrow_mut().status_message =
                                        if state.borrow().layout.bottom_panel_visible {
                                            "终端已打开 (Ctrl+` 关闭, Ctrl+C 中断)"
                                        } else {
                                            "终端已关闭"
                                        }
                                        .to_string();
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_A => {
                            if shift {
                                // Ctrl+Shift+A 切换右侧 AI 面板
                                EDITOR_STATE.with(|s| {
                                    if let Some(state) = s.borrow().as_ref() {
                                        let mut st = state.borrow_mut();
                                        st.layout.right_panel_visible =
                                            !st.layout.right_panel_visible;
                                        if st.layout.right_panel_visible
                                            && st.layout.right_panel_width < 1.0
                                        {
                                            st.layout.right_panel_width = 320.0;
                                        }
                                        st.status_message = if st.layout.right_panel_visible {
                                            "AI 面板已打开".to_string()
                                        } else {
                                            "AI 面板已关闭".to_string()
                                        };
                                        st.render();
                                    }
                                });
                            } else {
                                EDITOR_STATE.with(|s| {
                                    if let Some(state) = s.borrow().as_ref() {
                                        state.borrow_mut().select_all();
                                        state.borrow_mut().render();
                                    }
                                });
                            }
                        }
                        VK_F => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let selected = state.borrow().get_selected_text();
                                    if shift {
                                        state.borrow_mut().toggle_replace();
                                    } else {
                                        state.borrow_mut().toggle_find();
                                    }
                                    // 如果有选中文本，自动填充到查找框
                                    if let Some(text) = selected {
                                        if !text.is_empty() && text.len() < 200 {
                                            state.borrow_mut().find_query = text;
                                            state.borrow_mut().find_all();
                                        }
                                    }
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_H => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().toggle_replace();
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_Z => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    if shift {
                                        state.borrow_mut().redo();
                                    } else {
                                        state.borrow_mut().undo();
                                    }
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_Y => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().redo();
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_TAB => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    if shift {
                                        state.borrow_mut().prev_tab();
                                    } else {
                                        state.borrow_mut().next_tab();
                                    }
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_W | VK_F4 => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    // P2-8: 关闭前进行 dirty 检查
                                    state.borrow_mut().close_current_tab_checked();
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_1 | VK_NUMPAD1 => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().goto_tab(1);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_2 | VK_NUMPAD2 => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().goto_tab(2);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_3 | VK_NUMPAD3 => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().goto_tab(3);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_4 | VK_NUMPAD4 => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().goto_tab(4);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_5 | VK_NUMPAD5 => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().goto_tab(5);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_6 | VK_NUMPAD6 => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().goto_tab(6);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_7 | VK_NUMPAD7 => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().goto_tab(7);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_8 | VK_NUMPAD8 => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().goto_tab(8);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_9 | VK_NUMPAD9 => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let last = state.borrow().tab_count();
                                    state.borrow_mut().goto_tab(last);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        // P1-6: Ctrl+Left / Ctrl+Right 词级移动
                        VK_LEFT => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let mut st = state.borrow_mut();
                                    if shift {
                                        if st.selection_start.is_none() {
                                            st.start_selection();
                                        }
                                        st.move_cursor_word_left();
                                        st.update_selection();
                                    } else {
                                        if st.selection_start.is_some() {
                                            st.clear_selection();
                                        }
                                        st.move_cursor_word_left();
                                    }
                                    drop(st);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_RIGHT => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let mut st = state.borrow_mut();
                                    if shift {
                                        if st.selection_start.is_none() {
                                            st.start_selection();
                                        }
                                        st.move_cursor_word_right();
                                        st.update_selection();
                                    } else {
                                        if st.selection_start.is_some() {
                                            st.clear_selection();
                                        }
                                        st.move_cursor_word_right();
                                    }
                                    drop(st);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        // P1-6: Ctrl+Home / Ctrl+End 文件首末
                        VK_HOME => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().move_cursor_file_start();
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        VK_END => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().move_cursor_file_end();
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        // P1-6: Ctrl+D 添加下一个相同单词光标
                        VK_D => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().add_cursor_at_next_occurrence();
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        // P1-6: Ctrl+/ 切换行注释（OEM_2 为 / 键，需配合 Shift 实际生成 /，但 Ctrl+/ 是约定）
                        VK_OEM_2 => {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().toggle_line_comment();
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        // P1-6: Ctrl+Alt+Up / Ctrl+Alt+Down 列光标
                        VK_UP => {
                            let alt = GetKeyState(VK_MENU.0 as i32) < 0;
                            if alt {
                                EDITOR_STATE.with(|s| {
                                    if let Some(state) = s.borrow().as_ref() {
                                        state.borrow_mut().add_cursor_line_above();
                                        state.borrow_mut().render();
                                    }
                                });
                            }
                        }
                        VK_DOWN => {
                            let alt = GetKeyState(VK_MENU.0 as i32) < 0;
                            if alt {
                                EDITOR_STATE.with(|s| {
                                    if let Some(state) = s.borrow().as_ref() {
                                        state.borrow_mut().add_cursor_line_below();
                                        state.borrow_mut().render();
                                    }
                                });
                            }
                        }
                        _ => {}
                    }
                    return LRESULT(0);
                }

                // 非Ctrl按键
                let terminal_active = EDITOR_STATE.with(|s| {
                    s.borrow()
                        .as_ref()
                        .map(|state| state.borrow().terminal_panel.focused)
                        .unwrap_or(false)
                });
                let has_selection =
                    |st: &EditorState| st.selection_start.is_some() && st.selection_end.is_some();
                let ai_panel_active = EDITOR_STATE.with(|s| {
                    s.borrow()
                        .as_ref()
                        // C-10: 改用 input_focused 而非 right_panel_visible，避免面板可见即劫持回车
                        .map(|state| state.borrow().ai_panel.input_focused)
                        .unwrap_or(false)
                });
                let find_active = EDITOR_STATE.with(|s| {
                    s.borrow()
                        .as_ref()
                        .map(|state| {
                            state.borrow().find_visible
                                && state.borrow().find_focus
                                    != crate::editor::FindReplaceFocus::None
                        })
                        .unwrap_or(false)
                });
                match vk {
                    VK_RETURN => {
                        if terminal_active {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let input = state.borrow().terminal_panel.input_line.clone();
                                    state
                                        .borrow_mut()
                                        .terminal_panel
                                        .push_output(&format!("> {}", input));
                                    state.borrow_mut().terminal_panel.send_enter();
                                    state.borrow_mut().render();
                                }
                            });
                        } else if ai_panel_active {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let settings = state.borrow().app_settings.ai.clone();
                                    let _ = state.borrow_mut().ai_panel.send_message(&settings);
                                    state.borrow_mut().render();
                                }
                            });
                        } else if find_active {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let focus = state.borrow().find_focus;
                                    match focus {
                                        crate::editor::FindReplaceFocus::FindQuery => {
                                            state.borrow_mut().find_next();
                                        }
                                        crate::editor::FindReplaceFocus::ReplaceText => {
                                            state.borrow_mut().replace_current();
                                            state.borrow_mut().find_next();
                                        }
                                        _ => {}
                                    }
                                    state.borrow_mut().render();
                                }
                            });
                        } else {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let has_sel = has_selection(&state.borrow());
                                    if has_sel {
                                        state.borrow_mut().delete_selection();
                                    }
                                    // P1-1: 多光标模式下广播换行到所有光标
                                    state.borrow_mut().broadcast_insert_newline();
                                    state.borrow_mut().render();
                                }
                            });
                        }
                    }
                    VK_BACK => {
                        if terminal_active {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let mut st = state.borrow_mut();
                                    if !st.terminal_panel.input_line.is_empty() {
                                        st.terminal_panel.input_line.pop();
                                        st.terminal_panel.cursor_pos =
                                            st.terminal_panel.cursor_pos.saturating_sub(1);
                                    }
                                    st.render();
                                }
                            });
                        } else if ai_panel_active {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().ai_panel.backspace();
                                    state.borrow_mut().render();
                                }
                            });
                        } else if find_active {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let focus = state.borrow().find_focus;
                                    match focus {
                                        crate::editor::FindReplaceFocus::FindQuery => {
                                            state.borrow_mut().find_query.pop();
                                            state.borrow_mut().find_all();
                                        }
                                        crate::editor::FindReplaceFocus::ReplaceText => {
                                            state.borrow_mut().replace_text.pop();
                                        }
                                        _ => {}
                                    }
                                    state.borrow_mut().render();
                                }
                            });
                        } else {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let has_sel = has_selection(&state.borrow());
                                    if has_sel {
                                        state.borrow_mut().delete_selection();
                                    } else {
                                        // P1-1: 多光标模式下广播退格到所有光标
                                        state.borrow_mut().broadcast_delete_char();
                                    }
                                    state.borrow_mut().render();
                                }
                            });
                        }
                    }
                    VK_DELETE => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                let has_sel = has_selection(&state.borrow());
                                if has_sel {
                                    state.borrow_mut().delete_selection();
                                } else {
                                    state.borrow_mut().delete_forward();
                                }
                                state.borrow_mut().render();
                            }
                        });
                    }
                    VK_F3 => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                if shift {
                                    state.borrow_mut().find_prev();
                                } else {
                                    state.borrow_mut().find_next();
                                }
                                state.borrow_mut().render();
                            }
                        });
                    }
                    VK_ESCAPE => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                state.borrow_mut().close_find_replace();
                                state.borrow_mut().render();
                            }
                        });
                    }
                    VK_LEFT => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                let mut st = state.borrow_mut();
                                if shift {
                                    if st.selection_start.is_none() {
                                        st.start_selection();
                                    }
                                    st.move_cursor_left();
                                    st.update_selection();
                                } else {
                                    if st.selection_start.is_some() {
                                        st.clear_selection();
                                    }
                                    st.move_cursor_left();
                                }
                                drop(st);
                                state.borrow_mut().render();
                            }
                        });
                    }
                    VK_RIGHT => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                let mut st = state.borrow_mut();
                                if shift {
                                    if st.selection_start.is_none() {
                                        st.start_selection();
                                    }
                                    st.move_cursor_right();
                                    st.update_selection();
                                } else {
                                    if st.selection_start.is_some() {
                                        st.clear_selection();
                                    }
                                    st.move_cursor_right();
                                }
                                drop(st);
                                state.borrow_mut().render();
                            }
                        });
                    }
                    VK_UP => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                let mut st = state.borrow_mut();
                                if shift {
                                    if st.selection_start.is_none() {
                                        st.start_selection();
                                    }
                                    st.move_cursor_up();
                                    st.update_selection();
                                } else {
                                    if st.selection_start.is_some() {
                                        st.clear_selection();
                                    }
                                    st.move_cursor_up();
                                }
                                drop(st);
                                state.borrow_mut().render();
                            }
                        });
                    }
                    VK_DOWN => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                let mut st = state.borrow_mut();
                                if shift {
                                    if st.selection_start.is_none() {
                                        st.start_selection();
                                    }
                                    st.move_cursor_down();
                                    st.update_selection();
                                } else {
                                    if st.selection_start.is_some() {
                                        st.clear_selection();
                                    }
                                    st.move_cursor_down();
                                }
                                drop(st);
                                state.borrow_mut().render();
                            }
                        });
                    }
                    VK_HOME => {
                        // P1-6: Smart Home - 已在首个非空白位置时跳到行首 (col=0)
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                let mut st = state.borrow_mut();
                                // 计算当前行首个非空白位置，判断是否已在该位置
                                let already_at_smart = st
                                    .buffer
                                    .get_line(st.cursor_line)
                                    .map(|text| {
                                        let first_non_ws = text
                                            .char_indices()
                                            .skip_while(|(_, c)| c.is_whitespace())
                                            .map(|(i, _)| i)
                                            .next()
                                            .unwrap_or(text.len());
                                        st.cursor_col == first_non_ws
                                    })
                                    .unwrap_or(false);
                                if shift {
                                    if st.selection_start.is_none() {
                                        st.start_selection();
                                    }
                                    st.move_cursor_smart_home(already_at_smart);
                                    st.update_selection();
                                } else {
                                    if st.selection_start.is_some() {
                                        st.clear_selection();
                                    }
                                    st.move_cursor_smart_home(already_at_smart);
                                }
                                drop(st);
                                state.borrow_mut().render();
                            }
                        });
                    }
                    VK_END => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                let mut st = state.borrow_mut();
                                if shift {
                                    if st.selection_start.is_none() {
                                        st.start_selection();
                                    }
                                    st.move_cursor_end();
                                    st.update_selection();
                                } else {
                                    if st.selection_start.is_some() {
                                        st.clear_selection();
                                    }
                                    st.move_cursor_end();
                                }
                                drop(st);
                                state.borrow_mut().render();
                            }
                        });
                    }
                    VK_PRIOR => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                let page = state.borrow().window_height as f32 - 24.0;
                                state.borrow_mut().scroll(-page);
                                state.borrow_mut().render();
                            }
                        });
                    }
                    VK_NEXT => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                let page = state.borrow().window_height as f32 - 24.0;
                                state.borrow_mut().scroll(page);
                                state.borrow_mut().render();
                            }
                        });
                    }
                    VK_TAB => {
                        if find_active {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let focus = state.borrow().find_focus;
                                    let replace_visible = state.borrow().replace_visible;
                                    let new_focus = match focus {
                                        crate::editor::FindReplaceFocus::FindQuery => {
                                            if replace_visible {
                                                crate::editor::FindReplaceFocus::ReplaceText
                                            } else {
                                                crate::editor::FindReplaceFocus::FindQuery
                                            }
                                        }
                                        crate::editor::FindReplaceFocus::ReplaceText => {
                                            crate::editor::FindReplaceFocus::FindQuery
                                        }
                                        _ => crate::editor::FindReplaceFocus::FindQuery,
                                    };
                                    state.borrow_mut().find_focus = new_focus;
                                    state.borrow_mut().render();
                                }
                            });
                        } else {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let has_sel = has_selection(&state.borrow());
                                    if has_sel {
                                        state.borrow_mut().delete_selection();
                                    }
                                    state.borrow_mut().insert_tab();
                                    state.borrow_mut().render();
                                }
                            });
                        }
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as f32;
                // H-18: 提取光标屏幕坐标并转换为客户端坐标
                let screen_x = (lparam.0 & 0xFFFF) as i16 as i32;
                let screen_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                let mut client_point = windows::Win32::Foundation::POINT {
                    x: screen_x,
                    y: screen_y,
                };
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut client_point);
                // P0-3: Shift + 滚轮 → 横向滚动
                let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        let mut state = state.borrow_mut();
                        // UI-C01: ScreenToClient 返回物理像素，需转换为逻辑像素
                        let dpi_scale = state.dpi_scale;
                        let cursor_x = client_point.x as f32 / dpi_scale;
                        let cursor_y = client_point.y as f32 / dpi_scale;

                        // P0-3: Shift+滚轮 或 光标在编辑器区域内时 → 横向滚动
                        if shift {
                            let editor = state.layout.editor_region();
                            if cursor_x >= editor.x
                                && cursor_x < editor.x + editor.width
                                && cursor_y >= editor.y
                                && cursor_y < editor.y + editor.height
                            {
                                // Shift+滚轮向右滚动查看右侧内容
                                let char_width = state.text_renderer.char_width();
                                state.scroll_horizontal(-delta * char_width);
                                state.render();
                                return;
                            }
                        }

                        // 检查光标是否在底部终端面板区域内
                        if state.layout.bottom_panel_visible {
                            let bottom = state.layout.bottom_panel_region();
                            if bottom.contains(cursor_x, cursor_y) {
                                // 向上滚动(delta>0)查看更早输出，向下滚动回到最新
                                let lines = ((delta.abs() / 120.0).ceil() as usize).max(1);
                                if delta > 0.0 {
                                    state.terminal_panel.scroll_up(lines * 3);
                                } else {
                                    state.terminal_panel.scroll_down(lines * 3);
                                }
                                state.render();
                                return;
                            }
                        }
                        // 检查光标是否在侧边栏区域内
                        let sidebar = state.layout.sidebar_region();
                        if state.layout.sidebar_visible
                            && cursor_x >= sidebar.x
                            && cursor_x < sidebar.x + sidebar.width
                            && cursor_y >= sidebar.y
                            && cursor_y < sidebar.y + sidebar.height
                        {
                            state.scroll_sidebar(-delta);
                        } else {
                            state.scroll(-delta);
                        }
                        state.render();
                    }
                });
                LRESULT(0)
            }
            WM_MOUSEHWHEEL => {
                // P0-3: 横向滚轮（触控板水平滚动 / 鼠标侧键）
                let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as f32;
                let screen_x = (lparam.0 & 0xFFFF) as i16 as i32;
                let screen_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                let mut client_point = windows::Win32::Foundation::POINT {
                    x: screen_x,
                    y: screen_y,
                };
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut client_point);
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        let mut state = state.borrow_mut();
                        let dpi_scale = state.dpi_scale;
                        let cursor_x = client_point.x as f32 / dpi_scale;
                        let cursor_y = client_point.y as f32 / dpi_scale;
                        let editor = state.layout.editor_region();
                        // 仅在编辑器区域内响应横向滚轮
                        if cursor_x >= editor.x
                            && cursor_x < editor.x + editor.width
                            && cursor_y >= editor.y
                            && cursor_y < editor.y + editor.height
                        {
                            let char_width = state.text_renderer.char_width();
                            // delta > 0 表示向右滚动触控板，光标向右移动查看右侧内容
                            state.scroll_horizontal(-delta * char_width);
                            state.render();
                        }
                    }
                });
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
