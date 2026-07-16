#![allow(clippy::items_after_test_module)]

//! 内嵌同步终端面板。
//!
//! 使用 ConPTY（Windows 伪控制台）实现真正的同步终端，
//! 支持交互式命令、Tab 补全、方向键导航、ANSI 颜色等。
//! 所有按键直接发送到 ConPTY，由 shell 处理输入编辑和回显。

use std::collections::VecDeque;
use std::io::Read;
use std::sync::mpsc;
use std::thread;

use crate::conpty::{ConPtySession, PipeReader};

/// 终端启动结果（由后台线程产生，传回主线程）
type TerminalStartupResult = Result<(ConPtySession, mpsc::Receiver<Vec<u8>>), String>;

/// 终端面板状态
pub struct TerminalPanel {
    /// 是否可见
    pub visible: bool,
    /// 面板高度（像素）
    pub height: f32,
    /// 终端输出行缓存（ANSI 解析后的纯文本行）
    pub output_lines: VecDeque<String>,
    /// 最大缓存行数
    pub max_lines: usize,
    /// ConPTY 会话
    conpty: Option<ConPtySession>,
    /// 输出接收器（从读取线程接收 ConPTY 原始输出字节）
    output_receiver: Option<mpsc::Receiver<Vec<u8>>>,
    /// 启动结果接收器：后台线程启动 ConPTY 后通过此通道返回结果
    startup_receiver: Option<mpsc::Receiver<TerminalStartupResult>>,
    /// 是否运行中（包含正在启动的状态）
    pub running: bool,
    /// 工作目录
    pub cwd: String,
    /// 是否聚焦
    pub focused: bool,
    /// 输出滚动偏移（从底部算起的行数，0 表示贴底显示最新输出）
    pub scroll_offset: usize,
    /// ANSI 解析器
    ansi_parser: AnsiParser,
    /// 终端尺寸（字符列数）
    cols: i16,
    /// 终端尺寸（字符行数）
    rows: i16,
    /// 尺寸是否已与面板同步（首次同步不触发 resize，避免 ConPTY 清屏）
    size_synced: bool,
    /// ConPTY 启动时间（用于在初始阶段抑制清屏序列）
    conpty_start_time: Option<std::time::Instant>,
    /// AI Agent 待执行命令队列（终端就绪后自动发送）
    pending_commands: std::collections::VecDeque<String>,
}

impl TerminalPanel {
    pub fn new() -> Self {
        Self {
            visible: false,
            height: 200.0,
            output_lines: VecDeque::with_capacity(1000),
            max_lines: 1000,
            conpty: None,
            output_receiver: None,
            startup_receiver: None,
            running: false,
            cwd: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            focused: false,
            scroll_offset: 0,
            ansi_parser: AnsiParser::new(),
            cols: 80,
            rows: 24,
            size_synced: false,
            conpty_start_time: None,
            pending_commands: std::collections::VecDeque::new(),
        }
    }

    /// 显示/隐藏终端面板
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// 设置终端尺寸（字符行列数）。
    ///
    /// **注意**：当前已禁用 ConPTY resize 调用。
    /// `ResizePseudoConsole` 会导致 ConPTY 发送 `\x1b[2J` 清屏序列，
    /// 清空所有输出行，使终端看起来无响应（点击有反应但看不到任何内容）。
    /// 终端保持初始尺寸，仅更新记录值供下次启动使用。
    pub fn set_size(&mut self, cols: i16, rows: i16) {
        if cols <= 0 || rows <= 0 {
            return;
        }
        if !self.size_synced {
            tracing::info!(cols, rows, "set_size: 首次同步尺寸");
            self.cols = cols;
            self.rows = rows;
            self.size_synced = true;
            self.ansi_parser.set_visible_rows(rows as usize);
            return;
        }
        let col_diff = (cols - self.cols).abs();
        let row_diff = (rows - self.rows).abs();
        if col_diff <= 2 && row_diff <= 2 {
            return;
        }
        // 更新存储的尺寸，供下次 start() 使用，但不触发 ConPTY resize
        self.cols = cols;
        self.rows = rows;
        self.ansi_parser.set_visible_rows(rows as usize);
    }

    /// 获取终端光标位置 (row, col)，均为 0-indexed。
    /// 用于渲染光标。row 已被 clamp 到 output_lines 范围内。
    pub fn cursor_position(&self) -> (usize, usize) {
        let (row, col) = self.ansi_parser.cursor_position();
        let clamped_row = row.min(self.output_lines.len().saturating_sub(1));
        (clamped_row, col)
    }

    /// 启动终端会话（异步，不阻塞 UI 线程）
    pub fn start(&mut self) -> Result<(), String> {
        if self.running || self.startup_receiver.is_some() {
            return Ok(());
        }

        let (shell, args) = detect_default_shell();
        // 构建完整命令行
        let commandline = if args.is_empty() {
            shell.clone()
        } else {
            format!("{} {}", shell, args.join(" "))
        };
        let cwd = self.cwd.clone();
        let cols = self.cols;
        let rows = self.rows;
        let (tx, rx) = mpsc::channel();
        self.startup_receiver = Some(rx);
        self.running = true;
        self.size_synced = false; // 每次启动重置，首次 set_size 不触发 resize

        self.push_output(&format!("正在启动终端: {}...", commandline));

        thread::spawn(move || {
            match ConPtySession::spawn(&commandline, Some(&cwd), cols, rows) {
                Ok((session, read_handle)) => {
                    let (out_tx, out_rx) = mpsc::channel();
                    // 启动读取线程，从 ConPTY 输出管道读取字节
                    let mut reader = PipeReader::new(read_handle);
                    thread::spawn(move || {
                        tracing::info!("终端读取线程启动");
                        let mut buffer = [0u8; 4096];
                        let mut read_count = 0u32;
                        loop {
                            match reader.read(&mut buffer) {
                                Ok(0) => {
                                    tracing::info!("终端读取线程收到 EOF，退出");
                                    break;
                                }
                                Ok(n) => {
                                    read_count += 1;
                                    if read_count <= 3 {
                                        let preview: String = buffer[..n.min(200)]
                                            .iter()
                                            .map(|&b| {
                                                if (0x20..0x7f).contains(&b) {
                                                    char::from(b)
                                                } else if b == 0x1b {
                                                    '⟦'
                                                } else if b == 0x0d {
                                                    '⏎'
                                                } else if b == 0x0a {
                                                    '↓'
                                                } else {
                                                    '·'
                                                }
                                            })
                                            .collect();
                                        tracing::info!(bytes = n, count = read_count, preview = %preview, "终端读取线程读到数据");
                                    }
                                    if out_tx.send(buffer[..n].to_vec()).is_err() {
                                        tracing::info!("终端读取线程：接收端关闭，退出");
                                        break;
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(error = %e, "终端读取线程出错，退出");
                                    break;
                                }
                            }
                        }
                        tracing::info!(total_reads = read_count, "终端读取线程结束");
                    });
                    tracing::info!("终端启动成功，发送结果到主线程");
                    let _ = tx.send(Ok((session, out_rx)));
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        });

        Ok(())
    }

    /// 轮询终端启动结果（应在主线程每帧调用）
    pub fn poll_startup(&mut self) {
        if let Some(rx) = self.startup_receiver.take() {
            match rx.try_recv() {
                Ok(Ok((session, output_rx))) => {
                    tracing::info!("poll_startup: 终端启动成功，设置 conpty 和 output_receiver");
                    self.conpty = Some(session);
                    self.output_receiver = Some(output_rx);
                    self.conpty_start_time = Some(std::time::Instant::now());
                    // 清除"正在启动"提示，ConPTY 会输出 shell 提示符
                    self.output_lines.clear();
                }
                Ok(Err(e)) => {
                    tracing::error!(error = %e, "poll_startup: 终端启动失败");
                    self.push_output(&e);
                    self.running = false;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.startup_receiver = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    tracing::error!("poll_startup: 启动线程通道断开");
                    self.push_output("终端启动线程异常退出");
                    self.running = false;
                }
            }
        }
    }

    /// 发送原始字节到 ConPTY（子进程 stdin）
    pub fn send_bytes(&mut self, data: &[u8]) {
        if let Some(ref session) = self.conpty {
            match session.write_input(data) {
                Ok(()) => {
                    tracing::info!(bytes = data.len(), data_hex = %format!("{:02x?}", data), "send_bytes: 已发送字节到子进程");
                }
                Err(e) => {
                    tracing::error!(error = %e, bytes = data.len(), "send_bytes: write_input 失败");
                }
            }
        } else {
            tracing::warn!(
                bytes = data.len(),
                running = self.running,
                "send_bytes: conpty 为 None，无法发送（可能子进程已退出）"
            );
        }
    }

    /// AI Agent：将命令加入待执行队列，终端就绪后自动发送执行。
    pub fn queue_command(&mut self, command: String) {
        self.pending_commands.push_back(command);
    }

    /// 是否有待执行的命令
    pub fn has_pending_commands(&self) -> bool {
        !self.pending_commands.is_empty()
    }

    /// 刷新待执行命令：当 ConPTY 就绪且 shell 提示符已显示后，发送队列中的命令。
    /// 应在主线程每帧调用（与 poll_startup / flush_output 同级）。
    pub fn flush_pending_commands(&mut self) {
        if self.pending_commands.is_empty() || self.conpty.is_none() {
            return;
        }
        // 等待 shell 提示符就绪（启动后短暂延迟），避免命令被 shell 初始化吞掉
        if let Some(start) = self.conpty_start_time {
            if start.elapsed() < std::time::Duration::from_millis(600) {
                return;
            }
        } else {
            return;
        }
        while let Some(cmd) = self.pending_commands.pop_front() {
            self.send_bytes(cmd.as_bytes());
            self.send_bytes(b"\r");
        }
        // 发送后回到底部显示最新输出
        self.scroll_offset = 0;
    }

    /// 发送回车键，同时重置滚动到最新输出。
    ///
    /// ConPTY 模式下发送 `\r`（伪控制台自动转换为 `\r\n`）。
    /// 管道模式下发送 `\r\n`（cmd.exe 管道模式需要 CRLF 行结束符）。
    pub fn send_enter(&mut self) {
        self.scroll_offset = 0;
        let is_pipe = self.conpty.as_ref().map(|s| s.is_pipe()).unwrap_or(false);
        if is_pipe {
            self.send_bytes(b"\r\n");
        } else {
            self.send_bytes(b"\r");
        }
    }

    /// 发送退格键（DEL = 0x7f，cmd.exe 识别）
    pub fn send_backspace(&mut self) {
        self.send_bytes(b"\x7f");
    }

    /// 发送 Tab 键
    pub fn send_tab(&mut self) {
        self.send_bytes(b"\t");
    }

    /// 发送 Ctrl+C（中断信号）
    pub fn send_interrupt(&mut self) {
        self.send_bytes(b"\x03");
    }

    /// 发送方向键
    pub fn send_arrow(&mut self, direction: ArrowKey) {
        let seq: &[u8] = match direction {
            ArrowKey::Up => b"\x1b[A",
            ArrowKey::Down => b"\x1b[B",
            ArrowKey::Right => b"\x1b[C",
            ArrowKey::Left => b"\x1b[D",
        };
        self.send_bytes(seq);
    }

    /// 发送 Delete 键
    pub fn send_delete(&mut self) {
        self.send_bytes(b"\x1b[3~");
    }

    /// 发送 Home 键
    pub fn send_home(&mut self) {
        self.send_bytes(b"\x1b[H");
    }

    /// 发送 End 键
    pub fn send_end(&mut self) {
        self.send_bytes(b"\x1b[F");
    }

    /// 发送字符的 UTF-8 编码
    pub fn send_char(&mut self, c: char) {
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        self.send_bytes(s.as_bytes());
    }

    /// 停止终端
    pub fn stop(&mut self) {
        self.running = false;
        self.conpty = None;
        self.output_receiver = None;
        self.startup_receiver = None;
        self.size_synced = false;
        self.conpty_start_time = None;
    }

    /// 检查 ConPTY 子进程是否仍在运行
    pub fn is_alive(&self) -> bool {
        self.conpty.as_ref().map(|s| s.is_alive()).unwrap_or(false)
    }
}

impl Drop for TerminalPanel {
    fn drop(&mut self) {
        self.stop();
    }
}

impl TerminalPanel {
    /// 从接收器拉取输出（应在主线程每帧调用）
    ///
    /// 同时检测读取线程是否已退出（通道断开 = 子进程已结束），
    /// 此时清理 ConPTY 并提示用户进程已退出。
    pub fn flush_output(&mut self) {
        let has_rx = self.output_receiver.is_some();
        let has_conpty = self.conpty.is_some();
        if !has_rx {
            tracing::info!(
                has_rx,
                has_conpty,
                running = self.running,
                "flush_output: output_receiver 为 None，跳过"
            );
        }
        // 诊断：每 100 次调用记录一次，确认 flush_output 在持续运行
        static FLUSH_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let count = FLUSH_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if count.is_multiple_of(100) {
            tracing::info!(
                count,
                has_rx,
                has_conpty,
                running = self.running,
                "flush_output: 定期诊断"
            );
        }
        if let Some(rx) = self.output_receiver.take() {
            let mut total_bytes = 0usize;
            let mut msg_count = 0usize;
            let mut channel_closed = false;
            // ConPTY 启动后 3 秒内抑制清屏序列（\x1b[2J、\x1b[K），
            // 因为 ConPTY 初始化时会发送清屏+重绘序列，但不包含实际文本内容，
            // 会导致 cmd.exe 的提示符被清空。
            let suppress_clear = self
                .conpty_start_time
                .map(|t| t.elapsed() < std::time::Duration::from_secs(3))
                .unwrap_or(false);
            self.ansi_parser.suppress_clear = suppress_clear;
            // 非阻塞批量接收，减少轮询开销
            loop {
                match rx.try_recv() {
                    Ok(bytes) => {
                        total_bytes += bytes.len();
                        msg_count += 1;
                        self.ansi_parser
                            .feed(&bytes, &mut self.output_lines, self.max_lines);
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        channel_closed = true;
                        break;
                    }
                }
            }
            if msg_count > 0 {
                tracing::info!(
                    msgs = msg_count,
                    bytes = total_bytes,
                    lines = self.output_lines.len(),
                    "flush_output 拉取到终端输出"
                );
                // 记录前 5 行内容用于诊断 ANSI 解析结果
                for (i, line) in self.output_lines.iter().rev().take(5).enumerate() {
                    tracing::info!(idx = i, content = %line, "终端输出行");
                }
            }
            if channel_closed {
                // 读取线程已退出：子进程结束或管道关闭。
                // 获取退出码并提示用户，清理 ConPTY 资源。
                let exit_code = self
                    .conpty
                    .as_ref()
                    .and_then(|s| s.exit_code())
                    .unwrap_or(0);
                tracing::info!(exit_code, "终端子进程已退出");
                self.push_output(&format!("\n[进程已退出，退出码: 0x{:08X}]", exit_code));
                self.conpty = None;
                self.running = false;
            } else {
                self.output_receiver = Some(rx);
                // 新输出到达后自动滚动到底部
                if self.scroll_offset > 0 {
                    self.scroll_offset = 0;
                }
            }
        }
    }

    /// 添加输出行（直接追加，不经过 ANSI 解析）
    pub fn push_output(&mut self, text: &str) {
        for line in text.lines() {
            if self.output_lines.len() >= self.max_lines {
                self.output_lines.pop_front();
            }
            self.output_lines.push_back(line.to_string());
        }
        if self.scroll_offset > 0 {
            self.scroll_offset = 0;
        }
    }

    /// 获取可见的输出文本
    pub fn visible_output(&self) -> Vec<String> {
        self.output_lines.iter().cloned().collect()
    }

    /// 获取指定行数窗口的输出（用于滚动渲染）。
    pub fn visible_window(&self, visible_lines: usize) -> Vec<String> {
        let total = self.output_lines.len();
        if total == 0 || visible_lines == 0 {
            return Vec::new();
        }
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
        let max_offset = self.output_lines.len();
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

/// 方向键
pub enum ArrowKey {
    Up,
    Down,
    Left,
    Right,
}

/// ANSI 转义序列解析器（屏幕缓冲区模型）。
///
/// 使用光标行列跟踪来正确处理 ConPTY 的屏幕渲染序列：
/// - `\r` 回车（光标回到行首，不清除内容）
/// - `\n` 换行（光标下移一行）
/// - `\b` 退格（光标左移一列）
/// - `ESC [ ... H` 光标定位（行,列）— ConPTY 大量使用
/// - `ESC [ 2J` 清屏
/// - `ESC [ K` 清行（从光标到行尾）
/// - `ESC [ A/B/C/D` 光标移动
/// - `ESC [ ... m` SGR 颜色（剥离）
/// - `ESC ] ... BEL` OSC 序列（跳过）
///
/// lines VecDeque 即屏幕缓冲区，cursor_row/col 为光标位置（0-indexed）。
/// 写字符时在光标位置覆盖或追加，光标列右移。
struct AnsiParser {
    /// 光标行位置（0-indexed，指向 lines 中的索引）
    cursor_row: usize,
    /// 光标列位置（0-indexed，按字符计数）
    cursor_col: usize,
    /// 解析状态机
    state: ParseState,
    /// CSI 参数缓冲
    csi_buffer: String,
    /// 可见行数（用于 \x1b[2J 清屏时只清可见区域，保留滚动缓冲）
    visible_rows: usize,
    /// 是否抑制清屏序列（ConPTY 启动初期设为 true，避免初始化清屏清空提示符）
    pub suppress_clear: bool,
}

#[derive(PartialEq)]
enum ParseState {
    /// 正常文本
    Normal,
    /// 收到 ESC，等待下一个字节
    Escape,
    /// CSI 序列（ESC [）
    Csi,
    /// OSC 序列（ESC ]），跳过直到 BEL 或 ST
    Osc,
}

impl AnsiParser {
    fn new() -> Self {
        Self {
            cursor_row: 0,
            cursor_col: 0,
            state: ParseState::Normal,
            csi_buffer: String::new(),
            visible_rows: 24,
            suppress_clear: false,
        }
    }

    /// 设置可见行数（由 TerminalPanel::set_size 同步）
    fn set_visible_rows(&mut self, rows: usize) {
        self.visible_rows = rows.max(1);
    }

    /// 返回当前光标位置 (row, col)，均为 0-indexed。
    /// row 可能超出 output_lines 的长度（换行只移动光标不创建行）。
    fn cursor_position(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    /// 喂入原始字节，解析后更新屏幕缓冲区。
    fn feed(&mut self, data: &[u8], lines: &mut VecDeque<String>, max_lines: usize) {
        let text = String::from_utf8_lossy(data);
        for c in text.chars() {
            self.feed_char(c, lines);
        }
        // 滚动：超出最大行数时移除最早的行，并调整光标
        while lines.len() > max_lines {
            lines.pop_front();
            if self.cursor_row > 0 {
                self.cursor_row -= 1;
            }
        }
    }

    fn feed_char(&mut self, c: char, lines: &mut VecDeque<String>) {
        match self.state {
            ParseState::Normal => {
                match c {
                    '\x1b' => self.state = ParseState::Escape,
                    '\r' => self.cursor_col = 0,
                    '\n' => {
                        // 换行：光标下移一行，行由 write_char 惰性创建
                        self.cursor_row += 1;
                        self.cursor_col = 0;
                    }
                    '\x08' => {
                        // 退格：光标左移一列
                        if self.cursor_col > 0 {
                            self.cursor_col -= 1;
                        }
                    }
                    '\x07' => {
                        // 响铃（BEL），忽略
                    }
                    _ => self.write_char(c, lines),
                }
            }
            ParseState::Escape => {
                match c {
                    '[' => {
                        self.state = ParseState::Csi;
                        self.csi_buffer.clear();
                    }
                    ']' => {
                        // OSC 序列：跳过直到 BEL 或 ST
                        self.state = ParseState::Osc;
                    }
                    _ => {
                        // 其他 ESC 序列（如 ESC H = 设置 tab stop），忽略
                        self.state = ParseState::Normal;
                    }
                }
            }
            ParseState::Csi => {
                if c.is_ascii() && (c.is_ascii_digit() || c == ';' || c == '?') {
                    self.csi_buffer.push(c);
                } else if ('@'..='~').contains(&c) {
                    self.handle_csi(c, lines);
                    self.state = ParseState::Normal;
                } else {
                    self.state = ParseState::Normal;
                }
            }
            ParseState::Osc => {
                // 跳过 OSC 内容，直到 BEL(\x07) 或 ESC（ST 的开始）
                if c == '\x07' {
                    self.state = ParseState::Normal;
                } else if c == '\x1b' {
                    // ST = ESC \，转到 Escape 状态处理后续
                    self.state = ParseState::Escape;
                    self.csi_buffer.clear();
                }
            }
        }
    }

    /// 确保光标行存在，不足则创建空行
    fn ensure_line_exists(&self, lines: &mut VecDeque<String>) {
        while lines.len() <= self.cursor_row {
            lines.push_back(String::new());
        }
    }

    /// 在光标位置写入一个字符，光标列右移
    fn write_char(&mut self, c: char, lines: &mut VecDeque<String>) {
        self.ensure_line_exists(lines);
        if let Some(line) = lines.get_mut(self.cursor_row) {
            let char_count = line.chars().count();
            if self.cursor_col >= char_count {
                // 追加模式：用空格填充间隙后追加
                for _ in char_count..self.cursor_col {
                    line.push(' ');
                }
                line.push(c);
            } else {
                // 覆盖模式：替换光标位置的字符
                let mut chars: Vec<char> = line.chars().collect();
                chars[self.cursor_col] = c;
                line.clear();
                line.extend(chars.iter());
            }
            self.cursor_col += 1;
        }
    }

    /// 处理 CSI 序列
    fn handle_csi(&mut self, final_byte: char, lines: &mut VecDeque<String>) {
        // 检查是否为 private mode 序列（? 开头）
        let is_private = self.csi_buffer.starts_with('?');
        let params: Vec<u32> = self
            .csi_buffer
            .trim_start_matches('?')
            .split(';')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();

        match final_byte {
            // SGR（颜色/样式）— 剥离
            'm' => {}
            // 光标上移
            'A' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            // 光标下移
            'B' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_row += n;
                self.ensure_line_exists(lines);
            }
            // 光标右移
            'C' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_col += n;
            }
            // 光标左移
            'D' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            // 光标定位（行,列）— 1-indexed 参数
            'H' | 'f' => {
                let row = params.first().copied().unwrap_or(1).max(1) as usize;
                let col = params.get(1).copied().unwrap_or(1).max(1) as usize;
                self.cursor_row = row - 1;
                self.cursor_col = col - 1;
                self.ensure_line_exists(lines);
            }
            // 光标水平定位（列）— 1-indexed
            'G' => {
                let col = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_col = col - 1;
            }
            // 光标垂直定位（行）— 1-indexed
            'd' => {
                let row = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_row = row - 1;
                self.ensure_line_exists(lines);
            }
            // 清屏
            'J' => {
                if self.suppress_clear {
                    tracing::info!("AnsiParser: suppress_clear=true，跳过 \\x1b[J 清屏");
                    return;
                }
                let mode = params.first().copied().unwrap_or(0);
                match mode {
                    0 => {
                        // 从光标到屏底清除
                        self.ensure_line_exists(lines);
                        if let Some(line) = lines.get_mut(self.cursor_row) {
                            let char_count = line.chars().count();
                            if self.cursor_col < char_count {
                                let chars: Vec<char> = line.chars().take(self.cursor_col).collect();
                                line.clear();
                                line.extend(chars.iter());
                            }
                        }
                        // 移除光标行之后的所有行
                        while lines.len() > self.cursor_row + 1 {
                            lines.pop_back();
                        }
                    }
                    1 => {
                        // 从屏顶到光标清除
                        self.ensure_line_exists(lines);
                        for i in 0..self.cursor_row.min(lines.len()) {
                            lines[i].clear();
                        }
                        if let Some(line) = lines.get_mut(self.cursor_row) {
                            let chars: Vec<char> = line.chars().skip(self.cursor_col).collect();
                            line.clear();
                            line.extend(chars.iter());
                        }
                    }
                    2 | 3 => {
                        // 清全屏：只清可见区域（最后 visible_rows 行），保留滚动缓冲
                        let total = lines.len();
                        let visible_start = total.saturating_sub(self.visible_rows);
                        // 清空可见区域内的所有行内容，但不删除滚动缓冲中的历史行
                        for i in visible_start..total {
                            if let Some(line) = lines.get_mut(i) {
                                line.clear();
                            }
                        }
                        self.cursor_row = 0;
                        self.cursor_col = 0;
                    }
                    _ => {}
                }
            }
            // 清行
            'K' => {
                if self.suppress_clear {
                    return;
                }
                let mode = params.first().copied().unwrap_or(0);
                self.ensure_line_exists(lines);
                if let Some(line) = lines.get_mut(self.cursor_row) {
                    match mode {
                        0 => {
                            // 从光标到行尾清除
                            let char_count = line.chars().count();
                            if self.cursor_col < char_count {
                                let chars: Vec<char> = line.chars().take(self.cursor_col).collect();
                                line.clear();
                                line.extend(chars.iter());
                            }
                        }
                        1 => {
                            // 从行首到光标清除
                            let chars: Vec<char> = line.chars().skip(self.cursor_col).collect();
                            line.clear();
                            line.extend(chars.iter());
                        }
                        2 => {
                            line.clear();
                        }
                        _ => {}
                    }
                }
            }
            // 擦除字符（不移动光标和后续字符）
            'X' => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.ensure_line_exists(lines);
                if let Some(line) = lines.get_mut(self.cursor_row) {
                    let mut chars: Vec<char> = line.chars().collect();
                    for i in 0..n {
                        let pos = self.cursor_col + i;
                        if pos < chars.len() {
                            chars[pos] = ' ';
                        } else {
                            chars.push(' ');
                        }
                    }
                    line.clear();
                    line.extend(chars.iter());
                }
            }
            // private mode（?h/?l）：show/hide cursor, alternate screen 等 — 忽略
            'h' | 'l' if is_private => {}
            _ => {}
        }
    }
}

/// 检测默认 shell 及其启动参数，返回绝对路径
fn detect_default_shell() -> (String, Vec<String>) {
    // 优先使用 cmd.exe（ConPTY 模式下无需 /K，ConPTY 本身保持交互）
    if let Some(path) = find_executable("cmd.exe") {
        return (path, vec![]);
    }
    if let Some(path) = find_executable("pwsh.exe") {
        return (path, vec!["-NoExit".to_string()]);
    }
    if let Some(path) = find_executable("powershell.exe") {
        return (path, vec!["-NoExit".to_string()]);
    }
    // 最终回退：System32 下的 cmd.exe
    (r"C:\Windows\System32\cmd.exe".to_string(), vec![])
}

/// 在 PATH 和常见目录中查找可执行文件，返回绝对路径
fn find_executable(name: &str) -> Option<String> {
    if let Ok(paths) = std::env::var("PATH") {
        for path in paths.split(';') {
            let full = std::path::Path::new(path).join(name);
            if full.exists() {
                return Some(full.to_string_lossy().to_string());
            }
        }
    }
    let common_paths = [r"C:\Windows\System32", r"C:\Program Files\PowerShell\7"];
    for dir in &common_paths {
        let full = std::path::Path::new(dir).join(name);
        if full.exists() {
            return Some(full.to_string_lossy().to_string());
        }
    }
    None
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
        panel.scroll_offset = 1;
        let window = panel.visible_window(2);
        assert_eq!(window.len(), 2);
        assert_eq!(window[0], "c");
        assert_eq!(window[1], "d");

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

    // ANSI 解析器测试
    #[test]
    fn test_ansi_parser_plain_text() {
        let mut parser = AnsiParser::new();
        let mut lines = VecDeque::new();
        parser.feed(b"hello\nworld\n", &mut lines, 100);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "hello");
        assert_eq!(lines[1], "world");
    }

    #[test]
    fn test_ansi_parser_carriage_return() {
        let mut parser = AnsiParser::new();
        let mut lines = VecDeque::new();
        // "abc\rAB\n" → \r 光标回到行首，AB 覆盖位置 0 和 1，c 保留 → "ABc"
        parser.feed(b"abc\rAB\n", &mut lines, 100);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "ABc");
    }

    #[test]
    fn test_ansi_parser_cursor_positioning_multiline() {
        // 模拟 ConPTY 屏幕渲染：清屏 + 光标定位 + 多行文本
        let mut parser = AnsiParser::new();
        let mut lines = VecDeque::new();
        let input = b"\x1b[2J\x1b[HLine 1\x1b[2;1HLine 2\x1b[3;1HLine 3";
        parser.feed(input, &mut lines, 100);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "Line 1");
        assert_eq!(lines[1], "Line 2");
        assert_eq!(lines[2], "Line 3");
    }

    #[test]
    fn test_ansi_parser_cursor_positioning_with_col() {
        // 光标定位到指定列，验证列偏移写入
        let mut parser = AnsiParser::new();
        let mut lines = VecDeque::new();
        // 定位到行 1 列 5，写入 X
        parser.feed(b"\x1b[1;5HX", &mut lines, 100);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "    X"); // 4 个空格 + X
    }

    #[test]
    fn test_ansi_parser_clear_line_from_cursor() {
        // \x1b[K 清除从光标到行尾
        let mut parser = AnsiParser::new();
        let mut lines = VecDeque::new();
        parser.feed(b"hello\x1b[3G\x1b[K", &mut lines, 100);
        // \x1b[3G = 光标到列 3 (1-indexed → col 2)
        // \x1b[K = 清除从光标到行尾
        assert_eq!(lines[0], "he");
    }

    #[test]
    fn test_ansi_parser_osc_sequence() {
        // OSC 序列（设置窗口标题）应被跳过
        let mut parser = AnsiParser::new();
        let mut lines = VecDeque::new();
        parser.feed(b"\x1b]0;My Title\x07prompt$ ", &mut lines, 100);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "prompt$ ");
    }

    #[test]
    fn test_ansi_parser_crlf_preserves_content() {
        let mut parser = AnsiParser::new();
        let mut lines = VecDeque::new();
        // \r\n 应作为标准换行处理，不丢失行内容
        parser.feed(b"dir\r\n", &mut lines, 100);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "dir");
    }

    #[test]
    fn test_ansi_parser_crlf_multiple_lines() {
        let mut parser = AnsiParser::new();
        let mut lines = VecDeque::new();
        // 多行 \r\n 输出
        parser.feed(b"line1\r\nline2\r\nline3\r\n", &mut lines, 100);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "line1");
        assert_eq!(lines[1], "line2");
        assert_eq!(lines[2], "line3");
    }

    #[test]
    fn test_ansi_parser_cr_split_across_chunks() {
        let mut parser = AnsiParser::new();
        let mut lines = VecDeque::new();
        // \r 和 \n 跨越不同的 feed 调用（模拟网络分包）
        parser.feed(b"hello\r", &mut lines, 100);
        parser.feed(b"\nworld\n", &mut lines, 100);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "hello");
        assert_eq!(lines[1], "world");
    }

    #[test]
    fn test_ansi_parser_strips_sgr() {
        let mut parser = AnsiParser::new();
        let mut lines = VecDeque::new();
        // SGR 序列 \x1b[31m（红色）应被剥离
        parser.feed(b"\x1b[31mred\x1b[0m text\n", &mut lines, 100);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "red text");
    }

    #[test]
    fn test_ansi_parser_clear_screen() {
        let mut parser = AnsiParser::new();
        let mut lines = VecDeque::new();
        parser.feed(b"line1\nline2\n", &mut lines, 100);
        parser.feed(b"\x1b[2J", &mut lines, 100);
        // 清屏（ED 2/3J）只清空可见区域内行内容，保留滚动缓冲结构
        // —— 真实终端行为，参见 test_ansi_parser_cursor_positioning_multiline
        assert_eq!(lines.len(), 2);
        assert!(lines[0].is_empty());
        assert!(lines[1].is_empty());
    }

    #[test]
    fn test_ansi_parser_clear_screen_preserves_scrollback() {
        let mut parser = AnsiParser::new();
        parser.set_visible_rows(2); // 限制可见区为 2 行
        let mut lines = VecDeque::new();
        for i in 1..=5 {
            parser.feed(format!("line{}\n", i).as_bytes(), &mut lines, 100);
        }
        // 清屏：清空最后 visible_rows(2) 行内容，前 3 行作为滚动缓冲保留
        parser.feed(b"\x1b[2J", &mut lines, 100);
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "line1");
        assert_eq!(lines[1], "line2");
        assert_eq!(lines[2], "line3");
        assert_eq!(lines[3], "");
        assert_eq!(lines[4], "");
    }

    #[test]
    fn test_ansi_parser_backspace() {
        let mut parser = AnsiParser::new();
        let mut lines = VecDeque::new();
        parser.feed(b"abc\x08X\n", &mut lines, 100);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "abX");
    }

    #[test]
    fn test_ansi_parser_utf8_chinese() {
        let mut parser = AnsiParser::new();
        let mut lines = VecDeque::new();
        parser.feed("你好世界\n".as_bytes(), &mut lines, 100);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "你好世界");
    }
}
