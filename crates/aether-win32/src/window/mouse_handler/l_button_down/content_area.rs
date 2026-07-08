//! `WM_LBUTTONDOWN` 内容区域处理：活动栏 / 侧边栏 / 面板 / 编辑器。
//!
//! 从 `l_button_down.rs` 拆分而来，保持原有逻辑不变。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LRESULT};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::dialogs::Dialogs;
use crate::editor::EditorState;

use super::super::super::{invalidate_window, LP_THRESHOLD_MS, LP_TIMER_ID};

/// 活动栏点击 + 长按检测。
pub(super) unsafe fn lbd_activity_bar(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> Option<LRESULT> {
    let activity_region = layout.activity_bar_region();
    if !activity_region.contains(mouse_x, mouse_y) {
        return None;
    }
    let mut st = state.borrow_mut();
    let Some(idx) = st
        .activity_bar
        .hit_test(mouse_x, mouse_y, activity_region.y)
    else {
        return None;
    };
    // 长按检测
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
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    let view = st.activity_bar.items[idx].view;
    if view == crate::layout::ActivityBarView::AiAssistant {
        st.layout.right_panel_visible = !st.layout.right_panel_visible;
        if st.layout.right_panel_visible && st.layout.right_panel_width < 1.0 {
            st.layout.right_panel_width = 320.0;
        }
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
    invalidate_window(hwnd);
    Some(LRESULT(0))
}

/// 面板调整边框点击（右侧/底部面板拖拽区域）。
pub(super) unsafe fn lbd_panel_resizing(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> Option<LRESULT> {
    let editor_region = layout.editor_region();
    let right_panel_resize_zone = layout.right_panel_visible
        && (mouse_x >= editor_region.right() - 4.0 && mouse_x <= editor_region.right() + 4.0)
        && mouse_y >= editor_region.y
        && mouse_y < editor_region.y + editor_region.height;
    let bottom_panel_resize_zone = layout.bottom_panel_visible
        && (mouse_y >= editor_region.bottom() - 4.0 && mouse_y <= editor_region.bottom() + 4.0)
        && mouse_x >= editor_region.x
        && mouse_x < editor_region.x + editor_region.width;
    let mut st = state.borrow_mut();
    if right_panel_resize_zone {
        st.layout.right_panel_resizing = true;
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    if bottom_panel_resize_zone {
        st.layout.bottom_panel_resizing = true;
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    None
}

/// 侧边栏点击（SSH 管理面板 / 通用侧边栏）。
pub(super) unsafe fn lbd_sidebar(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> Option<LRESULT> {
    let sidebar_region = layout.sidebar_region();
    if !sidebar_region.contains(mouse_x, mouse_y) {
        return None;
    }
    let mut st = state.borrow_mut();
    if st.sidebar_content == crate::layout::SidebarContent::RemoteManagerPanel {
        drop(st);
        return lbd_ssh_manager_panel(hwnd, state, mouse_x, mouse_y);
    }
    let sidebar_rel_x = mouse_x - sidebar_region.x;
    let sidebar_rel_y = mouse_y - sidebar_region.y;
    if st.handle_sidebar_click(sidebar_rel_x, sidebar_rel_y) {
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    None
}

/// SSH 管理面板按钮点击（连接/编辑/删除/添加/保存/取消）。
unsafe fn lbd_ssh_manager_panel(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
) -> Option<LRESULT> {
    // 操作按钮（连接/编辑/删除）
    if lbd_ssh_manager_buttons(hwnd, state, mouse_x, mouse_y).is_some() {
        return Some(LRESULT(0));
    }
    // 添加按钮 + 保存/取消 + 回退
    let mut st = state.borrow_mut();
    let panel = &st.ssh_manager_panel;
    // 检测添加按钮
    if let Some(ref rect) = panel.add_btn_rect {
        if rect.contains(mouse_x, mouse_y) {
            st.ssh_manager_panel.start_add();
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 检测保存/取消按钮（编辑模式）
    if panel.editing {
        if let Some(ref rect) = panel.save_btn_rect {
            if rect.contains(mouse_x, mouse_y) {
                match st.save_ssh_server_from_form() {
                    Ok(()) => st.status_message = "服务器配置已保存".to_string(),
                    Err(e) => st.ssh_manager_panel.error_message = Some(e),
                }
                drop(st);
                invalidate_window(hwnd);
                return Some(LRESULT(0));
            }
        }
        if let Some(ref rect) = panel.cancel_btn_rect {
            if rect.contains(mouse_x, mouse_y) {
                st.ssh_manager_panel.cancel_edit();
                drop(st);
                invalidate_window(hwnd);
                return Some(LRESULT(0));
            }
        }
    }
    drop(st);
    invalidate_window(hwnd);
    Some(LRESULT(0))
}

/// SSH 管理面板操作按钮（连接/编辑/删除/认证方式切换）。
unsafe fn lbd_ssh_manager_buttons(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    let panel = &st.ssh_manager_panel;
    let mut clicked_btn = None;
    for &(idx, action, ref rect) in &panel.item_btn_rects {
        if rect.contains(mouse_x, mouse_y) {
            clicked_btn = Some((idx, action));
            break;
        }
    }
    let Some((idx, action)) = clicked_btn else {
        return None;
    };
    if idx < 997 {
        match action {
            0 => {
                if st.is_ssh_connected(idx) {
                    st.disconnect_ssh();
                } else {
                    st.connect_ssh_server(idx);
                }
            }
            1 => {
                if let Some(config) = st.ssh_servers().get(idx).cloned() {
                    st.ssh_manager_panel.start_edit(idx, &config);
                }
            }
            2 => st.delete_ssh_server(idx),
            _ => {}
        }
    } else if idx == 997 {
        st.ssh_manager_panel.start_add();
    } else if idx == 998 {
        match action {
            0 => match st.save_ssh_server_from_form() {
                Ok(()) => st.status_message = "服务器配置已保存".to_string(),
                Err(e) => st.ssh_manager_panel.error_message = Some(e),
            },
            1 => st.ssh_manager_panel.cancel_edit(),
            _ => {}
        }
    } else if idx == 999 {
        st.ssh_manager_panel.cycle_auth_type();
    }
    drop(st);
    invalidate_window(hwnd);
    Some(LRESULT(0))
}

/// 右侧 AI 面板点击（快捷操作 / Apply / 输入框）。
pub(super) unsafe fn lbd_right_panel(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> Option<LRESULT> {
    let right_panel_region = layout.right_panel_region();
    if !(layout.right_panel_visible && right_panel_region.contains(mouse_x, mouse_y)) {
        return None;
    }
    // C-10: 默认点击 AI 面板非输入框区域时取消输入框聚焦
    {
        let mut st = state.borrow_mut();
        st.ai_panel.input_focused = false;
    }
    if lbd_right_panel_actions(hwnd, state, mouse_x, mouse_y, &right_panel_region).is_some() {
        return Some(LRESULT(0));
    }
    lbd_right_panel_apply_input(hwnd, state, mouse_x, mouse_y, &right_panel_region)
}

/// AI 面板快捷操作按钮点击。
unsafe fn lbd_right_panel_actions(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    right_panel_region: &crate::layout::Region,
) -> Option<LRESULT> {
    let rp_rel_x = mouse_x - right_panel_region.x;
    let rp_rel_y = mouse_y - right_panel_region.y;
    let actions = crate::ai_panel::AiPanel::quick_actions();
    let margin = 10.0;
    let btn_w = (right_panel_region.width - margin * 2.0 - 8.0) / 2.0;
    let btn_h = 28.0;
    let btn_gap = 8.0;
    let action_start_y = 52.0;
    let action_rows = actions.len().div_ceil(2);
    let action_end_y = action_start_y + action_rows as f32 * (btn_h + 6.0) + 8.0;
    if !(rp_rel_y >= action_start_y && rp_rel_y < action_end_y) {
        return None;
    }
    for (i, action) in actions.iter().enumerate() {
        let col = i % 2;
        let row = i / 2;
        let bx = margin + col as f32 * (btn_w + btn_gap);
        let by = action_start_y + row as f32 * (btn_h + 6.0);
        if rp_rel_x >= bx && rp_rel_x < bx + btn_w && rp_rel_y >= by && rp_rel_y < by + btn_h {
            let st = state.borrow_mut();
            let selected_code = if let Some(text) = st.get_selected_text() {
                text
            } else {
                st.content
                    .buffer
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
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    None
}

/// AI 面板 Apply 按钮 + 输入框点击。
unsafe fn lbd_right_panel_apply_input(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    right_panel_region: &crate::layout::Region,
) -> Option<LRESULT> {
    let rp_rel_x = mouse_x - right_panel_region.x;
    let rp_rel_y = mouse_y - right_panel_region.y;
    let margin = 10.0;
    // Apply 按钮
    let apply_y = right_panel_region.height - 76.0;
    let apply_btn_w = 80.0;
    let apply_btn_h = 24.0;
    let apply_btn_x = right_panel_region.width - margin - apply_btn_w;
    if rp_rel_x >= apply_btn_x
        && rp_rel_x < apply_btn_x + apply_btn_w
        && rp_rel_y >= apply_y
        && rp_rel_y < apply_y + apply_btn_h
    {
        let mut st = state.borrow_mut();
        if let Some(code) = st.ai_panel.extract_last_code_block() {
            st.apply_ai_code(&code);
            st.status_message = "AI 代码已应用到编辑器".to_string();
        }
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    // 输入框
    let input_y = right_panel_region.height - 40.0;
    if rp_rel_y >= input_y
        && rp_rel_y < input_y + 32.0
        && rp_rel_x >= margin
        && rp_rel_x < right_panel_region.width - margin
    {
        let mut st = state.borrow_mut();
        st.ai_panel.input_focused = true;
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    None
}

/// 标签栏点击。
pub(super) unsafe fn lbd_tab_bar(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    let show_tab_bar = st.show_tab_bar();
    let tab_region = layout.tab_bar_region(show_tab_bar);
    if !tab_region.contains(mouse_x, mouse_y) {
        return None;
    }
    // Task 8.2: 点击标签体时延迟切换，记录拖拽起始位置等待 mouse_move 判定。
    // 关闭按钮和 "+" 按钮仍立即响应。
    if let Some(tab_idx) = st.tab_body_hit_test(mouse_x, mouse_y, tab_region.x) {
        st.tab_drag_start = Some((mouse_x as i32, mouse_y as i32));
        st.hover_tab = Some(tab_idx);
        return Some(LRESULT(0));
    }
    // 非标签体（关闭按钮 / "+" 按钮）→ 立即处理
    if st.handle_tab_bar_click(mouse_x, mouse_y, tab_region.x) {
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    None
}

/// 查找替换面板点击。
pub(super) unsafe fn lbd_find_panel(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    if !st.find_visible {
        return None;
    }
    let show_tab_bar = st.show_tab_bar();
    let editor_region = layout.editor_content_region(show_tab_bar);
    let panel_height = if st.replace_visible { 72.0 } else { 40.0 };
    let panel_width = editor_region.width.min(600.0);
    let panel_x = editor_region.x + editor_region.width - panel_width - 10.0;
    let panel_y = editor_region.y;
    if !(mouse_x >= panel_x
        && mouse_x < panel_x + panel_width
        && mouse_y >= panel_y
        && mouse_y < panel_y + panel_height)
    {
        return None;
    }
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
    invalidate_window(hwnd);
    Some(LRESULT(0))
}

/// 底部面板点击。
pub(super) unsafe fn lbd_bottom_panel(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> Option<LRESULT> {
    let bottom_panel_region = layout.bottom_panel_region();
    if !bottom_panel_region.contains(mouse_x, mouse_y) {
        return None;
    }
    let mut st = state.borrow_mut();
    st.terminal_panel.focused = true;
    drop(st);
    invalidate_window(hwnd);
    Some(LRESULT(0))
}

/// 欢迎页 / 编辑器区域点击。
pub(super) unsafe fn lbd_welcome_or_editor(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> Option<LRESULT> {
    let welcome_x = 0.0;
    let welcome_width = {
        let st = state.borrow();
        st.window_width as f32
    };
    let welcome_y = layout.top_offset();
    let welcome_height = {
        let st = state.borrow();
        st.window_height as f32
            - welcome_y
            - if layout.status_bar_visible {
                layout.status_bar_height
            } else {
                0.0
            }
    };
    let welcome_region =
        crate::layout::Region::new(welcome_x, welcome_y, welcome_width, welcome_height);
    if !welcome_region.contains(mouse_x, mouse_y) {
        return None;
    }
    let mut st = state.borrow_mut();
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
            lbd_welcome_action(hwnd, state, action);
            return Some(LRESULT(0));
        }
    } else {
        let editor_content = layout.editor_content_region(st.show_tab_bar());
        st.set_cursor_from_mouse(mouse_x, mouse_y, editor_content.x, editor_content.y);
        st.clear_selection();
        st.start_selection();
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    None
}

/// 欢迎页点击动作执行
unsafe fn lbd_welcome_action(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    action: crate::welcome::WelcomeAction,
) {
    match action {
        crate::welcome::WelcomeAction::OpenFolder => {
            if let Some(path) = Dialogs::open_folder_dialog(hwnd, "打开文件夹") {
                state.borrow_mut().open_folder(path);
                invalidate_window(hwnd);
            }
        }
        crate::welcome::WelcomeAction::NewProject => {
            state.borrow_mut().new_project();
            invalidate_window(hwnd);
        }
        crate::welcome::WelcomeAction::CloneRepo => {
            state.borrow_mut().clone_dialog.visible = true;
            state.borrow_mut().clone_dialog.reset();
            invalidate_window(hwnd);
        }
        crate::welcome::WelcomeAction::OpenRemote => {
            state.borrow_mut().ssh_dialog.visible = true;
            state.borrow_mut().ssh_dialog.reset();
            invalidate_window(hwnd);
        }
        crate::welcome::WelcomeAction::OpenRecentProject(path_str) => {
            let path = PathBuf::from(path_str);
            state.borrow_mut().open_folder(path);
            invalidate_window(hwnd);
        }
        crate::welcome::WelcomeAction::MoreRecentProjects => {
            if let Some(path) = Dialogs::open_folder_dialog(hwnd, "打开文件夹") {
                state.borrow_mut().open_folder(path);
                invalidate_window(hwnd);
            }
        }
    }
}
