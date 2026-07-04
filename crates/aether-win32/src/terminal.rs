use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

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
    /// 子进程stdout（用于读取输出）
    child_stdout: Option<Arc<Mutex<std::process::ChildStdout>>>,
    /// 子进程stderr（用于读取错误输出）
    child_stderr: Option<Arc<Mutex<std::process::ChildStderr>>>,
    /// 子进程句柄（用于终止进程）
    child_process: Option<std::process::Child>,
    /// 输出接收器（从读取线程接收终端输出）
    output_receiver: Option<mpsc::Receiver<String>>,
    /// 是否运行中
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
            child_stdout: None,
            child_stderr: None,
            child_process: None,
            output_receiver: None,
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

    /// 启动终端会话
    pub fn start(&mut self) -> Result<(), String> {
        let (shell, args) = detect_default_shell();

        let mut cmd = Command::new(&shell);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(&self.cwd)
            // CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
            // 避免在 Windows 上为 shell 弹出独立的控制台窗口，保持嵌入在编辑器底部面板
            .creation_flags(0x00000200 | 0x08000000);

        // 根据 shell 类型附加参数，确保进入持续交互模式：
        // - cmd.exe /K：执行完命令后保留窗口（对管道 stdin 也持续读取）
        // - powershell/pwsh -NoExit：启动后保持交互式 shell
        for arg in &args {
            cmd.arg(arg);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("启动终端失败: {}", e))?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        self.child_stdin = Some(Arc::new(Mutex::new(stdin)));
        self.child_stdout = Some(Arc::new(Mutex::new(stdout)));
        self.child_stderr = Some(Arc::new(Mutex::new(stderr)));
        self.child_process = Some(child);
        self.running = true;

        // 启动读取线程，使用 channel 传递输出到主线程
        let (tx, rx) = mpsc::channel();
        self.output_receiver = Some(rx);
        self.spawn_stdout_reader(tx.clone());
        self.spawn_stderr_reader(tx);

        self.push_output(&format!("终端已启动: {} {}\n", shell, args.join(" ")));
        Ok(())
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
                    // SEC-W02: 向子进程组发送 Ctrl+C，而非当前进程组
                    let pid = child.id();
                    let _ = GenerateConsoleCtrlEvent(CTRL_C_EVENT, pid);
                }
            }
        }
    }

    /// 停止终端
    pub fn stop(&mut self) {
        self.running = false;
        self.child_stdin = None;
        self.child_stdout = None;
        self.child_stderr = None;
        self.output_receiver = None;

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

    /// 启动 stdout 读取线程
    fn spawn_stdout_reader(&mut self, tx: mpsc::Sender<String>) {
        if let Some(stdout) = &self.child_stdout {
            let stdout = Arc::clone(stdout);
            thread::spawn(move || {
                let mut buffer = [0u8; 1024];
                // H-13: 保留跨缓冲区边界的 incomplete UTF-8 字节，避免中文乱码
                let mut leftover: Vec<u8> = Vec::new();
                loop {
                    if let Ok(mut stdout) = stdout.lock() {
                        match stdout.read(&mut buffer) {
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
                    }
                    // 增加轮询间隔，从 10ms 改为 50ms，减少 CPU 占用
                    thread::sleep(std::time::Duration::from_millis(50));
                }
                // 刷新剩余字节（可能是不完整的 UTF-8）
                if !leftover.is_empty() {
                    let _ = tx.send(String::from_utf8_lossy(&leftover).to_string());
                }
            });
        }
    }

    /// 启动 stderr 读取线程
    fn spawn_stderr_reader(&mut self, tx: mpsc::Sender<String>) {
        if let Some(stderr) = &self.child_stderr {
            let stderr = Arc::clone(stderr);
            thread::spawn(move || {
                let mut buffer = [0u8; 1024];
                // H-13: 保留跨缓冲区边界的 incomplete UTF-8 字节
                let mut leftover: Vec<u8> = Vec::new();
                loop {
                    if let Ok(mut stderr) = stderr.lock() {
                        match stderr.read(&mut buffer) {
                            Ok(0) => break, // EOF
                            Ok(n) => {
                                leftover.extend_from_slice(&buffer[..n]);
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
                    }
                    thread::sleep(std::time::Duration::from_millis(50));
                }
                if !leftover.is_empty() {
                    let _ = tx.send(String::from_utf8_lossy(&leftover).to_string());
                }
            });
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
