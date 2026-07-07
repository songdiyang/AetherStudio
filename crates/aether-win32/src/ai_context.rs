/// AI 聊天上下文附件类型
///
/// 这些附件决定模型在回答问题时能看到哪些项目信息。
/// 附件本身不直接持有数据，只作为“需要从 EditorState 中读取什么”的标记。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiContextAttachment {
    /// 当前活动文件（含完整路径与内容）
    CurrentFile,
    /// 当前选中的文本（含所在文件路径与行列）
    Selection,
    /// 所有打开的文件（路径 + 内容摘要）
    OpenFiles,
    /// 当前文件/工作区的 LSP 诊断列表
    Diagnostics,
    /// 工作区文件树（最近修改的文件优先）
    FileTree,
    /// 用户自定义文本（如粘贴的日志、错误信息）
    CustomText(String),
}

impl AiContextAttachment {
    pub fn label(&self) -> String {
        match self {
            Self::CurrentFile => "当前文件".to_string(),
            Self::Selection => "选区".to_string(),
            Self::OpenFiles => "打开文件".to_string(),
            Self::Diagnostics => "诊断".to_string(),
            Self::FileTree => "文件树".to_string(),
            Self::CustomText(_) => "自定义文本".to_string(),
        }
    }

    /// 在 UI 上显示的短标识
    pub fn short_label(&self) -> String {
        match self {
            Self::CurrentFile => "📄 当前文件".to_string(),
            Self::Selection => "🖱 选区".to_string(),
            Self::OpenFiles => "📑 打开文件".to_string(),
            Self::Diagnostics => "⚠ 诊断".to_string(),
            Self::FileTree => "🌲 文件树".to_string(),
            Self::CustomText(_) => "📝 自定义".to_string(),
        }
    }
}

/// 把一个代码片段包装成带路径/语言标记的文本块
pub fn wrap_code_block(path: &str, language: &str, content: &str) -> String {
    format!(
        "```\n// file: {} (language: {})\n{}\n```\n",
        path, language, content
    )
}

/// 限制字符串长度，超出时保留首尾并在中间省略
pub fn truncate_middle(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let keep = max_len / 2;
    let head = &s[..s.floor_char_boundary(keep)];
    let tail_start = s.floor_char_boundary(s.len() - keep);
    let tail = &s[tail_start..];
    format!(
        "{}\n...（已省略 {} 字符）...\n{}",
        head,
        s.len() - max_len,
        tail
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attachment_labels() {
        assert_eq!(AiContextAttachment::CurrentFile.label(), "当前文件");
        assert_eq!(AiContextAttachment::Selection.short_label(), "🖱 选区");
        assert_eq!(
            AiContextAttachment::CustomText("日志".to_string()).label(),
            "自定义文本"
        );
    }

    #[test]
    fn test_wrap_code_block() {
        let wrapped = wrap_code_block("src/main.rs", "rust", "fn main() {}");
        assert!(wrapped.contains("```"));
        assert!(wrapped.contains("src/main.rs"));
        assert!(wrapped.contains("fn main() {}"));
    }

    #[test]
    fn test_truncate_middle() {
        let short = "hello";
        assert_eq!(truncate_middle(short, 10), "hello");

        let long = "abcdefghijklmnopqrstuvwxyz";
        let truncated = truncate_middle(long, 10);
        assert!(truncated.contains("..."));
        assert!(truncated.starts_with("abcde"));
        assert!(truncated.ends_with("vwxyz"));
    }
}
