#![windows_subsystem = "windows"]

use aether_win32::launch::{
    acquire_single_instance, find_existing_window, send_to_existing_instance, LaunchArgs,
};
use aether_win32::window::run;

fn main() {
    let args = LaunchArgs::from_env();

    // 单实例控制：
    // - 如果已有实例运行，把启动参数发过去，本进程直接退出（或等待窗口关闭）
    // - 如果是第一个实例，继续初始化主窗口
    if !args.new_window && !acquire_single_instance() {
        if let Some(hwnd) = find_existing_window() {
            send_to_existing_instance(hwnd, &args);
            if args.wait {
                wait_for_window_close(hwnd);
            }
            std::process::exit(0);
        }
        // 找不到窗口时回退到继续启动（可能是窗口还没创建好）
    }

    run(args);
}

/// 轮询等待目标窗口关闭。
///
/// 用于 `--wait` 复用已有窗口时，让 CLI 进程保持到对应窗口关闭。
fn wait_for_window_close(hwnd: windows::Win32::Foundation::HWND) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::IsWindow;
        use windows::Win32::System::Threading::GetCurrentProcessId;

        // 避免自己等自己：如果 hwnd 属于当前进程则直接返回
        let mut own_pid = 0u32;
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut own_pid));
        if own_pid == GetCurrentProcessId() {
            return;
        }

        // 每 100ms 检查一次窗口是否仍然存在
        while IsWindow(hwnd).as_bool() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}
