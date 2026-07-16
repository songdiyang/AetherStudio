//! 鼠标事件处理模块。
//!
//! 从 `window.rs` 拆分而来，保持原有逻辑不变。
//! 包含小型鼠标处理函数；大型函数（`on_l_button_down`、`on_mouse_move`）
//! 拆分到子模块中以控制单文件行数。

mod l_button_down;
mod m_button_down;
mod mouse_move;
mod r_button_down;

pub(crate) use l_button_down::on_l_button_down;
pub(crate) use m_button_down::on_m_button_down;
pub(crate) use mouse_move::{compute_cursor_for_pos, on_mouse_move};
pub(crate) use r_button_down::{on_r_button_down, on_r_button_up};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{get_and_set_state, invalidate_window, EDITOR_STATE, LP_TIMER_ID};

/// WM_LBUTTONUP
pub(crate) unsafe fn on_l_button_up(
    hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let _ = KillTimer(hwnd, LP_TIMER_ID);
    let raw_x = (lparam.0 & 0xFFFF) as i16 as f32;
    let raw_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            let mut st = state.borrow_mut();
            st.end_selection();
            // 结束面板拖拽
            st.layout.right_panel_resizing = false;
            st.layout.bottom_panel_resizing = false;
            st.layout.sidebar_resizing = false;
            // 长按检测状态清理
            st.lbutton_down = false;
            st.lpress_target = None;
            st.lpress_start = None;
            // 自定义模式下：完成拖拽重排 + 持久化
            let persist_activity =
                st.activity_bar.customize_mode && st.activity_bar.drag_index.is_some();
            let persist_menu = st.menu_bar.customize_mode && st.menu_bar.drag_index.is_some();
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
            // Task 8.4: 标签拖拽重排或延迟切换
            let tab_handled = if let (Some(drag_idx), Some(drop_idx)) =
                (st.dragging_tab, st.tab_drop_index)
            {
                if drag_idx < st.tabs.len() && drop_idx <= st.tabs.len() && drag_idx != drop_idx {
                    st.reorder_tabs(drag_idx, drop_idx);
                    st.status_message = "标签已重排".to_string();
                }
                st.dragging_tab = None;
                st.tab_drop_index = None;
                st.tab_drag_start = None;
                true
            } else if st.tab_drag_start.is_some() {
                // 未进入拖拽模式 → 视为普通点击切换标签
                st.tab_drag_start = None;
                let dpi_scale = st.dpi_scale;
                let mouse_x = raw_x / dpi_scale;
                let mouse_y = raw_y / dpi_scale;
                let show_tab_bar = st.show_tab_bar();
                let tab_region = st.layout.tab_bar_region(show_tab_bar);
                if let Some(tab_idx) =
                    st.tab_body_hit_test(mouse_x, mouse_y, tab_region.x, tab_region.y)
                {
                    st.switch_tab(tab_idx);
                }
                true
            } else {
                false
            };
            // 仅在用户实际开始拖拽时才重绘
            if persist_activity || persist_menu || tab_handled {
                drop(st);
                invalidate_window(hwnd);
            }
        }
    });
    LRESULT(0)
}

/// WM_LBUTTONDBLCLK
pub(crate) unsafe fn on_l_button_dblclk(
    hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // P2-5: 双击选词
    let raw_x = (lparam.0 & 0xFFFF) as i16 as f32;
    let raw_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    if let Some(state) = get_and_set_state(hwnd) {
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
        let show_tab_bar = st.show_tab_bar();
        let editor_content = layout.editor_content_region(show_tab_bar);
        let editor_region = crate::layout::Region::new(
            editor_content.x,
            editor_content.y,
            editor_content.width,
            editor_content.height,
        );
        if editor_region.contains(mouse_x, mouse_y) {
            st.select_word_at_mouse(mouse_x, mouse_y, editor_content.x, editor_content.y);
            drop(st);
            invalidate_window(hwnd);
        }
    }
    LRESULT(0)
}

/// WM_MOUSEWHEEL
pub(crate) unsafe fn on_mouse_wheel(
    hwnd: HWND,
    _msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
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

            // SubTask 7.5: 光标在标签栏区域时 → 横向滚动标签栏
            let show_tab_bar = state.show_tab_bar();
            let tab_region = state.layout.tab_bar_region(show_tab_bar);
            if show_tab_bar && tab_region.contains(cursor_x, cursor_y) {
                if state.scroll_tab_bar(delta, tab_region.width) {
                    invalidate_window(hwnd);
                }
                return;
            }

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
                    invalidate_window(hwnd);
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
                    invalidate_window(hwnd);
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
            invalidate_window(hwnd);
        }
    });
    LRESULT(0)
}

/// WM_MOUSEHWHEEL
pub(crate) unsafe fn on_mouse_hwheel(
    hwnd: HWND,
    _msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
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
                invalidate_window(hwnd);
            }
        }
    });
    LRESULT(0)
}
