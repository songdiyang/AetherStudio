#![allow(clippy::items_after_test_module)]

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

/// 终端启动结果（由后台线程产生，传回主线程）
type TerminalStartupResult = Result<
    (
        Arc<Mutex<std::process::ChildStdin>>,
        std::process::Child,
        mpsc::Receiver<String>,
    ),
    String,
>;

/// 终端面板状态
/// 使用 std::process 实现跨平台终端模拟
pub struct TerminalPanel {
    /// 是否可见
    pub visible: bool,
    /// 面板高度（像素）
    pub height: f32,
    /// 终端输出行缓存
    pub output_lines: VecDeque<String>,
    /// 最大缓存行数
    pub max_lines: usize,
    /// 当前输入行
    pub input_line: String,
    /// 光标在行中的位置
    pub cursor_pos: usize,
    /// 子进程stdin（用于发送输入）
    child_stdin: Option<Arc<Mutex<std::process::ChildStdin>>>,
    /// 子进程句柄（用于终止进程）
    child_process: Option<std::process::Child>,
    /// 输出接收器（从读取线程接收终端输出）
    output_receiver: Option<mpsc::Receiver<String>>,
    /// 启动结果接收器：后台线程启动 shell 后通过此通道返回结果
    startup_receiver: Option<mpsc::Receiver<TerminalStartupResult>>,
    /// 是否运行中（包含正在启动的状态）
    pub running: bool,
    /// 工作目录
    pub cwd: String,
    /// 是否聚焦
    pub focused: bool,
    /// 输出滚动偏移（从底部算起的行数，0 表示贴底显示最新输出）
    pub scroll_offset: usize,
}

impl TerminalPanel {
    pub fn new() -> Self {
        Self {
            visible: false,
            height: 200.0,
            output_lines: VecDeque::with_capacity(1000),
            max_lines: 1000,
            input_line: String::new(),
            cursor_pos: 0,
            child_stdin: None,
            child_process: None,
            output_receiver: None,
            startup_receiver: None,
            running: false,
            cwd: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            focused: false,
            scroll_offset: 0,
        }
    }

    /// 显示/隐藏终端面板
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// 启动终端会话（异步，不阻塞 UI 线程）
    pub fn start(&mut self) -> Result<(), String> {
        if self.running || self.startup_receiver.is_some() {
            return Ok(());
        }

        let (shell, args) = detect_default_shell();
        let cwd = self.cwd.clone();
        let (tx, rx) = mpsc::channel();
        self.startup_receiver = Some(rx);
        self.running = true;

        self.push_output(&format!("正在启动终端: {}...", shell));

        thread::spawn(move || {
            let mut cmd = Command::new(&shell);
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .current_dir(&cwd)
                // CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
                // 避免在 Windows 上为 shell 弹出独立的控制台窗口，保持嵌入在编辑器底部面板
                .creation_flags(0x00000200 | 0x08000000);

            // 根据 shell 类型附加参数，确保进入持续交互模式：
            // - cmd.exe /K：执行完命令后保留窗口（对管道 stdin 也持续读取）
            // - powershell/pwsh -NoExit：启动后保持交互式 shell
            for arg in &args {
                cmd.arg(arg);
            }

            match cmd.spawn() {
                Ok(mut child) => {
                    let stdin = Arc::new(Mutex::new(child.stdin.take().unwrap()));
                    let stdout = child.stdout.take().unwrap();
                    let stderr = child.stderr.take().unwrap();

                    // 启动读取线程
                    let (out_tx, out_rx) = mpsc::channel();
                    spawn_reader(stdout, out_tx.clone());
                    spawn_reader(stderr, out_tx);

                    let _ = tx.send(Ok((stdin, child, out_rx)));
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("启动终端失败: {}", e)));
                }
            }
        });

        Ok(())
    }

    /// 轮询终端启动结果（应在主线程每帧调用）
    pub fn poll_startup(&mut self) {
        if let Some(rx) = self.startup_receiver.take() {
            match rx.try_recv() {
                Ok(Ok((stdin, child, output_rx))) => {
                    self.child_stdin = Some(stdin);
                    self.child_process = Some(child);
                    self.output_receiver = Some(output_rx);
                    self.push_output("终端已启动\n");
                }
                Ok(Err(e)) => {
                    self.push_output(&e);
                    self.running = false;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.startup_receiver = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.push_output("终端启动线程异常退出");
                    self.running = false;
                }
            }
        }
    }

    /// 向终端写入输入
    pub fn write_input(&mut self, text: &str) {
        if let Some(stdin) = &self.child_stdin {
            if let Ok(mut stdin) = stdin.lock() {
                let _ = stdin.write_all(text.as_bytes());
                let _ = stdin.flush();
            }
        }
    }

    /// 发送回车键
    /// 将当前输入行的内容连同换行符一起写入 shell stdin，
    /// 使命令真正被 shell 接收并执行
    pub fn send_enter(&mut self) {
        let command = format!("{}\r\n", self.input_line);
        self.write_input(&command);
        self.input_line.clear();
        self.cursor_pos = 0;
    }

    /// 发送 Ctrl+C
    pub fn send_interrupt(&mut self) {
        // H-20: 尝试向子进程发送 Ctrl+C 事件，而非杀死整个 shell
        // Windows 上使用 GenerateConsoleCtrlEvent 向子进程组发送信号
        if let Some(ref child) = self.child_process {
            #[cfg(windows)]
            {
                use windows::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_C_EVENT};
                unsafe {
                    // C-07: GenerateConsoleCtrlEvent 的第二个参数是进程组 ID。
                    // 子进程以 CREATE_NEW_PROCESS_GROUP 启动，因此其 PID 即为进程组 ID。
                    let process_group_id = child.id();
                    if let Err(e) = GenerateConsoleCtrlEvent(CTRL_C_EVENT, process_group_id) {
                        eprintln!("发送 Ctrl+C 失败: {:?}", e);
                    }
                }
            }
        }
    }

    /// 停止终端
    pub fn stop(&mut self) {
        self.running = false;
        self.child_stdin = None;
        self.output_receiver = None;
        self.startup_receiver = None;

        // 显式终止子进程，避免孤儿进程泄漏
        if let Some(mut child) = self.child_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// H-28: 为 TerminalPanel 实现 Drop，确保窗口关闭时子进程被终止
impl Drop for TerminalPanel {
    fn drop(&mut self) {
        self.stop();
    }
}

impl TerminalPanel {
    /// 从接收器拉取输出（应在主线程每帧调用）
    pub fn flush_output(&mut self) {
        // 先取出 receiver 避免借用冲突
        if let Some(rx) = self.output_receiver.take() {
            // 非阻塞批量接收，减少轮询开销
            while let Ok(text) = rx.try_recv() {
                self.push_output(&text);
            }
            // 放回 receiver
            self.output_receiver = Some(rx);
        }
    }

    /// 添加输出行
    pub fn push_output(&mut self, text: &str) {
        for line in text.lines() {
            if self.output_lines.len() >= self.max_lines {
                self.output_lines.pop_front();
            }
            self.output_lines.push_back(line.to_string());
        }
        // 新输出到达后自动滚动到底部（除非用户手动向上滚动浏览历史）
        if self.scroll_offset > 0 {
            self.scroll_offset = 0;
        }
    }

    /// 获取可见的输出文本
    pub fn visible_output(&self) -> Vec<String> {
        self.output_lines.iter().cloned().collect()
    }

    /// 获取指定行数窗口的输出（用于滚动渲染）。
    /// `visible_lines` 为可显示行数，返回从底部向上偏移 `scroll_offset` 行的窗口。
    pub fn visible_window(&self, visible_lines: usize) -> Vec<String> {
        let total = self.output_lines.len();
        if total == 0 || visible_lines == 0 {
            return Vec::new();
        }
        // 从末尾向前计算窗口结束位置（不含），考虑 scroll_offset
        let end = total.saturating_sub(self.scroll_offset);
        let start = end.saturating_sub(visible_lines);
        self.output_lines
            .iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .cloned()
            .collect()
    }

    /// 向上滚动（查看更早的历史输出）
    pub fn scroll_up(&mut self, lines: usize) {
        let total = self.output_lines.len();
        // 最大可滚动到顶部，滚动偏移不能超过 total
        let max_offset = total;
        self.scroll_offset = (self.scroll_offset + lines).min(max_offset);
    }

    /// 向下滚动（回到最新输出）
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    /// 清除输出
    pub fn clear(&mut self) {
        self.output_lines.clear();
        self.scroll_offset = 0;
    }
}

/// 启动通用读取线程（stdout/stderr）
/// H-13: 保留跨缓冲区边界的 incomplete UTF-8 字节，避免中文乱码
fn spawn_reader<R>(reader: R, tx: mpsc::Sender<String>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = reader;
        let mut buffer = [0u8; 1024];
        let mut leftover: Vec<u8> = Vec::new();
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    leftover.extend_from_slice(&buffer[..n]);
                    // 找到最后一个完整 UTF-8 字符的边界
                    let valid_len = match std::str::from_utf8(&leftover) {
                        Ok(s) => s.len(),
                        Err(e) => e.valid_up_to(),
                    };
                    if valid_len > 0 {
                        let text = String::from_utf8_lossy(&leftover[..valid_len]).to_string();
                        if tx.send(text).is_err() {
                            break; // 接收端已关闭
                        }
                        leftover.drain(..valid_len);
                    }
                }
                Err(_) => break,
            }
            // 增加轮询间隔，减少 CPU 占用
            thread::sleep(std::time::Duration::from_millis(50));
        }
        // 刷新剩余字节（可能是不完整的 UTF-8）
        if !leftover.is_empty() {
            let _ = tx.send(String::from_utf8_lossy(&leftover).to_string());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_new() {
        let panel = TerminalPanel::new();
        assert!(!panel.visible);
        assert_eq!(panel.height, 200.0);
        assert!(panel.output_lines.is_empty());
        assert_eq!(panel.scroll_offset, 0);
    }

    #[test]
    fn test_terminal_push_output_and_visible() {
        let mut panel = TerminalPanel::new();
        panel.push_output("line1\nline2\nline3");
        assert_eq!(panel.output_lines.len(), 3);
        assert_eq!(panel.visible_output(), vec!["line1", "line2", "line3"]);
    }

    #[test]
    fn test_terminal_visible_window() {
        let mut panel = TerminalPanel::new();
        panel.push_output("a\nb\nc\nd\ne");
        // scroll_offset=1 表示从底部向上偏移 1 行
        panel.scroll_offset = 1;
        let window = panel.visible_window(2);
        assert_eq!(window.len(), 2);
        assert_eq!(window[0], "c");
        assert_eq!(window[1], "d");

        // 不偏移时应显示最底部两行
        panel.scroll_offset = 0;
        let window = panel.visible_window(2);
        assert_eq!(window[0], "d");
        assert_eq!(window[1], "e");
    }

    #[test]
    fn test_terminal_scroll_up_down() {
        let mut panel = TerminalPanel::new();
        panel.push_output("a\nb\nc");
        panel.scroll_up(1);
        assert_eq!(panel.scroll_offset, 1);
        panel.scroll_down(1);
        assert_eq!(panel.scroll_offset, 0);
    }

    #[test]
    fn test_terminal_scroll_does_not_exceed_total() {
        let mut panel = TerminalPanel::new();
        panel.push_output("a\nb");
        panel.scroll_up(100);
        assert_eq!(panel.scroll_offset, 2);
    }

    #[test]
    fn test_terminal_clear() {
        let mut panel = TerminalPanel::new();
        panel.push_output("a\nb");
        panel.scroll_up(1);
        panel.clear();
        assert!(panel.output_lines.is_empty());
        assert_eq!(panel.scroll_offset, 0);
    }

    #[test]
    fn test_terminal_toggle() {
        let mut panel = TerminalPanel::new();
        panel.toggle();
        assert!(panel.visible);
        panel.toggle();
        assert!(!panel.visible);
    }

    #[test]
    fn test_terminal_visible_window_edge_cases() {
        let panel = TerminalPanel::new();
        assert!(panel.visible_window(0).is_empty());
        assert!(panel.visible_window(5).is_empty());
    }

    #[test]
    fn test_terminal_scroll_up_clamps_and_down_saturates() {
        let mut panel = TerminalPanel::new();
        panel.push_output("a\nb");
        panel.scroll_up(100);
        assert_eq!(panel.scroll_offset, 2);
        panel.scroll_down(100);
        assert_eq!(panel.scroll_offset, 0);
    }

    #[test]
    fn test_terminal_push_output_trims_and_resets_scroll() {
        let mut panel = TerminalPanel::new();
        panel.max_lines = 3;
        panel.scroll_up(2);
        panel.push_output("1\n2\n3\n4");
        assert_eq!(panel.output_lines.len(), 3);
        assert_eq!(panel.output_lines.front().unwrap(), "2");
        assert_eq!(panel.output_lines.back().unwrap(), "4");
        assert_eq!(panel.scroll_offset, 0);
    }

    #[test]
    fn test_terminal_push_output_preserves_multi_line_text() {
        let mut panel = TerminalPanel::new();
        panel.push_output("line1\nline2\r\nline3");
        assert_eq!(panel.visible_output().len(), 3);
    }

    #[test]
    fn test_terminal_visible_window_with_scroll() {
        let mut panel = TerminalPanel::new();
        panel.push_output("1\n2\n3\n4\n5");
        panel.scroll_offset = 2;
        let window = panel.visible_window(2);
        assert_eq!(window.len(), 2);
        assert_eq!(window[0], "2");
        assert_eq!(window[1], "3");
    }
}

/// 检测默认 shell 及其启动参数
/// 返回 (shell_path, args) 元组，args 用于确保 shell 在管道 stdin/stdout 下
/// 仍保持持续交互模式，而不是读取完输入后立即退出。
///
/// 默认优先使用 cmd.exe /K：在 Windows 管道重定向 stdin/stdout 的场景下，
/// cmd.exe /K 会稳定地显示提示符并逐行读取命令，最适合嵌入编辑器内部。
/// PowerShell 系列在管道非控制台环境下难以保证交互式提示符，因此作为备选。
fn detect_default_shell() -> (String, Vec<String>) {
    // 优先使用 cmd.exe /K，在嵌入式管道终端中最稳定
    if which_exists("cmd.exe") {
        return ("cmd.exe".to_string(), vec!["/K".to_string()]);
    }
    // 回退到 PowerShell 7
    if which_exists("pwsh.exe") {
        return ("pwsh.exe".to_string(), vec!["-NoExit".to_string()]);
    }
    // 回退到 PowerShell 5
    if which_exists("powershell.exe") {
        return ("powershell.exe".to_string(), vec!["-NoExit".to_string()]);
    }
    // 最后回退：仍尝试 cmd.exe（即使 PATH 中未找到）
    ("cmd.exe".to_string(), vec!["/K".to_string()])
}

fn which_exists(name: &str) -> bool {
    if let Ok(paths) = std::env::var("PATH") {
        for path in paths.split(';') {
            let full = std::path::Path::new(path).join(name);
            if full.exists() {
                return true;
            }
        }
    }
    let common_paths = [
        format!("C:\\Windows\\System32\\{}", name),
        format!("C:\\Program Files\\PowerShell\\7\\{}", name),
    ];
    for p in &common_paths {
        if std::path::Path::new(p).exists() {
            return true;
        }
    }
    false
}
