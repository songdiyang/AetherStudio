//! `WM_LBUTTONDOWN` 处理函数及辅助函数。
//!
//! 从 `window.rs` 拆分而来，保持原有逻辑不变。
//! 原函数 758 行，拆分为调度器 + 多个返回 `Option<LRESULT>` 的辅助函数。
//! 辅助函数按区域分组到子模块中以控制单文件行数。

use windows::Win32::Foundation::{HWND, LRESULT, WPARAM};

use super::super::get_and_set_state;

mod content_area;
mod top_area;

use content_area::*;
use top_area::*;

/// WM_LBUTTONDOWN：鼠标左键按下事件调度器。
pub(crate) unsafe fn on_l_button_down(
    hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> LRESULT {
    let raw_x = (lparam.0 & 0xFFFF) as i16 as f32;
    let raw_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    let Some(state) = get_and_set_state(hwnd) else {
        return LRESULT(0);
    };
    // 公共初始化：坐标转换、布局克隆、退出自定义模式
    let (mouse_x, mouse_y, layout) = {
        let mut st = state.borrow_mut();
        st.terminal_panel.focused = false;
        st.lbutton_down = true;
        let mouse_x = raw_x / st.dpi_scale;
        let mouse_y = raw_y / st.dpi_scale;
        let layout = st.layout.clone();
        let activity_region = layout.activity_bar_region();
        let titlebar_region = layout.title_bar_region();
        if st.activity_bar.customize_mode && !activity_region.contains(mouse_x, mouse_y) {
            st.activity_bar.exit_customize();
        }
        if st.menu_bar.customize_mode && !titlebar_region.contains(mouse_x, mouse_y) {
            st.menu_bar.exit_customize();
        }
        (mouse_x, mouse_y, layout)
    };
    // 按优先级依次尝试各区域处理器
    if let Some(r) = lbd_dialogs(hwnd, &state, mouse_x, mouse_y) {
        return r;
    }
    if let Some(r) = lbd_titlebar(hwnd, &state, mouse_x, mouse_y, &layout) {
        return r;
    }
    if let Some(r) = lbd_user_menu(hwnd, &state, mouse_x, mouse_y) {
        return r;
    }
    if let Some(r) = lbd_explorer_context_menu(hwnd, &state, mouse_x, mouse_y) {
        return r;
    }
    if let Some(r) = lbd_activity_bar_context_menu(hwnd, &state, mouse_x, mouse_y) {
        return r;
    }
    if let Some(r) = lbd_tab_context_menu(hwnd, &state, mouse_x, mouse_y) {
        return r;
    }
    if let Some(r) = lbd_submenu(hwnd, &state, mouse_x, mouse_y, &layout) {
        return r;
    }
    if let Some(r) = lbd_activity_bar(hwnd, &state, mouse_x, mouse_y, &layout) {
        return r;
    }
    if let Some(r) = lbd_panel_resizing(hwnd, &state, mouse_x, mouse_y, &layout) {
        return r;
    }
    if let Some(r) = lbd_sidebar(hwnd, &state, mouse_x, mouse_y, &layout) {
        return r;
    }
    if let Some(r) = lbd_right_panel(hwnd, &state, mouse_x, mouse_y, &layout) {
        return r;
    }
    if let Some(r) = lbd_tab_bar(hwnd, &state, mouse_x, mouse_y, &layout) {
        return r;
    }
    if let Some(r) = lbd_find_panel(hwnd, &state, mouse_x, mouse_y, &layout) {
        return r;
    }
    if let Some(r) = lbd_bottom_panel(hwnd, &state, mouse_x, mouse_y, &layout) {
        return r;
    }
    if let Some(r) = lbd_welcome_or_editor(hwnd, &state, mouse_x, mouse_y, &layout) {
        return r;
    }
    LRESULT(0)
}
