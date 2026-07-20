//! `WM_LBUTTONDOWN` 顶部区域处理：对话框 + 标题栏 + 菜单。
//!
//! 从 `l_button_down.rs` 拆分而来，保持原有逻辑不变。

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LRESULT};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::editor::EditorState;

use super::super::super::{
    invalidate_window, LP_THRESHOLD_MS, LP_TIMER_ID, TERM_REFRESH_MS, TERM_TIMER_ID,
};

/// 对话框优先拦截点击（SSH / 克隆 / 新建项目）。
pub(super) unsafe fn lbd_dialogs(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
) -> Option<LRESULT> {
    if let Some(r) = lbd_ssh_dialog(hwnd, state, mouse_x, mouse_y) {
        return Some(r);
    }
    if let Some(r) = lbd_clone_dialog(hwnd, state, mouse_x, mouse_y) {
        return Some(r);
    }
    lbd_new_project_dialog(hwnd, state, mouse_x, mouse_y)
}

/// SSH 连接对话框点击处理。
unsafe fn lbd_ssh_dialog(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    if !st.ssh_dialog.visible {
        return None;
    }
    if let Some(action) = st.handle_ssh_dialog_click(mouse_x, mouse_y) {
        match action {
            crate::ssh::DialogAction::Connect => {
                if st.ssh_connecting {
                    // 正在连接中，忽略重复点击
                } else if let Some(config) = st.ssh_dialog.to_config() {
                    st.start_ssh_connect(config);
                } else {
                    st.ssh_dialog.error_message = Some("请填写主机和用户名".to_string());
                }
            }
            crate::ssh::DialogAction::Cancel => {
                st.ssh_dialog.visible = false;
            }
            crate::ssh::DialogAction::None => {}
        }
    }
    drop(st);
    invalidate_window(hwnd);
    Some(LRESULT(0))
}

/// 克隆对话框点击处理。
unsafe fn lbd_clone_dialog(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    if !st.clone_dialog.visible {
        return None;
    }
    if let Some(action) = st.handle_clone_dialog_click(mouse_x, mouse_y) {
        match action {
            crate::ssh::DialogAction::Connect => {
                if st.clone_dialog.url.is_empty() {
                    st.clone_dialog.error_message = Some("请输入仓库 URL".to_string());
                } else if st.git_cloning {
                    // C-09: 正在克隆中，忽略重复点击
                } else {
                    drop(st);
                    if let Some(target_path) =
                        crate::dialogs::Dialogs::open_folder_dialog(hwnd, "选择克隆目标文件夹")
                    {
                        let mut st = state.borrow_mut();
                        let url = st.clone_dialog.url.clone();
                        st.start_git_clone(url, target_path);
                        drop(st);
                        invalidate_window(hwnd);
                        return Some(LRESULT(0));
                    }
                    invalidate_window(hwnd);
                    return Some(LRESULT(0));
                }
            }
            crate::ssh::DialogAction::Cancel => {
                st.clone_dialog.visible = false;
            }
            crate::ssh::DialogAction::None => {}
        }
    }
    drop(st);
    invalidate_window(hwnd);
    Some(LRESULT(0))
}

/// 新建项目对话框点击处理。
unsafe fn lbd_new_project_dialog(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    if !st.new_project_dialog.visible {
        return None;
    }
    let action = st.handle_new_project_dialog_click(mouse_x, mouse_y);
    match action {
        crate::new_project_dialog::NewProjectDialogAction::Confirm => {
            st.confirm_new_project();
        }
        crate::new_project_dialog::NewProjectDialogAction::Cancel => {
            st.close_new_project_dialog();
        }
        crate::new_project_dialog::NewProjectDialogAction::FocusInput => {
            st.new_project_dialog.focus_field = 0;
        }
        crate::new_project_dialog::NewProjectDialogAction::None => {}
    }
    drop(st);
    invalidate_window(hwnd);
    Some(LRESULT(0))
}

/// 标题栏区域点击（窗口控制按钮 / 面板切换 / 菜单 / 拖动）。
pub(super) unsafe fn lbd_titlebar(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> Option<LRESULT> {
    let titlebar_region = layout.title_bar_region();
    if !titlebar_region.contains(mouse_x, mouse_y) {
        return None;
    }
    // 关闭用户菜单（如果打开）
    {
        let mut st = state.borrow_mut();
        if st.user_menu.is_open {
            st.user_menu.close();
        }
    }
    // 窗口控制按钮 + 工具栏按钮
    if let Some(r) = lbd_titlebar_controls(hwnd, state, mouse_x, &titlebar_region) {
        return Some(r);
    }
    // 菜单项
    if let Some(r) = lbd_titlebar_menu(hwnd, state, mouse_x, mouse_y, &titlebar_region) {
        return Some(r);
    }
    // 标题栏拖动
    lbd_titlebar_drag(hwnd, state)
}

/// 标题栏窗口控制按钮 + 工具栏按钮。
unsafe fn lbd_titlebar_controls(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    titlebar_region: &crate::layout::Region,
) -> Option<LRESULT> {
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

    let mut st = state.borrow_mut();
    if mouse_x >= minimize_x {
        if mouse_x >= close_x {
            drop(st);
            let _ = DestroyWindow(hwnd);
            return Some(LRESULT(0));
        } else if mouse_x >= maximize_x {
            let is_max = st.is_maximized;
            drop(st);
            if is_max {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            } else {
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
            }
            return Some(LRESULT(0));
        } else {
            drop(st);
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
            return Some(LRESULT(0));
        }
    } else if mouse_x >= user_btn_x {
        // 用户菜单
        st.user_menu.toggle();
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    } else if mouse_x >= settings_btn_x {
        // 设置：打开设置标签页
        st.open_settings_tab();
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    } else if mouse_x >= right_panel_btn_x {
        st.layout.toggle_right_panel();
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    } else if mouse_x >= bottom_panel_btn_x {
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
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    } else if mouse_x >= left_sidebar_btn_x {
        st.layout.toggle_sidebar();
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    } else if mouse_x >= forward_btn_x {
        // 前进：暂无历史导航，仅作为占位
        st.status_message = "前进（待实现）".to_string();
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    } else if mouse_x >= back_btn_x {
        // 返回：暂无历史导航，仅作为占位
        st.status_message = "返回（待实现）".to_string();
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    None
}

/// 标题栏菜单项点击 + 长按检测。
unsafe fn lbd_titlebar_menu(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    titlebar_region: &crate::layout::Region,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    let idx = st
        .menu_bar
        .hit_test(mouse_x, mouse_y - titlebar_region.y, titlebar_region.height)?;
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
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    let was_active = st.menu_bar.active_index == Some(idx);
    st.menu_bar.close_all();
    if !was_active {
        st.menu_bar.expand(idx);
    }
    drop(st);
    invalidate_window(hwnd);
    Some(LRESULT(0))
}

/// 标题栏拖动开始（点击了标题栏但非按钮/菜单区域）。
unsafe fn lbd_titlebar_drag(hwnd: HWND, state: &Rc<RefCell<EditorState>>) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    st.menu_bar.close_all();
    drop(st);
    let _ = ReleaseCapture();
    let _ = SendMessageW(
        hwnd,
        WM_NCLBUTTONDOWN,
        windows::Win32::Foundation::WPARAM(HTCAPTION as usize),
        windows::Win32::Foundation::LPARAM(0),
    );
    Some(LRESULT(0))
}

/// 用户菜单项点击。
pub(super) unsafe fn lbd_user_menu(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    if !st.user_menu.is_open {
        return None;
    }
    let Some(idx) = st.user_menu.hit_test_menu(mouse_x, mouse_y) else {
        // 点击菜单外部，关闭菜单
        st.user_menu.close();
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    };
    let item = st.user_menu.items[idx].clone();
    match item {
        crate::user_menu::UserMenuItem::EditorSettings => {
            st.user_menu.close();
            st.open_settings_tab();
            drop(st);
            invalidate_window(hwnd);
            Some(LRESULT(0))
        }
        crate::user_menu::UserMenuItem::AetherSettings => {
            st.user_menu.close();
            st.status_message = "Aether 设置（待实现）".to_string();
            drop(st);
            invalidate_window(hwnd);
            Some(LRESULT(0))
        }
        crate::user_menu::UserMenuItem::HelpDocs => {
            st.user_menu.close();
            st.status_message = "帮助文档（待实现）".to_string();
            drop(st);
            invalidate_window(hwnd);
            Some(LRESULT(0))
        }
        crate::user_menu::UserMenuItem::FeatureRequest => {
            st.user_menu.close();
            st.status_message = "提交功能建议（待实现）".to_string();
            drop(st);
            invalidate_window(hwnd);
            Some(LRESULT(0))
        }
        crate::user_menu::UserMenuItem::BugReport => {
            st.user_menu.close();
            st.status_message = "问题反馈（待实现）".to_string();
            drop(st);
            invalidate_window(hwnd);
            Some(LRESULT(0))
        }
        crate::user_menu::UserMenuItem::Logout => {
            st.user_menu.close();
            st.status_message = "退出登录（待实现）".to_string();
            drop(st);
            invalidate_window(hwnd);
            Some(LRESULT(0))
        }
        _ => None,
    }
}

/// 资源管理器空白区域上下文菜单点击处理。
///
/// - 菜单展开时，点击菜单项执行对应动作并关闭菜单；
/// - 点击菜单外部则关闭菜单。
pub(super) unsafe fn lbd_explorer_context_menu(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    if !st.explorer_context_menu.is_open {
        return None;
    }
    // 命中菜单项 → 执行动作
    if let Some(idx) = st.explorer_context_menu.hit_test_menu(mouse_x, mouse_y) {
        let item = st.explorer_context_menu.items[idx];
        st.explorer_context_menu.close();
        // 标记侧边栏脏区域（动作可能触发文件树刷新或内联输入）
        let region = st.layout.sidebar_region().clone();
        st.dirty_tracker.mark_region(
            region.x,
            region.y,
            region.width,
            region.height,
            crate::dirty_rect::DirtyRegionType::Sidebar,
        );
        drop(st);
        // 执行动作（需要单独借用）
        state.borrow_mut().execute_explorer_context_action(item);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    // 点击菜单外部 → 关闭菜单
    st.explorer_context_menu.close();
    drop(st);
    invalidate_window(hwnd);
    Some(LRESULT(0))
}

/// 文件节点右键上下文菜单点击处理。
///
/// - 菜单可见时，点击菜单项执行对应动作并关闭菜单；
/// - 点击菜单外部则关闭菜单。
pub(super) unsafe fn lbd_file_node_context_menu(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    if !st.file_node_context_menu.is_open {
        return None;
    }
    // 命中菜单项 → 执行动作
    if let Some(idx) = st.file_node_context_menu.hit_test_menu(mouse_x, mouse_y) {
        let item = st.file_node_context_menu.items[idx];
        let node_idx = st.file_node_context_menu.target_node;
        st.file_node_context_menu.close();
        // 标记侧边栏脏区域
        let region = st.layout.sidebar_region().clone();
        st.dirty_tracker.mark_region(
            region.x,
            region.y,
            region.width,
            region.height,
            crate::dirty_rect::DirtyRegionType::Sidebar,
        );
        drop(st);
        // 执行动作（需要单独借用）
        if let Some(node_idx) = node_idx {
            state
                .borrow_mut()
                .execute_file_node_context_action(item, node_idx);
        }
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    // 点击菜单外部 → 关闭菜单
    st.file_node_context_menu.close();
    drop(st);
    invalidate_window(hwnd);
    Some(LRESULT(0))
}

/// 标签右键上下文菜单点击处理。
///
/// - 菜单可见时，点击菜单项执行对应动作并关闭菜单；
/// - 点击菜单外部则关闭菜单。
pub(super) unsafe fn lbd_tab_context_menu(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    if !st.tab_context_menu.visible {
        return None;
    }
    // 命中菜单项 → 执行动作
    if let Some(item_idx) = st.tab_context_menu.hit_test(mouse_x, mouse_y) {
        // disabled 项不响应
        let enabled = st
            .tab_context_menu
            .items
            .get(item_idx)
            .map(|i| i.enabled)
            .unwrap_or(false);
        if !enabled {
            return Some(LRESULT(0));
        }
        let cmd = st.tab_context_menu.items[item_idx].command;
        let tab_idx = st.tab_context_menu.tab_index;
        st.tab_context_menu.hide();
        drop(st);
        // 执行命令（需要单独借用）
        let mut st = state.borrow_mut();
        match cmd {
            crate::tab_context_menu::TabContextMenuCommand::Close => {
                if let Some(idx) = tab_idx {
                    st.close_tab(idx);
                }
            }
            crate::tab_context_menu::TabContextMenuCommand::CloseOthers => {
                if let Some(idx) = tab_idx {
                    st.close_other_tabs(idx);
                }
            }
            crate::tab_context_menu::TabContextMenuCommand::CloseToTheRight => {
                if let Some(idx) = tab_idx {
                    st.close_tabs_to_the_right(idx);
                }
            }
            crate::tab_context_menu::TabContextMenuCommand::CloseAll => {
                st.close_all_tabs();
            }
            crate::tab_context_menu::TabContextMenuCommand::CopyPath => {
                if let Some(idx) = tab_idx {
                    if let Some(path) = st.tabs.get(idx).and_then(|t| t.file_path().cloned()) {
                        st.copy_text_to_clipboard(&path.to_string_lossy());
                    }
                }
            }
            crate::tab_context_menu::TabContextMenuCommand::RevealInExplorer => {
                if let Some(idx) = tab_idx {
                    if let Some(path) = st.tabs.get(idx).and_then(|t| t.file_path().cloned()) {
                        let _ = std::process::Command::new("explorer.exe")
                            .args(["/select,", &path.to_string_lossy()])
                            .spawn();
                    }
                }
            }
            crate::tab_context_menu::TabContextMenuCommand::Separator => {}
        }
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    // 点击菜单外部 → 关闭菜单
    st.tab_context_menu.hide();
    drop(st);
    invalidate_window(hwnd);
    Some(LRESULT(0))
}

/// 活动栏右键上下文菜单点击处理。
///
/// 菜单可见时拦截所有左键点击：命中菜单项则执行对应命令，
/// 命中菜单外部则关闭菜单。
pub(super) unsafe fn lbd_activity_bar_context_menu(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    if !st.activity_bar_context_menu.visible {
        return None;
    }
    // 命中菜单项 → 执行动作
    if let Some(item_idx) = st.activity_bar_context_menu.hit_test(mouse_x, mouse_y) {
        let enabled = st
            .activity_bar_context_menu
            .items
            .get(item_idx)
            .map(|i| i.enabled)
            .unwrap_or(false);
        if !enabled {
            return Some(LRESULT(0));
        }
        let cmd = st.activity_bar_context_menu.items[item_idx].command;
        st.activity_bar_context_menu.hide();
        drop(st);
        // 执行命令（需要单独借用）
        let mut st = state.borrow_mut();
        use crate::activity_bar_context_menu::ActivityBarContextMenuCommand as C;
        use crate::layout::ActivityBarView;
        match cmd {
            C::HideActivityBar => {
                st.layout.activity_bar_visible = false;
            }
            C::CustomizeSort => {
                // 活动栏自定义排序：当前为占位实现，提示用户功能待实现
                st.status_message = "活动栏自定义排序（待实现）".to_string();
                st.status_bar.update_status("活动栏自定义排序（待实现）");
            }
            C::SwitchToExplorer => {
                st.switch_activity_view(ActivityBarView::Explorer);
            }
            C::SwitchToSourceControl => {
                st.switch_activity_view(ActivityBarView::SourceControl);
            }
            C::SwitchToTerminal => {
                st.switch_activity_view(ActivityBarView::Terminal);
            }
            C::SwitchToRemoteManager => {
                st.switch_activity_view(ActivityBarView::RemoteManager);
            }
            C::SwitchToAiAssistant => {
                // AI 助手使用右侧面板，强制显示
                st.layout.right_panel_visible = true;
                if st.layout.right_panel_width < 1.0 {
                    st.layout.right_panel_width = 320.0;
                }
                st.activity_bar.switch_to_view(ActivityBarView::AiAssistant);
                st.activity_view = ActivityBarView::AiAssistant;
                st.ai_panel.input_focused = false;
            }
            C::Separator => {}
        }
        drop(st);
        invalidate_window(hwnd);
        return Some(LRESULT(0));
    }
    // 点击菜单外部 → 关闭菜单
    st.activity_bar_context_menu.hide();
    drop(st);
    invalidate_window(hwnd);
    Some(LRESULT(0))
}

/// 子菜单项点击（菜单展开后的下拉项）。
pub(super) unsafe fn lbd_submenu(
    hwnd: HWND,
    state: &Rc<RefCell<EditorState>>,
    mouse_x: f32,
    mouse_y: f32,
    layout: &crate::layout::LayoutManager,
) -> Option<LRESULT> {
    let mut st = state.borrow_mut();
    let active_idx = st.menu_bar.active_index?;
    let &submenu_x = st.menu_bar.item_x_positions.get(active_idx)?;
    let titlebar_region = layout.title_bar_region();
    let submenu_y = titlebar_region.y + titlebar_region.height;
    let sub_idx = st
        .menu_bar
        .hit_test_submenu(active_idx, mouse_x, mouse_y, submenu_x, submenu_y)?;
    if let Some(item) = st.menu_bar.items.get(active_idx) {
        if let Some(menu_item) = item.items.get(sub_idx) {
            if menu_item.enabled && menu_item.command_id != crate::menu_bar::CommandId::None {
                let cmd = menu_item.command_id;
                st.menu_bar.close_all();
                drop(st);
                state.borrow_mut().execute_command(cmd, hwnd);
                invalidate_window(hwnd);
                return Some(LRESULT(0));
            }
        }
    }
    None
}
