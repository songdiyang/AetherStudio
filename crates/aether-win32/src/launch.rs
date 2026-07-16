pub use aether_shared::launch::{GotoPosition, LaunchArgs};

use windows::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SendMessageW, WM_COPYDATA};

/// 单实例互斥体名称
const AETHER_MUTEX_NAME: &str = "AetherEditorSingleInstanceMutex_0_1_0";
/// 主窗口类名，与 window.rs 中的 CLASS_NAME 保持一致
const AETHER_CLASS_NAME: &str = "AetherEditor";

/// 尝试创建单实例互斥体。
///
/// 返回 `true` 表示当前是第一个实例；返回 `false` 表示已有实例在运行。
pub fn acquire_single_instance() -> bool {
    unsafe {
        use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
        use windows::Win32::System::Threading::CreateMutexW;

        let name: Vec<u16> = AETHER_MUTEX_NAME.encode_utf16().chain(Some(0)).collect();
        let handle = CreateMutexW(None, false, windows::core::PCWSTR(name.as_ptr()));

        if handle.is_err() {
            // 创建失败时保守处理：认为自己是唯一实例
            return true;
        }

        // CreateMutexW 成功但 GetLastError 为 ERROR_ALREADY_EXISTS 表示互斥体已存在
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// 查找已运行的主窗口句柄。
pub fn find_existing_window() -> Option<HWND> {
    unsafe {
        let class: Vec<u16> = AETHER_CLASS_NAME.encode_utf16().chain(Some(0)).collect();
        let Ok(hwnd) = FindWindowW(windows::core::PCWSTR(class.as_ptr()), None) else {
            return None;
        };

        if hwnd.0.is_null() {
            None
        } else {
            Some(hwnd)
        }
    }
}

/// 将启动参数发送给已运行的主窗口。
///
/// 通过 WM_COPYDATA 传递 JSON 序列化的 LaunchArgs。
/// 返回是否发送成功。
pub fn send_to_existing_instance(hwnd: HWND, args: &LaunchArgs) -> bool {
    unsafe {
        let json = serde_json::to_string(args).unwrap_or_default();
        let wide: Vec<u16> = json.encode_utf16().chain(Some(0)).collect();
        let data_len = (wide.len() * std::mem::size_of::<u16>()) as u32;

        let cds = COPYDATASTRUCT {
            dwData: 0,
            cbData: data_len,
            lpData: wide.as_ptr() as *mut core::ffi::c_void,
        };

        // WM_COPYDATA 会阻塞直到接收方处理完毕，适合传递路径这种需要同步确认的数据
        let _ = SendMessageW(
            hwnd,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&cds as *const _ as isize),
        );

        true
    }
}

/// 从 WM_COPYDATA 的 lparam 中还原 LaunchArgs
///
/// # Safety
///
/// 调用者必须保证 `lparam` 指向一个有效的 `COPYDATASTRUCT` 结构，
/// 且其中的 `lpData` 指向一段长度至少为 `cbData` 字节的有效内存。
pub unsafe fn parse_copydata_lparam(lparam: LPARAM) -> Option<LaunchArgs> {
    let cds = &*(lparam.0 as *const COPYDATASTRUCT);
    if cds.dwData != 0 || cds.cbData == 0 || cds.lpData.is_null() {
        return None;
    }

    let len = (cds.cbData as usize) / std::mem::size_of::<u16>();
    let slice = std::slice::from_raw_parts(cds.lpData as *const u16, len);

    // 找到终止的 null 或直接使用整个缓冲区
    let end = slice.iter().position(|&c| c == 0).unwrap_or(len);
    let json = String::from_utf16(&slice[..end]).ok()?;
    serde_json::from_str(&json).ok()
}

/// 处理 WM_COPYDATA 时返回给 SendMessageW 的值
pub fn copydata_result(handled: bool) -> LRESULT {
    if handled {
        LRESULT(1)
    } else {
        LRESULT(0)
    }
}
