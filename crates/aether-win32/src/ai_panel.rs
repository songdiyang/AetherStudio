use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aether_ai::{AiClient, AiStreamEvent, ChatMessage};
use aether_shared::settings::AiSettings;

use crate::ai_agent::{parse_edits, AiEdit};
use crate::ai_context::{truncate_middle, AiContextAttachment};
use crate::ai_prompt::{build_chat_prompt, AiMode};
use crate::diff_view::DiffView;
use crate::editor::EditorState;

/// 脱敏错误消息，避免泄漏 API 密钥等敏感信息
/// SEC-C04: 用于 test_connection 路径等所有 UI 错误展示
/// AI-M04: 扩展覆盖 x-api-key、URL 参数、响应体中的密钥
/// H-02: 循环移除所有 Bearer/x-api-key/authorization 出现，而非仅首个
///
/// 注意：当前代码路径已改用 `AiError::safe_display()`，此函数保留供
/// 需要对原始字符串（如日志）做脱敏的场景使用。
#[allow(dead_code)]
pub fn sanitize_error(err: &str) -> String {
    let mut result = err.to_string();

    // H-02: 循环移除所有 "Bearer xxx" 出现（之前仅处理首个，多 Token 时第二个泄露）
    while let Some(pos) = result.find("Bearer ") {
        let start = pos + 7;
        let end = result[start..]
            .find(|c: char| c.is_whitespace() || c == '\n' || c == '\r')
            .map(|p| start + p)
            .unwrap_or(result.len());
        if end > start {
            result.replace_range(start..end, "[REDACTED]");
        } else {
            break;
        }
    }
    // H-02: 循环移除所有 x-api-key 头（支持冒号和等号分隔，大小写不敏感）
    let lower = result.to_lowercase();
    let mut search_from = 0;
    while let Some(rel_pos) = lower[search_from..].find("x-api-key") {
        let pos = search_from + rel_pos;
        // 跳过 "x-api-key" 本身（9 字符）
        let mut value_start = pos + 9;
        // 跳过分隔符（: 或 =）和可选空格
        let rest = &result[value_start..];
        let trimmed_start = rest
            .find(|c: char| c != ':' && c != '=' && c != ' ' && c != '\t')
            .map(|p| value_start + p)
            .unwrap_or(value_start);
        value_start = trimmed_start;
        let end = result[value_start..]
            .find(|c: char| ['\n', '\r'].contains(&c))
            .map(|p| value_start + p)
            .unwrap_or(result.len());
        if end > value_start {
            result.replace_range(value_start..end, "[REDACTED]");
        }
        search_from = pos + 9;
        if search_from >= result.len() {
            break;
        }
    }

    // H-02: 循环移除所有 authorization 头（大小写不敏感）
    let lower = result.to_lowercase();
    let mut search_from = 0;
    while let Some(rel_pos) = lower[search_from..].find("authorization") {
        let pos = search_from + rel_pos;
        let mut value_start = pos + 13; // "authorization" = 13 字符
        let rest = &result[value_start..];
        let trimmed_start = rest
            .find(|c: char| ![':', '=', ' ', '\t'].contains(&c))
            .map(|p| value_start + p)
            .unwrap_or(value_start);
        value_start = trimmed_start;
        let end = result[value_start..]
            .find(|c: char| ['\n', '\r'].contains(&c))
            .map(|p| value_start + p)
            .unwrap_or(result.len());
        if end > value_start {
            result.replace_range(value_start..end, "[REDACTED]");
        }
        search_from = pos + 13;
        if search_from >= result.len() {
            break;
        }
    }

    // 限制长度（H-02: 在 UTF-8 字符边界截断，避免半截 Token 可见）
    if result.len() > 500 {
        let safe_len = result.floor_char_boundary(500);
        result.truncate(safe_len);
        result.push_str("...");
    }
    result
}

/// AI 助手消息
#[derive(Clone, Debug)]
pub struct AiMessage {
    pub role: AiRole,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiRole {
    User,
    Assistant,
    System,
}

/// AI 快捷操作类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiQuickAction {
    Explain,
    Refactor,
    Fix,
    Complete,
    Comment,
    Optimize,
    Test,
    Doc,
}

impl AiQuickAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Explain => "解释代码",
            Self::Refactor => "重构代码",
            Self::Fix => "修复问题",
            Self::Complete => "补全代码",
            Self::Comment => "添加注释",
            Self::Optimize => "优化性能",
            Self::Test => "生成测试",
            Self::Doc => "生成文档",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Explain => "💡",
            Self::Refactor => "🔧",
            Self::Fix => "🩹",
            Self::Complete => "✨",
            Self::Comment => "📝",
            Self::Optimize => "🚀",
            Self::Test => "🧪",
            Self::Doc => "📚",
        }
    }

    pub fn build_prompt(&self, code: &str) -> String {
        match self {
            Self::Explain => format!("请解释以下代码的功能和工作原理，用中文回答：\n\n```\n{}\n```", code),
            Self::Refactor => format!("请重构以下代码，提高可读性和可维护性，保持功能不变，用中文简要说明修改：\n\n```\n{}\n```", code),
            Self::Fix => format!("以下代码可能有问题，请分析并修复，用中文说明问题：\n\n```\n{}\n```", code),
            Self::Complete => format!("请补全以下代码（继续编写后续逻辑）：\n\n```\n{}\n```", code),
            Self::Comment => format!("请为以下代码添加清晰的中文注释：\n\n```\n{}\n```", code),
            Self::Optimize => format!("请优化以下代码的性能，用中文说明优化点：\n\n```\n{}\n```", code),
            Self::Test => format!("请为以下代码生成单元测试（使用适当的测试框架），用中文说明：\n\n```\n{}\n```", code),
            Self::Doc => format!("请为以下代码生成文档说明（函数文档、参数说明等），用中文：\n\n```\n{}\n```", code),
        }
    }
}

/// 流式响应的共享状态
#[derive(Clone, Debug, Default)]
pub struct AiStreamState {
    /// 已累积但尚未被 UI 取走的 token
    pub partial: String,
    /// 流是否已结束
    pub done: bool,
    /// 流式过程中发生的错误
    pub error: Option<String>,
}

/// AI 助手面板状态
#[derive(Clone, Debug)]
pub struct AiPanel {
    /// 是否可见
    pub visible: bool,
    /// 聊天历史
    pub messages: Vec<AiMessage>,
    /// 当前输入
    pub input: String,
    /// 是否正在生成回复
    pub is_generating: bool,
    /// 滚动偏移
    pub scroll_y: f32,
    /// 选中的快捷操作
    pub selected_action: Option<AiQuickAction>,
    /// 悬停的快捷操作
    pub hover_action: Option<AiQuickAction>,
    /// Apply 按钮悬停状态
    pub hover_apply_button: bool,
    /// 快捷操作行数（用于滚动计算）
    pub action_rows: usize,
    /// AI-H01: 后台线程流式状态，UI 渲染时轮询此字段
    pub stream_state: Arc<Mutex<AiStreamState>>,
    /// C-10: 输入框是否聚焦。仅当聚焦时才拦截键盘输入，避免面板可见即劫持编辑器
    pub input_focused: bool,
    /// 当前 AI 模式（Ask / Edit / Agent）
    pub mode: AiMode,
    /// 已附加的上下文项
    pub attachments: Vec<AiContextAttachment>,
    /// 模式切换按钮命中区域 (mode, x, y, w, h)
    pub mode_button_regions: Vec<(AiMode, f32, f32, f32, f32)>,
    /// 附件 chip 命中区域 (index, x, y, w, h)
    pub attachment_chip_regions: Vec<(usize, f32, f32, f32, f32)>,
    /// 变更列表项命中区域 (index, x, y, w, h) — 0=预览, 1=接受, 2=拒绝
    pub change_action_regions: Vec<(usize, u8, f32, f32, f32, f32)>,
    /// 悬停的附件 chip 索引
    pub hover_attachment: Option<usize>,
    /// 当前等待用户确认的 AI 编辑（Agent/Edit 模式）
    pub pending_edits: Vec<AiEdit>,
    /// Diff 预览视图
    pub diff_view: DiffView,
    /// 是否显示 diff 预览
    pub show_diff_view: bool,
    /// 变更列表中当前选中的索引
    pub selected_change_index: usize,
}

impl AiPanel {
    pub fn new() -> Self {
        Self {
            visible: false,
            messages: vec![AiMessage {
                role: AiRole::System,
                content: "你好！我是 AI 助手，可以帮助你解释代码、重构、修复问题、生成测试等。你可以直接输入问题，或选中代码后使用快捷操作。".to_string(),
            }],
            input: String::new(),
            is_generating: false,
            scroll_y: 0.0,
            selected_action: None,
            hover_action: None,
            hover_apply_button: false,
            action_rows: 2,
            stream_state: Arc::new(Mutex::new(AiStreamState::default())),
            input_focused: false,
            mode: AiMode::Ask,
            attachments: Vec::new(),
            mode_button_regions: Vec::new(),
            attachment_chip_regions: Vec::new(),
            change_action_regions: Vec::new(),
            hover_attachment: None,
            pending_edits: Vec::new(),
            diff_view: DiffView::new(),
            show_diff_view: false,
            selected_change_index: 0,
        }
    }

    /// 添加用户消息
    pub fn add_user_message(&mut self, content: String) {
        self.messages.push(AiMessage {
            role: AiRole::User,
            content,
        });
    }

    /// 添加助手消息
    pub fn add_assistant_message(&mut self, content: String) {
        self.messages.push(AiMessage {
            role: AiRole::Assistant,
            content,
        });
    }

    /// 获取所有快捷操作
    pub fn quick_actions() -> &'static [AiQuickAction] {
        &[
            AiQuickAction::Explain,
            AiQuickAction::Refactor,
            AiQuickAction::Fix,
            AiQuickAction::Complete,
            AiQuickAction::Comment,
            AiQuickAction::Optimize,
            AiQuickAction::Test,
            AiQuickAction::Doc,
        ]
    }

    /// 发送消息（AI-H01: 非阻塞 — HTTP 调用在后台线程执行，结果通过 stream_state 流式返回）
    pub fn send_message(&mut self, settings: &AiSettings) -> Result<String, String> {
        self.send_message_internal(settings, self.input.clone(), AiMode::Ask, None)
    }

    /// 发送消息，并附带当前编辑器的上下文
    pub fn send_message_with_context(
        &mut self,
        settings: &AiSettings,
        editor: &EditorState,
        mode: AiMode,
    ) -> Result<String, String> {
        let context = editor.gather_context(&self.attachments);
        self.send_message_internal(settings, self.input.clone(), mode, Some(context))
    }

    /// 发送消息，使用已经准备好的上下文字符串
    pub fn send_message_with_prepared_context(
        &mut self,
        settings: &AiSettings,
        context: String,
        mode: AiMode,
    ) -> Result<String, String> {
        self.send_message_internal(settings, self.input.clone(), mode, Some(context))
    }

    fn send_message_internal(
        &mut self,
        settings: &AiSettings,
        user_input: String,
        mode: AiMode,
        context: Option<String>,
    ) -> Result<String, String> {
        if user_input.is_empty() {
            return Err("输入为空".to_string());
        }

        // H-17: 限制并发线程数 — 正在生成时拒绝新请求，防止无限制 spawn 线程
        if self.is_generating {
            return Err("正在等待上一次回复，请稍后再试".to_string());
        }

        // 限制输入长度（M-03）
        const MAX_INPUT_LEN: usize = 10000;
        let user_input = if user_input.len() > MAX_INPUT_LEN {
            let safe_len = user_input.floor_char_boundary(MAX_INPUT_LEN);
            user_input[..safe_len].to_string()
        } else {
            user_input
        };

        self.add_user_message(user_input.clone());
        self.input.clear();
        self.is_generating = true;
        self.clear_pending_changes();
        // 重置流式状态
        if let Ok(mut s) = self.stream_state.lock() {
            *s = AiStreamState::default();
        }

        // 限制消息历史长度（M-05: 滑动窗口，保留最近 20 条）
        const MAX_HISTORY: usize = 20;
        if self.messages.len() > MAX_HISTORY + 1 {
            let system_msgs: Vec<AiMessage> = self
                .messages
                .iter()
                .filter(|m| m.role == AiRole::System)
                .cloned()
                .collect();
            let non_system: Vec<AiMessage> = self
                .messages
                .iter()
                .filter(|m| m.role != AiRole::System)
                .cloned()
                .collect();
            let recent_start = non_system.len().saturating_sub(MAX_HISTORY);
            let recent: Vec<AiMessage> = non_system.into_iter().skip(recent_start).collect();
            self.messages = system_msgs;
            self.messages.extend(recent);
        }

        let settings = settings.clone();
        let context = context.unwrap_or_default();
        let messages = build_chat_prompt(&settings, &context, &user_input, mode);
        let stream_state = Arc::clone(&self.stream_state);

        std::thread::spawn(move || {
            let client = AiClient::new(&settings);
            match client.chat_completion_stream(&messages) {
                Ok(rx) => {
                    while let Ok(event) = rx.recv() {
                        match event {
                            AiStreamEvent::Token(token) => {
                                if let Ok(mut s) = stream_state.lock() {
                                    s.partial.push_str(&token);
                                }
                            }
                            AiStreamEvent::Done => {
                                if let Ok(mut s) = stream_state.lock() {
                                    s.done = true;
                                }
                                break;
                            }
                            AiStreamEvent::Error(err) => {
                                if let Ok(mut s) = stream_state.lock() {
                                    s.error = Some(format!("请求失败: {}", sanitize_error(&err)));
                                    s.done = true;
                                }
                                break;
                            }
                        }
                    }
                }
                // H-21: 使用 safe_display() 替代 sanitize_error(&Display)，
                // 不包含已截断但仍可能有敏感信息的 API 响应体
                Err(e) => {
                    if let Ok(mut s) = stream_state.lock() {
                        s.error = Some(format!("请求失败: {}", e.safe_display()));
                        s.done = true;
                    }
                }
            }
        });

        Ok("请求已提交".to_string())
    }

    /// 使用快捷操作发送代码（AI-H01: 非阻塞流式版本）
    pub fn send_quick_action(
        &mut self,
        action: AiQuickAction,
        code: &str,
        settings: &AiSettings,
    ) -> Result<String, String> {
        if code.trim().is_empty() {
            let msg = "请先打开文件或输入代码，再使用 AI 快捷操作。".to_string();
            self.add_assistant_message(msg.clone());
            return Ok(msg);
        }

        // H-17: 限制并发线程数 — 正在生成时拒绝新请求，防止无限制 spawn 线程
        if self.is_generating {
            return Err("正在等待上一次回复，请稍后再试".to_string());
        }

        // 限制代码长度（M-03）
        const MAX_CODE_LEN: usize = 50000;
        let code = if code.len() > MAX_CODE_LEN {
            let safe_len = code.floor_char_boundary(MAX_CODE_LEN);
            &code[..safe_len]
        } else {
            code
        };

        let prompt = action.build_prompt(code);
        self.add_user_message(format!("[{}]\n{}", action.label(), code));
        self.is_generating = true;
        self.clear_pending_changes();
        if let Ok(mut s) = self.stream_state.lock() {
            *s = AiStreamState::default();
        }

        let settings = settings.clone();
        let messages = vec![ChatMessage::user(prompt)];
        let stream_state = Arc::clone(&self.stream_state);

        std::thread::spawn(move || {
            let client = AiClient::new(&settings);
            match client.chat_completion_stream(&messages) {
                Ok(rx) => {
                    while let Ok(event) = rx.recv() {
                        match event {
                            AiStreamEvent::Token(token) => {
                                if let Ok(mut s) = stream_state.lock() {
                                    s.partial.push_str(&token);
                                }
                            }
                            AiStreamEvent::Done => {
                                if let Ok(mut s) = stream_state.lock() {
                                    s.done = true;
                                }
                                break;
                            }
                            AiStreamEvent::Error(err) => {
                                if let Ok(mut s) = stream_state.lock() {
                                    s.error = Some(format!("请求失败: {}", sanitize_error(&err)));
                                    s.done = true;
                                }
                                break;
                            }
                        }
                    }
                }
                // H-21: 使用 safe_display() 替代 sanitize_error(&Display)，
                // 不包含已截断但仍可能有敏感信息的 API 响应体
                Err(e) => {
                    if let Ok(mut s) = stream_state.lock() {
                        s.error = Some(format!("请求失败: {}", e.safe_display()));
                        s.done = true;
                    }
                }
            }
        });

        Ok("请求已提交".to_string())
    }

    /// 输入字符
    pub fn input_char(&mut self, ch: char) {
        self.input.push(ch);
    }

    /// 退格
    pub fn backspace(&mut self) {
        self.input.pop();
    }

    /// 清除输入
    pub fn clear_input(&mut self) {
        self.input.clear();
    }

    /// 清除所有对话
    pub fn clear_history(&mut self) {
        self.messages.clear();
        self.messages.push(AiMessage {
            role: AiRole::System,
            content: "你好！我是 AI 助手，可以帮助你解释代码、重构、修复问题、生成测试等。"
                .to_string(),
        });
        if let Ok(mut s) = self.stream_state.lock() {
            *s = AiStreamState::default();
        }
        self.is_generating = false;
    }

    /// AI-H01: 轮询后台线程结果，应在渲染循环中调用
    pub fn check_background_result(&mut self) {
        if !self.is_generating {
            return;
        }
        let delta = {
            if let Ok(mut s) = self.stream_state.lock() {
                let partial = std::mem::take(&mut s.partial);
                let done = s.done;
                let error = s.error.take();
                if done {
                    s.done = false;
                }
                Some((partial, done, error))
            } else {
                None
            }
        };
        if let Some((partial, done, error)) = delta {
            if !partial.is_empty() {
                if let Some(last) = self.messages.last_mut() {
                    if last.role == AiRole::Assistant {
                        last.content.push_str(&partial);
                    } else {
                        self.add_assistant_message(partial);
                    }
                } else {
                    self.add_assistant_message(partial);
                }
            }
            if let Some(err) = error {
                self.add_assistant_message(err);
                self.is_generating = false;
                return;
            }
            if done {
                self.is_generating = false;
                if matches!(self.mode, AiMode::Edit | AiMode::Agent) {
                    self.parse_pending_edits(None);
                }
            }
        }
    }

    /// 解析最后一条助手消息中的编辑标记，生成 diff 预览
    pub fn parse_pending_edits(&mut self, default_path: Option<&Path>) {
        let Some(text) = self.last_assistant_text() else {
            return;
        };
        let default = default_path.map(|p| p.to_string_lossy().to_string());
        self.pending_edits = parse_edits(&text, default.as_deref());
        self.diff_view = DiffView::from_edits(&self.pending_edits, None);
        self.show_diff_view = !self.pending_edits.is_empty();
    }

    /// 在已知工作区目录的情况下重新生成 diff 视图
    pub fn rebuild_diff_view(&mut self, current_folder: Option<&Path>) {
        self.diff_view = DiffView::from_edits(&self.pending_edits, current_folder);
    }

    /// 清除当前待确认的编辑
    pub fn clear_pending_changes(&mut self) {
        self.pending_edits.clear();
        self.diff_view = DiffView::new();
        self.show_diff_view = false;
        self.selected_change_index = 0;
    }

    /// 接受所有已接受的编辑并应用到编辑器
    pub fn apply_accepted_changes(
        &mut self,
        editor: &mut EditorState,
    ) -> std::result::Result<Vec<PathBuf>, String> {
        let edits: Vec<AiEdit> = self.diff_view.to_edits();
        if edits.is_empty() {
            return Ok(Vec::new());
        }
        let applied = editor.apply_ai_workspace_edits(&edits)?;
        self.clear_pending_changes();
        Ok(applied)
    }

    /// 拒绝所有待确认编辑
    pub fn reject_all_changes(&mut self) {
        self.clear_pending_changes();
    }

    /// 从最后一条助手消息中提取代码块
    pub fn extract_last_code_block(&self) -> Option<String> {
        for msg in self.messages.iter().rev() {
            if msg.role == AiRole::Assistant {
                return Self::extract_code_blocks(&msg.content);
            }
        }
        None
    }

    /// 提取所有代码块（```...``` 之间的内容）
    fn extract_code_blocks(text: &str) -> Option<String> {
        let mut result = String::new();
        let mut in_code = false;
        let mut code_content = String::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                if in_code {
                    if !code_content.is_empty() {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str(&code_content);
                    }
                    code_content.clear();
                    in_code = false;
                } else {
                    in_code = true;
                }
            } else if in_code {
                if !code_content.is_empty() {
                    code_content.push('\n');
                }
                code_content.push_str(line);
            }
        }

        // AI-L01: 未闭合代码围栏时，将累积内容也加入结果
        if in_code && !code_content.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&code_content);
        }

        if !result.is_empty() {
            Some(result)
        } else {
            None
        }
    }

    /// 获取最后一条助手消息的纯文本（去掉代码块标记）
    pub fn last_assistant_text(&self) -> Option<String> {
        for msg in self.messages.iter().rev() {
            if msg.role == AiRole::Assistant {
                return Some(msg.content.clone());
            }
        }
        None
    }

    /// 切换附件：已存在则移除，否则添加
    pub fn toggle_attachment(&mut self, attachment: AiContextAttachment) {
        let pos = self
            .attachments
            .iter()
            .position(|a| match (a, &attachment) {
                (AiContextAttachment::CurrentFile, AiContextAttachment::CurrentFile) => true,
                (AiContextAttachment::Selection, AiContextAttachment::Selection) => true,
                (AiContextAttachment::OpenFiles, AiContextAttachment::OpenFiles) => true,
                (AiContextAttachment::Diagnostics, AiContextAttachment::Diagnostics) => true,
                (AiContextAttachment::FileTree, AiContextAttachment::FileTree) => true,
                (AiContextAttachment::CustomText(x), AiContextAttachment::CustomText(y)) => x == y,
                _ => false,
            });
        if let Some(idx) = pos {
            self.attachments.remove(idx);
        } else {
            self.attachments.push(attachment);
        }
    }

    /// 清除所有上下文附件
    pub fn clear_attachments(&mut self) {
        self.attachments.clear();
    }

    /// 当前已附加的上下文文本摘要（用于 UI 展示）
    pub fn attachment_summary(&self) -> String {
        if self.attachments.is_empty() {
            return String::new();
        }
        let labels: Vec<String> = self.attachments.iter().map(|a| a.short_label()).collect();
        format!("上下文: {}", labels.join(" "))
    }

    /// 限制并格式化自定义文本附件
    pub fn prepare_custom_text(text: &str) -> AiContextAttachment {
        AiContextAttachment::CustomText(truncate_middle(text, 2000))
    }

    /// 命中测试：模式切换按钮
    pub fn hit_test_mode_button(&self, px: f32, py: f32) -> Option<AiMode> {
        for (mode, x, y, w, h) in &self.mode_button_regions {
            if px >= *x && px <= *x + *w && py >= *y && py <= *y + *h {
                return Some(*mode);
            }
        }
        None
    }

    /// 命中测试：附件 chip（返回索引）
    pub fn hit_test_attachment(&self, px: f32, py: f32) -> Option<usize> {
        for (idx, x, y, w, h) in &self.attachment_chip_regions {
            if px >= *x && px <= *x + *w && py >= *y && py <= *y + *h {
                return Some(*idx);
            }
        }
        None
    }

    /// 命中测试：变更列表操作按钮 (文件索引, 操作类型 0=预览 1=接受 2=拒绝)
    pub fn hit_test_change_action(&self, px: f32, py: f32) -> Option<(usize, u8)> {
        for (idx, action, x, y, w, h) in &self.change_action_regions {
            if px >= *x && px <= *x + *w && py >= *y && py <= *y + *h {
                return Some((*idx, *action));
            }
        }
        None
    }

    /// 清除所有命中区域（每帧渲染前调用）
    pub fn clear_hit_regions(&mut self) {
        self.mode_button_regions.clear();
        self.attachment_chip_regions.clear();
        self.change_action_regions.clear();
    }
}
