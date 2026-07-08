//! WM_KEYDOWN 非 Ctrl 编辑器按键处理。
//!
//! 从 `window.rs` 拆分而来，保持原有逻辑不变。
//! 处理 Return/Back/Delete/F3/Escape/方向键/Home/End/PageUp/Down/Tab。

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

use crate::editor::EditorState;

use super::super::{invalidate_window, EDITOR_STATE};

/// 非 Ctrl 按键总分发
pub(crate) unsafe fn okd_edit_dispatch(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) {
    match vk {
        VK_RETURN => okd_edit_return(hwnd),
        VK_BACK => okd_edit_back(hwnd),
        VK_DELETE | VK_F3 | VK_ESCAPE => okd_edit_delete_misc(hwnd, vk, shift),
        VK_LEFT | VK_RIGHT => okd_edit_left_right(hwnd, vk, shift),
        VK_UP | VK_DOWN => okd_edit_up_down(hwnd, vk, shift),
        VK_HOME | VK_END | VK_PRIOR | VK_NEXT => okd_edit_home_end_page(hwnd, vk, shift),
        VK_TAB => okd_edit_tab(hwnd),
        _ => {}
    }
}

/// 判断当前是否有选中文本
fn has_selection(st: &EditorState) -> bool {
    st.content.selection_start.is_some() && st.content.selection_end.is_some()
}

/// VK_RETURN：终端/AI/查找/编辑器各自的回车处理
unsafe fn okd_edit_return(hwnd: HWND) {
    let terminal_active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().terminal_panel.focused)
            .unwrap_or(false)
    });
    let ai_panel_active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            // C-10: 改用 input_focused 而非 right_panel_visible，避免面板可见即劫持回车
            .map(|state| state.borrow().ai_panel.input_focused)
            .unwrap_or(false)
    });
    let find_active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| {
                state.borrow().find_visible
                    && state.borrow().find_focus != crate::editor::FindReplaceFocus::None
            })
            .unwrap_or(false)
    });
    if terminal_active {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                let input = state.borrow().terminal_panel.input_line.clone();
                state
                    .borrow_mut()
                    .terminal_panel
                    .push_output(&format!("> {}", input));
                state.borrow_mut().terminal_panel.send_enter();
                invalidate_window(hwnd);
            }
        });
    } else if ai_panel_active {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                let settings = state.borrow().app_settings.ai.clone();
                let mode = state.borrow().ai_panel.mode;
                let attachments = state.borrow().ai_panel.attachments.clone();
                let context = state.borrow().gather_context(&attachments);
                let _ = state
                    .borrow_mut()
                    .ai_panel
                    .send_message_with_prepared_context(&settings, context, mode);
                invalidate_window(hwnd);
            }
        });
    } else if find_active {
        okd_edit_return_find(hwnd);
    } else {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                let has_sel = has_selection(&state.borrow());
                if has_sel {
                    state.borrow_mut().delete_selection();
                }
                // P1-1: 多光标模式下广播换行到所有光标
                state.borrow_mut().broadcast_insert_newline();
                invalidate_window(hwnd);
            }
        });
    }
}

/// 查找面板激活时 VK_RETURN 的处理
unsafe fn okd_edit_return_find(hwnd: HWND) {
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
            invalidate_window(hwnd);
        }
    });
}

/// VK_BACK：终端/AI/查找/编辑器各自的退格处理
unsafe fn okd_edit_back(hwnd: HWND) {
    let terminal_active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().terminal_panel.focused)
            .unwrap_or(false)
    });
    let ai_panel_active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().ai_panel.input_focused)
            .unwrap_or(false)
    });
    let find_active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| {
                state.borrow().find_visible
                    && state.borrow().find_focus != crate::editor::FindReplaceFocus::None
            })
            .unwrap_or(false)
    });
    if terminal_active {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                let mut st = state.borrow_mut();
                if !st.terminal_panel.input_line.is_empty() {
                    st.terminal_panel.input_line.pop();
                    st.terminal_panel.cursor_pos = st.terminal_panel.cursor_pos.saturating_sub(1);
                }
                invalidate_window(hwnd);
            }
        });
    } else if ai_panel_active {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                state.borrow_mut().ai_panel.backspace();
                invalidate_window(hwnd);
            }
        });
    } else if find_active {
        okd_edit_back_find(hwnd);
    } else {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                let has_sel = has_selection(&state.borrow());
                if has_sel {
                    state.borrow_mut().delete_selection();
                } else {
                    // P1-1: 多光标模式下广播退格到所有光标
                    state.borrow_mut().broadcast_delete_char();
                }
                invalidate_window(hwnd);
            }
        });
    }
}

/// 查找面板激活时 VK_BACK 的处理
unsafe fn okd_edit_back_find(hwnd: HWND) {
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
            invalidate_window(hwnd);
        }
    });
}

/// VK_DELETE/F3/Escape：删除/查找下一个/关闭查找
unsafe fn okd_edit_delete_misc(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) {
    match vk {
        VK_DELETE => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let has_sel = has_selection(&state.borrow());
                    if has_sel {
                        state.borrow_mut().delete_selection();
                    } else {
                        state.borrow_mut().delete_forward();
                    }
                    invalidate_window(hwnd);
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
                    invalidate_window(hwnd);
                }
            });
        }
        VK_ESCAPE => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().close_find_replace();
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
}

/// VK_LEFT/VK_RIGHT：光标左右移动（含 Shift 选择）
unsafe fn okd_edit_left_right(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) {
    match vk {
        VK_LEFT => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let mut st = state.borrow_mut();
                    if shift {
                        if st.content.selection_start.is_none() {
                            st.start_selection();
                        }
                        st.move_cursor_left();
                        st.update_selection();
                    } else {
                        if st.content.selection_start.is_some() {
                            st.clear_selection();
                        }
                        st.move_cursor_left();
                    }
                    drop(st);
                    invalidate_window(hwnd);
                }
            });
        }
        VK_RIGHT => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let mut st = state.borrow_mut();
                    if shift {
                        if st.content.selection_start.is_none() {
                            st.start_selection();
                        }
                        st.move_cursor_right();
                        st.update_selection();
                    } else {
                        if st.content.selection_start.is_some() {
                            st.clear_selection();
                        }
                        st.move_cursor_right();
                    }
                    drop(st);
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
}

/// VK_UP/VK_DOWN：光标上下移动（含 Shift 选择）
unsafe fn okd_edit_up_down(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) {
    match vk {
        VK_UP => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let mut st = state.borrow_mut();
                    if shift {
                        if st.content.selection_start.is_none() {
                            st.start_selection();
                        }
                        st.move_cursor_up();
                        st.update_selection();
                    } else {
                        if st.content.selection_start.is_some() {
                            st.clear_selection();
                        }
                        st.move_cursor_up();
                    }
                    drop(st);
                    invalidate_window(hwnd);
                }
            });
        }
        VK_DOWN => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let mut st = state.borrow_mut();
                    if shift {
                        if st.content.selection_start.is_none() {
                            st.start_selection();
                        }
                        st.move_cursor_down();
                        st.update_selection();
                    } else {
                        if st.content.selection_start.is_some() {
                            st.clear_selection();
                        }
                        st.move_cursor_down();
                    }
                    drop(st);
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
}

/// VK_HOME/END/PRIOR/NEXT：行首末/文件首末/翻页
unsafe fn okd_edit_home_end_page(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) {
    match vk {
        VK_HOME => okd_edit_home(hwnd, shift),
        VK_END => okd_edit_end(hwnd, shift),
        VK_PRIOR => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let page = state.borrow().window_height as f32 - 24.0;
                    state.borrow_mut().scroll(-page);
                    invalidate_window(hwnd);
                }
            });
        }
        VK_NEXT => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let page = state.borrow().window_height as f32 - 24.0;
                    state.borrow_mut().scroll(page);
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
}

/// VK_HOME：Smart Home（已在首个非空白位置时跳到行首）
unsafe fn okd_edit_home(hwnd: HWND, shift: bool) {
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            let mut st = state.borrow_mut();
            // 计算当前行首个非空白位置，判断是否已在该位置
            let already_at_smart = st
                .content
                .buffer
                .get_line(st.content.cursor_line)
                .map(|text| {
                    let first_non_ws = text
                        .char_indices()
                        .skip_while(|(_, c)| c.is_whitespace())
                        .map(|(i, _)| i)
                        .next()
                        .unwrap_or(text.len());
                    st.content.cursor_col == first_non_ws
                })
                .unwrap_or(false);
            if shift {
                if st.content.selection_start.is_none() {
                    st.start_selection();
                }
                st.move_cursor_smart_home(already_at_smart);
                st.update_selection();
            } else {
                if st.content.selection_start.is_some() {
                    st.clear_selection();
                }
                st.move_cursor_smart_home(already_at_smart);
            }
            drop(st);
            invalidate_window(hwnd);
        }
    });
}

/// VK_END：行末（含 Shift 选择到行末）
unsafe fn okd_edit_end(hwnd: HWND, shift: bool) {
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            let mut st = state.borrow_mut();
            if shift {
                if st.content.selection_start.is_none() {
                    st.start_selection();
                }
                st.move_cursor_end();
                st.update_selection();
            } else {
                if st.content.selection_start.is_some() {
                    st.clear_selection();
                }
                st.move_cursor_end();
            }
            drop(st);
            invalidate_window(hwnd);
        }
    });
}

/// VK_TAB：查找面板焦点切换/编辑器 Tab 键
unsafe fn okd_edit_tab(hwnd: HWND) {
    let find_active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| {
                state.borrow().find_visible
                    && state.borrow().find_focus != crate::editor::FindReplaceFocus::None
            })
            .unwrap_or(false)
    });
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
                invalidate_window(hwnd);
            }
        });
    } else {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                // P3.3: 若有内联补全建议，Tab 接受建议；否则插入制表符
                let accepted = {
                    let mut st = state.borrow_mut();
                    st.accept_inline_completion()
                };
                if accepted {
                    invalidate_window(hwnd);
                } else {
                    let has_sel = has_selection(&state.borrow());
                    if has_sel {
                        state.borrow_mut().delete_selection();
                    }
                    state.borrow_mut().insert_tab();
                    invalidate_window(hwnd);
                }
            }
        });
    }
}
