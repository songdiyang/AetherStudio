//! WM_KEYDOWN Ctrl+ 快捷键处理。
//!
//! 从 `window.rs` 拆分而来，保持原有逻辑不变。
//! 按逻辑分组为多个辅助函数，每组 ≤ 80 行。

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::dialogs::Dialogs;

use super::super::{invalidate_window, EDITOR_STATE, TERM_REFRESH_MS, TERM_TIMER_ID};

/// Ctrl+ 快捷键总分发
pub(crate) unsafe fn okd_ctrl_dispatch(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) {
    okd_ctrl_file_ops(hwnd, vk, shift);
    okd_ctrl_view(hwnd, vk, shift);
    okd_ctrl_view_shortcuts(hwnd, vk, shift);
    okd_ctrl_zoom_cmd(hwnd, vk);
    okd_ctrl_clipboard(hwnd, vk, shift);
    okd_ctrl_find_undo(hwnd, vk, shift);
    okd_ctrl_tabs(hwnd, vk, shift);
    okd_ctrl_tab_nums(hwnd, vk);
    okd_ctrl_word_move(hwnd, vk, shift);
    okd_ctrl_file_nav(hwnd, vk);
    okd_ctrl_column(hwnd, vk, shift);
    okd_ctrl_terminal_clear(hwnd, vk);
}

/// Ctrl+L：终端聚焦时清屏（发送 Form Feed 给 shell）
unsafe fn okd_ctrl_terminal_clear(hwnd: HWND, vk: VIRTUAL_KEY) {
    if vk != VK_L {
        return;
    }
    let term_focused = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().terminal_panel.focused)
            .unwrap_or(false)
    });
    if term_focused {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                // 发送 Ctrl+L (0x0C Form Feed)，shell 会执行清屏并重新绘制提示符
                state.borrow_mut().terminal_panel.send_bytes(b"\x0c");
                invalidate_window(hwnd);
            }
        });
    }
}

/// Ctrl+O/K/S/N：文件/文件夹打开、保存、新建项目
unsafe fn okd_ctrl_file_ops(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) {
    match vk {
        VK_O => {
            if let Some(path) = Dialogs::open_file_dialog(hwnd, "打开文件", &[]) {
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().load_file(path);
                        invalidate_window(hwnd);
                    }
                });
            }
        }
        VK_K => {
            if let Some(path) = Dialogs::open_folder_dialog(hwnd, "打开文件夹") {
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().open_folder(path);
                        invalidate_window(hwnd);
                    }
                });
            }
        }
        VK_S => {
            if shift {
                if let Some(path) = Dialogs::save_file_dialog(hwnd, "另存为", "untitled.txt") {
                    EDITOR_STATE.with(|s| {
                        if let Some(state) = s.borrow().as_ref() {
                            state.borrow_mut().save_as(path);
                            invalidate_window(hwnd);
                        }
                    });
                }
            } else {
                let need_dialog = EDITOR_STATE.with(|s| {
                    s.borrow()
                        .as_ref()
                        .map(|state| state.borrow().content.file_path.is_none())
                        .unwrap_or(true)
                });
                if need_dialog {
                    if let Some(path) = Dialogs::save_file_dialog(hwnd, "保存文件", "untitled.txt")
                    {
                        EDITOR_STATE.with(|s| {
                            if let Some(state) = s.borrow().as_ref() {
                                state.borrow_mut().save_as(path);
                                invalidate_window(hwnd);
                            }
                        });
                    }
                } else {
                    EDITOR_STATE.with(|s| {
                        if let Some(state) = s.borrow().as_ref() {
                            state.borrow_mut().save_file();
                            invalidate_window(hwnd);
                        }
                    });
                }
            }
        }
        VK_N => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().new_project();
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
}

/// Ctrl+Space/B/P/`：补全、侧栏、命令面板、终端切换
unsafe fn okd_ctrl_view(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) {
    match vk {
        VK_SPACE => {
            // Phase H1: Ctrl+Space 触发 LSP 补全请求
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().request_completion();
                    // 不立即 render：补全结果到达后由 WM_APP+3 触发重绘
                }
            });
        }
        VK_B => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().layout.toggle_sidebar();
                    invalidate_window(hwnd);
                }
            });
        }
        VK_P => {
            if shift {
                // Task 13.6: Ctrl+Shift+P 打开命令面板并设为 > 前缀（VS Code 行为）
                // 原 Ctrl+Shift+G 的 > 前缀行为迁移至此
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().command_palette.show();
                        state.borrow_mut().command_palette.update_query(">");
                        invalidate_window(hwnd);
                    }
                });
            } else {
                // P2-3: Ctrl+P 也打开命令面板（VS Code 中为 Quick Open；此处复用命令面板）
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().command_palette.show();
                        invalidate_window(hwnd);
                    }
                });
            }
        }
        VK_OEM_3 => {
            // Ctrl+` 切换底部终端面板
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().layout.toggle_terminal_panel();
                    if state.borrow().layout.bottom_panel_visible {
                        // 打开时聚焦终端并按需启动 shell
                        state.borrow_mut().terminal_panel.focused = true;
                        state.borrow_mut().set_terminal_ime_bypass(true);
                        if !state.borrow().terminal_panel.running {
                            let _ = state.borrow_mut().terminal_panel.start();
                        }
                        // 启动周期刷新定时器以显示异步输出
                        let _ = SetTimer(hwnd, TERM_TIMER_ID, TERM_REFRESH_MS, None);
                    } else {
                        state.borrow_mut().terminal_panel.focused = false;
                        state.borrow_mut().set_terminal_ime_bypass(false);
                        // 关闭时停止刷新定时器
                        let _ = KillTimer(hwnd, TERM_TIMER_ID);
                    }
                    state.borrow_mut().status_message =
                        if state.borrow().layout.bottom_panel_visible {
                            "终端已打开 (Ctrl+` 关闭, Ctrl+C 中断)"
                        } else {
                            "终端已关闭"
                        }
                        .to_string();
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
}

/// Task 13: Ctrl+,/J/Shift+E：设置、底部面板、资源管理器视图
unsafe fn okd_ctrl_view_shortcuts(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) {
    match vk {
        // SubTask 13.1: Ctrl+, 打开设置面板（临时实现：显示状态消息并打开右侧面板）
        VK_OEM_COMMA => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let mut st = state.borrow_mut();
                    st.status_message = "设置面板（待实现）".to_string();
                    st.status_bar.update_status("设置面板（待实现）");
                    st.layout.right_panel_visible = true;
                    if st.layout.right_panel_width < 1.0 {
                        st.layout.right_panel_width = 320.0;
                    }
                    drop(st);
                    invalidate_window(hwnd);
                }
            });
        }
        // SubTask 13.2: Ctrl+J 切换底部面板（与 Ctrl+` 行为一致）
        VK_J => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().layout.toggle_terminal_panel();
                    if state.borrow().layout.bottom_panel_visible {
                        state.borrow_mut().terminal_panel.focused = true;
                        state.borrow_mut().set_terminal_ime_bypass(true);
                        if !state.borrow().terminal_panel.running {
                            let _ = state.borrow_mut().terminal_panel.start();
                        }
                        let _ = SetTimer(hwnd, TERM_TIMER_ID, TERM_REFRESH_MS, None);
                    } else {
                        state.borrow_mut().terminal_panel.focused = false;
                        state.borrow_mut().set_terminal_ime_bypass(false);
                        let _ = KillTimer(hwnd, TERM_TIMER_ID);
                    }
                    state.borrow_mut().status_message =
                        if state.borrow().layout.bottom_panel_visible {
                            "底部面板已打开 (Ctrl+J 关闭)"
                        } else {
                            "底部面板已关闭"
                        }
                        .to_string();
                    invalidate_window(hwnd);
                }
            });
        }
        // SubTask 13.5: Ctrl+Shift+E 切换到资源管理器视图
        VK_E if shift => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let mut st = state.borrow_mut();
                    st.activity_bar
                        .switch_to_view(crate::layout::ActivityBarView::Explorer);
                    st.activity_view = crate::layout::ActivityBarView::Explorer;
                    if !st.layout.sidebar_visible {
                        st.layout.toggle_sidebar();
                    }
                    st.sidebar_content =
                        crate::layout::SidebarContent::from_view(st.activity_view);
                    st.status_message = "已切换到资源管理器".to_string();
                    drop(st);
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
}

/// Ctrl+=/-/0/G：字体缩放、命令面板前缀
unsafe fn okd_ctrl_zoom_cmd(hwnd: HWND, vk: VIRTUAL_KEY) {
    match vk {
        VK_OEM_PLUS | VK_ADD => {
            // P2-3: Ctrl+= 放大字体
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().zoom_font(Some(1.0));
                    invalidate_window(hwnd);
                }
            });
        }
        VK_OEM_MINUS | VK_SUBTRACT => {
            // P2-3: Ctrl+- 缩小字体
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().zoom_font(Some(-1.0));
                    invalidate_window(hwnd);
                }
            });
        }
        VK_0 | VK_NUMPAD0 => {
            // P2-3: Ctrl+0 重置字体大小
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().zoom_font(None);
                    invalidate_window(hwnd);
                }
            });
        }
        VK_G => {
            if GetKeyState(VK_SHIFT.0 as i32) < 0 {
                // Task 13.6: Ctrl+Shift+G 切换到源代码管理视图
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        let mut st = state.borrow_mut();
                        st.activity_bar
                            .switch_to_view(crate::layout::ActivityBarView::SourceControl);
                        st.activity_view = crate::layout::ActivityBarView::SourceControl;
                        if !st.layout.sidebar_visible {
                            st.layout.toggle_sidebar();
                        }
                        st.sidebar_content =
                            crate::layout::SidebarContent::from_view(st.activity_view);
                        st.status_message = "已切换到源代码管理".to_string();
                        drop(st);
                        invalidate_window(hwnd);
                    }
                });
            } else {
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().command_palette.show();
                        state.borrow_mut().command_palette.update_query(":");
                        invalidate_window(hwnd);
                    }
                });
            }
        }
        _ => {}
    }
}

/// Ctrl+C/X/V/A：复制/剪切/粘贴/全选（含终端 Ctrl+C 和 AI 面板切换）
unsafe fn okd_ctrl_clipboard(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) {
    match vk {
        VK_C => {
            // 终端聚焦时 Ctrl+C 中断子进程；否则执行复制
            let term_focused = EDITOR_STATE.with(|s| {
                s.borrow()
                    .as_ref()
                    .map(|state| state.borrow().terminal_panel.focused)
                    .unwrap_or(false)
            });
            if term_focused {
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().terminal_panel.send_interrupt();
                        state.borrow_mut().status_message = "终端已中断 (Ctrl+C)".to_string();
                        invalidate_window(hwnd);
                    }
                });
            } else {
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().copy();
                        invalidate_window(hwnd);
                    }
                });
            }
        }
        VK_X => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().cut();
                    invalidate_window(hwnd);
                }
            });
        }
        VK_V => {
            // 终端聚焦时，从剪贴板粘贴到 ConPTY；否则粘贴到编辑器
            let term_focused = EDITOR_STATE.with(|s| {
                s.borrow()
                    .as_ref()
                    .map(|state| state.borrow().terminal_panel.focused)
                    .unwrap_or(false)
            });
            if term_focused {
                if let Some(text) = crate::editor::EditorState::get_clipboard_text() {
                    EDITOR_STATE.with(|s| {
                        if let Some(state) = s.borrow().as_ref() {
                            state.borrow_mut().terminal_panel.send_bytes(text.as_bytes());
                            invalidate_window(hwnd);
                        }
                    });
                }
            } else {
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().paste();
                        invalidate_window(hwnd);
                    }
                });
            }
        }
        VK_A => {
            if shift {
                // Ctrl+Shift+A 切换右侧 AI 面板
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        let mut st = state.borrow_mut();
                        st.layout.right_panel_visible = !st.layout.right_panel_visible;
                        if st.layout.right_panel_visible && st.layout.right_panel_width < 1.0 {
                            st.layout.right_panel_width = 320.0;
                        }
                        st.status_message = if st.layout.right_panel_visible {
                            "AI 面板已打开".to_string()
                        } else {
                            "AI 面板已关闭".to_string()
                        };
                        invalidate_window(hwnd);
                    }
                });
            } else {
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().select_all();
                        invalidate_window(hwnd);
                    }
                });
            }
        }
        _ => {}
    }
}

/// Ctrl+F/H/Z/Y：查找/替换/撤销/重做
unsafe fn okd_ctrl_find_undo(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) {
    match vk {
        VK_F => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let selected = state.borrow().get_selected_text();
                    if shift {
                        state.borrow_mut().toggle_replace();
                    } else {
                        state.borrow_mut().toggle_find();
                    }
                    // 如果有选中文本，自动填充到查找框
                    if let Some(text) = selected {
                        if !text.is_empty() && text.len() < 200 {
                            state.borrow_mut().find_query = text;
                            state.borrow_mut().find_all();
                        }
                    }
                    invalidate_window(hwnd);
                }
            });
        }
        VK_H => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().toggle_replace();
                    invalidate_window(hwnd);
                }
            });
        }
        VK_Z => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    if shift {
                        state.borrow_mut().redo();
                    } else {
                        state.borrow_mut().undo();
                    }
                    invalidate_window(hwnd);
                }
            });
        }
        VK_Y => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().redo();
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
}

/// Ctrl+Tab/W/F4：标签页切换/关闭
unsafe fn okd_ctrl_tabs(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) {
    match vk {
        VK_TAB => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    if shift {
                        state.borrow_mut().prev_tab();
                    } else {
                        state.borrow_mut().next_tab();
                    }
                    invalidate_window(hwnd);
                }
            });
        }
        VK_W | VK_F4 => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    // P2-8: 关闭前进行 dirty 检查
                    state.borrow_mut().close_current_tab_checked();
                    invalidate_window(hwnd);
                }
            });
        }
        // SubTask 13.3: Ctrl+Shift+T 恢复最后关闭的标签
        VK_T if shift => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().reopen_last_closed_tab();
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
}

/// Ctrl+1-9：跳转到指定标签页
unsafe fn okd_ctrl_tab_nums(hwnd: HWND, vk: VIRTUAL_KEY) {
    match vk {
        VK_1 | VK_NUMPAD1 => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().goto_tab(1);
                    invalidate_window(hwnd);
                }
            });
        }
        VK_2 | VK_NUMPAD2 => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().goto_tab(2);
                    invalidate_window(hwnd);
                }
            });
        }
        VK_3 | VK_NUMPAD3 => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().goto_tab(3);
                    invalidate_window(hwnd);
                }
            });
        }
        VK_4 | VK_NUMPAD4 => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().goto_tab(4);
                    invalidate_window(hwnd);
                }
            });
        }
        VK_5 | VK_NUMPAD5 => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().goto_tab(5);
                    invalidate_window(hwnd);
                }
            });
        }
        VK_6 | VK_NUMPAD6 => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().goto_tab(6);
                    invalidate_window(hwnd);
                }
            });
        }
        VK_7 | VK_NUMPAD7 => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().goto_tab(7);
                    invalidate_window(hwnd);
                }
            });
        }
        VK_8 | VK_NUMPAD8 => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().goto_tab(8);
                    invalidate_window(hwnd);
                }
            });
        }
        VK_9 | VK_NUMPAD9 => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let last = state.borrow().tab_count();
                    state.borrow_mut().goto_tab(last);
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
}

/// Ctrl+Left/Right：词级移动（含 Shift 选择）
unsafe fn okd_ctrl_word_move(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) {
    match vk {
        // P1-6: Ctrl+Left / Ctrl+Right 词级移动
        VK_LEFT => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    let mut st = state.borrow_mut();
                    if shift {
                        if st.content.selection_start.is_none() {
                            st.start_selection();
                        }
                        st.move_cursor_word_left();
                        st.update_selection();
                    } else {
                        if st.content.selection_start.is_some() {
                            st.clear_selection();
                        }
                        st.move_cursor_word_left();
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
                        st.move_cursor_word_right();
                        st.update_selection();
                    } else {
                        if st.content.selection_start.is_some() {
                            st.clear_selection();
                        }
                        st.move_cursor_word_right();
                    }
                    drop(st);
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
}

/// Ctrl+Home/End/D/OEM_2：文件首末/添加光标/行注释
unsafe fn okd_ctrl_file_nav(hwnd: HWND, vk: VIRTUAL_KEY) {
    match vk {
        // P1-6: Ctrl+Home / Ctrl+End 文件首末
        VK_HOME => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().move_cursor_file_start();
                    invalidate_window(hwnd);
                }
            });
        }
        VK_END => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().move_cursor_file_end();
                    invalidate_window(hwnd);
                }
            });
        }
        // P1-6: Ctrl+D 添加下一个相同单词光标
        VK_D => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().add_cursor_at_next_occurrence();
                    invalidate_window(hwnd);
                }
            });
        }
        // P1-6: Ctrl+/ 切换行注释（OEM_2 为 / 键，需配合 Shift 实际生成 /，但 Ctrl+/ 是约定）
        VK_OEM_2 => {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().toggle_line_comment();
                    invalidate_window(hwnd);
                }
            });
        }
        _ => {}
    }
}

/// Ctrl+I/Up/Down：内联补全/列光标
unsafe fn okd_ctrl_column(hwnd: HWND, vk: VIRTUAL_KEY, shift: bool) {
    match vk {
        // P3.3: Ctrl+Shift+I 手动触发内联补全（占位 AI）
        VK_I => {
            if shift {
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().request_inline_completion();
                        invalidate_window(hwnd);
                    }
                });
            }
        }
        // P1-6: Ctrl+Alt+Up / Ctrl+Alt+Down 列光标
        VK_UP => {
            let alt = GetKeyState(VK_MENU.0 as i32) < 0;
            if alt {
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().add_cursor_line_above();
                        invalidate_window(hwnd);
                    }
                });
            }
        }
        VK_DOWN => {
            let alt = GetKeyState(VK_MENU.0 as i32) < 0;
            if alt {
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().add_cursor_line_below();
                        invalidate_window(hwnd);
                    }
                });
            }
        }
        _ => {}
    }
}
