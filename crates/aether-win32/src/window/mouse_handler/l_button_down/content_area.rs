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
    // 对话标签条：切换 / 关闭 / 新建 / 历史（命中区为渲染时注册的绝对坐标）
    if let Some(result) = lbd_right_panel_tabs(hwnd, state, mouse_x, mouse_y) {
        return Some(result);
    }
    // 思考过程块：点击标题折叠/展开（命中区为渲染时注册的绝对坐标）
    {
        let hit = {
            let st = state.borrow();
            st.ai_panel
                .reasoning_toggle_regions
                .iter()
                .find(|(_, rx, ry, rw, rh)| {
                    mouse_x >= *rx && mouse_x < *rx + *rw && mouse_y >= *ry && mouse_y < *ry + *rh
                })
                .map(|(i, ..)| *i)
        };
        if let Some(i) = hit {
            let mut st = state.borrow_mut();
            if let Some(msg) = st.ai_panel.messages.get_mut(i) {
                msg.reasoning_collapsed = !msg.reasoning_collapsed;
            }
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 先检测输入框和按钮点击，如果命中则直接返回（不取消聚焦）
    if let Some(result) =
        lbd_right_panel_apply_input(hwnd, state, mouse_x, mouse_y, &right_panel_region)
    {
        return Some(result);
    }

    // 检测代码块保存按钮点击
    // 简化实现：检测是否在消息区域右侧的代码块保存按钮位置
    let rp_rel_x = mouse_x - right_panel_region.x;
    let rp_rel_y = mouse_y - right_panel_region.y;
    let margin = 10.0f32;
    let content_right = right_panel_region.width - margin;
    let save_btn_w = 60.0f32;
    let save_btn_x = content_right - save_btn_w - 4.0;

    // 遍历消息查找代码块位置
    {
        let st = state.borrow();
        let chat_top = 52.0f32; // 标题 + 分隔线后的起始位置
        let _chat_bottom = right_panel_region.height - 80.0f32; // 输入框上方
        let mut msg_y = chat_top - st.ai_panel.scroll_y;
        let seg_pad = 6.0f32;
        let msg_gap = 12.0f32;
        let seg_gap = 4.0f32;
        let label_h = 14.0f32;

        for msg in &st.ai_panel.messages {
            if msg.role == crate::ai_panel::AiRole::System {
                continue;
            }
            let is_user = msg.role == crate::ai_panel::AiRole::User;
            msg_y += label_h;

            // 按 ``` 代码围栏拆分
            let mut segments: Vec<(bool, String)> = Vec::new();
            {
                let mut in_code = false;
                let mut buf: Vec<&str> = Vec::new();
                for line in msg.content.lines() {
                    if line.trim_start().starts_with("```") {
                        if !buf.is_empty() {
                            segments.push((in_code, buf.join("\n")));
                            buf.clear();
                        }
                        in_code = !in_code;
                        continue;
                    }
                    buf.push(line);
                }
                if !buf.is_empty() {
                    segments.push((in_code, buf.join("\n")));
                }
            }

            for (is_code, seg_text) in &segments {
                // 估算段高度（简化）
                let line_count = seg_text.lines().count().max(1);
                let seg_h = if *is_code {
                    (line_count as f32 * 16.0 + seg_pad * 2.0).max(30.0)
                } else {
                    (line_count as f32 * 16.0 + seg_pad * 2.0).max(20.0)
                };

                // 检查是否在视口内且是代码块
                if *is_code && !is_user && !seg_text.is_empty() {
                    let save_btn_y = msg_y + 2.0;
                    let save_btn_h = 18.0f32;
                    if rp_rel_y >= save_btn_y
                        && rp_rel_y < save_btn_y + save_btn_h
                        && rp_rel_x >= save_btn_x
                        && rp_rel_x < save_btn_x + save_btn_w
                    {
                        // 点击了保存按钮 - 先收集需要的信息，然后释放借用
                        let code_to_save = seg_text.clone();
                        let suggested_name = msg
                            .content
                            .lines()
                            .find(|l| {
                                l.trim_start().starts_with("```")
                                    && !l.trim_start().starts_with("```\n")
                            })
                            .and_then(crate::ai_panel::AiPanel::extract_filename_from_fence);
                        drop(st);
                        let mut st_mut = state.borrow_mut();
                        match st_mut.save_ai_code_block(&code_to_save, suggested_name.as_deref()) {
                            Ok(path) => {
                                st_mut.status_message = format!("已保存: {}", path.display());
                            }
                            Err(e) => {
                                st_mut.status_message = format!("保存失败: {}", e);
                            }
                        }
                        drop(st_mut);
                        invalidate_window(hwnd);
                        return Some(LRESULT(0));
                    }
                }
                msg_y += seg_h + seg_gap;
            }
            msg_y += msg_gap;
        }
    }

    // 模式切换 / 上下文附件 / 变更列表按钮（基于渲染时注册的绝对坐标命中区）
    if lbd_right_panel_ai_controls(hwnd, state, mouse_x, mouse_y).is_some() {
        return Some(LRESULT(0));
    }
    // C-10: 点击 AI 面板非输入框/按钮区域时取消输入框聚焦
    {
        let mut st = state.borrow_mut();
        st.ai_panel.input_focused = false;
    }
    None
}

/// AI 面板：对话标签条点击（历史按钮 / 关闭标签 / 切换标签 / 新建对话 / 历史条目）。
/// 命中区为渲染时注册的绝对窗口坐标，直接用 mouse_x/mouse_y 测试。
unsafe fn lbd_right_panel_tabs(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
) -> Option<LRESULT> {
    let hit = |regions: &[(usize, f32, f32, f32, f32)]| -> Option<usize> {
        regions
            .iter()
            .find(|(_, rx, ry, rw, rh)| {
                mouse_x >= *rx && mouse_x < *rx + *rw && mouse_y >= *ry && mouse_y < *ry + *rh
            })
            .map(|(i, ..)| *i)
    };
    // 1. 历史记录按钮：切换历史列表展开
    {
        let mut st = state.borrow_mut();
        if let Some((hx, hy, hw, hh)) = st.ai_panel.history_button_region {
            if mouse_x >= hx && mouse_x < hx + hw && mouse_y >= hy && mouse_y < hy + hh {
                st.ai_panel.history_open = !st.ai_panel.history_open;
                if st.ai_panel.history_open {
                    st.refresh_ai_history();
                } else {
                    st.ai_panel.close_history_detail();
                }
                drop(st);
                invalidate_window(hwnd);
                return Some(LRESULT(0));
            }
        }
    }
    // 1.1 历史详情视图：返回列表
    {
        let back_hit = {
            let st = state.borrow();
            st.ai_panel
                .history_detail_back_region
                .filter(|_| st.ai_panel.history_open)
                .map(|(rx, ry, rw, rh)| {
                    mouse_x >= rx && mouse_x < rx + rw && mouse_y >= ry && mouse_y < ry + rh
                })
                .unwrap_or(false)
        };
        if back_hit {
            state.borrow_mut().ai_panel.close_history_detail();
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 1.2 历史详情视图：恢复此对话为活动标签页
    {
        let restore_hit = {
            let st = state.borrow();
            st.ai_panel
                .history_detail_restore_region
                .filter(|_| st.ai_panel.history_open)
                .map(|(rx, ry, rw, rh)| {
                    mouse_x >= rx && mouse_x < rx + rw && mouse_y >= ry && mouse_y < ry + rh
                })
                .unwrap_or(false)
        };
        if restore_hit {
            state.borrow_mut().ai_panel.restore_history_detail();
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 1.3 历史面板「清空全部」（需二次确认）
    {
        let clear_hit = {
            let st = state.borrow();
            st.ai_panel
                .history_clear_all_region
                .filter(|_| st.ai_panel.history_open)
                .map(|(rx, ry, rw, rh)| {
                    mouse_x >= rx && mouse_x < rx + rw && mouse_y >= ry && mouse_y < ry + rh
                })
                .unwrap_or(false)
        };
        if clear_hit {
            let mut st = state.borrow_mut();
            if crate::dialogs::Dialogs::confirm_yes_no(
                hwnd,
                "清空历史记录",
                "确定清空全部历史对话吗？此操作不可恢复。",
            ) {
                match st.ai_panel.clear_all_history() {
                    Ok(n) => st.status_message = format!("已清空 {} 条历史记录", n),
                    Err(e) => st.status_message = format!("清空历史失败: {}", e),
                }
            }
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 1.4 历史时间筛选按钮
    {
        let filter_hit = {
            let st = state.borrow();
            if st.ai_panel.history_open {
                hit(&st.ai_panel.history_time_filter_regions)
            } else {
                None
            }
        };
        if let Some(fi) = filter_hit {
            let mut st = state.borrow_mut();
            if let Some(f) = crate::ai_panel::HistoryTimeFilter::ALL.get(fi).copied() {
                st.ai_panel.set_history_time_filter(f);
            }
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 1.5 历史类型筛选按钮
    {
        let filter_hit = {
            let st = state.borrow();
            if st.ai_panel.history_open {
                hit(&st.ai_panel.history_type_filter_regions)
            } else {
                None
            }
        };
        if let Some(fi) = filter_hit {
            let mut st = state.borrow_mut();
            if let Some(tf) = crate::ai_panel::HISTORY_TYPE_FILTERS.get(fi) {
                st.ai_panel
                    .set_history_type_filter(tf.map(|s| s.to_string()));
            }
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 1.6 历史分页：上一页 / 下一页
    {
        let page_dir = {
            let st = state.borrow();
            let in_region = |region: Option<(f32, f32, f32, f32)>| {
                region
                    .filter(|_| st.ai_panel.history_open)
                    .map(|(rx, ry, rw, rh)| {
                        mouse_x >= rx && mouse_x < rx + rw && mouse_y >= ry && mouse_y < ry + rh
                    })
                    .unwrap_or(false)
            };
            if in_region(st.ai_panel.history_page_prev_region) {
                Some(-1i32)
            } else if in_region(st.ai_panel.history_page_next_region) {
                Some(1i32)
            } else {
                None
            }
        };
        if let Some(dir) = page_dir {
            let mut st = state.borrow_mut();
            if dir < 0 {
                st.ai_panel.history_prev_page();
            } else {
                st.ai_panel.history_next_page();
            }
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 1.7 历史条目删除按钮（需二次确认；优先于条目点击）
    {
        let del_hit = {
            let st = state.borrow();
            if st.ai_panel.history_open {
                hit(&st.ai_panel.history_delete_regions)
            } else {
                None
            }
        };
        if let Some(i) = del_hit {
            let mut st = state.borrow_mut();
            let title = st
                .ai_panel
                .history
                .get(i)
                .map(|m| m.title.clone())
                .unwrap_or_default();
            let msg = format!("确定删除这条历史对话吗？\n\n{}", title);
            if crate::dialogs::Dialogs::confirm_yes_no(hwnd, "删除历史记录", &msg) {
                if let Err(e) = st.ai_panel.delete_history_item(i) {
                    st.status_message = format!("删除历史失败: {}", e);
                }
            }
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 2. 历史条目：打开详情视图（详情中可恢复会话）
    {
        let hist_hit = {
            let st = state.borrow();
            if st.ai_panel.history_open {
                hit(&st.ai_panel.history_item_regions)
            } else {
                None
            }
        };
        if let Some(i) = hist_hit {
            state.borrow_mut().open_ai_history_item(i);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 2.1 历史面板「仅当前工作区」开关
    {
        let toggle_hit = {
            let st = state.borrow();
            st.ai_panel
                .history_ws_toggle_region
                .filter(|_| st.ai_panel.history_open)
                .map(|(rx, ry, rw, rh)| {
                    mouse_x >= rx && mouse_x < rx + rw && mouse_y >= ry && mouse_y < ry + rh
                })
                .unwrap_or(false)
        };
        if toggle_hit {
            let mut st = state.borrow_mut();
            st.ai_panel.toggle_history_workspace_only();
            st.refresh_ai_history();
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 2.2 Playbook 策略库按钮：切换管理面板
    {
        let mut st = state.borrow_mut();
        if let Some((px, py, pw, ph)) = st.ai_panel.playbook_button_region {
            if mouse_x >= px && mouse_x < px + pw && mouse_y >= py && mouse_y < py + ph {
                st.ai_panel.toggle_playbook_panel();
                drop(st);
                invalidate_window(hwnd);
                return Some(LRESULT(0));
            }
        }
    }
    // 2.3 Playbook 条目删除按钮（需二次确认）
    {
        let del_hit = {
            let st = state.borrow();
            if st.ai_panel.playbook_open {
                hit(&st.ai_panel.playbook_delete_regions)
            } else {
                None
            }
        };
        if let Some(i) = del_hit {
            let mut st = state.borrow_mut();
            let content = st
                .ai_panel
                .playbook_items
                .get(i)
                .map(|b| b.content.clone())
                .unwrap_or_default();
            let msg = format!("确定删除这条策略吗？\n\n{}", content);
            if crate::dialogs::Dialogs::confirm_yes_no(hwnd, "删除策略条目", &msg) {
                if let Err(e) = st.ai_panel.delete_playbook_item(i) {
                    st.status_message = format!("删除策略失败: {}", e);
                }
            }
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 3. 关闭标签（优先于切换，避免点 × 时误切换）
    {
        let close_hit = {
            let st = state.borrow();
            hit(&st.ai_panel.tab_close_regions)
        };
        if let Some(i) = close_hit {
            let mut st = state.borrow_mut();
            st.ai_panel.snapshot_active_into_slot();
            st.ai_panel.close_conversation(i);
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 4. 切换标签
    {
        let tab_hit = {
            let st = state.borrow();
            hit(&st.ai_panel.tab_regions)
        };
        if let Some(i) = tab_hit {
            state.borrow_mut().ai_panel.switch_to(i);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }
    // 5. 新建对话
    {
        let mut st = state.borrow_mut();
        if let Some((px, py, pw, ph)) = st.ai_panel.new_tab_region {
            if mouse_x >= px && mouse_x < px + pw && mouse_y >= py && mouse_y < py + ph {
                st.ai_panel.new_conversation();
                drop(st);
                invalidate_window(hwnd);
                return Some(LRESULT(0));
            }
        }
    }
    None
}

/// AI 面板：模式切换 / 上下文附件切换。
///
/// 使用渲染时注册的绝对坐标命中区（mode_button_regions / attachment_chip_regions），
/// 与旧的硬编码坐标处理器互不冲突。
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
    let input_margin = 8.0;

    // ===== 当前模型下拉（底部工具栏，在对话框内切换当前使用的模型）=====
    // 几何与 render_ai_assistant_sidebar 中的模型按钮/弹层保持一致（相对右侧面板坐标）。
    {
        let toolbar_y = right_panel_region.height - 30.0;
        let toolbar_h = 26.0f32;
        let agent_btn_w = 80.0f32;
        let model_btn_x = margin + input_margin + agent_btn_w + 6.0;
        let model_btn_w = 140.0f32;

        let menu_open = state.borrow().ai_panel.model_menu_open;
        if menu_open {
            // 收集已启用模型（可作为"当前使用"的候选）
            let models: Vec<(String, String)> = {
                let st = state.borrow();
                st.app_settings
                    .ai_models
                    .iter()
                    .filter(|m| m.enabled)
                    .map(|m| {
                        let label = if !m.display_name.is_empty() {
                            m.display_name.clone()
                        } else if !m.model.is_empty() {
                            m.model.clone()
                        } else {
                            "(未命名模型)".to_string()
                        };
                        (m.id.clone(), label)
                    })
                    .collect()
            };
            if !models.is_empty() {
                let item_h = 30.0f32;
                let menu_w = model_btn_w.max(200.0);
                let menu_x = model_btn_x;
                let menu_bottom = toolbar_y - 4.0;
                let menu_h = models.len() as f32 * item_h + 8.0;
                let menu_top = menu_bottom - menu_h;
                // 命中某个菜单项 → 切换为当前使用的模型并持久化
                if rp_rel_x >= menu_x
                    && rp_rel_x < menu_x + menu_w
                    && rp_rel_y >= menu_top
                    && rp_rel_y < menu_bottom
                {
                    let idx = ((rp_rel_y - (menu_top + 4.0)) / item_h).floor() as i32;
                    if idx >= 0 && (idx as usize) < models.len() {
                        let (id, label) = models[idx as usize].clone();
                        let mut st = state.borrow_mut();
                        st.app_settings.active_model_id = Some(id.clone());
                        // 同步设置面板高亮（设置页已加载时生效）
                        st.settings_panel.active_model_id = Some(id);
                        match st.app_settings.save() {
                            Ok(_) => st.status_message = format!("已切换当前模型：{}", label),
                            Err(e) => st.status_message = format!("切换模型失败：{}", e),
                        }
                        st.ai_panel.model_menu_open = false;
                        drop(st);
                        invalidate_window(hwnd);
                        return Some(LRESULT(0));
                    }
                }
            }
            // 菜单展开时点击其它区域 → 收起
            state.borrow_mut().ai_panel.model_menu_open = false;
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
        // 命中模型按钮 → 展开下拉
        if rp_rel_x >= model_btn_x
            && rp_rel_x < model_btn_x + model_btn_w
            && rp_rel_y >= toolbar_y
            && rp_rel_y < toolbar_y + toolbar_h
        {
            state.borrow_mut().ai_panel.model_menu_open = true;
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }

    // ===== 输入框区域（新设计：参考图样式，总高度 80.0）=====
    let input_area_h = 80.0f32;
    let input_y = right_panel_region.height - input_area_h;
    let text_input_y = input_y + 6.0; // 中间文本输入区域
    let text_input_h = 36.0f32;

    // 检测是否在输入框区域内（先检测输入框，避免被其他按钮逻辑覆盖）
    if rp_rel_y >= text_input_y
        && rp_rel_y < text_input_y + text_input_h
        && rp_rel_x >= margin + input_margin
        && rp_rel_x < right_panel_region.width - margin - input_margin
    {
        let mut st = state.borrow_mut();
        st.ai_panel.input_focused = true;
        st.ai_panel.caret_visible = true;
        // 点击输入框时将光标移到末尾
        st.ai_panel.caret_pos = st.ai_panel.input.len();
        let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(
            hwnd,
            crate::window::CARET_TIMER_ID,
            530,
            None,
        );
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }

    // 底部工具栏按钮检测（发送按钮等）
    let toolbar_sep_y = input_y + input_area_h - 34.0;
    let toolbar_y = toolbar_sep_y + 4.0;
    let _toolbar_h = 26.0f32;

    // 发送按钮（蓝色背景，最右侧）
    let send_btn_size = 24.0f32;
    let right_btn_area_x = right_panel_region.width - margin - input_margin;
    let send_btn_x = right_btn_area_x - send_btn_size;
    let send_btn_y = toolbar_y + 1.0;
    if rp_rel_x >= send_btn_x
        && rp_rel_x < send_btn_x + send_btn_size
        && rp_rel_y >= send_btn_y
        && rp_rel_y < send_btn_y + send_btn_size
    {
        // 发送消息（使用当前模式 + 编辑器上下文，与 Enter 键行为一致，
        // 以便 Agent 模式收到工具指令并输出 FILE/RUN 标记）
        let mut st = state.borrow_mut();
        let settings = st.app_settings.active_ai_settings();
        let mode = st.ai_panel.mode;
        let attachments = st.ai_panel.attachments.clone();
        let context = st.gather_context(&attachments);
        if let Err(e) = st
            .ai_panel
            .send_message_with_prepared_context(&settings, context, mode)
        {
            st.status_message = e;
        } else {
            st.status_message = "AI 请求已发送".to_string();
            let _ = SetTimer(hwnd, AI_TIMER_ID, AI_REFRESH_MS, None);
        }
        st.ai_panel.input_focused = false;
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }

    // 停止生成按钮（当正在生成时显示）
    let is_gen = state.borrow().ai_panel.is_generating;
    if is_gen {
        let stop_w = 96.0f32;
        let stop_x = margin + input_margin;
        let stop_y = input_y + 4.0; // 在输入框卡片上方区域
        let stop_h = 26.0f32;
        if rp_rel_x >= stop_x
            && rp_rel_x < stop_x + stop_w
            && rp_rel_y >= stop_y
            && rp_rel_y < stop_y + stop_h
        {
            state.borrow_mut().ai_panel.stop_generation();
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
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
    if let Some(kind) = st
        .settings_panel
        .hit_test_dropdown_trigger(mouse_x, mouse_y)
    {
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
        // 切换导航时退出模型编辑态，返回「模型」页默认展示列表
        st.settings_panel.model_editing = false;
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }

    // 5. 模型编辑表单（AI 配置）：显隐按钮 / 温度滑块 / 保存 / 测试连接 / 返回列表
    // AI 配置已并入「模型」页，仅在 model_editing 编辑态下展示与响应。
    if st.settings_panel.active_tab == crate::settings::SettingsTab::Ai
        || (st.settings_panel.active_tab == crate::settings::SettingsTab::Models
            && st.settings_panel.model_editing)
    {
        // API 密钥显隐切换
        if st.settings_panel.hit_test_api_key_toggle(mouse_x, mouse_y) {
            st.settings_panel.toggle_api_key_visibility();
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
        // 温度滑块：点击轨道即定位，并进入拖拽
        if let Some(v) = st.settings_panel.hit_test_temp_slider(mouse_x, mouse_y) {
            st.settings_panel.temperature = format!("{:.1}", v);
            st.settings_panel.temp_slider_dragging = true;
            st.settings_panel.active_field = None;
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
        if let Some(btn) = st.settings_panel.hit_test_button(mouse_x, mouse_y) {
            match btn {
                crate::settings::SettingsButton::BackToModels => {
                    // 返回模型列表：不保存（草稿/未保存编辑将被丢弃，只有「保存」才写入）
                    st.settings_panel.model_editing = false;
                }
                crate::settings::SettingsButton::Save => {
                    st.save_ai_settings_with_test();
                }
                crate::settings::SettingsButton::TestConnection => {
                    st.start_ai_test_connection();
                }
            }
            let started_test = st.settings_panel.is_testing;
            st.settings_panel.active_field = None;
            drop(st);
            // 测试期间启动后台刷新定时器，结果到达后自动停止
            if started_test {
                let _ = SetTimer(hwnd, AI_TIMER_ID, AI_REFRESH_MS, None);
            }
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
    }

    // 5b. 模型管理页（列表视图）：模型按钮点击
    // 命中区由 render_models_management 以绝对坐标注册（原点为 editor_content_region），
    // 因此这里必须用绝对 mouse_x/mouse_y 命中测试，与标签栏 / 编辑表单保持一致。
    if st.settings_panel.active_tab == crate::settings::SettingsTab::Models
        && !st.settings_panel.model_editing
    {
        if let Some((btn, model_id)) = st.settings_panel.hit_test_model_button(mouse_x, mouse_y) {
            match btn {
                crate::settings::ModelButton::Add => {
                    // 新建：进入空白草稿编辑表单，但不加入列表、不持久化；
                    // 只有点击「保存」（连接验证通过）后才会真正创建并保存该模型。
                    st.settings_panel.begin_new_model_draft();
                    st.settings_panel.model_editing = true;
                }
                crate::settings::ModelButton::Edit => {
                    // 编辑：加载该模型字段进表单，不持久化；改动只有点击「保存」后才写入。
                    let fallback = st.app_settings.ai.clone();
                    st.settings_panel.active_model_id = Some(model_id.clone());
                    st.settings_panel.load_active_model_fields(&fallback);
                    st.settings_panel.model_editing = true;
                }
                crate::settings::ModelButton::Delete => {
                    st.settings_panel.delete_model(&model_id);
                    if st.settings_panel.active_model_id.is_none() {
                        st.settings_panel.active_model_id =
                            st.settings_panel.models.first().map(|m| m.id.clone());
                    }
                    let fallback = st.app_settings.ai.clone();
                    st.settings_panel.load_active_model_fields(&fallback);
                    st.persist_models();
                }
                crate::settings::ModelButton::ToggleEnabled => {
                    st.settings_panel.toggle_model_enabled(&model_id);
                    st.persist_models();
                }
            }
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
        // 点击模型卡片 → 设为激活模型
        if let Some(model_id) = st.settings_panel.hit_test_model_item(mouse_x, mouse_y) {
            st.settings_panel.selected_model_id = Some(model_id.clone());
            let fallback = st.app_settings.ai.clone();
            st.settings_panel.set_active_model(&model_id, &fallback);
            st.persist_models();
            drop(st);
            invalidate_window(hwnd);
            return Some(LRESULT(0));
        }
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
