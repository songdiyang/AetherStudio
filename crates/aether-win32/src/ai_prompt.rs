use aether_ai::ChatMessage;
use aether_shared::settings::AiSettings;

/// AI 聊天/编辑模式
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
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

/// 根据设置、上下文和模式构建"系统前缀"消息（system + Agent 能力 + 模式说明 + 工作区上下文）。
///
/// 注意：本函数**不含**对话历史与当前用户输入——调用方需在其后追加经窗口切片的会话历史
/// （见 `AiPanel::history_to_chat_messages`），以保证同一轮对话的上下文连续性。
pub fn build_chat_prompt(settings: &AiSettings, context: &str, mode: AiMode) -> Vec<ChatMessage> {
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

    // 始终注入 Agent 能力说明（除纯 Ask 模式外）：让 AI 明确知道自己拥有直接操作
    // 项目文件与终端的权限，避免回退到“我无法访问文件系统”的默认行为。
    if !matches!(mode, AiMode::Ask) {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: agent_capabilities_prompt().to_string(),
        });
    }

    // 模式指令追加到 system 后，作为一次性的行为说明
    let mode_instruction = mode_instruction(mode);
    if !mode_instruction.is_empty() {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: mode_instruction.to_string(),
        });
    }

    // 上下文（工作区/文件等附件）——作为一条 system 消息注入，随每轮请求提供当前上下文
    if !context.is_empty() {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: format!("这是当前项目的相关上下文：\n\n{}", context),
        });
    }

    messages
}

/// Agent 能力说明：强制告知 AI 拥有与用户同级的文件/终端操作权限。
///
/// 这段说明会作为独立的 system 消息注入，用于覆盖模型“我无法访问文件系统”的先验。
pub fn agent_capabilities_prompt() -> &'static str {
    r#"你是集成在 Aether 编辑器中的 AI 助手，拥有与用户完全同等的项目操作权限。你不是只能聊天或贴代码的助手——你可以直接在当前工作区创建、修改、删除文件和文件夹，也可以执行终端命令。编辑器会自动解析并执行你输出的下列标记，落盘到磁盘并刷新文件树，无需用户手动保存。

【创建 / 修改 / 删除文件】
<<<<<<< FILE 相对路径 >>>>>>>
原代码片段（创建新文件或整文件替换时此处留空）
=======
新内容（删除文件时此处留空）
>>>>>>> END FILE 相对路径 >>>>>>>

【执行终端命令】
<<<<<<< RUN >>>>>>>
命令一行一条
>>>>>>> END RUN >>>>>>>

必须遵守：
1. 当用户要求“生成/创建/新建/写一个……文件或脚本”时，必须使用 FILE 标记直接创建文件，而不是只贴代码块或让用户点“保存”。
2. 当用户要求“修改/直接改文件”时，使用 FILE 标记给出修改（原片段需能在目标文件中精确匹配；整文件替换时原片段留空）。
3. 路径相对于当前打开的工作区根目录；路径中不存在的目录会被自动创建（支持 src/utils/x.py 这类多级目录）。
4. 需要运行/编译/安装时，用 RUN 标记在集成终端执行命令（Windows PowerShell）。
5. 严禁输出“我无法访问文件系统”“请你手动保存/复制”之类的话——你确实有权限直接操作，直接给出标记即可。
6. 先用一两句话简述你的操作计划，然后给出标记块。"#
}

fn default_system_prompt() -> &'static str {
    "你是一名专业的编程助手。请用中文回答，代码保持简洁、正确、可维护。"
}

fn mode_instruction(mode: AiMode) -> &'static str {
    match mode {
        AiMode::Ask => "",
        AiMode::Edit => {
            r#"当前为 Edit 模式：你给出的 FILE 标记修改会先以差异预览形式展示，由用户确认后再应用。请聚焦于当前文件的精确修改。"#
        }
        AiMode::Agent => {
            r#"当前为 Agent 模式：你可以规划多步骤任务，直接创建/修改/删除文件并执行终端命令，无需用户逐步确认。请主动、完整地完成用户的目标。"#
        }
    }
}
