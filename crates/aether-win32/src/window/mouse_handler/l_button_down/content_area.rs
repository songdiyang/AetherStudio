//! `WM_LBUTTONDOWN` 内容区域处理：活动栏 / 侧边栏 / 面板 / 编辑器。
//!
//! 从 `l_button_down.rs` 拆分而来，保持原有逻辑不变。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LRESULT};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::dialogs::Dialogs;
use crate::editor::{BottomPanelTab, EditorState};

use super::super::super::{
    invalidate_window, AI_REFRESH_MS, AI_TIMER_ID, LP_THRESHOLD_MS, LP_TIMER_ID,
};

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
    let idx = st
        .activity_bar
        .hit_test(mouse_x, mouse_y, activity_region.y)?;
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
    if sidebar_resize_zone {
        st.layout.sidebar_resizing = true;
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
    let (idx, action) = clicked_btn?;
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

/// 右侧 AI 面板点击（模式切换 / 上下文附件 / 变更列表 / Apply / 输入框）。
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
    // 模式切换 / 上下文附件 / 变更列表按钮（基于渲染时注册的绝对坐标命中区）
    if lbd_right_panel_ai_controls(hwnd, state, mouse_x, mouse_y).is_some() {
        return Some(LRESULT(0));
    }
    lbd_right_panel_apply_input(hwnd, state, mouse_x, mouse_y, &right_panel_region)
}

/// AI 面板：模式切换 / 上下文附件切换 / 变更列表接受·拒绝·预览。
///
/// 使用渲染时注册的绝对坐标命中区（mode_button_regions / attachment_chip_regions /
/// change_action_regions），与旧的硬编码坐标处理器互不冲突。
unsafe fn lbd_right_panel_ai_controls(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
) -> Option<LRESULT> {
    // 1. 模式切换按钮
    {
        let mut st = state.borrow_mut();
        if let Some(mode) = st.ai_panel.hit_test_mode_button(mouse_x, mouse_y) {
            st.ai_panel.mode = mode;
            st.status_message = format!("AI 模式：{}", mode.label());
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 2. 上下文附件切换按钮（索引 → toggleable_attachments）
    {
        let mut st = state.borrow_mut();
        if let Some(i) = st.ai_panel.hit_test_attachment(mouse_x, mouse_y) {
            let items = crate::ai_panel::AiPanel::toggleable_attachments();
            if let Some(att) = items.get(i) {
                let att = att.clone();
                st.ai_panel.toggle_attachment(att);
            }
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 3. 变更列表按钮（idx == usize::MAX 为批量操作；否则 0=预览 1=接受 2=拒绝）
    let hit = {
        let st = state.borrow();
        st.ai_panel.hit_test_change_action(mouse_x, mouse_y)
    };
    if let Some((idx, action)) = hit {
        if idx == usize::MAX {
            match action {
                1 => state.borrow_mut().ai_apply_pending_changes(),
                2 => state.borrow_mut().ai_panel.reject_all_changes(),
                _ => {}
            }
        } else {
            match action {
                0 => {
                    let mut st = state.borrow_mut();
                    if idx < st.ai_panel.diff_view.files.len() {
                        st.ai_panel.diff_view.selected_index = idx;
                    }
                }
                1 => state.borrow_mut().ai_accept_change_file(idx),
                2 => state.borrow_mut().ai_reject_change_file(idx),
                _ => {}
            }
        }
        invalidate_window(hwnd);
        return Some(LRESULT(0));
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

/// 设置页点击（导航标签 / 字段聚焦 / 下拉选择 / 保存 / 测试连接）。
///
/// 设置页渲染在编辑区，各命中区由 render_settings_sidebar 以绝对坐标注册，此处直接命中测试。
pub(super) unsafe fn lbd_settings_page(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> Option<LRESULT> {
    {
        let st = state.borrow();
        if !st.active_tab_is_settings() {
            return None;
        }
    }
    let editor_region = layout.editor_region();
    if !editor_region.contains(mouse_x, mouse_y) {
        return None;
    }

    let mut st = state.borrow_mut();

    // 1. 下拉展开时，优先处理选项点击
    if st.settings_panel.open_dropdown.is_some() {
        if let Some((kind, idx)) = st.settings_panel.hit_test_dropdown_item(mouse_x, mouse_y) {
            match kind {
                crate::settings::SettingsDropdownKind::Provider => {
                    st.settings_panel.select_provider_by_index(idx);
                }
                crate::settings::SettingsDropdownKind::Model => {
                    st.settings_panel.select_model_by_index(idx);
                }
            }
            st.settings_panel.open_dropdown = None;
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }

    // 2. 下拉触发区 → 开/关（以当前状态切换）
    if let Some(kind) = st.settings_panel.hit_test_dropdown_trigger(mouse_x, mouse_y) {
        st.settings_panel.open_dropdown = if st.settings_panel.open_dropdown == Some(kind) {
            None
        } else {
            Some(kind)
        };
        st.settings_panel.active_field = None;
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }

    // 3. 若下拉处于展开且点击了别处 → 先关闭，再继续尝试其它命中
    if st.settings_panel.open_dropdown.is_some() {
        st.settings_panel.open_dropdown = None;
    }

    // 4. 导航标签切换
    if let Some(tab) = st.settings_panel.hit_test_tab(mouse_x, mouse_y) {
        st.settings_panel.active_tab = tab;
        st.settings_panel.active_field = None;
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }

    // 5. 按钮：保存 / 测试连接
    if let Some(btn) = st.settings_panel.hit_test_button(mouse_x, mouse_y) {
        let mut started_test = false;
        match btn {
            crate::settings::SettingsButton::Save => st.save_ai_settings(),
            crate::settings::SettingsButton::TestConnection => {
                st.start_ai_test_connection();
                started_test = st.settings_panel.is_testing;
            }
        }
        st.settings_panel.active_field = None;
        drop(st);
        // 测试期间启动后台刷新定时器，结果到达后自动停止
        if started_test {
            let _ = SetTimer(hwnd, AI_TIMER_ID, AI_REFRESH_MS, None);
        }
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }

    // 6. 输入字段聚焦
    if let Some(field) = st.settings_panel.hit_test_field(mouse_x, mouse_y) {
        st.settings_panel.active_field = Some(field);
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }

    // 7. 点击设置区空白 → 清除聚焦与下拉，消费点击
    st.settings_panel.active_field = None;
    st.settings_panel.open_dropdown = None;
    drop(st);
    invalidate_window(hwnd);
    Some(LRESULT(0))
}

/// 标签栏点击。
///
/// 重要：处理关闭按钮时，**不能**在 `borrow_mut()` 持有期间弹出模态确认对话框。
/// `MessageBoxW` / `TaskDialog` 等模态对话框会启动自己的消息循环，期间会派发
/// `WM_PAINT` / `WM_KILLFOCUS` 等消息，这些消息处理函数会再次尝试 `borrow_mut()`，
/// 触发 `RefCell already borrowed` panic，导致应用卡死。
///
/// 解决方案：先在 `borrow()` 下完成所有点击检测，drop borrow 后再执行；
/// 关闭路径额外提取 dirty 状态信息，drop borrow 后弹窗，确认后重新
/// `borrow_mut()` 执行关闭。
pub(super) unsafe fn lbd_tab_bar(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> Option<LRESULT> {
    // 阶段 1：在 borrow_mut() 内处理拖拽预备（标签体命中 → 延迟切换）
    {
        let mut st = state.borrow_mut();
        let show_tab_bar = st.show_tab_bar();
        let tab_region = layout.tab_bar_region(show_tab_bar);
        if !tab_region.contains(mouse_x, mouse_y) {
            return None;
        }
        if let Some(tab_idx) = st.tab_body_hit_test(mouse_x, mouse_y, tab_region.x, tab_region.y) {
            st.tab_drag_start = Some((mouse_x as i32, mouse_y as i32));
            st.hover_tab = Some(tab_idx);
            return Some(LRESULT(0));
        }
    }

    // 阶段 2：在 borrow() 下检测点击类型（不修改 state）
    enum TabBarAction {
        CloseTab {
            index: usize,
            is_dirty: bool,
            file_name: String,
        },
        SwitchTab(usize),
    }
    let action: Option<TabBarAction> = {
        let st = state.borrow();
        let show_tab_bar = st.show_tab_bar();
        if !show_tab_bar {
            return None;
        }
        let tab_region = layout.tab_bar_region(show_tab_bar);
        let editor_x = tab_region.x;

        // "+" 新建按钮
        if let Some((pl, pt, pr, pb)) = st.plus_button_rect {
            if mouse_x >= pl && mouse_x < pr && mouse_y >= pt && mouse_y < pb {
                return handle_new_tab(state, hwnd);
            }
        }

        if mouse_x < editor_x {
            return None;
        }

        // 遍历 tab_layouts 检测关闭按钮或 tab 体
        let rel_x = mouse_x - editor_x + st.tab_scroll_x;
        let mut found: Option<TabBarAction> = None;
        for layout_entry in &st.tab_layouts {
            if rel_x >= layout_entry.x && rel_x < layout_entry.x + layout_entry.width {
                // 关闭按钮
                if rel_x >= layout_entry.close_x
                    && rel_x < layout_entry.close_x + layout_entry.close_width
                {
                    let index = layout_entry.index;
                    let is_active = index == st.active_tab;
                    let (is_dirty, file_name) = if is_active {
                        let dirty = st.content.is_dirty;
                        let name = st
                            .content
                            .file_path
                            .as_ref()
                            .and_then(|p| p.file_name())
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "未命名".to_string());
                        (dirty, name)
                    } else {
                        let dirty = st.tabs.get(index).map(|t| t.is_dirty()).unwrap_or(false);
                        let name = st
                            .tabs
                            .get(index)
                            .and_then(|t| t.file_path())
                            .and_then(|p| p.file_name())
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "未命名".to_string());
                        (dirty, name)
                    };
                    found = Some(TabBarAction::CloseTab {
                        index,
                        is_dirty,
                        file_name,
                    });
                    break;
                }
                // tab 体（不在关闭按钮区域）→ 立即切换
                found = Some(TabBarAction::SwitchTab(layout_entry.index));
                break;
            }
        }
        found
    };

    // 阶段 3：drop borrow 后执行
    match action {
        Some(TabBarAction::CloseTab {
            index,
            is_dirty,
            file_name,
        }) => {
            if is_dirty {
                let msg = format!("{} 有未保存的修改，是否保存并关闭？", file_name);
                // 弹窗在 borrow 释放后进行 → 不触发 RefCell panic
                let confirmed = crate::dialogs::Dialogs::confirm_yes_no(hwnd, "关闭标签页", &msg);
                if !confirmed {
                    state.borrow_mut().status_message = "已取消关闭".to_string();
                    invalidate_window(hwnd);
                    return Some(LRESULT(0));
                }
            }
            // 弹窗结束后重新 borrow_mut() 执行关闭
            state.borrow_mut().close_tab(index);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
        Some(TabBarAction::SwitchTab(idx)) => {
            state.borrow_mut().switch_tab(idx);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
        None => {}
    }
    None
}

/// 新建标签页（保持 borrow_mut() 持有时间最短）。
unsafe fn handle_new_tab(state: &Rc<RefCell<EditorState>>, hwnd: HWND) -> Option<LRESULT> {
    {
        let mut st = state.borrow_mut();
        st.new_tab();
        st.status_message = "已新建标签页".to_string();
    }
    invalidate_window(hwnd);
    Some(LRESULT(0))
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
    tracing::info!(
        mx = mouse_x,
        my = mouse_y,
        visible = layout.bottom_panel_visible,
        running = state.borrow().terminal_panel.running,
        tab = ?state.borrow().bottom_panel_tab,
        "lbd_bottom_panel: 点击底部面板"
    );

    // 标签栏 hit test：与 render_bottom_panel 中标签的坐标布局完全一致
    // (tab_height=28, tab_w=60, 间距=4, 起始 x=region.x+10)
    // 命中则切换 bottom_panel_tab，不进入终端 focus 逻辑。
    let tab_height: f32 = 28.0;
    let tab_w: f32 = 60.0;
    let tab_gap: f32 = 4.0;
    let tab_start_x = bottom_panel_region.x + 10.0;
    let tab_top = bottom_panel_region.y + 2.0;
    let tab_bottom = tab_top + tab_height - 2.0;
    if mouse_y >= tab_top && mouse_y <= tab_bottom {
        let rel_x = mouse_x - tab_start_x;
        if rel_x >= 0.0 {
            let step = tab_w + tab_gap;
            let idx_f = rel_x / step;
            let idx = idx_f as i32;
            // 必须在标签的 x 范围内（防止落在 tab 间隙时误判）
            let in_tab_x = (rel_x - idx_f * step) < tab_w;
            if idx >= 0 && in_tab_x {
                let tab = match idx {
                    0 => BottomPanelTab::Terminal,
                    1 => BottomPanelTab::Problems,
                    _ => return None,
                };
                let mut st = state.borrow_mut();
                if st.bottom_panel_tab != tab {
                    tracing::info!(?tab, "lbd_bottom_panel: 切换底部面板 tab");
                    st.bottom_panel_tab = tab;
                    // 切到问题面板时取消终端 focus，避免低层钩子继续拦截 Backspace
                    if tab == BottomPanelTab::Problems && st.terminal_panel.focused {
                        st.terminal_panel.focused = false;
                        st.set_terminal_ime_bypass(false);
                    }
                    drop(st);
                    invalidate_window(hwnd);
                }
                return Some(LRESULT(0));
            }
        }
    }

    // 检测点击关闭按钮（×）—— 与 render_bottom_panel 中绘制的位置保持一致
    const CLOSE_BTN_SIZE: f32 = 28.0;
    const TITLE_BAR_H: f32 = 30.0;
    let close_btn_x = bottom_panel_region.x + bottom_panel_region.width - CLOSE_BTN_SIZE;
    let close_btn_y = bottom_panel_region.y;
    if mouse_x >= close_btn_x
        && mouse_x <= close_btn_x + CLOSE_BTN_SIZE
        && mouse_y >= close_btn_y
        && mouse_y <= close_btn_y + TITLE_BAR_H
    {
        tracing::info!("lbd_bottom_panel: 点击关闭按钮，关闭底部面板");
        let mut st = state.borrow_mut();
        st.layout.toggle_terminal_panel();
        st.terminal_panel.focused = false;
        st.set_terminal_ime_bypass(false);
        let _ = KillTimer(hwnd, super::super::super::TERM_TIMER_ID);
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }

    // 点击在标签栏之外但在底部面板内：
    // - 终端 tab：focus 终端，自动启动
    // - 问题 tab：暂不响应（问题项点击后续在问题面板内部实现）
    if state.borrow().bottom_panel_tab != BottomPanelTab::Terminal {
        return Some(LRESULT(0));
    }

    let mut st = state.borrow_mut();
    st.terminal_panel.focused = true;
    st.set_terminal_ime_bypass(true);
    // 如果终端未运行，点击时自动启动
    if !st.terminal_panel.running {
        tracing::info!("lbd_bottom_panel: 终端未运行，自动启动");
        let _ = st.terminal_panel.start();
    }
    // 确保刷新定时器在运行（覆盖从按钮打开/关闭后定时器可能未启动的情况）
    let _ = SetTimer(
        hwnd,
        super::super::super::TERM_TIMER_ID,
        super::super::super::TERM_REFRESH_MS,
        None,
    );
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
            - if layout.bottom_panel_visible {
                layout.bottom_panel_height
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
