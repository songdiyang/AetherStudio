//! 杂项窗口消息（WM_*）处理函数。
//!
//! 从 `window.rs` 拆分而来，保持原有逻辑不变。

use std::path::PathBuf;

use aether_lsp::client::LspEvent;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, ScreenToClient, PAINTSTRUCT};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{
    compute_cursor_for_pos, create_editor_window, get_and_set_state, invalidate_window,
    AI_ARCHIVE_TIMER_ID, AI_TIMER_ID, CARET_TIMER_ID, EDITOR_STATE, HIGHLIGHT_TIMER_ID,
    HOVER_TIMER_ID, LP_THRESHOLD_MS, LP_TIMER_ID, TERM_TIMER_ID,
};
use crate::auto_save::{AUTOSAVE_DEBOUNCE_TIMER_ID, AUTOSAVE_PERIODIC_TIMER_ID};

/// WM_TIMER
pub(crate) unsafe fn on_timer(hwnd: HWND, _msg: u32, wparam: WPARAM, _lparam: LPARAM) -> LRESULT {
    if wparam.0 == HOVER_TIMER_ID {
        return on_timer_hover(hwnd);
    }
    if wparam.0 == TERM_TIMER_ID {
        return on_timer_term_refresh(hwnd);
    }
    if wparam.0 == CARET_TIMER_ID {
        return on_timer_caret(hwnd);
    }
    if wparam.0 == AI_TIMER_ID {
        return on_timer_ai_refresh(hwnd);
    }
    if wparam.0 == HIGHLIGHT_TIMER_ID {
        return on_timer_highlight_refresh(hwnd);
    }
    if wparam.0 == LP_TIMER_ID {
        return on_timer_long_press(hwnd);
    }
    if wparam.0 == AUTOSAVE_DEBOUNCE_TIMER_ID {
        return on_timer_autosave_debounce(hwnd);
    }
    if wparam.0 == AUTOSAVE_PERIODIC_TIMER_ID {
        return on_timer_autosave_periodic(hwnd);
    }
    if wparam.0 == AI_ARCHIVE_TIMER_ID {
        return on_timer_ai_archive(hwnd);
    }
    LRESULT(0)
}

/// AI 温数据归档：周期检查空闲会话并异步归档进 MemoryStore（SQLite）
unsafe fn on_timer_ai_archive(hwnd: HWND) -> LRESULT {
    if let Some(state) = get_and_set_state(hwnd) {
        state.borrow_mut().ai_panel.trigger_warm_archive();
    }
    LRESULT(0)
}

/// P3.4: Hover tooltip 防抖定时器触发
unsafe fn on_timer_hover(hwnd: HWND) -> LRESULT {
    let _ = KillTimer(hwnd, HOVER_TIMER_ID);
    if let Some(state) = get_and_set_state(hwnd) {
        let mut st = state.borrow_mut();
        // 仅在仍有悬停目标时计算 tooltip
        if st.hover_file_node.is_some() || st.hover_remote_node.is_some() {
            if let Some(text) = st.compute_hover_tooltip_text() {
                // tooltip 定位：鼠标右下方，预留 16px 间距
                let tx = st.hover_last_mouse_x + 16.0;
                let ty = st.hover_last_mouse_y + 16.0;
                let max_w = 400.0;
                st.hover_tooltip = Some(crate::editor::HoverTooltip::new(text, tx, ty, max_w));
                drop(st);
                invalidate_window(hwnd);
            }
        }
    }
    LRESULT(0)
}

/// 终端刷新：周期性重绘以显示异步到达的 shell 输出。
unsafe fn on_timer_term_refresh(hwnd: HWND) -> LRESULT {
    // render() 内部会调用 flush_output 拉取子进程输出。
    // 底部终端面板不可见时自动停止定时器，避免空转。
    let still_visible = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.borrow().layout.bottom_panel_visible)
            .unwrap_or(false)
    });
    if !still_visible {
        let _ = KillTimer(hwnd, TERM_TIMER_ID);
    } else if get_and_set_state(hwnd).is_some() {
        invalidate_window(hwnd);
    }
    LRESULT(0)
}

/// AI 后台刷新：流式生成或测试连接期间周期性重绘，两者均结束后自动停止。
unsafe fn on_timer_ai_refresh(hwnd: HWND) -> LRESULT {
    let active = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| {
                let st = state.borrow();
                st.ai_panel.any_generating() || st.settings_panel.is_testing
            })
            .unwrap_or(false)
    });
    if active {
        invalidate_window(hwnd);
    } else {
        let _ = KillTimer(hwnd, AI_TIMER_ID);
    }
    LRESULT(0)
}

/// 语法高亮刷新：打开文件后周期性重绘，直到当前 buffer 版本的后台高亮结果到达并着色，
/// 随后自动停止。解决“文件打开后停留在无高亮纯文本、直到下一次无关事件才着色”的卡顿感。
unsafe fn on_timer_highlight_refresh(hwnd: HWND) -> LRESULT {
    // done == true 表示：当前 buffer 版本的高亮请求已发出且结果已被消费。
    let done = EDITOR_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| {
                let st = state.borrow();
                st.hl_request_version == st.content.buffer_version
                    && !st.bg_highlighter.has_pending()
            })
            .unwrap_or(true)
    });
    if done {
        let _ = KillTimer(hwnd, HIGHLIGHT_TIMER_ID);
    } else {
        invalidate_window(hwnd);
    }
    LRESULT(0)
}

/// 新建项目对话框输入框光标闪烁
unsafe fn on_timer_caret(hwnd: HWND) -> LRESULT {
    if let Some(state) = get_and_set_state(hwnd) {
        let mut st = state.borrow_mut();
        let mut need_invalidate = false;
        let mut any_active = false;
        if st.new_project_dialog.visible {
            st.new_project_dialog.caret_visible = !st.new_project_dialog.caret_visible;
            need_invalidate = true;
            any_active = true;
        }
        if st.file_tree_input.is_some() {
            if let Some(input) = st.file_tree_input.as_mut() {
                input.caret_visible = !input.caret_visible;
            }
            need_invalidate = true;
            any_active = true;
        }
        // AI 助手输入框光标闪烁（右侧面板）
        if st.ai_panel.input_focused {
            st.ai_panel.caret_visible = !st.ai_panel.caret_visible;
            let rp = st.layout.right_panel_region().clone();
            st.dirty_tracker.mark_region(
                rp.x,
                rp.y,
                rp.width,
                rp.height,
                crate::dirty_rect::DirtyRegionType::RightPanel,
            );
            need_invalidate = true;
            any_active = true;
        }
        // 无任何活跃输入时停止定时器，避免空转
        if !any_active {
            let _ = KillTimer(hwnd, CARET_TIMER_ID);
        }
        if need_invalidate {
            let region = st.layout.sidebar_region().clone();
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
    }
    LRESULT(0)
}

/// 长按检测定时器触发
unsafe fn on_timer_long_press(hwnd: HWND) -> LRESULT {
    let _ = KillTimer(hwnd, LP_TIMER_ID);
    if let Some(state) = get_and_set_state(hwnd) {
        let mut st = state.borrow_mut();
        if st.lbutton_down {
            if let Some(target) = st.lpress_target {
                // 检查按下时间是否达到长按阈值
                if let Some(start) = st.lpress_start {
                    if start.elapsed() >= std::time::Duration::from_millis(LP_THRESHOLD_MS as u64) {
                        let idx = st.lpress_index;
                        match target {
                            crate::input::PressTarget::ActivityBar => {
                                st.activity_bar.begin_drag(idx);
                                st.status_message =
                                    "活动栏自定义模式（拖拽排序，Esc 退出）".to_string();
                            }
                            crate::input::PressTarget::MenuBar => {
                                st.menu_bar.begin_drag(idx);
                                st.status_message =
                                    "菜单栏自定义模式（拖拽排序，Esc 退出）".to_string();
                            }
                        }
                        st.lpress_start = None;
                        drop(st);
                        invalidate_window(hwnd);
                        return LRESULT(0);
                    }
                }
            }
        }
    }
    LRESULT(0)
}

/// 自动保存：空闲防抖定时器触发
unsafe fn on_timer_autosave_debounce(hwnd: HWND) -> LRESULT {
    if let Some(state) = get_and_set_state(hwnd) {
        state.borrow_mut().on_autosave_debounce_timer();
        invalidate_window(hwnd);
    }
    LRESULT(0)
}

/// 自动保存：周期兜底定时器触发
unsafe fn on_timer_autosave_periodic(hwnd: HWND) -> LRESULT {
    if let Some(state) = get_and_set_state(hwnd) {
        state.borrow_mut().on_autosave_periodic_timer();
        invalidate_window(hwnd);
    }
    LRESULT(0)
}
pub(crate) unsafe fn on_wm_app_2(
    hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    // 新建窗口请求
    let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
    create_editor_window(instance.into(), Some(hwnd));
    LRESULT(0)
}

/// msg if msg == WM_APP + 3
/// LSP 事件转发：tokio task 通过 PostMessageW 将 LspEvent 投递到 UI 线程。
/// 由 EditorState::new() 中 spawn 的事件 forwarder task 发送。
/// 处理诊断更新、补全结果、悬停结果等 LSP 事件。
pub(crate) unsafe fn on_wm_app_3(
    _hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let raw = lparam.0 as usize;
    // H-09: 立即重建 Box 确保 Rust drop 语义保证清理，即使后续处理 panic 也不会内存泄漏
    let _event_guard = unsafe { Box::from_raw(raw as *mut LspEvent) };
    let event: &LspEvent = &_event_guard;
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            state.borrow_mut().handle_lsp_event(event.clone());
        }
    });
    // REQ-P1-07: 不直接调用 render()，触发 WM_PAINT 统一渲染，避免双重渲染
    invalidate_window(_hwnd);
    LRESULT(0)
}

/// msg if msg == WM_APP + 7
pub(crate) unsafe fn on_wm_app_7(
    _hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // 文件夹异步扫描批次完成
    let raw = lparam.0 as usize;
    // H-09: 立即重建 Box 确保 Rust drop 语义保证清理，即使 EDITOR_STATE 为 None
    // 或 on_folder_scan_batch_ref panic 也不会内存泄漏
    let _batch_guard = unsafe { Box::from_raw(raw as *mut crate::editor::ScannedBatch) };
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            // 由 window 层负责重建 Box 并持有 _batch_guard；向 editor 传引用，
            // 避免 editor 内部再次 from_raw 同一块内存造成 double-free。
            state.borrow_mut().on_folder_scan_batch_ref(&_batch_guard);
        }
    });
    // REQ-P1-07: 不直接调用 render()，触发 WM_PAINT 统一渲染，避免双重渲染
    invalidate_window(_hwnd);
    LRESULT(0)
}

/// msg if msg == WM_APP + 4
pub(crate) unsafe fn on_wm_app_4(
    hwnd: HWND,
    _msg: u32,
    wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    // C-09: SSH 异步连接完成
    let raw = wparam.0;
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            state.borrow_mut().on_ssh_connect_complete(raw);
            invalidate_window(hwnd);
        }
    });
    LRESULT(0)
}

/// msg if msg == WM_APP + 5
pub(crate) unsafe fn on_wm_app_5(
    hwnd: HWND,
    _msg: u32,
    wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    // C-09: Git 异步克隆完成
    let raw = wparam.0;
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            state.borrow_mut().on_git_clone_complete(raw);
            invalidate_window(hwnd);
        }
    });
    LRESULT(0)
}

/// msg if msg == WM_APP + 6
pub(crate) unsafe fn on_wm_app_6(
    hwnd: HWND,
    _msg: u32,
    wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    // P0-1: 远程子目录异步列目录完成
    let raw = wparam.0;
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            state.borrow_mut().on_ssh_list_dir_complete(raw);
            invalidate_window(hwnd);
        }
    });
    LRESULT(0)
}

/// WM_DROPFILES
pub(crate) unsafe fn on_dropfiles(
    hwnd: HWND,
    _msg: u32,
    wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::Shell::{DragFinish, DragQueryFileW, HDROP};
    let hdrop = HDROP(wparam.0 as *mut std::ffi::c_void);

    let file_count = DragQueryFileW(hdrop, u32::MAX, None);
    for i in 0..file_count {
        let path_len = DragQueryFileW(hdrop, i, None);
        if path_len == 0 {
            continue;
        }
        let mut path_buf = vec![0u16; (path_len + 1) as usize];
        let _ = DragQueryFileW(hdrop, i, Some(&mut path_buf));
        if let Ok(path_str) = String::from_utf16(&path_buf[..path_len as usize]) {
            let path = PathBuf::from(path_str);
            if path.is_dir() {
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().open_folder(path);
                        invalidate_window(hwnd);
                    }
                });
                break;
            } else {
                EDITOR_STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        state.borrow_mut().load_file(path);
                        invalidate_window(hwnd);
                    }
                });
            }
        }
    }
    DragFinish(hdrop);
    LRESULT(0)
}

/// WM_SIZE
pub(crate) unsafe fn on_size(hwnd: HWND, _msg: u32, wparam: WPARAM, _lparam: LPARAM) -> LRESULT {
    let mut client_rect = RECT::default();
    if GetClientRect(hwnd, &mut client_rect).is_ok() {
        let width = (client_rect.right - client_rect.left) as u32;
        let height = (client_rect.bottom - client_rect.top) as u32;
        let is_max = wparam.0 == SIZE_MAXIMIZED as usize;
        let is_min = wparam.0 == SIZE_MINIMIZED as usize;
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                let mut st = state.borrow_mut();
                st.is_maximized = is_max;
                if !is_min {
                    st.resize(width, height);
                }
                drop(st);
                if !is_min {
                    invalidate_window(hwnd);
                }
            }
        });
    }
    LRESULT(0)
}

/// WM_DPICHANGED
pub(crate) unsafe fn on_dpichanged(
    hwnd: HWND,
    _msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let new_dpi = (wparam.0 & 0xFFFF) as f32;
    let new_scale = new_dpi / 96.0;

    if lparam.0 != 0 {
        let suggested_rect: *const RECT = lparam.0 as *const RECT;
        let rect = &*suggested_rect;
        let _ = SetWindowPos(
            hwnd,
            None,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }

    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            let mut st = state.borrow_mut();
            st.dpi_scale = new_scale;
            st.render_ctx.set_dpi(new_dpi);
            st.text_renderer.set_dpi_scale(new_scale);
            // REQ-P2-07: DPI 变化时重新缩放布局常量
            st.layout.apply_dpi_scale(new_scale);
            // REQ-P2-04: IME 候选/合成窗口尺寸按新 DPI 缩放
            st.ime.set_dpi_scale(new_scale);
            st.status_message =
                format!("DPI: {} ({}%)", new_dpi as u32, (new_scale * 100.0) as u32);
            // UI-M09: DPI 切换后重建渲染目标，确保尺寸与新 DPI 匹配
            let _ = st.init_render_target();
            // P3-1: DPI 切换后必须重建 text_format_cache 与 brush_cache，
            // 否则缓存的 IDWriteTextFormat 仍使用旧 font_size，导致渲染尺寸不一致
            // Theme 已 derive(Copy)，按值拷贝避免借用冲突
            let theme = st.theme;
            let font_size = st.text_renderer.font_size();
            st.render_ctx.init_common_resources(&theme, font_size);
            drop(st);
            invalidate_window(hwnd);
        }
    });
    LRESULT(0)
}

/// WM_NCACTIVATE
pub(crate) unsafe fn on_ncactivate(
    _hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    // 阻止系统绘制非激活状态的边框（白色边框）
    // 返回 TRUE 表示已处理，不绘制系统默认的 NC 激活指示器
    LRESULT(1)
}

/// WM_NCCALCSIZE
pub(crate) unsafe fn on_nccalcsize(
    _hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    // 移除系统非客户区边框，避免白色边框线
    // 返回 0 表示客户区覆盖整个窗口，不绘制系统边框
    LRESULT(0)
}

/// WM_NCHITTEST
pub(crate) unsafe fn on_nchittest(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // 自定义命中测试，实现无边框窗口的调整大小和拖动
    let x = ((lparam.0 & 0xFFFF) as i16) as i32;
    let y = (((lparam.0 >> 16) & 0xFFFF) as i16) as i32;
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_ok() {
        // UI-H05: 根据 DPI 缩放边框大小，确保高 DPI 下可点击区域足够大
        use windows::Win32::UI::HiDpi::GetDpiForWindow;
        let dpi = GetDpiForWindow(hwnd);
        let scale = dpi as f32 / 96.0;
        let border_size = (8.0 * scale) as i32;
        let left = x - rect.left;
        let top = y - rect.top;
        let right = rect.right - x;
        let bottom = rect.bottom - y;

        let mut result = HTCLIENT;
        if top < border_size {
            if left < border_size {
                result = HTTOPLEFT;
            } else if right < border_size {
                result = HTTOPRIGHT;
            } else {
                result = HTTOP;
            }
        } else if bottom < border_size {
            if left < border_size {
                result = HTBOTTOMLEFT;
            } else if right < border_size {
                result = HTBOTTOMRIGHT;
            } else {
                result = HTBOTTOM;
            }
        } else if left < border_size {
            result = HTLEFT;
        } else if right < border_size {
            result = HTRIGHT;
        } else {
            // 标题栏区域全部返回 HTCLIENT，由 WM_LBUTTONDOWN 统一处理菜单/按钮点击和拖动
            // 不返回 HTCAPTION/HTCLOSE 等系统码，因为 WS_POPUP 窗口系统不会正确处理它们
        }
        return LRESULT(result as isize);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// WM_ERASEBKGND
pub(crate) unsafe fn on_erasebkgnd(
    _hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    // 阻止系统擦除背景，避免白色闪烁
    LRESULT(1)
}

/// WM_PAINT
pub(crate) unsafe fn on_paint(hwnd: HWND, _msg: u32, _wparam: WPARAM, _lparam: LPARAM) -> LRESULT {
    let mut ps = PAINTSTRUCT::default();
    let _hdc = BeginPaint(hwnd, &mut ps);

    // REQ-P1-??: Windows 触发 WM_PAINT（如 InvalidateRect）时，若内部脏区追踪为空，
    // 强制标记全窗口重绘。否则 render() 会跳过绘制，导致上一帧内容残留（重影）。
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            let mut st = state.borrow_mut();
            if !st.dirty_tracker.has_dirty() {
                st.dirty_tracker.mark_full_window();
            }
        }
    });

    // C-04: 渲染路径中存在 40+ 处 D2D 资源创建 .unwrap()，设备丢失（GPU 驱动崩溃、
    // 显示模式切换）时会 panic。此处统一 catch_unwind 记录诊断并优雅跳过本次绘制，
    // 避免逐个替换 unwrap 的同时也保证 panic 不传播。
    let render_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        EDITOR_STATE.with(|s| {
            if let Some(state) = s.borrow().as_ref() {
                state.borrow_mut().render();
            }
        });
    }));
    if let Err(payload) = render_result {
        let msg = payload
            .downcast_ref::<&'static str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("<non-string panic>");
        eprintln!("[C-04] render panic recovered (D2D device loss?): {}", msg);
    }
    let _ = EndPaint(hwnd, &ps);
    LRESULT(0)
}

/// REQ-P0-05: WM_SETFOCUS — 窗口获得焦点
///
/// 同 `on_kill_focus`，使用 `try_borrow_mut()` 防止模态对话框消息循环
/// 重入时 panic。
pub(crate) unsafe fn on_set_focus(
    hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    get_and_set_state(hwnd);
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            if let Ok(mut st) = state.try_borrow_mut() {
                st.focus_manager.on_set_focus();
            }
        }
    });
    LRESULT(0)
}

/// REQ-P0-05: WM_KILLFOCUS — 窗口失去焦点
///
/// 使用 `try_borrow_mut()` 而非 `borrow_mut()`：当用户关闭标签页时，
/// `close_tab` → `close_current_tab_checked` 会弹出模态确认对话框，
/// 对话框的消息循环可能派发 `WM_KILLFOCUS`，此时 `EditorState` 已被
/// `close_tab` 的调用方持有 `borrow_mut()`，直接 `borrow_mut()` 会 panic。
/// `try_borrow_mut` 在此场景下优雅跳过，避免应用程序崩溃。
pub(crate) unsafe fn on_kill_focus(
    hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    get_and_set_state(hwnd);
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            if let Ok(mut st) = state.try_borrow_mut() {
                st.focus_manager.on_kill_focus();
                // 自动保存：失焦立即保存（用户离开编辑场景的瞬间落盘）
                st.autosave_on_focus_loss();
            } else {
                tracing::debug!("on_kill_focus: EditorState 已被借用（可能是模态对话框），跳过");
            }
        }
    });
    invalidate_window(hwnd);
    LRESULT(0)
}

/// WM_SETCURSOR
///
/// 根据鼠标所在 UI 区域设置对应的光标类型（Arrow/IBeam/Hand/SizeWE/SizeNS）。
/// 仅在客户区（HTCLIENT）内拦截；非客户区（resize 边框等）交由 DefWindowProcW 处理。
pub(crate) unsafe fn on_setcursor(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // lparam 低字 = 命中测试码，高字 = 鼠标消息 ID
    let hit_test = (lparam.0 & 0xFFFF) as u16 as i32;
    // 非客户区（resize 边框、标题栏系统区等）→ 交由系统处理
    if hit_test != HTCLIENT as i32 {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    // 获取鼠标位置（屏幕坐标，物理像素），转换为客户端坐标
    // GetMessagePos 返回 DWORD：低字 = x，高字 = y（屏幕坐标）
    let pos = GetMessagePos();
    let mut client_pt = POINT {
        x: (pos & 0xFFFF) as i16 as i32,
        y: ((pos >> 16) & 0xFFFF) as i16 as i32,
    };
    let _ = ScreenToClient(hwnd, &mut client_pt);
    // 计算光标类型并设置
    let cursor = compute_cursor_for_pos(hwnd, client_pt.x, client_pt.y);
    if let Ok(hcursor) = LoadCursorW(None, cursor.idc_cursor()) {
        let _ = SetCursor(hcursor);
    }
    // 返回 TRUE 阻止默认处理
    LRESULT(1)
}

/// _
pub(crate) unsafe fn on_default(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
