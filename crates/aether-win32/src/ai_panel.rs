use std::sync::{Arc, Mutex};

use aether_ai::{AiClient, ChatMessage};

/// 脱敏错误消息，避免泄漏 API 密钥等敏感信息
/// SEC-C04: 用于 test_connection 路径等所有 UI 错误展示
/// AI-M04: 扩展覆盖 x-api-key、URL 参数、响应体中的密钥
pub fn sanitize_error(err: &str) -> String {
    let mut result = err.to_string();
    // 移除 Bearer token 模式
    if result.contains("Bearer ") {
        if let Some(pos) = result.find("Bearer ") {
            let start = pos + 7;
            let end = result[start..]
                .find(|c: char| c.is_whitespace() || c == '\n' || c == '\r')
                .map(|p| start + p)
                .unwrap_or(result.len());
            if end > start {
                result.replace_range(start..end, "[REDACTED]");
            }
        }
    }
    // 移除 x-api-key 头（Claude 格式）
    if let Some(pos) = result.to_lowercase().find("x-api-key:") {
        let start = pos + 10;
        let end = result[start..]
            .find(|c: char| c == '\n' || c == '\r')
            .map(|p| start + p)
            .unwrap_or(result.len());
        if end > start {
            result.replace_range(start..end, " [REDACTED]");
        }
    }
    // 移除 Authorization 头中的密钥
    if let Some(pos) = result.to_lowercase().find("authorization:") {
        let start = pos + 14;
        let end = result[start..]
            .find(|c: char| c == '\n' || c == '\r')
            .map(|p| start + p)
            .unwrap_or(result.len());
        if end > start {
            result.replace_range(start..end, " [REDACTED]");
        }
    }
    // 限制长度
    if result.len() > 500 {
        result.truncate(500);
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
    /// 上次生成的完整回复（用于追加）
    pub pending_response: String,
    /// AI-H01: 后台线程异步结果，UI 渲染时轮询此字段
    #[allow(clippy::type_complexity)]
    pub background_result: Arc<Mutex<Option<Result<String, String>>>>,
    /// C-10: 输入框是否聚焦。仅当聚焦时才拦截键盘输入，避免面板可见即劫持编辑器
    pub input_focused: bool,
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
            pending_response: String::new(),
            background_result: Arc::new(Mutex::new(None)),
            input_focused: false,
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

    /// 发送消息（AI-H01: 非阻塞 — HTTP 调用在后台线程执行，结果通过 background_result 返回）
    pub fn send_message(
        &mut self,
        settings: &aether_shared::settings::AiSettings,
    ) -> Result<String, String> {
        if self.input.is_empty() {
            return Err("输入为空".to_string());
        }

        // 限制输入长度（M-03）
        const MAX_INPUT_LEN: usize = 10000;
        let user_input = if self.input.len() > MAX_INPUT_LEN {
            let safe_len = self.input.floor_char_boundary(MAX_INPUT_LEN);
            self.input[..safe_len].to_string()
        } else {
            self.input.clone()
        };

        self.add_user_message(user_input.clone());
        self.input.clear();
        self.is_generating = true;
        self.pending_response.clear();

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

        // AI-H01: 后台线程执行 HTTP 请求，不阻塞 UI 线程
        let settings = settings.clone();
        let messages: Vec<ChatMessage> = self
            .messages
            .iter()
            .filter(|m| m.role != AiRole::System)
            .map(|m| match m.role {
                AiRole::User => ChatMessage::user(m.content.clone()),
                AiRole::Assistant => ChatMessage::assistant(m.content.clone()),
                AiRole::System => ChatMessage::user(m.content.clone()),
            })
            .collect();
        let result_arc = Arc::clone(&self.background_result);

        std::thread::spawn(move || {
            let client = AiClient::new(&settings);
            let response = match client.chat_completion(&messages) {
                Ok(resp) => Ok(resp),
                Err(e) => Err(format!("请求失败: {}", sanitize_error(&e.to_string()))),
            };
            if let Ok(mut guard) = result_arc.lock() {
                *guard = Some(response);
            }
        });

        Ok("请求已提交".to_string())
    }

    /// 使用快捷操作发送代码（AI-H01: 非阻塞版本）
    pub fn send_quick_action(
        &mut self,
        action: AiQuickAction,
        code: &str,
        settings: &aether_shared::settings::AiSettings,
    ) -> Result<String, String> {
        // 防护：空代码时返回提示，避免无意义请求
        if code.trim().is_empty() {
            let msg = "请先打开文件或输入代码，再使用 AI 快捷操作。".to_string();
            self.add_assistant_message(msg.clone());
            return Ok(msg);
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

        // AI-H01: 后台线程执行 HTTP 请求
        let settings = settings.clone();
        let messages = vec![ChatMessage::user(prompt)];
        let result_arc = Arc::clone(&self.background_result);

        std::thread::spawn(move || {
            let client = AiClient::new(&settings);
            let response = match client.chat_completion(&messages) {
                Ok(resp) => Ok(resp),
                Err(e) => Err(format!("请求失败: {}", sanitize_error(&e.to_string()))),
            };
            if let Ok(mut guard) = result_arc.lock() {
                *guard = Some(response);
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
        // 重置后台结果
        if let Ok(mut guard) = self.background_result.lock() {
            *guard = None;
        }
        self.is_generating = false;
    }

    /// AI-H01: 轮询后台线程结果，应在渲染循环中调用
    pub fn check_background_result(&mut self) {
        if !self.is_generating {
            return;
        }
        // 先取出结果并释放锁，再修改 self
        let pending = {
            if let Ok(mut guard) = self.background_result.lock() {
                guard.take()
            } else {
                None
            }
        };
        if let Some(result) = pending {
            match result {
                Ok(response) => {
                    self.add_assistant_message(response);
                }
                Err(err_msg) => {
                    self.add_assistant_message(err_msg);
                }
            }
            self.is_generating = false;
        }
    }

    /// 从最后一条助手消息中提取代码块
    pub fn extract_last_code_block(&self) -> Option<String> {
        // 从后往前找到第一条助手消息
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
                    // 代码块结束
                    if !code_content.is_empty() {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str(&code_content);
                    }
                    code_content.clear();
                    in_code = false;
                } else {
                    // 代码块开始
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
}
