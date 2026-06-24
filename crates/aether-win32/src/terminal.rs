use std::collections::VecDeque;
use std::io::{Read, Write};
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
    /// 输出接收器（从读取线程接收终端输出）
    output_receiver: Option<mpsc::Receiver<String>>,
    /// 是否运行中
    pub running: bool,
    /// 工作目录
    pub cwd: String,
    /// 是否聚焦
    pub focused: bool,
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
            output_receiver: None,
            running: false,
            cwd: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            focused: false,
        }
    }

    /// 显示/隐藏终端面板
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// 启动终端会话
    pub fn start(&mut self) -> Result<(), String> {
        let shell = detect_default_shell();

        let mut child = Command::new(&shell)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(&self.cwd)
            .spawn()
            .map_err(|e| format!("启动终端失败: {}", e))?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        self.child_stdin = Some(Arc::new(Mutex::new(stdin)));
        self.child_stdout = Some(Arc::new(Mutex::new(stdout)));
        self.child_stderr = Some(Arc::new(Mutex::new(stderr)));
        self.running = true;

        // 启动读取线程，使用 channel 传递输出到主线程
        let (tx, rx) = mpsc::channel();
        self.output_receiver = Some(rx);
        self.spawn_stdout_reader(tx.clone());
        self.spawn_stderr_reader(tx);

        self.push_output(&format!("终端已启动: {}\n", shell));
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
    pub fn send_enter(&mut self) {
        self.write_input("\r\n");
        self.input_line.clear();
        self.cursor_pos = 0;
    }

    /// 发送 Ctrl+C
    pub fn send_interrupt(&mut self) {
        // 在 Windows 上发送 Ctrl+C 比较复杂
        // 简化实现：直接重启终端
        self.stop();
        let _ = self.start();
    }

    /// 停止终端
    pub fn stop(&mut self) {
        self.running = false;
        self.child_stdin = None;
        self.child_stdout = None;
        self.child_stderr = None;
        self.output_receiver = None;
    }

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
    }

    /// 启动 stdout 读取线程
    fn spawn_stdout_reader(&mut self, tx: mpsc::Sender<String>) {
        if let Some(stdout) = &self.child_stdout {
            let stdout = Arc::clone(stdout);
            thread::spawn(move || {
                let mut buffer = [0u8; 1024];
                loop {
                    if let Ok(mut stdout) = stdout.lock() {
                        match stdout.read(&mut buffer) {
                            Ok(0) => break, // EOF
                            Ok(n) => {
                                let text = String::from_utf8_lossy(&buffer[..n]).to_string();
                                if tx.send(text).is_err() {
                                    break; // 接收端已关闭
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    // 增加轮询间隔，从 10ms 改为 50ms，减少 CPU 占用
                    thread::sleep(std::time::Duration::from_millis(50));
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
                loop {
                    if let Ok(mut stderr) = stderr.lock() {
                        match stderr.read(&mut buffer) {
                            Ok(0) => break, // EOF
                            Ok(n) => {
                                let text = String::from_utf8_lossy(&buffer[..n]).to_string();
                                if tx.send(text).is_err() {
                                    break; // 接收端已关闭
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    thread::sleep(std::time::Duration::from_millis(50));
                }
            });
        }
    }

    /// 获取可见的输出文本
    pub fn visible_output(&self) -> Vec<String> {
        self.output_lines.iter().cloned().collect()
    }

    /// 清除输出
    pub fn clear(&mut self) {
        self.output_lines.clear();
    }
}

/// 检测默认 shell
fn detect_default_shell() -> String {
    // 优先使用 PowerShell 7
    if which_exists("pwsh.exe") {
        return "pwsh.exe".to_string();
    }
    // 回退到 PowerShell 5
    if which_exists("powershell.exe") {
        return "powershell.exe".to_string();
    }
    // 最后回退到 cmd
    "cmd.exe".to_string()
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
