use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::dialogs::Dialogs;
use crate::editor::{EditorState, ScannedBatch};

const CLASS_NAME: &str = "AetherEditor";
const WINDOW_TITLE: &str = "Aether";

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

/// 检测系统高对比度模式是否激活（SPI_GETHIGHCONTRAST）
pub fn detect_high_contrast() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SPI_GETHIGHCONTRAST,
    };
    // HIGHCONTRASTW 结构体（手动定义，避免 feature flag 依赖）
    #[repr(C)]
    #[derive(Default)]
    struct HighContrastW {
        cb_size: u32,
        dw_flags: u32,
    }
    // HCF_HIGHCONTRASTON = 0x00000001
    const HCF_HIGHCONTRASTON: u32 = 0x00000001;
    unsafe {
        let mut hc = HighContrastW::default();
        hc.cb_size = std::mem::size_of::<HighContrastW>() as u32;
        let ok = SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            std::mem::size_of::<HighContrastW>() as u32,
            Some(&mut hc as *mut _ as *mut std::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        ok.is_ok() && (hc.dw_flags & HCF_HIGHCONTRASTON) != 0
    }
}

/// 将系统颜色（COLOR_*）转换为 D2D1_COLOR_F（RGB 归一化 0.0-1.0，不透明）
pub fn sys_color_to_d2d(
    color_index: windows::Win32::Graphics::Gdi::SYS_COLOR_INDEX,
) -> windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
    use windows::Win32::Graphics::Gdi::GetSysColor;
    use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
    let c = unsafe { GetSysColor(color_index) };
    D2D1_COLOR_F {
        r: ((c & 0xFF) as f32) / 255.0,
        g: (((c >> 8) & 0xFF) as f32) / 255.0,
        b: (((c >> 16) & 0xFF) as f32) / 255.0,
        a: 1.0,
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

        // Windows 11 备选：Mica 效果
        let mica_enabled: windows::Win32::Foundation::BOOL = true.into();
        let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            hwnd,
            windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE(1029i32),
            &mica_enabled as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<windows::Win32::Foundation::BOOL>() as u32,
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

thread_local! {
    static EDITOR_STATE: RefCell<Option<Rc<RefCell<EditorState>>>> = RefCell::new(None);
}

/// 设置当前活跃窗口的编辑器状态
fn set_active_state(state: Rc<RefCell<EditorState>>) {
    EDITOR_STATE.with(|s| {
        *s.borrow_mut() = Some(state);
    });
}

/// 从窗口的 GWLP_USERDATA 获取状态，并设为当前活跃状态
unsafe fn get_window_state(hwnd: HWND) -> Option<Rc<RefCell<EditorState>>> {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RefCell<EditorState>;
    if ptr.is_null() {
        return None;
    }
    let rc = Rc::from_raw(ptr);
    let cloned = rc.clone();
    let _ = Rc::into_raw(rc);
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
    // 获取主显示器 DPI 并计算缩放后的窗口大小
    let (_dpi_scale, scaled_width, scaled_height) = get_dpi_scaled_size(1280, 800);

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
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        scaled_width,
        scaled_height,
        owner.unwrap_or(HWND(std::ptr::null_mut())),
        None,
        instance,
        None,
    )
    .unwrap();

    // 启用 DWM Acrylic / Mica 效果
    enable_dwm_acrylic(hwnd);

    let state = EditorState::new(hwnd).unwrap();
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
        let state_ref = Rc::from_raw(state_ptr as *mut RefCell<EditorState>);
        state_ref.borrow_mut().dpi_scale = scale;
        // 重新获取所有权，避免释放
        let _ = Rc::into_raw(state_ref);
    }

    // 获取实际客户区物理像素尺寸
    let mut client_rect = RECT::default();
    if GetClientRect(hwnd, &mut client_rect).is_ok() {
        let w = (client_rect.right - client_rect.left) as u32;
        let h = (client_rect.bottom - client_rect.top) as u32;
        if w > 0 && h > 0 {
            let state_ref = Rc::from_raw(state_ptr as *mut RefCell<EditorState>);
            state_ref.borrow_mut().resize(w, h);
            let _ = Rc::into_raw(state_ref);
        }
    }

    // 初始化渲染目标并首次渲染
    {
        let state_ref = Rc::from_raw(state_ptr as *mut RefCell<EditorState>);
        let _ = state_ref.borrow_mut().init_render_target();
        state_ref.borrow_mut().render();
        // 设为当前活跃状态
        set_active_state(state_ref.clone());
        let _ = Rc::into_raw(state_ref);
    }

    hwnd
}

/// WM_SIZE: 窗口尺寸变化
unsafe fn on_size(hwnd: HWND, wparam: WPARAM, _lparam: LPARAM) -> LRESULT {
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

/// WM_DPICHANGED: DPI 变化
unsafe fn on_dpi_changed(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
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
            st.menu_bar.layout_dirty = true;
            st.status_message =
                format!("DPI: {} ({}%)", new_dpi as u32, (new_scale * 100.0) as u32);
            drop(st);
            state.borrow_mut().render();
        }
    });
    LRESULT(0)
}

/// WM_SETTINGCHANGE: 系统主题变化
unsafe fn on_setting_change(_hwnd: HWND, _wparam: WPARAM, _lparam: LPARAM) -> LRESULT {
    let hc = detect_high_contrast();
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            let mut st = state.borrow_mut();
            if st.high_contrast != hc {
                st.high_contrast = hc;
                st.theme.glass_enabled = !hc;
                st.menu_bar.layout_dirty = true;
            }
            drop(st);
            state.borrow_mut().render();
        }
    });
    LRESULT(0)
}

/// WM_NCHITTEST: 自定义命中测试
unsafe fn on_nc_hittest(hwnd: HWND, _wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let x = ((lparam.0 & 0xFFFF) as i16) as i32;
    let y = (((lparam.0 >> 16) & 0xFFFF) as i16) as i32;
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_ok() {
        let border_size = 8;
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
        }
        return LRESULT(result as isize);
    }
    DefWindowProcW(hwnd, WM_NCHITTEST, _wparam, lparam)
}

/// WM_PAINT: 重绘
unsafe fn on_paint(hwnd: HWND, _wparam: WPARAM, _lparam: LPARAM) -> LRESULT {
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

/// WM_MOUSEWHEEL: 鼠标滚轮
unsafe fn on_mouse_wheel(_hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as f32;
    let cursor_x = (lparam.0 & 0xFFFF) as i16 as f32;
    let cursor_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            let mut state = state.borrow_mut();
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

/// WM_DESTROY: 窗口销毁
unsafe fn on_destroy(hwnd: HWND, _wparam: WPARAM, _lparam: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RefCell<EditorState>;
    if !ptr.is_null() {
        let rc = Rc::from_raw(ptr);
        // 如果 thread-local 活跃状态仍指向该窗口，一并清除，
        // 避免窗口关闭后 EditorState 被继续持有多久释放
        EDITOR_STATE.with(|s| {
            if let Some(active) = s.borrow().as_ref() {
                if Rc::ptr_eq(active, &rc) {
                    *s.borrow_mut() = None;
                }
            }
        });
        drop(rc);
        let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
    }
    PostQuitMessage(0);
    LRESULT(0)
}

/// WM_APP + 2: 新建窗口请求
unsafe fn on_new_window(hwnd: HWND, _wparam: WPARAM, _lparam: LPARAM) -> LRESULT {
    let instance =
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
    create_editor_window(instance.into(), Some(hwnd));
    LRESULT(0)
}

/// WM_APP + 3: 文件树批量扫描结果
unsafe fn on_file_tree_batch(_hwnd: HWND, _wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if lparam.0 == 0 {
        return LRESULT(0);
    }
    let batch = *unsafe { Box::from_raw(lparam.0 as *mut ScannedBatch) };
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            {
                let _completed = state.borrow_mut().apply_scanned_batch(batch);
            }
            state.borrow_mut().render();
        }
    });
    LRESULT(0)
}

unsafe fn on_lbutton_up(_hwnd: HWND, _wparam: WPARAM, _lparam: LPARAM) -> LRESULT {
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            let mut st = state.borrow_mut();
            st.end_selection();
            st.layout.right_panel_resizing = false;
            st.layout.bottom_panel_resizing = false;
        }
    });
    LRESULT(0)
}

/// WM_CHAR: 字符输入
unsafe fn on_char(_hwnd: HWND, wparam: WPARAM, _lparam: LPARAM) -> LRESULT {
    let ch = (wparam.0 & 0xFFFF) as u16;
    if ch >= 32 && ch != 127 {
        if let Some(c) = char::from_u32(ch as u32) {
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
                    .map(|state| {
                        state.borrow().sidebar_content
                            == crate::layout::SidebarContent::AiAssistantPanel
                    })
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
                        state.borrow_mut().insert_char(c);
                        state.borrow_mut().render();
                    }
                });
            }
        }
    }
    LRESULT(0)
}

/// WM_SYSKEYDOWN: 系统按键（Alt/F10）
unsafe fn on_syskeydown(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let vk = VIRTUAL_KEY(wparam.0 as u16);
    let alt = GetKeyState(VK_MENU.0 as i32) < 0;

    // F10：激活菜单栏（标准 Windows 菜单激活键）
    if vk == VK_F10 {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                let already_active = state.borrow().menu_bar.keyboard_active;
                if already_active {
                    state.borrow_mut().menu_bar.close_menu();
                } else {
                    state.borrow_mut().menu_bar.activate_first();
                }
                state.borrow_mut().render();
            }
        });
        return LRESULT(0);
    }

    // Alt+字母：助记符激活菜单项
    if alt {
        if let Some(c) = char::from_u32(vk.0 as u32) {
            if c.is_ascii_alphabetic() {
                let hit = EDITOR_STATE.with(|s| {
                    s.borrow().as_ref().map(|state| {
                        state.borrow_mut().menu_bar.activate_by_mnemonic(c)
                    }).unwrap_or(false)
                });
                if hit {
                    EDITOR_STATE.with(|s| {
                        if let Some(state) = s.borrow().as_ref() {
                            state.borrow_mut().render();
                        }
                    });
                    return LRESULT(0);
                }
            }
        }
    }
    // 未处理的系统键交给默认处理
    DefWindowProcW(hwnd, WM_SYSKEYDOWN, wparam, lparam)
}

/// WM_LBUTTONDOWN handler
unsafe fn on_lbuttondown(hwnd: HWND, _wparam: WPARAM, lparam: LPARAM) -> LRESULT {
            let raw_x = (lparam.0 & 0xFFFF) as i16 as f32;
            let raw_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
            if let Some(state) = get_window_state(hwnd) {
                let mut st = state.borrow_mut();
                // 默认取消终端焦点，只有点击底部面板时才聚焦
                st.terminal_panel.focused = false;
                // 将物理像素转换为逻辑像素(DIP)
                let mouse_x = raw_x / st.dpi_scale;
                let mouse_y = raw_y / st.dpi_scale;
                let layout = st.layout.clone();

                // 对话框优先拦截点击
                if st.ssh_dialog.visible {
                    if let Some(action) = st.handle_ssh_dialog_click(mouse_x, mouse_y) {
                        match action {
                            crate::ssh::DialogAction::Connect => {
                                if let Some(config) = st.ssh_dialog.to_config() {
                                    let mut session = crate::ssh::RemoteSession::new(config);
                                    match session.connect() {
                                        Ok(()) => {
                                            st.remote_session = Some(session);
                                            // 尝试列出远程根目录
                                            if let Some(session) = st.remote_session.as_ref() {
                                                match session.list_current_dir() {
                                                    Ok(entries) => {
                                                        st.remote_file_tree = Some(crate::ssh::RemoteFileTree::from_entries("/", entries));
                                                        st.sidebar_content = crate::layout::SidebarContent::RemoteFileTree;
                                                        st.status_message =
                                                            "SSH 连接成功".to_string();
                                                    }
                                                    Err(e) => {
                                                        st.status_message = format!(
                                                            "SSH 连接成功，但无法列出目录: {}",
                                                            e
                                                        );
                                                    }
                                                }
                                            }
                                            st.ssh_dialog.visible = false;
                                        }
                                        Err(e) => {
                                            st.ssh_dialog.error_message = Some(e);
                                        }
                                    }
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
                                } else {
                                    // 打开文件夹选择对话框
                                    drop(st);
                                    if let Some(target_path) =
                                        crate::dialogs::Dialogs::open_folder_dialog(
                                            hwnd,
                                            "选择克隆目标文件夹",
                                        )
                                    {
                                        let mut st = state.borrow_mut();
                                        let url = st.clone_dialog.url.clone();
                                        let result = crate::git::GitIntegration::clone_repo(
                                            &url,
                                            &target_path,
                                        );
                                        match result {
                                            Ok(_) => {
                                                st.clone_dialog.visible = false;
                                                st.status_message = format!(
                                                    "克隆成功: {}",
                                                    target_path.display()
                                                );
                                                st.open_folder_async(hwnd, target_path);
                                            }
                                            Err(e) => {
                                                st.clone_dialog.error_message = Some(e);
                                            }
                                        }
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
                    let btn_width = 46.0;
                    let close_x = titlebar_region.x + titlebar_region.width - btn_width;
                    let maximize_x = close_x - btn_width;
                    let minimize_x = maximize_x - btn_width;

                    // 先检测是否点击了窗口控制按钮区域
                    let panel_btn_width = 32.0;
                    let right_panel_btn_x = minimize_x - panel_btn_width;
                    let bottom_panel_btn_x = right_panel_btn_x - panel_btn_width;

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
                        // 切换右侧面板可见性
                        st.layout.right_panel_visible = !st.layout.right_panel_visible;
                        drop(st);
                        state.borrow_mut().render();
                        return LRESULT(0);
                    } else if mouse_x >= bottom_panel_btn_x {
                        // 切换底部面板可见性
                        st.layout.bottom_panel_visible = !st.layout.bottom_panel_visible;
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
                        let view = st.activity_bar.items[idx].view;
                        st.activity_bar.switch_to(idx);
                        st.activity_view = view;
                        st.sidebar_content = crate::layout::SidebarContent::from_view(view);
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
                    let sidebar_rel_x = mouse_x - sidebar_region.x;
                    let sidebar_rel_y = mouse_y - sidebar_region.y;

                    if st.sidebar_content == crate::layout::SidebarContent::SettingsPanel {
                        let mut handled = false;
                        if let Some(field) = st
                            .settings_panel
                            .hit_test_field(sidebar_rel_x, sidebar_rel_y)
                        {
                            st.settings_panel.active_field = Some(field);
                            handled = true;
                        } else if let Some(button) = st
                            .settings_panel
                            .hit_test_button(sidebar_rel_x, sidebar_rel_y)
                        {
                            match button {
                                crate::settings::SettingsButton::Save => {
                                    let ai_settings = st.settings_panel.to_ai_settings();
                                    st.app_settings.ai = ai_settings;
                                    let _ = st.app_settings.save();
                                    st.settings_panel.test_status = "设置已保存".to_string();
                                    st.settings_panel.is_testing = false;
                                    handled = true;
                                }
                                crate::settings::SettingsButton::TestConnection => {
                                    st.settings_panel.is_testing = true;
                                    st.settings_panel.test_status = "测试中...".to_string();
                                    let ai_settings = st.settings_panel.to_ai_settings();
                                    drop(st);
                                    let result = aether_ai::AiClient::new(&ai_settings)
                                        .test_connection();
                                    let mut st = state.borrow_mut();
                                    st.settings_panel.is_testing = false;
                                    match result {
                                        Ok(resp) => {
                                            let preview =
                                                resp.chars().take(60).collect::<String>();
                                            st.settings_panel.test_status =
                                                format!("成功: {}", preview);
                                        }
                                        Err(e) => {
                                            st.settings_panel.test_status =
                                                format!("失败: {}", e);
                                        }
                                    }
                                    drop(st);
                                    state.borrow_mut().render();
                                    return LRESULT(0);
                                }
                            }
                        }
                        if handled {
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }
                    } else if st.sidebar_content
                        == crate::layout::SidebarContent::AiAssistantPanel
                    {
                        // AI 面板点击处理
                        let mut handled = false;
                        let actions = crate::ai_panel::AiPanel::quick_actions();
                        let margin = 10.0;
                        let btn_w = (sidebar_region.width - margin * 2.0 - 8.0) / 2.0;
                        let btn_h = 28.0;
                        let btn_gap = 8.0;
                        let action_start_y = 52.0; // 标题 + 分隔线 + 边距
                        let action_rows = (actions.len() + 1) / 2;
                        let action_end_y =
                            action_start_y + action_rows as f32 * (btn_h + 6.0) + 8.0;

                        // 检测快捷操作按钮点击
                        if sidebar_rel_y >= action_start_y && sidebar_rel_y < action_end_y {
                            for (i, action) in actions.iter().enumerate() {
                                let col = i % 2;
                                let row = i / 2;
                                let bx = margin + col as f32 * (btn_w + btn_gap);
                                let by = action_start_y + row as f32 * (btn_h + 6.0);
                                if sidebar_rel_x >= bx
                                    && sidebar_rel_x < bx + btn_w
                                    && sidebar_rel_y >= by
                                    && sidebar_rel_y < by + btn_h
                                {
                                    // 获取选中的代码
                                    let selected_code =
                                        if let Some(text) = st.get_selected_text() {
                                            text
                                        } else {
                                            // 如果没有选中文本，使用当前文件内容（简化）
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
                        let apply_y = sidebar_region.height - 76.0;
                        let apply_btn_w = 80.0;
                        let apply_btn_h = 24.0;
                        let apply_btn_x = sidebar_region.width - margin - apply_btn_w;
                        if sidebar_rel_x >= apply_btn_x
                            && sidebar_rel_x < apply_btn_x + apply_btn_w
                            && sidebar_rel_y >= apply_y
                            && sidebar_rel_y < apply_y + apply_btn_h
                        {
                            if let Some(code) = st.ai_panel.extract_last_code_block() {
                                st.apply_ai_code(&code);
                                st.status_message = "AI 代码已应用到编辑器".to_string();
                            }
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }

                        // 检测输入框点击
                        let input_y = sidebar_region.height - 40.0;
                        if sidebar_rel_y >= input_y
                            && sidebar_rel_y < input_y + 32.0
                            && sidebar_rel_x >= margin
                            && sidebar_rel_x < sidebar_region.width - margin
                        {
                            // 点击输入框，不处理（键盘输入由 WM_CHAR 处理）
                            handled = true;
                        }

                        if handled {
                            drop(st);
                            state.borrow_mut().render();
                            return LRESULT(0);
                        }
                    } else if st.handle_sidebar_click(sidebar_rel_x, sidebar_rel_y) {
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

                // 5. 欢迎页/编辑器区域点击
                let welcome_x = if layout.activity_bar_visible {
                    layout.activity_bar_width
                } else {
                    0.0
                };
                let welcome_width = st.window_width as f32 - welcome_x;
                let welcome_y = layout.menu_bar_height;
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
                                        state.borrow_mut().open_folder_async(hwnd, path);
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
                                    state.borrow_mut().open_folder_async(hwnd, path);
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


/// WM_MOUSEMOVE handler
unsafe fn on_mousemove(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
            let raw_x = (lparam.0 & 0xFFFF) as i16 as f32;
            let raw_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
            let is_dragging = wparam.0 & 0x0001 != 0; // MK_LBUTTON

            if let Some(state) = get_window_state(hwnd) {
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

                // 更新标题栏区域悬停（包含菜单项和窗口控制按钮）
                let old_titlebar_hover = st.titlebar_hover_button;
                let titlebar_region = layout.title_bar_region();
                if titlebar_region.contains(mouse_x, mouse_y) {
                    let btn_width = 46.0;
                    let close_x = titlebar_region.x + titlebar_region.width - btn_width;
                    let maximize_x = close_x - btn_width;
                    let minimize_x = maximize_x - btn_width;

                    // 检测窗口控制按钮悬停
                    let panel_btn_width = 32.0;
                    let right_panel_btn_x = minimize_x - panel_btn_width;
                    let bottom_panel_btn_x = right_panel_btn_x - panel_btn_width;

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
                    let btn_width = 46.0;
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
                    if st.sidebar_content == crate::layout::SidebarContent::SettingsPanel {
                        false
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
                    && st.sidebar_content == crate::layout::SidebarContent::SettingsPanel
                {
                    let old_hover = st.settings_panel.hover_button.clone();
                    let rel_x = mouse_x - sidebar_region.x;
                    let rel_y = mouse_y - sidebar_region.y;
                    st.settings_panel.hover_button =
                        st.settings_panel.hit_test_button(rel_x, rel_y);
                    old_hover != st.settings_panel.hover_button
                } else {
                    false
                };

                // 更新 AI 面板快捷操作悬停
                let ai_hover_changed = if sidebar_region.contains(mouse_x, mouse_y)
                    && st.sidebar_content == crate::layout::SidebarContent::AiAssistantPanel
                {
                    let old_hover = st.ai_panel.hover_action;
                    let rel_x = mouse_x - sidebar_region.x;
                    let rel_y = mouse_y - sidebar_region.y;
                    let actions = crate::ai_panel::AiPanel::quick_actions();
                    let margin = 10.0;
                    let btn_w = (sidebar_region.width - margin * 2.0 - 8.0) / 2.0;
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
                    let apply_y = sidebar_region.height - 76.0;
                    let apply_btn_w = 80.0;
                    let apply_btn_h = 24.0;
                    let apply_btn_x = sidebar_region.width - margin - apply_btn_w;
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


/// WM_KEYDOWN handler
unsafe fn on_keydown(hwnd: HWND, wparam: WPARAM, _lparam: LPARAM) -> LRESULT {
            let vk = VIRTUAL_KEY(wparam.0 as u16);
            let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
            let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;

            // 菜单键盘导航优先处理：菜单处于键盘激活态时拦截方向键/Enter/Esc
            let menu_keyboard_active = EDITOR_STATE.with(|s| {
                s.borrow()
                    .as_ref()
                    .map(|state| state.borrow().menu_bar.keyboard_active)
                    .unwrap_or(false)
            });
            if menu_keyboard_active && !ctrl {
                match vk {
                    VK_LEFT => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                state.borrow_mut().menu_bar.navigate_left();
                                state.borrow_mut().render();
                            }
                        });
                        return LRESULT(0);
                    }
                    VK_RIGHT => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                state.borrow_mut().menu_bar.navigate_right();
                                state.borrow_mut().render();
                            }
                        });
                        return LRESULT(0);
                    }
                    VK_UP => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                state.borrow_mut().menu_bar.navigate_up();
                                state.borrow_mut().render();
                            }
                        });
                        return LRESULT(0);
                    }
                    VK_DOWN => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                let need_expand = state.borrow().menu_bar.active_index.is_none();
                                if need_expand {
                                    state.borrow_mut().menu_bar.expand_focused();
                                } else {
                                    state.borrow_mut().menu_bar.navigate_down();
                                }
                                state.borrow_mut().render();
                            }
                        });
                        return LRESULT(0);
                    }
                    VK_RETURN => {
                        let cmd_opt = EDITOR_STATE.with(|s| {
                            s.borrow().as_ref().and_then(|state| {
                                let mut st = state.borrow_mut();
                                let cmd = st.menu_bar.focused_command();
                                if cmd.is_some() {
                                    st.menu_bar.close_menu();
                                }
                                cmd
                            })
                        });
                        if let Some(cmd) = cmd_opt {
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    state.borrow_mut().execute_command(cmd, hwnd);
                                    state.borrow_mut().render();
                                }
                            });
                        }
                        return LRESULT(0);
                    }
                    VK_ESCAPE => {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                state.borrow_mut().menu_bar.close_menu();
                                state.borrow_mut().render();
                            }
                        });
                        return LRESULT(0);
                    }
                    _ => {}
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
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                state.borrow_mut().settings_panel.backspace();
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
                                if let Some(config) = st.ssh_dialog.to_config() {
                                    let mut session = crate::ssh::RemoteSession::new(config);
                                    match session.connect() {
                                        Ok(()) => {
                                            st.remote_session = Some(session);
                                            if let Some(session) = st.remote_session.as_ref() {
                                                match session.list_current_dir() {
                                                    Ok(entries) => {
                                                        st.remote_file_tree = Some(crate::ssh::RemoteFileTree::from_entries("/", entries));
                                                        st.sidebar_content = crate::layout::SidebarContent::RemoteFileTree;
                                                        st.status_message = "SSH 连接成功".to_string();
                                                    }
                                                    Err(e) => {
                                                        st.status_message = format!("SSH 连接成功，但无法列出目录: {}", e);
                                                    }
                                                }
                                            }
                                            st.ssh_dialog.visible = false;
                                        }
                                        Err(e) => {
                                            st.ssh_dialog.error_message = Some(e);
                                        }
                                    }
                                } else {
                                    st.ssh_dialog.error_message = Some("请填写主机和用户名".to_string());
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
                                } else {
                                    let url = st.clone_dialog.url.clone();
                                    drop(st);
                                    if let Some(target_path) =
                                        crate::dialogs::Dialogs::open_folder_dialog(
                                            hwnd,
                                            "选择克隆目标文件夹",
                                        )
                                    {
                                        let mut st = state.borrow_mut();
                                        let result = crate::git::GitIntegration::clone_repo(
                                            &url,
                                            &target_path,
                                        );
                                        match result {
                                            Ok(_) => {
                                                st.clone_dialog.visible = false;
                                                st.status_message = format!(
                                                    "克隆成功: {}",
                                                    target_path.display()
                                                );
                                                st.open_folder_async(hwnd, target_path);
                                            }
                                            Err(e) => {
                                                st.clone_dialog.error_message = Some(e);
                                            }
                                        }
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
                                    state.borrow_mut().open_folder_async(hwnd, path);
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
                        }
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
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                state.borrow_mut().copy();
                                state.borrow_mut().render();
                            }
                        });
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
                                state.borrow_mut().layout.toggle_bottom_panel();
                                if state.borrow().layout.bottom_panel_visible
                                    && !state.borrow().terminal_panel.running
                                {
                                    let _ = state.borrow_mut().terminal_panel.start();
                                }
                                state.borrow_mut().status_message =
                                    if state.borrow().layout.bottom_panel_visible {
                                        "终端已打开 (Ctrl+` 关闭)"
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
                            // Ctrl+Shift+A 切换 AI 面板
                            EDITOR_STATE.with(|s| {
                                if let Some(state) = s.borrow().as_ref() {
                                    let mut st = state.borrow_mut();
                                    if st.sidebar_content
                                        == crate::layout::SidebarContent::AiAssistantPanel
                                        && st.layout.sidebar_visible
                                    {
                                        st.layout.sidebar_visible = false;
                                        st.status_message = "AI 面板已关闭".to_string();
                                    } else {
                                        st.activity_view =
                                            crate::layout::ActivityBarView::AiAssistant;
                                        st.sidebar_content =
                                            crate::layout::SidebarContent::AiAssistantPanel;
                                        st.layout.sidebar_visible = true;
                                        st.status_message = "AI 面板已打开".to_string();
                                    }
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
                                state.borrow_mut().close_current_tab();
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
                    .map(|state| {
                        state.borrow().sidebar_content
                            == crate::layout::SidebarContent::AiAssistantPanel
                    })
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
                                state.borrow_mut().insert_newline();
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
                                    state.borrow_mut().delete_char();
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
                    EDITOR_STATE.with(|s| {
                        if let Some(state) = s.borrow().as_ref() {
                            let mut st = state.borrow_mut();
                            if shift {
                                if st.selection_start.is_none() {
                                    st.start_selection();
                                }
                                st.move_cursor_home();
                                st.update_selection();
                            } else {
                                if st.selection_start.is_some() {
                                    st.clear_selection();
                                }
                                st.move_cursor_home();
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


extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_LBUTTONDOWN => on_lbuttondown(hwnd, wparam, lparam),
            WM_MOUSEMOVE => on_mousemove(hwnd, wparam, lparam),
            WM_LBUTTONUP => on_lbutton_up(hwnd, wparam, lparam),
            WM_DESTROY => on_destroy(hwnd, wparam, lparam),
            msg if msg == WM_APP + 2 => on_new_window(hwnd, wparam, lparam),
            msg if msg == WM_APP + 3 => on_file_tree_batch(hwnd, wparam, lparam),
            WM_SIZE => on_size(hwnd, wparam, lparam),
            WM_DPICHANGED => on_dpi_changed(hwnd, wparam, lparam),
            WM_SETTINGCHANGE => on_setting_change(hwnd, wparam, lparam),
            WM_NCACTIVATE => LRESULT(1),
            WM_NCCALCSIZE => LRESULT(0),
            WM_NCHITTEST => on_nc_hittest(hwnd, wparam, lparam),
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => on_paint(hwnd, wparam, lparam),
            WM_CHAR => on_char(hwnd, wparam, lparam),
            WM_SYSKEYDOWN => on_syskeydown(hwnd, wparam, lparam),
            WM_KEYDOWN => on_keydown(hwnd, wparam, lparam),
            WM_MOUSEWHEEL => on_mouse_wheel(hwnd, wparam, lparam),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
