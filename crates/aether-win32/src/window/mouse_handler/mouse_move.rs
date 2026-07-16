//! `WM_MOUSEMOVE` 处理函数及辅助函数。
//!
//! 从 `window.rs` 拆分而来，保持原有逻辑不变。
//! 原函数 380 行，拆分为调度器 + 多个辅助函数。

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LRESULT, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::cursor::CursorType;
use crate::editor::EditorState;

use super::super::{
    get_and_set_state, invalidate_window, EDITOR_STATE, HOVER_DELAY_MS, HOVER_MOVE_TOLERANCE,
    HOVER_TIMER_ID, LP_MOVE_TOLERANCE, LP_TIMER_ID,
};

/// WM_MOUSEMOVE：鼠标移动事件调度器。
pub(crate) unsafe fn on_mouse_move(
    hwnd: HWND,
    _msg: u32,
    wparam: WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> LRESULT {
    let raw_x = (lparam.0 & 0xFFFF) as i16 as f32;
    let raw_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    let is_dragging = wparam.0 & 0x0001 != 0; // MK_LBUTTON
    let Some(state) = get_and_set_state(hwnd) else {
        return LRESULT(0);
    };
    let (mouse_x, mouse_y, layout) = {
        let st = state.borrow_mut();
        let mouse_x = raw_x / st.dpi_scale;
        let mouse_y = raw_y / st.dpi_scale;
        let layout = st.layout.clone();
        (mouse_x, mouse_y, layout)
    };
    // 早期返回：对话框悬停 / 自定义模式拖拽
    if let Some(r) = omm_early_returns(hwnd, &state, mouse_x, mouse_y, is_dragging, &layout) {
        return r;
    }
    // 悬停状态更新（每个辅助函数返回是否有变化）
    let titlebar_changed = omm_titlebar_menu_hover(&state, mouse_x, mouse_y, &layout);
    let (tab_changed, editor_content) = omm_activity_tab_hover(&state, mouse_x, mouse_y, &layout);
    let tree_changed = omm_file_tree_hover(&state, mouse_x, mouse_y, &layout);
    let settings_changed = omm_settings_hover(&state, mouse_x, mouse_y, &layout);
    let ai_changed = omm_ai_hover(&state, mouse_x, mouse_y, &layout);
    let welcome_changed = omm_welcome_hover(&state, mouse_x, mouse_y, &layout);
    let status_bar_changed = omm_status_bar_hover(&state, mouse_x, mouse_y, &layout);
    let any_hover_changed = titlebar_changed
        || tab_changed
        || tree_changed
        || settings_changed
        || ai_changed
        || welcome_changed
        || status_bar_changed;
    // 拖拽光标 + 面板拖拽调整
    if let Some(r) = omm_resize_drag(hwnd, &state, mouse_x, mouse_y, is_dragging, &layout) {
        return r;
    }
    // Hover tooltip 防抖
    omm_hover_tooltip(hwnd, &state, mouse_x, mouse_y, &layout);
    // UI Tooltip 状态更新（500ms 延迟显示、4px 移动容差）
    let tooltip_changed = omm_tooltip_state(hwnd, &state, mouse_x, mouse_y);
    // 最终失效判定
    if any_hover_changed || tooltip_changed {
        invalidate_window(hwnd);
    } else if is_dragging {
        let mut st = state.borrow_mut();
        st.set_cursor_from_mouse(mouse_x, mouse_y, editor_content.x, editor_content.y);
        st.update_selection();
        drop(st);
        invalidate_window(hwnd);
    }
    LRESULT(0)
}

/// 早期返回：对话框悬停 + 长按取消 + 自定义模式拖拽。
unsafe fn omm_early_returns(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    is_dragging: bool,
    layout: &crate::layout::LayoutManager,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    // 资源管理器空白区域上下文菜单：更新 hover 状态
    if st.explorer_context_menu.is_open {
        let changed = st.explorer_context_menu.update_hover(mouse_x, mouse_y);
        drop(st);
        if changed {
            invalidate_window(hwnd);
        }
        return Some(LRESULT(0));
    }
    // 标签右键上下文菜单：更新 hover 状态
    if st.tab_context_menu.visible {
        let changed = st.tab_context_menu.update_hover(mouse_x, mouse_y);
        drop(st);
        if changed {
            invalidate_window(hwnd);
        }
        return Some(LRESULT(0));
    }
    // 活动栏右键上下文菜单：更新 hover 状态
    if st.activity_bar_context_menu.visible {
        let changed = st.activity_bar_context_menu.update_hover(mouse_x, mouse_y);
        drop(st);
        if changed {
            invalidate_window(hwnd);
        }
        return Some(LRESULT(0));
    }
    // 对话框悬停处理
    if st.ssh_dialog.visible {
        st.handle_ssh_dialog_hover(mouse_x, mouse_y);
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    if st.clone_dialog.visible {
        st.handle_clone_dialog_hover(mouse_x, mouse_y);
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    // 长按检测：移动超过容差则取消
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
    let activity_dragging = st.activity_bar.customize_mode && st.activity_bar.drag_index.is_some();
    let menu_dragging = st.menu_bar.customize_mode && st.menu_bar.drag_index.is_some();
    if is_dragging && activity_dragging {
        let bar_y = layout.activity_bar_region().y;
        st.activity_bar.drop_index = Some(st.activity_bar.drop_index_at(mouse_y, bar_y));
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    if is_dragging && menu_dragging {
        st.menu_bar.drop_index = Some(st.menu_bar.drop_index_at(mouse_x));
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    // Task 8.3: 标签拖拽——检测阈值进入拖拽模式，或更新 drop_index
    if is_dragging && st.tab_drag_start.is_some() {
        if st.dragging_tab.is_none() {
            // 判定是否超过 3px 阈值（dx*dx + dy*dy > 9）
            let (sx, sy) = st.tab_drag_start.unwrap();
            let dx = mouse_x - sx as f32;
            let dy = mouse_y - sy as f32;
            if dx * dx + dy * dy > 9.0 {
                // 进入拖拽模式：使用当前 hover_tab 作为拖拽目标
                if let Some(hover) = st.hover_tab {
                    st.dragging_tab = Some(hover);
                    st.tab_drop_index = Some(hover);
                    drop(st);
                    invalidate_window(hwnd);
                    return Some(LRESULT(0));
                }
            }
        } else {
            // 已在拖拽模式：更新 drop_index
            let show_tab_bar = st.show_tab_bar();
            let editor_content = layout.editor_content_region(show_tab_bar);
            let new_drop = st.tab_drop_index_at(mouse_x, editor_content.x);
            let changed = st.tab_drop_index != Some(new_drop);
            st.tab_drop_index = Some(new_drop);
            drop(st);
            if changed {
                invalidate_window(hwnd);
            }
            return Some(LRESULT(0));
        }
    }
    None
}

/// 标题栏 + 菜单栏悬停更新。返回是否有变化。
unsafe fn omm_titlebar_menu_hover(
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> bool {
    let mut st = state.borrow_mut();
    let titlebar_region = layout.title_bar_region();
    // 标题栏按钮悬停（与 render_title_bar 布局保持一致）
    let old_titlebar_hover = st.titlebar_hover_button;
    if titlebar_region.contains(mouse_x, mouse_y) {
        let btn_width = 40.0;
        let close_x = titlebar_region.x + titlebar_region.width - btn_width;
        let maximize_x = close_x - btn_width;
        let minimize_x = maximize_x - btn_width;

        let tool_btn_size = 28.0f32;
        let tool_btn_gap = 2.0f32;
        let user_btn_size = 24.0f32;
        let user_btn_x = minimize_x - tool_btn_gap - user_btn_size;
        let settings_btn_x = user_btn_x - tool_btn_gap - tool_btn_size;
        let right_panel_btn_x = settings_btn_x - tool_btn_gap - tool_btn_size;
        let bottom_panel_btn_x = right_panel_btn_x - tool_btn_gap - tool_btn_size;
        let left_sidebar_btn_x = bottom_panel_btn_x - tool_btn_gap - tool_btn_size;
        let divider_x = left_sidebar_btn_x - tool_btn_gap - 4.0;
        let forward_btn_x = divider_x - tool_btn_gap - tool_btn_size;
        let back_btn_x = forward_btn_x - tool_btn_gap - tool_btn_size;

        if mouse_x >= minimize_x {
            if mouse_x >= close_x {
                st.titlebar_hover_button = Some(2);
            } else if mouse_x >= maximize_x {
                st.titlebar_hover_button = Some(1);
            } else {
                st.titlebar_hover_button = Some(0);
            }
        } else if mouse_x >= user_btn_x {
            st.titlebar_hover_button = Some(3);
        } else if mouse_x >= settings_btn_x {
            st.titlebar_hover_button = Some(4);
        } else if mouse_x >= right_panel_btn_x {
            st.titlebar_hover_button = Some(5);
        } else if mouse_x >= bottom_panel_btn_x {
            st.titlebar_hover_button = Some(6);
        } else if mouse_x >= left_sidebar_btn_x {
            st.titlebar_hover_button = Some(7);
        } else if mouse_x >= forward_btn_x {
            st.titlebar_hover_button = Some(8);
        } else if mouse_x >= back_btn_x {
            st.titlebar_hover_button = Some(9);
        } else {
            st.titlebar_hover_button = None;
        }
    } else {
        st.titlebar_hover_button = None;
    }
    let titlebar_changed = old_titlebar_hover != st.titlebar_hover_button;
    // 菜单栏悬停
    let old_menu_hover = st.menu_bar.hover_index;
    if titlebar_region.contains(mouse_x, mouse_y) {
        let btn_width = 40.0;
        let minimize_x = titlebar_region.x + titlebar_region.width - btn_width * 3.0;
        if mouse_x < minimize_x {
            st.menu_bar.hover_index =
                st.menu_bar
                    .hit_test(mouse_x, mouse_y - titlebar_region.y, titlebar_region.height);
        } else {
            st.menu_bar.hover_index = None;
        }
    } else {
        st.menu_bar.hover_index = None;
    }
    titlebar_changed || old_menu_hover != st.menu_bar.hover_index
}

/// 活动栏 + 标签栏悬停更新。返回 (是否有变化, editor_content)。
unsafe fn omm_activity_tab_hover(
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> (bool, crate::layout::Region) {
    let mut st = state.borrow_mut();
    // 活动栏悬停
    let activity_region = layout.activity_bar_region();
    st.activity_bar.hover_index = st
        .activity_bar
        .hit_test(mouse_x, mouse_y, activity_region.y);
    // 标签栏悬停
    let editor_content = layout.editor_content_region(st.show_tab_bar());
    let old_hover = st.hover_tab;
    st.update_hover_tab(mouse_x, mouse_y, editor_content.x);
    (old_hover != st.hover_tab, editor_content)
}

/// 文件树 / SSH 管理面板 / 源代码管理面板悬停更新。返回是否有变化。
unsafe fn omm_file_tree_hover(
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> bool {
    let mut st = state.borrow_mut();
    let sidebar_region = layout.sidebar_region();
    let _old_tree_hover = st.hover_file_node;
    if sidebar_region.contains(mouse_x, mouse_y) {
        if st.sidebar_content == crate::layout::SidebarContent::RemoteManagerPanel {
            // SSH 管理面板悬停检测
            let old_hover = st.ssh_manager_panel.hover;
            let old_action = st.ssh_manager_panel.hover_action;
            let mut new_hover_action = None;
            let btn_rects = st.ssh_manager_panel.item_btn_rects.clone();
            for &(idx, action, ref rect) in &btn_rects {
                if rect.contains(mouse_x, mouse_y) {
                    new_hover_action = Some((idx, action));
                    break;
                }
            }
            st.ssh_manager_panel.hover_action = new_hover_action;
            if new_hover_action.is_none() {
                if let Some(ref rect) = st.ssh_manager_panel.add_btn_rect {
                    if rect.contains(mouse_x, mouse_y) {
                        st.ssh_manager_panel.hover_action = Some((997, 0));
                    }
                }
            }
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
        } else if st.sidebar_content == crate::layout::SidebarContent::SourceControlPanel {
            let old_hover = st.git.hover_button.clone();
            st.update_git_panel_hover(mouse_x - sidebar_region.x, mouse_y - sidebar_region.y);
            old_hover != st.git.hover_button
        } else {
            st.update_file_tree_hover(mouse_x - sidebar_region.x, mouse_y - sidebar_region.y)
        }
    } else {
        let old = st.hover_file_node.take();
        old.is_some()
    }
}

/// 设置面板悬停更新。返回是否有变化。
unsafe fn omm_settings_hover(
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> bool {
    let mut st = state.borrow_mut();
    let sidebar_region = layout.sidebar_region();
    if sidebar_region.contains(mouse_x, mouse_y)
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
        // 模型管理页悬停检测
        if st.settings_panel.active_tab == crate::settings::SettingsTab::Models {
            let editor_region = layout.editor_region();
            if editor_region.contains(mouse_x, mouse_y) {
                let rel_x = mouse_x - editor_region.x;
                let rel_y = mouse_y - editor_region.y;
                // 检测模型项悬停
                let new_hover_id = st.settings_panel.hit_test_model_item(rel_x, rel_y);
                if st.settings_panel.hover_model_id != new_hover_id {
                    st.settings_panel.hover_model_id = new_hover_id.clone();
                    changed = true;
                }
                // 检测模型按钮悬停
                let new_hover_btn = st.settings_panel.hit_test_model_button(rel_x, rel_y);
                let (new_btn, new_btn_id) = match new_hover_btn {
                    Some((btn, id)) => (Some(btn), Some(id)),
                    None => (None, None),
                };
                if st.settings_panel.hover_model_button != new_btn {
                    st.settings_panel.hover_model_button = new_btn;
                    changed = true;
                }
                if st.settings_panel.hover_model_button_id != new_btn_id {
                    st.settings_panel.hover_model_button_id = new_btn_id;
                    changed = true;
                }
            } else {
                if st.settings_panel.hover_model_id.is_some() {
                    st.settings_panel.hover_model_id = None;
                    changed = true;
                }
                if st.settings_panel.hover_model_button.is_some() {
                    st.settings_panel.hover_model_button = None;
                    changed = true;
                }
                if st.settings_panel.hover_model_button_id.is_some() {
                    st.settings_panel.hover_model_button_id = None;
                    changed = true;
                }
            }
        }
        changed
    }
}

/// AI 面板悬停更新。返回是否有变化。
unsafe fn omm_ai_hover(
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> bool {
    let mut st = state.borrow_mut();
    let right_panel_region = layout.right_panel_region();
    if layout.right_panel_visible && right_panel_region.contains(mouse_x, mouse_y) {
        let rel_x = mouse_x - right_panel_region.x;
        let rel_y = mouse_y - right_panel_region.y;
        let margin = 10.0;
        let apply_y = right_panel_region.height - 76.0;
        let apply_btn_w = 80.0;
        let apply_btn_h = 24.0;
        let apply_btn_x = right_panel_region.width - margin - apply_btn_w;
        let old_apply_hover = st.ai_panel.hover_apply_button;
        st.ai_panel.hover_apply_button = rel_x >= apply_btn_x
            && rel_x < apply_btn_x + apply_btn_w
            && rel_y >= apply_y
            && rel_y < apply_y + apply_btn_h;
        old_apply_hover != st.ai_panel.hover_apply_button
    } else {
        let old = st.ai_panel.hover_apply_button;
        st.ai_panel.hover_apply_button = false;
        old
    }
}

/// 欢迎页悬停更新。返回是否有变化。
unsafe fn omm_welcome_hover(
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> bool {
    let mut st = state.borrow_mut();
    let old_welcome_hover = st.welcome_hover_action.clone();
    if st.show_welcome() {
        let welcome_x = 0.0;
        let welcome_y = layout.top_offset();
        let welcome_width = st.window_width as f32;
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
    old_welcome_hover != st.welcome_hover_action
}

/// SubTask 10.1: 状态栏分区悬停更新。返回是否有变化。
///
/// 当鼠标位于状态栏区域内时，调用 `hit_test` 检测命中的分区：
/// - 若命中且分区 `clickable` 为 true，设置 `hover_index = Some(idx)`
/// - 否则 `hover_index = None`
///
/// `hover_index` 变化时返回 true，触发 `invalidate_window` 重绘以显示 hover 高亮。
unsafe fn omm_status_bar_hover(
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> bool {
    let mut st = state.borrow_mut();
    let status_region = layout.status_bar_region();
    let old_hover = st.status_bar.hover_index;
    let new_hover = if layout.status_bar_visible && status_region.contains(mouse_x, mouse_y) {
        let rel_x = mouse_x - status_region.x;
        let rel_y = mouse_y - status_region.y;
        match st.status_bar.hit_test(rel_x, rel_y, status_region.width) {
            Some(idx) => {
                if st
                    .status_bar
                    .sections
                    .get(idx)
                    .is_some_and(|sec| sec.clickable)
                {
                    Some(idx)
                } else {
                    None
                }
            }
            None => None,
        }
    } else {
        None
    };
    st.status_bar.hover_index = new_hover;
    old_hover != st.status_bar.hover_index
}

/// 拖拽光标设置 + 面板拖拽调整。返回 Some 表示已处理（需提前返回）。
unsafe fn omm_resize_drag(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    is_dragging: bool,
    layout: &crate::layout::LayoutManager,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    let editor_region = layout.editor_region();
    let right_panel_resize_zone = layout.right_panel_visible
        && (mouse_x >= editor_region.right() - 4.0 && mouse_x <= editor_region.right() + 4.0)
        && mouse_y >= editor_region.y
        && mouse_y < editor_region.y + editor_region.height;
    let bottom_region = layout.bottom_panel_region();
    let bottom_panel_resize_zone = layout.bottom_panel_visible
        && (mouse_y >= bottom_region.y - 4.0 && mouse_y <= bottom_region.y + 4.0)
        && mouse_x >= bottom_region.x
        && mouse_x < bottom_region.x + bottom_region.width;
    // 侧边栏右侧调整区域
    let sidebar_region = layout.sidebar_region();
    let sidebar_resize_zone = layout.sidebar_visible
        && (mouse_x >= sidebar_region.right() - 4.0 && mouse_x <= sidebar_region.right() + 4.0)
        && mouse_y >= sidebar_region.y
        && mouse_y < sidebar_region.y + sidebar_region.height;
    // 更新 hover 状态
    st.hover_sidebar_resize = sidebar_resize_zone;
    // 设置拖拽光标
    if right_panel_resize_zone || st.layout.right_panel_resizing {
        let hcursor = LoadCursorW(None, IDC_SIZEWE).unwrap_or_default();
        let _ = SetCursor(hcursor);
    } else if sidebar_resize_zone || st.layout.sidebar_resizing {
        let hcursor = LoadCursorW(None, IDC_SIZEWE).unwrap_or_default();
        let _ = SetCursor(hcursor);
    } else if bottom_panel_resize_zone || st.layout.bottom_panel_resizing {
        let hcursor = LoadCursorW(None, IDC_SIZENS).unwrap_or_default();
        let _ = SetCursor(hcursor);
    } else if st.welcome_hover_action.is_some() {
        let hcursor = LoadCursorW(None, IDC_HAND).unwrap_or_default();
        let _ = SetCursor(hcursor);
    }
    // 处理拖拽调整
    if is_dragging {
        if st.layout.right_panel_resizing {
            let delta = mouse_x - editor_region.right();
            st.layout.resize_right_panel(-delta);
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        } else if st.layout.sidebar_resizing {
            let delta = mouse_x - sidebar_region.right();
            st.layout.resize_sidebar(delta);
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        } else if st.layout.bottom_panel_resizing {
            let delta = mouse_y - bottom_region.y;
            st.layout.resize_bottom_panel(-delta);
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    None
}

/// P3.4: Hover tooltip 防抖逻辑。
unsafe fn omm_hover_tooltip(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) {
    let mut st = state.borrow_mut();
    let sidebar_region = layout.sidebar_region();
    let in_sidebar = sidebar_region.contains(mouse_x, mouse_y)
        && matches!(
            st.sidebar_content,
            crate::layout::SidebarContent::FileTree | crate::layout::SidebarContent::RemoteFileTree
        );
    let has_hover_node = st.hover_file_node.is_some() || st.hover_remote_node.is_some();
    let dx = mouse_x - st.hover_last_mouse_x;
    let dy = mouse_y - st.hover_last_mouse_y;
    let moved_beyond_tolerance = dx.abs() > HOVER_MOVE_TOLERANCE || dy.abs() > HOVER_MOVE_TOLERANCE;
    if (moved_beyond_tolerance || !in_sidebar || !has_hover_node) && st.hover_tooltip.is_some() {
        st.hover_tooltip = None;
    }
    if in_sidebar && has_hover_node {
        let _ = SetTimer(hwnd, HOVER_TIMER_ID, HOVER_DELAY_MS, None);
    } else {
        let _ = KillTimer(hwnd, HOVER_TIMER_ID);
    }
    st.hover_last_mouse_x = mouse_x;
    st.hover_last_mouse_y = mouse_y;
}

/// UI Tooltip 状态更新：500ms 延迟显示、4px 移动容差。
///
/// 返回 true 表示 tooltip 可见性发生变化，需要 invalidate。
///
/// 状态机：
/// 1. hover_key 变化（含进入/离开元素）：更新 hover_key/anchor/timer_start，清空 visible_text
/// 2. hover_key 相同且鼠标移动 > 4px：重置 anchor/timer_start，清空 visible_text
/// 3. hover_key 相同且静止 ≥ 500ms：设置 visible_text + show_pos
unsafe fn omm_tooltip_state(
    _hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
) -> bool {
    use crate::tooltip::{TOOLTIP_DELAY_MS, TOOLTIP_MOVE_TOLERANCE};
    use windows::Win32::System::SystemInformation::GetTickCount64;

    let mut st = state.borrow_mut();
    let (new_key, tooltip_text) = st.compute_tooltip_hover_key();
    let key_changed = new_key != st.tooltip_state.hover_key;

    // 分支 1：hover_key 变化
    if key_changed {
        let was_visible = st.tooltip_state.visible_text.is_some();
        st.tooltip_state.hover_key = new_key.clone();
        st.tooltip_state.anchor = POINT {
            x: mouse_x as i32,
            y: mouse_y as i32,
        };
        st.tooltip_state.timer_start = if new_key.is_some() {
            Some(GetTickCount64())
        } else {
            None
        };
        st.tooltip_state.visible_text = None;
        // 离开元素或切换元素时，若之前有显示，需要重绘清除
        return was_visible;
    }

    // hover_key 相同且为 None：无需任何操作
    if new_key.is_none() {
        return false;
    }

    // 分支 2：检查鼠标移动距离
    let dx = mouse_x - st.tooltip_state.anchor.x as f32;
    let dy = mouse_y - st.tooltip_state.anchor.y as f32;
    let dist = (dx * dx + dy * dy).sqrt();
    if dist > TOOLTIP_MOVE_TOLERANCE {
        let was_visible = st.tooltip_state.visible_text.is_some();
        st.tooltip_state.anchor = POINT {
            x: mouse_x as i32,
            y: mouse_y as i32,
        };
        st.tooltip_state.timer_start = Some(GetTickCount64());
        if was_visible {
            st.tooltip_state.visible_text = None;
            return true;
        }
        return false;
    }

    // 分支 3：检查 timer_start 是否到达 500ms
    if let Some(start) = st.tooltip_state.timer_start {
        if st.tooltip_state.visible_text.is_none() {
            let now = GetTickCount64();
            if now - start >= TOOLTIP_DELAY_MS {
                if let Some(text) = tooltip_text {
                    st.tooltip_state.visible_text = Some(text);
                    st.tooltip_state.show_pos = (mouse_x, mouse_y);
                    return true;
                }
            }
        }
    }

    false
}

/// WM_SETCURSOR 调用：根据鼠标位置和当前 hover 状态返回 CursorType。
///
/// 输入 `x`/`y` 为客户端物理像素坐标（来自 `ScreenToClient`）。
/// 内部转换为逻辑像素后与布局区域比对。**只读访问状态**，不修改任何字段。
///
/// 检查顺序：
/// 1. 对话框/命令面板打开 → Arrow
/// 2. 欢迎页 hover 项 → Hand
/// 3. 标题栏按钮/菜单项 hover → Hand
/// 4. 活动栏 hover → Hand
/// 5. 面板拖拽中 → 固定 SizeWE/SizeNS
/// 6. 标签栏 hover → Hand
/// 7. 侧边栏分隔条 → SizeWE
/// 8. 右侧面板分隔条 → SizeWE
/// 9. 底部面板分隔条 → SizeNS
/// 10. 编辑器内容区 → IBeam
/// 11. 状态栏 clickable 分区 → Hand
/// 12. 默认 → Arrow
pub(crate) unsafe fn compute_cursor_for_pos(_hwnd: HWND, x: i32, y: i32) -> CursorType {
    EDITOR_STATE.with(|s| {
        let s = s.borrow();
        let Some(state) = s.as_ref() else {
            return CursorType::Arrow;
        };
        let st = state.borrow();

        // 转换为逻辑像素
        let mouse_x = x as f32 / st.dpi_scale;
        let mouse_y = y as f32 / st.dpi_scale;
        let layout = st.layout.clone();

        // 1. 对话框/命令面板打开时返回默认箭头
        if st.ssh_dialog.visible || st.clone_dialog.visible || st.command_palette.visible {
            return CursorType::Arrow;
        }

        // 2. 欢迎页 hover 项 → Hand
        if st.welcome_hover_action.is_some() {
            return CursorType::Hand;
        }

        // 3. 标题栏区域：按钮 hover 或菜单项 hover → Hand
        let titlebar_region = layout.title_bar_region();
        if titlebar_region.contains(mouse_x, mouse_y) {
            if st.titlebar_hover_button.is_some() || st.menu_bar.hover_index.is_some() {
                return CursorType::Hand;
            }
            // 标题栏空白区（拖动区）→ Arrow
            return CursorType::Arrow;
        }

        // 4. 活动栏 hover → Hand
        let activity_region = layout.activity_bar_region();
        if activity_region.contains(mouse_x, mouse_y) && st.activity_bar.hover_index.is_some() {
            return CursorType::Hand;
        }

        // 5. 面板拖拽中：固定 resize 光标（无论当前位置）
        if layout.right_panel_resizing {
            return CursorType::SizeWE;
        }
        if layout.bottom_panel_resizing {
            return CursorType::SizeNS;
        }

        let editor_region = layout.editor_region();
        let editor_content = layout.editor_content_region(st.show_tab_bar());

        // 6. 标签栏 hover → Hand
        let tab_bar_region = layout.tab_bar_region(st.show_tab_bar());
        if tab_bar_region.contains(mouse_x, mouse_y) && st.hover_tab.is_some() {
            return CursorType::Hand;
        }

        // 7. 侧边栏分隔条（sidebar 右边缘 4px 容差）
        if layout.sidebar_visible {
            let sidebar_right = layout.sidebar_region().right();
            if (mouse_x - sidebar_right).abs() <= 4.0
                && mouse_y >= editor_region.y
                && mouse_y < editor_region.y + editor_region.height
            {
                return CursorType::SizeWE;
            }
        }

        // 8. 右侧面板分隔条（right_panel 左边缘 4px 容差）
        if layout.right_panel_visible {
            let right_panel_left = layout.right_panel_region().x;
            if (mouse_x - right_panel_left).abs() <= 4.0
                && mouse_y >= editor_region.y
                && mouse_y < editor_region.y + editor_region.height
            {
                return CursorType::SizeWE;
            }
        }

        // 9. 底部面板分隔条（bottom_panel 顶部 4px 容差）
        if layout.bottom_panel_visible {
            let bottom_panel_top = layout.bottom_panel_region().y;
            if (mouse_y - bottom_panel_top).abs() <= 4.0
                && mouse_x >= editor_region.x
                && mouse_x < editor_region.x + editor_region.width
            {
                return CursorType::SizeNS;
            }
        }

        // 10. 编辑器内容区 → IBeam
        if editor_content.contains(mouse_x, mouse_y) {
            return CursorType::IBeam;
        }

        // 11. 状态栏 → Hand（clickable 分区）
        let status_region = layout.status_bar_region();
        if status_region.contains(mouse_x, mouse_y) {
            let rel_x = mouse_x - status_region.x;
            let rel_y = mouse_y - status_region.y;
            if let Some(idx) = st.status_bar.hit_test(rel_x, rel_y, status_region.width) {
                if st
                    .status_bar
                    .sections
                    .get(idx)
                    .is_some_and(|sec| sec.clickable)
                {
                    return CursorType::Hand;
                }
            }
            return CursorType::Arrow;
        }

        // 12. 默认 → Arrow
        CursorType::Arrow
    })
}
