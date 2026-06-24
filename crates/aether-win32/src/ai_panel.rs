use aether_ai::{AiClient, ChatMessage};

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

    /// 发送消息（同步阻塞，简化实现）
    pub fn send_message(
        &mut self,
        settings: &aether_shared::settings::AiSettings,
    ) -> Result<String, String> {
        if self.input.is_empty() {
            return Err("输入为空".to_string());
        }

        let user_input = self.input.clone();
        self.add_user_message(user_input);
        self.input.clear();
        self.is_generating = true;
        self.pending_response.clear();

        let client = AiClient::new(settings);
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

        match client.chat_completion(&messages) {
            Ok(response) => {
                self.add_assistant_message(response.clone());
                self.is_generating = false;
                Ok(response)
            }
            Err(e) => {
                self.is_generating = false;
                let err_msg = format!("请求失败: {}", e);
                self.add_assistant_message(err_msg.clone());
                Err(err_msg)
            }
        }
    }

    /// 使用快捷操作发送代码
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

        let prompt = action.build_prompt(code);
        self.add_user_message(format!("[{}]\n{}", action.label(), code));
        self.is_generating = true;

        let client = AiClient::new(settings);
        let messages = vec![ChatMessage::user(prompt)];

        match client.chat_completion(&messages) {
            Ok(response) => {
                self.add_assistant_message(response.clone());
                self.is_generating = false;
                Ok(response)
            }
            Err(e) => {
                self.is_generating = false;
                let err_msg = format!("请求失败: {}", e);
                self.add_assistant_message(err_msg.clone());
                Err(err_msg)
            }
        }
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
