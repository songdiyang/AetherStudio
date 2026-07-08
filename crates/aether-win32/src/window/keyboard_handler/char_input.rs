//! WM_CHAR 处理：字符输入分发。
//!
//! 从 `window.rs` 拆分而来，保持原有逻辑不变。
//! 调度器处理 UTF-16 代理对，然后将可打印字符按优先级分发到各 UI 面板/对话框/编辑器。

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

use super::super::{get_and_set_state, invalidate_window, EDITOR_STATE, PENDING_HIGH_SURROGATE};

/// WM_CHAR
pub(crate) unsafe fn on_char(hwnd: HWND, _msg: u32, wparam: WPARAM, _lparam: LPARAM) -> LRESULT {
    // C-12: 键盘消息进入时先同步 thread_local 到当前窗口状态，
    // 防止 Alt+Tab / 任务栏切换焦点后键盘输入路由到错误窗口的 EditorState
    get_and_set_state(hwnd);
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
            // 按优先级依次尝试各输入目标，首个匹配的处理器消费字符
            if let Some(r) = oc_file_tree_input(hwnd, c) {
                return r;
            }
            if let Some(r) = oc_settings_field(hwnd, c) {
                return r;
            }
            if let Some(r) = oc_search_panel(hwnd, c) {
                return r;
            }
            if let Some(r) = oc_ssh_dialog(hwnd, c) {
                return r;
            }
            if let Some(r) = oc_clone_dialog(hwnd, c) {
                return r;
            }
            if let Some(r) = oc_new_project(hwnd, c) {
                return r;
            }
            if let Some(r) = oc_ssh_manager(hwnd, c) {
                return r;
            }
            if let Some(r) = oc_command_palette(hwnd, c) {
                return r;
            }
            if let Some(r) = oc_find_replace(hwnd, c) {
                return r;
            }
            if let Some(r) = oc_terminal(hwnd, c) {
                return r;
            }
            if let Some(r) = oc_ai_panel(hwnd, c) {
                return r;
            }
            oc_editor_default(hwnd, c);
        }
    }
    LRESULT(0)
}

/// 文件树内联输入框激活时，输入字符到当前值
unsafe fn oc_file_tree_input(hwnd: HWND, c: char) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().file_tree_input.is_some())
            .unwrap_or(false)
    });
    if active {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                let mut st = state.borrow_mut();
                let region = st.layout.sidebar_region().clone();
                if let Some(input) = st.file_tree_input.as_mut() {
                    input.value.push(c);
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
        Some(LRESULT(0))
    } else {
        None
    }
}

/// Settings panel active field routing
unsafe fn oc_settings_field(hwnd: HWND, c: char) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().settings_panel.active_field.is_some())
            .unwrap_or(false)
    });
    if active {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                state.borrow_mut().settings_panel.input_char(c);
                invalidate_window(hwnd);
            }
        });
        Some(LRESULT(0))
    } else {
        None
    }
}

/// 搜索面板可见时，输入字符进入搜索查询
unsafe fn oc_search_panel(hwnd: HWND, c: char) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().search_panel.visible)
            .unwrap_or(false)
    });
    if active {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                state.borrow_mut().search_panel.input_char(c);
                invalidate_window(hwnd);
            }
        });
        Some(LRESULT(0))
    } else {
        None
    }
}

/// SSH 对话框激活时，输入字符进入对话框
unsafe fn oc_ssh_dialog(hwnd: HWND, c: char) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().ssh_dialog.visible)
            .unwrap_or(false)
    });
    if active {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                state.borrow_mut().handle_ssh_dialog_key(c);
                invalidate_window(hwnd);
            }
        });
        Some(LRESULT(0))
    } else {
        None
    }
}

/// 克隆对话框激活时，输入字符进入对话框
unsafe fn oc_clone_dialog(hwnd: HWND, c: char) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().clone_dialog.visible)
            .unwrap_or(false)
    });
    if active {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                state.borrow_mut().handle_clone_dialog_key(c);
                invalidate_window(hwnd);
            }
        });
        Some(LRESULT(0))
    } else {
        None
    }
}

/// 新建项目对话框激活时，输入字符进入项目名称
unsafe fn oc_new_project(hwnd: HWND, c: char) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().new_project_dialog.visible)
            .unwrap_or(false)
    });
    if active {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                state.borrow_mut().new_project_dialog.project_name.push(c);
                state.borrow_mut().new_project_dialog.error_message = None;
                invalidate_window(hwnd);
            }
        });
        Some(LRESULT(0))
    } else {
        None
    }
}

/// SSH 管理面板编辑模式：输入字符到当前焦点字段
unsafe fn oc_ssh_manager(hwnd: HWND, c: char) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| {
                state.borrow().sidebar_content == crate::layout::SidebarContent::RemoteManagerPanel
                    && state.borrow().ssh_manager_panel.editing
            })
            .unwrap_or(false)
    });
    if active {
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
                invalidate_window(hwnd);
            }
        });
        Some(LRESULT(0))
    } else {
        None
    }
}

/// 命令面板激活时，输入字符进入搜索框
unsafe fn oc_command_palette(hwnd: HWND, c: char) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().command_palette.visible)
            .unwrap_or(false)
    });
    if active {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                state.borrow_mut().command_palette.append_query(c);
                invalidate_window(hwnd);
            }
        });
        Some(LRESULT(0))
    } else {
        None
    }
}

/// 查找替换面板激活时，输入字符进入查找/替换框
unsafe fn oc_find_replace(hwnd: HWND, c: char) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| {
                state.borrow().find_visible
                    && state.borrow().find_focus != crate::editor::FindReplaceFocus::None
            })
            .unwrap_or(false)
    });
    if active {
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
                            state.borrow_mut().content.cursor_line = line;
                            state.borrow_mut().content.cursor_col = col;
                            state.borrow_mut().content.selection_start = Some((line, col));
                            state.borrow_mut().content.selection_end =
                                Some((line, col + state.borrow().find_query.len()));
                        }
                    }
                    crate::editor::FindReplaceFocus::ReplaceText => {
                        state.borrow_mut().replace_text.push(c);
                    }
                    _ => {}
                }
                invalidate_window(hwnd);
            }
        });
        Some(LRESULT(0))
    } else {
        None
    }
}

/// 终端面板激活时，输入字符进入终端
unsafe fn oc_terminal(hwnd: HWND, c: char) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().terminal_panel.focused)
            .unwrap_or(false)
    });
    if active {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                state.borrow_mut().terminal_panel.input_line.push(c);
                state.borrow_mut().terminal_panel.cursor_pos += 1;
                invalidate_window(hwnd);
            }
        });
        Some(LRESULT(0))
    } else {
        None
    }
}

/// AI 面板输入框聚焦时，输入字符进入 AI 输入
unsafe fn oc_ai_panel(hwnd: HWND, c: char) -> Option<LRESULT> {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().ai_panel.input_focused)
            .unwrap_or(false)
    });
    if active {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                state.borrow_mut().ai_panel.input_char(c);
                invalidate_window(hwnd);
            }
        });
        Some(LRESULT(0))
    } else {
        None
    }
}

/// 编辑器默认：广播字符到所有光标
unsafe fn oc_editor_default(hwnd: HWND, c: char) {
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            // P1-1: 多光标模式下广播到所有光标
            state.borrow_mut().broadcast_insert_char(c);
            invalidate_window(hwnd);
        }
    });
}
