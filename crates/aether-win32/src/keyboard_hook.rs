//! 低层键盘钩子 (WH_KEYBOARD_LL) - 系统级拦截 Backspace 等按键
//!
//! # 设计目标
//!
//! 解决"任意语言 IME 拦截 Backspace 导致终端里的汉字无法删除"问题。
//!
//! ## 为什么需要这个模块
//!
//! Windows IME（包括中文微软拼音、日文 Microsoft IME、韩文等）即使处于
//! "开启但未合成"状态也会在 IMM32 钩子层系统级拦截 Backspace，
//! 导致 `WM_KEYDOWN` 根本到不了我们的窗口过程。无论怎么改 `ImmSetOpenStatus`、
//! `ImmAssociateContext`、`DefWindowProcW` 都没用——消息在到达窗口前就被吃了。
//!
//! ## 解决方案
//!
//! 用 `SetWindowsHookExW(WH_KEYBOARD_LL, ...)` 安装低层键盘钩子，在所有
//! IME 钩子链之前看到按键。我们只过滤 Backspace 等"应直达终端"的键，
//! 用 `LRESULT(1)` 抑制原事件并 `PostMessageW` 自定义消息到主窗口，
//! 主窗口再把 `\x7f` 字节送进 ConPTY 输入管道。
//!
//! ## 多语言通用
//!
//! 这个方案对所有 IME 一视同仁：中文/日文/韩文/印地/泰文……
//! 终端里输入的字符都能正常删除。

use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_BACK, VK_DELETE, VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN, VIRTUAL_KEY};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_APP,
};

/// 自定义消息：主窗口收到后向终端发送一个字节序列（lparam 携带字节指针）
pub const WM_TERMINAL_INPUT_BYTES: u32 = WM_APP + 20;
/// 自定义消息：主窗口收到后向终端发送一个 Backspace (`\x7f`)
pub const WM_TERMINAL_BACKSPACE: u32 = WM_APP + 21;
/// 自定义消息：主窗口收到后向终端发送 Delete 转义序列 (`\x1b[3~`)
pub const WM_TERMINAL_DELETE: u32 = WM_APP + 22;
/// 自定义消息：主窗口收到后向终端发送方向键 ANSI 序列
/// wparam 编码方向: 0=Up, 1=Down, 2=Left, 3=Right
pub const WM_TERMINAL_ARROW: u32 = WM_APP + 23;

/// 主窗口 HWND（用于在钩子线程里 PostMessage）
static HOOK_HWND: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
/// 钩子句柄
static HOOK_HANDLE: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
/// 终端是否聚焦（由主线程在 focus 变化时更新，钩子线程读取）
pub static TERMINAL_FOCUSED_FLAG: AtomicBool = AtomicBool::new(false);
/// IME 是否处于合成期（true 时 Backspace/Delete/方向键应让 IME 处理而不是终端）
pub static IME_COMPOSING_FLAG: AtomicBool = AtomicBool::new(false);

/// 安装低层键盘钩子。`hwnd` 是接收自定义消息的目标窗口。
///
/// 返回是否安装成功。
///
/// # Safety
///
/// 必须在主消息循环线程上调用。`hwnd` 必须是有效窗口句柄（不需要是
/// 钩子线程自己的窗口，但必须能通过 PostMessageW 投递消息）。
///
/// # 实现说明：使用全局钩子（dwThreadId=0）
///
/// 微软文档对 `SetWindowsHookExW` 有一条关键限制：
/// "If the dwThreadId parameter is zero or specifies the identifier of a thread
///  created by a different process, the lpfn parameter must point to a hook
///  procedure in a DLL."
///
/// 也就是说 **dwThreadId = 0（全局钩子）** 时 lpfn **必须**在 DLL 中。
/// 但 `WH_KEYBOARD_LL` 例外 —— Windows 在 `kernel32!LoadLibraryW` 之前
/// 对 LL 钩子有特殊处理，允许钩子过程在 EXE 进程的主代码段中，只要
/// 显式传 `hMod = GetModuleHandleW(None)`。
///
/// 反过来，`dwThreadId = current_thread` 会报 `0x80070595`
/// (ERROR_HOOK_TYPE_NOT_ALLOWED "此挂接程序只可整体设置")，所以 LL 钩子
/// **必须**是全局的（dwThreadId=0）但 hMod 必须是当前进程。
pub unsafe fn install(hwnd: HWND) -> bool {
    use windows::Win32::Foundation::HINSTANCE;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;

    HOOK_HWND.store(hwnd.0 as *mut _, Ordering::SeqCst);
    // 显式获取当前进程模块句柄，绕过 hMod 不能为 NULL 的限制
    // GetModuleHandleW 返回 HMODULE（InterfaceType），
    // SetWindowsHookExW 期望 HINSTANCE（CopyType），需显式 .into() 转换
    let hinstance: HINSTANCE = match GetModuleHandleW(None) {
        Ok(h) => h.into(),
        Err(e) => {
            tracing::error!(error = %e, "获取 GetModuleHandleW 失败");
            return false;
        }
    };
    let hook_result = SetWindowsHookExW(
        WH_KEYBOARD_LL,
        Some(low_level_keyboard_proc),
        hinstance, // 当前进程模块句柄；不能为 None
        0,         // 0 = 全局钩子，所有线程的键盘事件都会被我们拦截
    );
    let hook = match hook_result {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(
                hmod = ?hinstance.0,
                error = %e,
                "键盘钩子安装失败（SetWindowsHookExW 返回 Err）"
            );
            return false;
        }
    };
    if hook.0.is_null() {
        tracing::error!(hmod = ?hinstance.0, "键盘钩子安装失败（HHOOK 为 null）");
        return false;
    }
    HOOK_HANDLE.store(hook.0 as *mut _, Ordering::SeqCst);
    tracing::info!(
        hmod = ?hinstance.0,
        "键盘钩子安装成功（全局 LL 钩子 + 显式 hMod）"
    );
    true
}

/// 卸载低层键盘钩子
///
/// # Safety
///
/// 必须在主窗口的消息循环线程上调用，且 `install` 已经成功过。
/// 此函数会通过 `UnhookWindowsHookEx` 卸载全局安装的钩子；
/// 重复调用是安全的（no-op），但并发调用未定义。
pub unsafe fn uninstall() {
    let handle_ptr = HOOK_HANDLE.load(Ordering::SeqCst);
    if !handle_ptr.is_null() {
        let hook = HHOOK(handle_ptr);
        let _ = UnhookWindowsHookEx(hook);
        HOOK_HANDLE.store(std::ptr::null_mut(), Ordering::SeqCst);
    }
    HOOK_HWND.store(std::ptr::null_mut(), Ordering::SeqCst);
}

/// 主线程更新终端焦点状态（供钩子线程读取）
pub fn set_terminal_focused(focused: bool) {
    TERMINAL_FOCUSED_FLAG.store(focused, Ordering::SeqCst);
}

/// 主线程更新 IME 合成状态（供钩子线程读取）
pub fn set_ime_composing(composing: bool) {
    IME_COMPOSING_FLAG.store(composing, Ordering::SeqCst);
}

/// 低层键盘钩子回调：在系统级看到所有键盘事件。
/// 我们只关心 Backspace/Delete/方向键，且只在前台窗口是我们的窗口时拦截。
unsafe extern "system" fn low_level_keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    // n_code < 0 时必须直接 CallNextHookEx
    if n_code < 0 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    let hwnd_ptr = HOOK_HWND.load(Ordering::SeqCst);
    if hwnd_ptr.is_null() {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }
    let our_hwnd = HWND(hwnd_ptr as *mut _);

    // 只处理键盘按下事件（WM_KEYDOWN = 0x0100）
    if w_param.0 != 0x0100 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);

    // 跳过程序注入的按键（防止 SendInput 触发的"假"按键被错误处理）
    const LLKHF_INJECTED: u32 = 0x10;
    let injected = (kb.flags.0 & LLKHF_INJECTED) != 0;
    if injected {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    // 只在前台窗口是我们的窗口时拦截，避免影响其他应用
    let foreground = GetForegroundWindow();
    if foreground.0 != our_hwnd.0 {
        // 调试级别日志：默认关闭。需要排查时把环境变量 RUST_LOG=hook=debug 打开
        tracing::trace!(
            vk = kb.vkCode,
            our_hwnd = ?our_hwnd.0,
            foreground_hwnd = ?foreground.0,
            "键盘钩子: 前台窗口非本窗口，放行"
        );
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    let terminal_focused = TERMINAL_FOCUSED_FLAG.load(Ordering::SeqCst);
    let ime_composing = IME_COMPOSING_FLAG.load(Ordering::SeqCst);

    if !terminal_focused || ime_composing {
        tracing::trace!(
            vk = kb.vkCode,
            terminal_focused,
            ime_composing,
            "键盘钩子: 终端未聚焦或 IME 合成中，放行"
        );
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    // 终端聚焦 + 未合成：拦截编辑键，路由到 ConPTY
    enum SuppressAction {
        Backspace,
        Delete,
        Arrow(u32),
    }
    let action = match virtual_key_from_u16(kb.vkCode as u16) {
        VK_BACK => Some(SuppressAction::Backspace),
        VK_DELETE => Some(SuppressAction::Delete),
        VK_UP => Some(SuppressAction::Arrow(0)),
        VK_DOWN => Some(SuppressAction::Arrow(1)),
        VK_LEFT => Some(SuppressAction::Arrow(2)),
        VK_RIGHT => Some(SuppressAction::Arrow(3)),
        _ => None,
    };

    if let Some(act) = action {
        match act {
            SuppressAction::Backspace => {
                tracing::debug!("键盘钩子: 拦截 Backspace，路由到终端");
                let _ = PostMessageW(our_hwnd, WM_TERMINAL_BACKSPACE, WPARAM(0), LPARAM(0));
            }
            SuppressAction::Delete => {
                tracing::debug!("键盘钩子: 拦截 Delete，路由到终端");
                let _ = PostMessageW(our_hwnd, WM_TERMINAL_DELETE, WPARAM(0), LPARAM(0));
            }
            SuppressAction::Arrow(dir) => {
                tracing::debug!(dir, "键盘钩子: 拦截方向键，路由到终端");
                let _ = PostMessageW(our_hwnd, WM_TERMINAL_ARROW, WPARAM(dir as usize), LPARAM(0));
            }
        }
        // 抑制原事件：返回 1 表示"已处理，不要传递给下一个钩子或目标窗口"
        return LRESULT(1);
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}

/// 将 u16 转换为 VIRTUAL_KEY 枚举（用于 match）。`VK_BACK` 是 `VIRTUAL_KEY(0x08)` 等
#[inline]
#[allow(non_snake_case)]
fn virtual_key_from_u16(v: u16) -> VIRTUAL_KEY {
    VIRTUAL_KEY(v)
}

// ===== 主窗口消息处理 =====

/// WM_TERMINAL_BACKSPACE 处理器：发送 `\x7f` 到 ConPTY
///
/// # Safety
///
/// 必须在主窗口的窗口过程线程中调用。读取 EDITOR_STATE 时假定其已被
/// `get_and_set_state` 同步为当前窗口状态。
pub unsafe fn handle_backspace_msg(hwnd: HWND) -> LRESULT {
    send_to_terminal(hwnd, b"\x7f");
    LRESULT(0)
}

/// WM_TERMINAL_DELETE 处理器：发送 Delete 转义序列到 ConPTY
///
/// # Safety
///
/// 必须在主窗口的窗口过程线程中调用。读取 EDITOR_STATE 时假定其已被
/// `get_and_set_state` 同步为当前窗口状态。
pub unsafe fn handle_delete_msg(hwnd: HWND) -> LRESULT {
    send_to_terminal(hwnd, b"\x1b[3~");
    LRESULT(0)
}

/// WM_TERMINAL_ARROW 处理器：wparam 编码方向: 0=Up, 1=Down, 2=Left, 3=Right
///
/// # Safety
///
/// 必须在主窗口的窗口过程线程中调用。读取 EDITOR_STATE 时假定其已被
/// `get_and_set_state` 同步为当前窗口状态。`dir` 必须是 0/1/2/3 之一，
/// 其他值会产生空序列。
pub unsafe fn handle_arrow_msg(hwnd: HWND, dir: u32) -> LRESULT {
    let seq: &[u8] = match dir {
        0 => b"\x1b[A", // Up
        1 => b"\x1b[B", // Down
        2 => b"\x1b[D", // Left
        3 => b"\x1b[C", // Right
        _ => b"",
    };
    if !seq.is_empty() {
        send_to_terminal(hwnd, seq);
    }
    LRESULT(0)
}

/// 从主线程向当前 ConPTY 发送字节。线程安全：通过 EDITOR_STATE 访问。
unsafe fn send_to_terminal(_hwnd: HWND, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    crate::window::EDITOR_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            let mut st = state.borrow_mut();
            if st.terminal_panel.focused && st.terminal_panel.running {
                st.terminal_panel.send_bytes(bytes);
                drop(st);
                crate::window::invalidate_window(_hwnd);
            }
        }
    });
}
