//! ConPTY 集成烟雾测试
//!
//! 验证 `aether_win32::conpty::ConPtySession` 在真实 Windows 环境下：
//! 1. 能成功启动 `cmd.exe` 并保持运行
//! 2. 能读到子进程的初始输出（banner）
//! 3. 能接收用户输入并把回显文本写回管道
//!
//! 本测试取代了早期用于诊断 ConPTY 行为的 `test_conpty.cs` / `send_ctrl_grave.ps1`
//! 一次性脚本。相较 C# 版本，本测试直接调用生产代码（而非重复实现 Win32 绑定），
//! 可通过 `cargo test -p aether-win32 --test conpty_smoke` 反复运行。
//!
//! 注：ConPTY 与匿名管道两种后端在 cmd.exe 行为上略有差异（是否回显、
//! 是否包含 ANSI 控制序列），本测试对回显文本做子串匹配，两种后端均能通过。

use std::io::Read;
use std::sync::mpsc;
use std::time::Duration;

use aether_win32::conpty::{ConPtySession, PipeReader};

/// ConPTY/管道相关测试串行化：CreatePseudoConsole/CreatePipe 在并行运行时
/// 会互相干扰（同一进程内多对 ConPTY 同时存在时行为未定义），所以强制串行。
static CONPTY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 优先用 `%COMSPEC%`，否则回退到常见路径。
fn cmd_path() -> String {
    std::env::var("COMSPEC")
        .ok()
        .filter(|p| std::path::Path::new(p).exists())
        .unwrap_or_else(|| r"C:\Windows\System32\cmd.exe".to_string())
}

/// 在后台线程里持续读取管道直到 EOF，通道返回累计字节。
///
/// 之所以需要后台线程 + 通道 + 超时：
/// - `PipeReader::read` 是阻塞 `ReadFile`，主线程直接调会卡死
/// - 让子进程执行 `exit` 后管道自然关闭，read 循环收到 0 后退出
/// - 主线程用 `recv_timeout` 兜底，防止极端情况下永久挂起
fn drain_to_eof(mut reader: PipeReader, timeout: Duration) -> Vec<u8> {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut collected = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF：管道关闭，子进程退出
                Ok(n) => collected.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        let _ = tx.send(collected);
    });
    rx.recv_timeout(timeout).unwrap_or_default()
}

/// 烟雾测试 1：spawn 应成功，cmd.exe 至少 500ms 内不退出。
#[test]
fn conpty_smoke_spawns_cmd_and_stays_alive() {
    let _lock = CONPTY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (session, _read_handle) =
        ConPtySession::spawn(&cmd_path(), None, 80, 24).expect("spawn 失败");
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        session.is_alive(),
        "cmd.exe 在 500ms 内退出，启动方式可能有误"
    );
    let backend = if session.is_pipe() { "pipe" } else { "ConPTY" };
    println!(
        "✓ conpty_smoke[1]: cmd.exe 启动并保持运行 (backend={})",
        backend
    );
}

/// 烟雾测试 2：spawn 后应能读到 cmd.exe 的初始 banner 输出。
#[test]
fn conpty_smoke_reads_initial_banner() {
    let _lock = CONPTY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (session, read_handle) =
        ConPtySession::spawn(&cmd_path(), None, 80, 24).expect("spawn 失败");

    // 写一个无害命令 + exit，确保管道最终会关闭，drain_to_eof 能返回
    session
        .write_input(b"ver\r\nexit\r\n")
        .expect("write_input 失败");

    let bytes = drain_to_eof(PipeReader::new(read_handle), Duration::from_secs(5));
    assert!(
        !bytes.is_empty(),
        "应能读取到 cmd.exe 的输出，但读到 0 bytes"
    );
    // 抓一段 ASCII 可读文本作概览，方便排错
    let preview: String = bytes
        .iter()
        .take(160)
        .copied()
        .filter(|b| (0x20..=0x7e).contains(b) || *b == b'\n' || *b == b'\r')
        .map(|b| b as char)
        .collect();
    println!(
        "✓ conpty_smoke[2]: 读取到 {} bytes 初始输出，前 160 可打印字符: {:?}",
        bytes.len(),
        preview
    );
}

/// 烟雾测试 3：写入 echo 命令后，回显的标记字符串必须出现在输出中。
#[test]
fn conpty_smoke_round_trip_echo() {
    let _lock = CONPTY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (session, read_handle) =
        ConPtySession::spawn(&cmd_path(), None, 80, 24).expect("spawn 失败");

    // 用一个高熵且不可能自然出现在 banner 中的字符串作为标记
    let marker = "AETHER_CONPTY_PROBE_7E2F1A";
    let cmd = format!("echo {}\r\nexit\r\n", marker);
    session
        .write_input(cmd.as_bytes())
        .expect("write_input 失败");

    let bytes = drain_to_eof(PipeReader::new(read_handle), Duration::from_secs(5));
    let output = String::from_utf8_lossy(&bytes);
    assert!(
        output.contains(marker),
        "输出中应包含回显文本 '{}'，实际输出 ({} bytes):\n{}",
        marker,
        bytes.len(),
        output
    );
    println!(
        "✓ conpty_smoke[3]: 回显 '{}' 出现在 {} bytes 输出中",
        marker,
        bytes.len()
    );
}
