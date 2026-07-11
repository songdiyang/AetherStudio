//! IME（输入法）相关窗口消息处理。
//!
//! 从 `window.rs` 拆分而来，保持原有逻辑不变。

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

use super::{get_and_set_state, invalidate_window, EDITOR_STATE};

/// WM_IME_STARTCOMPOSITION
pub(crate) unsafe fn on_ime_startcomposition(
    _hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    // P0-2: IME 开始合成。仅做位置初始化，IME 候选/合成窗口位置由
    // 渲染时 set_candidate_window_position 同步。返回 0 表示已处理。
    LRESULT(0)
}

/// WM_IME_COMPOSITION
pub(crate) unsafe fn on_ime_composition(
    hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // C-12: 键盘消息进入时先同步 thread_local 到当前窗口状态
    get_and_set_state(hwnd);
    let lparam_flags = lparam.0 as u32;
    const GCS_COMPSTR: u32 = 0x0008;
    const GCS_RESULTSTR: u32 = 0x0800;

    // 优先处理结果串（已提交文本）：将合成串清空，并插入提交字符
    if lparam_flags & GCS_RESULTSTR != 0 {
        let result = EDITOR_STATE.with(|s| {
            s.borrow()
                .as_ref()
                .and_then(|state| state.borrow().ime.get_result_string())
        });
        if let Some(text) = result {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().commit_composition(text);
                    invalidate_window(hwnd);
                }
            });
        }
        // 结果串提交后合成期已结束（虽然 WM_IME_ENDCOMPOSITION 可能稍后才到），
        // 提前重置标志让 Backspace 立即可达终端，避免"提交汉字后无法立即删除"的问题
        crate::keyboard_hook::set_ime_composing(false);
        // 结果串已包含完整提交，不再处理合成串
        return LRESULT(0);
    }

    // 处理合成串（pre-edit text）：仅更新显示，不修改 buffer
    if lparam_flags & GCS_COMPSTR != 0 {
        let comp = EDITOR_STATE.with(|s| {
            s.borrow()
                .as_ref()
                .and_then(|state| state.borrow().ime.get_composition_string())
        });
        if let Some(text) = comp {
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().set_composition(text);
                    invalidate_window(hwnd);
                }
            });
            // 通知低层钩子：进入合成期，Backspace/Delete 等交给 IME 处理
            crate::keyboard_hook::set_ime_composing(true);
        } else {
            // 合成串为空：IME 已清除合成状态
            EDITOR_STATE.with(|s| {
                if let Some(state) = s.borrow().as_ref() {
                    state.borrow_mut().clear_composition();
                    invalidate_window(hwnd);
                }
            });
            crate::keyboard_hook::set_ime_composing(false);
        }
        return LRESULT(0);
    }

    // 无 GCS 标志：IME 取消当前合成
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            state.borrow_mut().clear_composition();
            invalidate_window(hwnd);
        }
    });
    LRESULT(0)
}

/// WM_IME_ENDCOMPOSITION
pub(crate) unsafe fn on_ime_endcomposition(
    hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    // P0-2: IME 结束合成。清除合成串显示。
    EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            state.borrow_mut().clear_composition();
            // 终端聚焦时结束合成后立即关闭 IME，
            // 让用户能立即用 Backspace 删除终端内容
            let terminal_focused = state.borrow().terminal_panel.focused;
            if terminal_focused {
                state.borrow_mut().ime.set_ime_open(false);
            }
            invalidate_window(hwnd);
        }
    });
    // 通知低层钩子：合成期已结束，后续 Backspace 等可由钩子拦截到终端
    crate::keyboard_hook::set_ime_composing(false);
    LRESULT(0)
}

/// WM_IME_CHAR
pub(crate) unsafe fn on_ime_char(
    _hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    // P0-2: 阻止 TranslateMessage 从 WM_IME_CHAR 产生 WM_CHAR，
    // 避免中文输入字符被 WM_CHAR 重复插入。
    // 提交文本已通过 WM_IME_COMPOSITION + GCS_RESULTSTR 处理。
    LRESULT(0)
}
