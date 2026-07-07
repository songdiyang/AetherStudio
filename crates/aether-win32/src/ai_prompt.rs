use aether_ai::ChatMessage;
use aether_shared::settings::AiSettings;

/// AI 聊天/编辑模式
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiMode {
    /// 普通问答，不对输出做结构化解析
    Ask,
    /// 要求输出可应用到当前文件的编辑块
    Edit,
    /// 允许多步骤规划并输出多个文件编辑
    Agent,
}

impl AiMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::Edit => "Edit",
            Self::Agent => "Agent",
        }
    }
}

/// 根据设置、上下文、用户输入和模式构建聊天消息列表
pub fn build_chat_prompt(
    settings: &AiSettings,
    context: &str,
    user_input: &str,
    mode: AiMode,
) -> Vec<ChatMessage> {
    let mut messages = Vec::new();

    // system prompt
    let system_content = settings
        .system_prompt
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| default_system_prompt().to_string());
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: system_content,
    });

    // 模式指令追加到 system 后，作为一次性的行为说明
    let mode_instruction = mode_instruction(mode);
    if !mode_instruction.is_empty() {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: mode_instruction.to_string(),
        });
    }

    // 上下文
    if !context.is_empty() {
        messages.push(ChatMessage::user(format!(
            "这是当前项目的相关上下文：\n\n{}",
            context
        )));
    }

    // 用户输入
    messages.push(ChatMessage::user(user_input.to_string()));

    messages
}

fn default_system_prompt() -> &'static str {
    "你是一名专业的编程助手。请用中文回答，代码保持简洁、正确、可维护。"
}

fn mode_instruction(mode: AiMode) -> &'static str {
    match mode {
        AiMode::Ask => "",
        AiMode::Edit => {
            r#"当用户要求修改代码时，请按以下标记格式输出修改，以便编辑器直接应用：

<<<<<<< FILE 相对路径 >>>>>>>
...原代码片段（可为空表示全文件替换）...
=======
...修改后的代码片段...
>>>>>>> END FILE 相对路径 >>>>>>>

请确保原代码片段能在文件中被精确找到。如果只需要修改当前文件，可以省略路径。"#
        }
        AiMode::Agent => {
            r#"你是 Agent 模式。你可以规划多步骤任务，并通过以下标记格式批量修改项目文件：

<<<<<<< FILE 相对路径 >>>>>>>
...原代码片段（可为空表示全文件替换）...
=======
...修改后的代码片段...
>>>>>>> END FILE 相对路径 >>>>>>>

你可以创建新文件：把原代码片段留空即可。回答中请先简要说明计划，再给出修改块。"#
        }
    }
}
