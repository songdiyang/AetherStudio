//! WM_KEYDOWN 处理：键盘按键分发。
//!
//! 从 `window.rs` 拆分而来，保持原有逻辑不变。
//! 调度器提取公共状态 (vk, ctrl, shift)，然后按优先级调用辅助函数。
//! Ctrl+ 快捷键拆分到 `key_down_ctrl`，非 Ctrl 编辑器按键拆分到 `key_down_edit`。

use std::path::PathBuf;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::dialogs::Dialogs;

use super::super::{get_and_set_state, invalidate_window, EDITOR_STATE};

/// WM_KEYDOWN
pub(crate) unsafe fn on_key_down(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // C-12: 键盘消息进入时先同步 thread_local 到当前窗口状态
    get_and_set_state(hwnd);
    let vk = VIRTUAL_KEY(wparam.0 as u16);
    let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
    let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;

    // IME 合成期间：直接交给默认窗口过程，让 IMM32 处理按键
    // （中文/日文 IME 会在合成期拦截 Backspace/字母/方向键来更新或取消合成串）
    // 修复：之前我们 return LRESULT(0) 消费了消息，导致 Backspace 等无法更新合成
    let ime_composing = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().composition.is_some())
            .unwrap_or(false)
    });
    if ime_composing {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    if let Some(r) = okd_file_tree_input(hwnd, vk) {
        return r;
    }
    if let Some(r) = okd_explorer_context_menu(hwnd, vk) {
        return r;
    }
    if let Some(r) = okd_file_node_context_menu(hwnd, vk) {
        return r;
    }
    if let Some(r) = okd_tab_context_menu(hwnd, vk) {
        return r;
    }
    if let Some(r) = okd_activity_bar_context_menu(hwnd, vk) {
        return r;
    }
    if let Some(r) = okd_escape_customize(hwnd, vk) {
        return r;
    }
    if let Some(r) = okd_search_panel(hwnd, vk, ctrl) {
        return r;
    }
    if let Some(r) = okd_welcome_nav(hwnd, vk, ctrl) {
        return r;
    }
    if let Some(r) = okd_completion_nav(hwnd, vk, ctrl) {
        return r;
    }
    if let Some(r) = okd_settings_field(hwnd, vk, shift) {
        return r;
    }
    if let Some(r) = okd_ssh_dialog(hwnd, vk, ctrl) {
        return r;
    }
    if let Some(r) = okd_clone_dialog(hwnd, vk, ctrl) {
        return r;
    }
    if let Some(r) = okd_new_project_dialog(hwnd, vk, ctrl, msg, wparam) {
        return r;
    }
    if let Some(r) = okd_ssh_manager(hwnd, vk) {
        return r;
    }
    if let Some(r) = okd_command_palette(hwnd, vk) {
        return r;
    }

    if ctrl {
        // 文件树输入框激活时，吞掉所有 Ctrl 快捷键防止编辑器误响应
        let ft_active = EDITOR_STATE.with(|s| {
            s.borrow()
                .as_ref()
                .map(|state| state.borrow().file_tree_input.is_some())
                .unwrap_or(false)
        });
        if ft_active {
            return LRESULT(0);
        }
        super::key_down_ctrl::okd_ctrl_dispatch(hwnd, vk, shift);
        return LRESULT(0);
    }

    // SubTask 13.4: Alt+Left/Right 触发返回/前进导航
    if GetKeyState(VK_MENU.0 as i32) < 0 {
        if let Some(r) = okd_alt_nav(hwnd, vk) {
            return r;
        }
    }

    // F2: 文件树有选中节点时触发重命名
    if vk == VK_F2 {
        let has_selection = EDITOR_STATE.with(|s| {
            s.borrow()
                .as_ref()
                .map(|state| state.borrow().selected_file_node.is_some())
                .unwrap_or(false)
        });
        if has_selection {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let mut st = state.borrow_mut();
                    if let Some(node_idx) = st.selected_file_node {
                        st.start_file_tree_input(crate::editor::FileTreeInputKind::Rename);
                        st.file_tree_input.as_mut().unwrap().target_node = Some(node_idx);
                    }
                    drop(st);
                    invalidate_window(hwnd);
                }
            });
            return LRESULT(0);
        }
    }

    // 文件树输入框激活时，吞掉所有非 Ctrl 编辑器按键（方向键等），
    // 防止编辑器光标移动 / 删除等操作。字符输入由 WM_CHAR 处理。
    let ft_active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().file_tree_input.is_some())
            .unwrap_or(false)
    });
    if ft_active {
        return LRESULT(0);
    }

    // AI 面板输入框聚焦时，处理方向键、Home/End、Delete
    if let Some(r) = okd_ai_panel_input(hwnd, vk) {
        return r;
    }

    super::key_down_edit::okd_edit_dispatch(hwnd, vk, shift);
    LRESULT(0)
}

/// SubTask 13.4: Alt+Left/Right 导航（返回/前进）。
/// 与标题栏返回/前进按钮行为一致：当前为占位实现，显示状态消息。
unsafe fn okd_alt_nav(hwnd: HWND, vk: VIRTUAL_KEY) -> Option<LRESULT> {
    match vk {
        VK_LEFT => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().status_message = "返回（待实现）".to_string();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_RIGHT => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().status_message = "前进（待实现）".to_string();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        _ => None,
    }
}

/// 文件树内联输入框的 Enter / Escape / Backspace / Delete 处理
///
/// 仅消费已明确处理的按键（VK_RETURN / VK_ESCAPE / VK_BACK / VK_DELETE）。
/// 其他键返回 None 让消息继续分发，但 `on_key_down` 会在编辑器按键分发前
/// 检查 file_tree_input 是否激活并吞掉，防止编辑器误响应。
unsafe fn okd_file_tree_input(hwnd: HWND, vk: VIRTUAL_KEY) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().file_tree_input.is_some())
            .unwrap_or(false)
    });
    if !active {
        return None;
    }

    if vk == VK_RETURN {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                state.borrow_mut().confirm_file_tree_input();
                invalidate_window(hwnd);
            }
        });
        return Some(LRESULT(0));
    }
    if vk == VK_ESCAPE {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                state.borrow_mut().cancel_file_tree_input();
                invalidate_window(hwnd);
            }
        });
        return Some(LRESULT(0));
    }
    if vk == VK_BACK || vk == VK_DELETE {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                let mut st = state.borrow_mut();
                let region = st.layout.sidebar_region().clone();
                if let Some(input) = st.file_tree_input.as_mut() {
                    // 优先清除 IME 合成串（如果在合成中按退格/删除，
                    // IME 通常自行处理，但作为安全兜底也清除本地合成状态）
                    if input.composition.is_some() {
                        input.composition = None;
                    } else {
                        input.value.pop();
                    }
                    input.caret_visible = true;
                }
                st.dirty_tracker.mark_region(
                    region.x,
                    region.y,
                    region.width,
                    region.height,
                    crate::dirty_rect::DirtyRegionType::Sidebar,
                );
                drop(st);
                invalidate_window(hwnd);
            }
        });
        return Some(LRESULT(0));
    }

    // 其他键不在此处消费，由 on_key_down 统一拦截防止编辑器响应
    None
}

/// 文件节点上下文菜单打开时，按 Escape 关闭；按 F2 触发重命名
unsafe fn okd_file_node_context_menu(hwnd: HWND, vk: VIRTUAL_KEY) -> Option<LRESULT> {
    let open = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().file_node_context_menu.is_open)
            .unwrap_or(false)
    });
    if !open {
        return None;
    }
    if vk == VK_ESCAPE {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                state.borrow_mut().file_node_context_menu.close();
                invalidate_window(hwnd);
            }
        });
        return Some(LRESULT(0));
    }
    // 菜单打开时其他键不处理（避免误触编辑器）
    Some(LRESULT(0))
}

/// 资源管理器空白区域上下文菜单打开时，按 Escape 关闭
unsafe fn okd_explorer_context_menu(hwnd: HWND, vk: VIRTUAL_KEY) -> Option<LRESULT> {
    let open = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().explorer_context_menu.is_open)
            .unwrap_or(false)
    });
    if !open {
        return None;
    }
    if vk != VK_ESCAPE {
        // 菜单打开时其他键不处理（避免误触编辑器），仅响应 Esc
        return Some(LRESULT(0));
    }
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            state.borrow_mut().explorer_context_menu.close();
            invalidate_window(hwnd);
        }
    });
    Some(LRESULT(0))
}

/// 标签右键上下文菜单打开时，按 Escape 关闭
unsafe fn okd_tab_context_menu(hwnd: HWND, vk: VIRTUAL_KEY) -> Option<LRESULT> {
    let open = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().tab_context_menu.visible)
            .unwrap_or(false)
    });
    if !open {
        return None;
    }
    if vk != VK_ESCAPE {
        // 菜单打开时其他键不处理（避免误触编辑器），仅响应 Esc
        return Some(LRESULT(0));
    }
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            state.borrow_mut().tab_context_menu.hide();
            invalidate_window(hwnd);
        }
    });
    Some(LRESULT(0))
}

/// 活动栏右键上下文菜单打开时，按 Escape 关闭
unsafe fn okd_activity_bar_context_menu(hwnd: HWND, vk: VIRTUAL_KEY) -> Option<LRESULT> {
    let open = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().activity_bar_context_menu.visible)
            .unwrap_or(false)
    });
    if !open {
        return None;
    }
    if vk != VK_ESCAPE {
        // 菜单打开时其他键不处理（避免误触编辑器），仅响应 Esc
        return Some(LRESULT(0));
    }
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            state.borrow_mut().activity_bar_context_menu.hide();
            invalidate_window(hwnd);
        }
    });
    Some(LRESULT(0))
}

/// 自定义模式下按 Escape 退出
unsafe fn okd_escape_customize(hwnd: HWND, vk: VIRTUAL_KEY) -> Option<LRESULT> {
    if vk != VK_ESCAPE {
        return None;
    }
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
                invalidate_window(hwnd);
            }
        });
        Some(LRESULT(0))
    } else {
        None
    }
}

/// 全局搜索面板键盘处理
unsafe fn okd_search_panel(hwnd: HWND, vk: VIRTUAL_KEY, ctrl: bool) -> Option<LRESULT> {
    let visible = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().search_panel.visible)
            .unwrap_or(false)
    });
    if !visible || ctrl {
        return None;
    }
    match vk {
        VK_ESCAPE => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().search_panel.hide();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_BACK => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().search_panel.backspace();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_RETURN => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let root = state.borrow().current_folder.clone();
                    state.borrow_mut().search_panel.search(root.as_deref());
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_DOWN => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().search_panel.select_next();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_UP => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().search_panel.select_prev();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        _ => None,
    }
}

/// 欢迎页键盘导航：Tab/↓ next, Shift+Tab/↑ prev, Enter 触发
unsafe fn okd_welcome_nav(hwnd: HWND, vk: VIRTUAL_KEY, ctrl: bool) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().show_welcome())
            .unwrap_or(false)
    });
    if !active || ctrl {
        return None;
    }
    match vk {
        VK_TAB | VK_DOWN => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().welcome_focus_next();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_UP => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().welcome_focus_prev();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_RETURN => {
            okd_welcome_enter(hwnd);
            Some(LRESULT(0))
        }
        _ => None,
    }
}

/// 欢迎页 Enter 键处理：执行焦点对应的动作
unsafe fn okd_welcome_enter(hwnd: HWND) {
    let action = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .and_then(|state| state.borrow().welcome_focus_action.clone())
    });
    if let Some(action) = action {
        match action {
            crate::welcome::WelcomeAction::OpenFolder => {
                if let Some(path) = Dialogs::open_folder_dialog(hwnd, "打开文件夹") {
                    EDITOR_STATE.with(|s| {
                        if let Some(state) = s.borrow().as_ref() {
                            state.borrow_mut().open_folder(path);
                            invalidate_window(hwnd);
                        }
                    });
                }
            }
            crate::welcome::WelcomeAction::OpenRecentProject(path_str) => {
                let path = PathBuf::from(path_str);
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().open_folder(path);
                        invalidate_window(hwnd);
                    }
                });
            }
            crate::welcome::WelcomeAction::MoreRecentProjects => {
                if let Some(path) = Dialogs::open_folder_dialog(hwnd, "打开文件夹") {
                    EDITOR_STATE.with(|s| {
                        if let Some(state) = s.borrow().as_ref() {
                            state.borrow_mut().open_folder(path);
                            invalidate_window(hwnd);
                        }
                    });
                }
            }
            _ => {}
        }
    }
}

/// Phase H2: 补全弹窗可见时拦截导航键（↑↓/Enter/Esc）
unsafe fn okd_completion_nav(hwnd: HWND, vk: VIRTUAL_KEY, ctrl: bool) -> Option<LRESULT> {
    if ctrl {
        return None;
    }
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().completion_visible)
            .unwrap_or(false)
    });
    if !active {
        return None;
    }
    match vk {
        VK_UP => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().completion_prev();
                }
            });
            invalidate_window(hwnd);
            Some(LRESULT(0))
        }
        VK_DOWN => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().completion_next();
                }
            });
            invalidate_window(hwnd);
            Some(LRESULT(0))
        }
        VK_RETURN => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().completion_accept();
                }
            });
            invalidate_window(hwnd);
            Some(LRESULT(0))
        }
        VK_ESCAPE => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().completion_cancel();
                }
            });
            invalidate_window(hwnd);
            Some(LRESULT(0))
        }
        _ => None,
    }
}

/// Settings field active - intercept keyboard input
unsafe fn okd_settings_field(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) -> Option<LRESULT> {
    let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().settings_panel.active_field.is_some())
            .unwrap_or(false)
    });
    if !active {
        return None;
    }
    match vk {
        VK_ESCAPE => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().settings_panel.active_field = None;
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_RETURN => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().settings_panel.active_field = None;
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_BACK => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().settings_panel.backspace();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_DELETE => {
            // UI-M05: Delete 键应清除字段而非执行 Backspace（删除末尾字符）
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().settings_panel.delete_forward();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_TAB => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    if shift {
                        state.borrow_mut().settings_panel.prev_field();
                    } else {
                        state.borrow_mut().settings_panel.next_field();
                    }
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_V if ctrl => {
            // 设置面板字段支持 Ctrl+V 粘贴
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    if let Some(text) = crate::editor::EditorState::get_clipboard_text() {
                        state.borrow_mut().settings_panel.paste_text(&text);
                        invalidate_window(hwnd);
                    }
                }
            });
            Some(LRESULT(0))
        }
        _ => {
            // Prevent editor from processing other keys while field is active
            Some(LRESULT(0))
        }
    }
}

/// SSH 对话框激活时优先处理键盘
unsafe fn okd_ssh_dialog(hwnd: HWND, vk: VIRTUAL_KEY, ctrl: bool) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().ssh_dialog.visible)
            .unwrap_or(false)
    });
    if !active {
        return None;
    }
    match vk {
        VK_ESCAPE => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().ssh_dialog.visible = false;
                    invalidate_window(hwnd);
                }
            });
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
                        st.ssh_dialog.error_message = Some("请填写主机和用户名".to_string());
                    }
                    drop(st);
                    invalidate_window(hwnd);
                }
            });
        }
        VK_TAB => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().ssh_dialog.next_field();
                    invalidate_window(hwnd);
                }
            });
        }
        VK_BACK => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().handle_ssh_dialog_backspace();
                    invalidate_window(hwnd);
                }
            });
        }
        VK_V if ctrl => {
            // P2-4: Ctrl+V 粘贴到当前 SSH 对话框字段
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().paste_into_ssh_dialog();
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
    // ssh_dialog_active 时所有未匹配键也被消费（原始代码 return LRESULT(0)）
    Some(LRESULT(0))
}

/// 克隆对话框激活时优先处理键盘
unsafe fn okd_clone_dialog(hwnd: HWND, vk: VIRTUAL_KEY, ctrl: bool) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().clone_dialog.visible)
            .unwrap_or(false)
    });
    if !active {
        return None;
    }
    match vk {
        VK_ESCAPE => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().clone_dialog.visible = false;
                    invalidate_window(hwnd);
                }
            });
        }
        VK_RETURN => {
            okd_clone_dialog_enter(hwnd);
        }
        VK_BACK => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().handle_clone_dialog_backspace();
                    invalidate_window(hwnd);
                }
            });
        }
        VK_V if ctrl => {
            // P2-4: Ctrl+V 粘贴到克隆对话框 URL 字段
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().paste_into_clone_dialog();
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
    // clone_dialog_active 时所有未匹配键也被消费（原始代码 return LRESULT(0)）
    Some(LRESULT(0))
}

/// 克隆对话框 Enter 键处理：验证 URL 并启动克隆
unsafe fn okd_clone_dialog_enter(hwnd: HWND) {
    EDITOR_STATE.with(|s| -> LRESULT {
        if let Some(state) = s.borrow().as_ref() {
            let mut st = state.borrow_mut();
            if st.clone_dialog.url.is_empty() {
                st.clone_dialog.error_message = Some("请输入仓库 URL".to_string());
                drop(st);
                invalidate_window(hwnd);
            } else if st.git_cloning {
                // C-09: 正在克隆中，忽略
                drop(st);
            } else {
                let url = st.clone_dialog.url.clone();
                drop(st);
                if let Some(target_path) =
                    crate::dialogs::Dialogs::open_folder_dialog(hwnd, "选择克隆目标文件夹")
                {
                    // C-09: Git 克隆移至后台线程，避免阻塞 UI
                    let mut st = state.borrow_mut();
                    st.start_git_clone(url, target_path);
                    drop(st);
                    invalidate_window(hwnd);
                    return LRESULT(0);
                }
                // 文件夹对话框取消
                invalidate_window(hwnd);
            }
        }
        LRESULT(0)
    });
}

/// 新建项目对话框键盘处理
unsafe fn okd_new_project_dialog(
    hwnd: HWND,
    vk: VIRTUAL_KEY,
    ctrl: bool,
    msg: u32,
    wparam: WPARAM,
) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().new_project_dialog.visible)
            .unwrap_or(false)
    });
    if !active {
        return None;
    }
    match vk {
        VK_ESCAPE => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().close_new_project_dialog();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_RETURN => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().confirm_new_project();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_BACK => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().new_project_dialog.project_name.pop();
                    state.borrow_mut().new_project_dialog.error_message = None;
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_V if ctrl => {
            // Ctrl+V 粘贴到项目名称输入框
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().paste_into_new_project_dialog();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        _ => {
            // 普通字符键交给 DefWindowProc，确保能生成 WM_CHAR
            Some(DefWindowProcW(hwnd, msg, wparam, LPARAM(0)))
        }
    }
}

/// SSH 管理面板编辑模式键盘处理
unsafe fn okd_ssh_manager(hwnd: HWND, vk: VIRTUAL_KEY) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| {
                state.borrow().sidebar_content == crate::layout::SidebarContent::RemoteManagerPanel
                    && state.borrow().ssh_manager_panel.editing
            })
            .unwrap_or(false)
    });
    if !active {
        return None;
    }
    match vk {
        VK_ESCAPE => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().ssh_manager_panel.cancel_edit();
                    invalidate_window(hwnd);
                }
            });
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
                    invalidate_window(hwnd);
                }
            });
        }
        VK_TAB => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let mut st = state.borrow_mut();
                    st.ssh_manager_panel.focus_field = (st.ssh_manager_panel.focus_field + 1) % 5;
                    drop(st);
                    invalidate_window(hwnd);
                }
            });
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
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
    // ssh_mgr_editing 时所有未匹配键也被消费（原始代码 return LRESULT(0)）
    Some(LRESULT(0))
}

/// 命令面板激活时优先处理键盘导航
unsafe fn okd_command_palette(hwnd: HWND, vk: VIRTUAL_KEY) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().command_palette.visible)
            .unwrap_or(false)
    });
    if !active {
        return None;
    }
    match vk {
        VK_ESCAPE => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().command_palette.hide();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_RETURN => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    if let Some(cmd) = state.borrow().command_palette.selected_command() {
                        let hwnd = state.borrow().hwnd;
                        state.borrow_mut().execute_command(cmd, hwnd);
                    }
                    state.borrow_mut().command_palette.hide();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_UP => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().command_palette.select_prev();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_DOWN => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().command_palette.select_next();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_BACK => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().command_palette.backspace_query();
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        _ => None,
    }
}

/// AI 面板输入框聚焦时处理方向键、Home/End、Delete
unsafe fn okd_ai_panel_input(hwnd: HWND, vk: VIRTUAL_KEY) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().ai_panel.input_focused)
            .unwrap_or(false)
    });
    if !active {
        return None;
    }
    match vk {
        VK_LEFT => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().ai_panel.move_caret_left();
                    state.borrow_mut().ai_panel.caret_visible = true;
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_RIGHT => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().ai_panel.move_caret_right();
                    state.borrow_mut().ai_panel.caret_visible = true;
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_HOME => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().ai_panel.move_caret_home();
                    state.borrow_mut().ai_panel.caret_visible = true;
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_END => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().ai_panel.move_caret_end();
                    state.borrow_mut().ai_panel.caret_visible = true;
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        VK_DELETE => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().ai_panel.delete();
                    state.borrow_mut().ai_panel.caret_visible = true;
                    invalidate_window(hwnd);
                }
            });
            Some(LRESULT(0))
        }
        _ => None,
    }
}
