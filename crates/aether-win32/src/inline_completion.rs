/// Inline Completion（幽灵文本）状态管理
///
/// P3.1: 为 AI 写代码提供最小数据结构。当前不绑定具体 AI provider，
/// 只保存建议文本、触发位置、接受状态，并提供生命周期控制。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineCompletion {
    /// 建议插入的完整文本
    pub text: String,
    /// 触发建议时的光标行
    pub trigger_line: usize,
    /// 触发建议时的光标列（字节偏移）
    pub trigger_col: usize,
    /// 建议版本号，用于区分新旧建议
    pub version: u64,
}

impl InlineCompletion {
    pub fn new(text: String, trigger_line: usize, trigger_col: usize, version: u64) -> Self {
        Self {
            text,
            trigger_line,
            trigger_col,
            version,
        }
    }

    /// 建议是否为空
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// Inline Completion 服务
///
/// P3.1→P3.5: 从占位升级为基于模式的智能补全。
/// 当用户输入特定关键字时提供代码片段建议（如 `fn`→`fn name() {\n    \n}`）。
/// 后续可扩展为异步请求 aether-ai 模型。
pub struct InlineCompletionService {
    counter: u64,
}

impl InlineCompletionService {
    pub fn new() -> Self {
        Self { counter: 0 }
    }

    /// 根据当前上下文请求建议。
    ///
    /// 基于光标前缀匹配常见代码模式，返回对应的代码片段。
    /// 返回 `None` 表示无匹配建议。
    pub fn request(&mut self, prefix: &str, _suffix: &str) -> Option<InlineCompletion> {
        let suggestion = suggest_completion(prefix)?;
        self.counter += 1;
        Some(InlineCompletion::new(
            suggestion,
            0,
            0,
            self.counter,
        ))
    }

    /// 取消当前请求（占位：异步实现时取消 in-flight 请求）
    pub fn cancel(&mut self) {
        // 占位：异步实现时取消 in-flight 请求
    }
}

impl Default for InlineCompletionService {
    fn default() -> Self {
        Self::new()
    }
}

/// 基于前缀的代码片段建议（语言无关的常见模式）。
///
/// 匹配规则：取前缀末尾的“单词”（连续字母/下划线/斜杠），
/// 如果该单词是已知关键字则返回对应片段。
/// 片段中 `\n` 表示换行，`    ` 表示缩进。
pub fn suggest_completion(prefix: &str) -> Option<String> {
    // 特殊处理：注释标记（/* 中 * 不是单词字符，需单独匹配）
    if prefix.ends_with("/*") {
        return Some("  */".to_string());
    }

    // 提取前缀末尾的单词及其前面的文本
    let (before, word) = extract_last_word_with_prefix(prefix)?;
    if word.is_empty() {
        return None;
    }

    // 检查关键字前面是否只有空白（用于区分声明 vs 调用）
    let is_line_start = before.chars().all(|c| c.is_whitespace());

    let suggestion = match word {
        "fn" if is_line_start => " name() {\n    \n}",
        "if" if is_line_start => " condition {\n    \n}",
        "for" if is_line_start => " item in iterable {\n    \n}",
        "while" if is_line_start => " condition {\n    \n}",
        "match" if is_line_start => " value {\n    _ => {}\n}",
        "let" if is_line_start => " name = ",
        "let" => " name = ",
        "mut" if is_line_start => " ",
        "pub" if is_line_start => " ",
        "struct" if is_line_start => " Name {\n    \n}",
        "enum" if is_line_start => " Name {\n    \n}",
        "impl" if is_line_start => " Type {\n    \n}",
        "trait" if is_line_start => " Name {\n    \n}",
        "use" if is_line_start => " ",
        "mod" if is_line_start => " ",
        "const" if is_line_start => " NAME: Type = ",
        "static" if is_line_start => " NAME: Type = ",
        "return" if is_line_start => " ",
        "//" => " ",
        "todo" => "!()",
        "dbg" => "!()",
        "println" => "!()",
        "print" => "!()",
        "eprintln" => "!()",
        "vec" => "![",
        _ => return None,
    };

    Some(suggestion.to_string())
}

/// 提取字符串末尾的“单词”（连续的字母、下划线、斜杠）及其前面的文本。
///
/// 返回 `(before, word)`，其中 `before` 是单词之前的所有文本，`word` 是末尾单词。
/// 例如 `"foo bar fn"` → `Some(("foo bar ", "fn"))`，
/// `"abc123"` → `Some(("123", "abc"))`（注：数字被跳过，"abc" 是末尾单词），
/// `"//"` → `Some(("", "//"))`。
fn extract_last_word_with_prefix(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    // Step 1: 跳过尾部非单词字符，找到单词结尾
    let mut end = bytes.len();
    while end > 0 {
        let b = bytes[end - 1];
        if b.is_ascii_alphabetic() || b == b'_' || b == b'/' {
            break;
        }
        end -= 1;
    }
    if end == 0 {
        return None;
    }

    // Step 2: 从单词结尾向前扫描，找到单词起始
    let mut start = end;
    while start > 0 {
        let b = bytes[start - 1];
        if b.is_ascii_alphabetic() || b == b'_' || b == b'/' {
            start -= 1;
        } else {
            break;
        }
    }

    if start == end {
        return None;
    }
    Some((&s[..start], &s[start..end]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inline_completion_new_and_fields() {
        let comp = InlineCompletion::new("hello".to_string(), 3, 5, 42);
        assert_eq!(comp.text, "hello");
        assert_eq!(comp.trigger_line, 3);
        assert_eq!(comp.trigger_col, 5);
        assert_eq!(comp.version, 42);
    }

    #[test]
    fn test_inline_completion_is_empty() {
        let empty = InlineCompletion::new(String::new(), 0, 0, 1);
        assert!(empty.is_empty());

        let non_empty = InlineCompletion::new("x".to_string(), 0, 0, 1);
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_service_request_returns_some_with_incrementing_version() {
        let mut svc = InlineCompletionService::new();
        let r1 = svc.request("fn", "").expect("应返回建议");
        let r2 = svc.request("fn", "").expect("应返回建议");
        assert!(r2.version > r1.version, "版本号应递增");
        assert!(!r1.text.is_empty(), "建议不应为空文本");
    }

    #[test]
    fn test_service_default_equals_new() {
        let mut a = InlineCompletionService::new();
        let mut b = InlineCompletionService::default();
        let ra = a.request("fn", "");
        let rb = b.request("fn", "");
        assert_eq!(ra.map(|c| c.version), rb.map(|c| c.version));
    }

    #[test]
    fn test_service_cancel_is_noop() {
        let mut svc = InlineCompletionService::new();
        svc.cancel();
        assert!(svc.request("fn", "b").is_some());
    }

    #[test]
    fn test_service_request_returns_none_for_no_match() {
        let mut svc = InlineCompletionService::new();
        // 无匹配的随机文本应返回 None
        assert!(svc.request("xyzqwerty", "").is_none());
    }

    #[test]
    fn test_suggest_completion_keywords() {
        assert_eq!(suggest_completion("fn"), Some(" name() {\n    \n}".to_string()));
        assert_eq!(suggest_completion("if"), Some(" condition {\n    \n}".to_string()));
        assert_eq!(suggest_completion("for"), Some(" item in iterable {\n    \n}".to_string()));
        assert_eq!(suggest_completion("while"), Some(" condition {\n    \n}".to_string()));
        assert_eq!(suggest_completion("match"), Some(" value {\n    _ => {}\n}".to_string()));
        assert_eq!(suggest_completion("struct"), Some(" Name {\n    \n}".to_string()));
        assert_eq!(suggest_completion("enum"), Some(" Name {\n    \n}".to_string()));
        assert_eq!(suggest_completion("impl"), Some(" Type {\n    \n}".to_string()));
    }

    #[test]
    fn test_suggest_completion_with_indent() {
        // 行首关键字（前面有缩进）仍应匹配
        assert_eq!(suggest_completion("    fn"), Some(" name() {\n    \n}".to_string()));
        assert_eq!(suggest_completion("\tfn"), Some(" name() {\n    \n}".to_string()));
    }

    #[test]
    fn test_suggest_completion_non_line_start() {
        // 非行首的 let 仍可补全（`x.let` 不匹配，但 `foo let` 匹配）
        assert_eq!(suggest_completion("foo let"), Some(" name = ".to_string()));
    }

    #[test]
    fn test_suggest_completion_macros() {
        assert_eq!(suggest_completion("todo"), Some("!()".to_string()));
        assert_eq!(suggest_completion("dbg"), Some("!()".to_string()));
        assert_eq!(suggest_completion("println"), Some("!()".to_string()));
        assert_eq!(suggest_completion("vec"), Some("![".to_string()));
    }

    #[test]
    fn test_suggest_completion_comments() {
        assert_eq!(suggest_completion("//"), Some(" ".to_string()));
        assert_eq!(suggest_completion("/*"), Some("  */".to_string()));
    }

    #[test]
    fn test_suggest_completion_no_match() {
        assert_eq!(suggest_completion("hello"), None);
        assert_eq!(suggest_completion(""), None);
        assert_eq!(suggest_completion("123"), None);
    }

    #[test]
    fn test_extract_last_word_with_prefix() {
        assert_eq!(extract_last_word_with_prefix("fn"), Some(("", "fn")));
        assert_eq!(extract_last_word_with_prefix("foo fn"), Some(("foo ", "fn")));
        assert_eq!(extract_last_word_with_prefix("    fn"), Some(("    ", "fn")));
        assert_eq!(extract_last_word_with_prefix("//"), Some(("", "//")));
        // "abc123" 中 123 是尾部非单词字符，被跳过；末尾单词是 "abc"，前面无文本
        assert_eq!(extract_last_word_with_prefix("abc123"), Some(("", "abc")));
        assert_eq!(extract_last_word_with_prefix("hello world"), Some(("hello ", "world")));
        assert_eq!(extract_last_word_with_prefix(""), None);
        assert_eq!(extract_last_word_with_prefix("   "), None);
        assert_eq!(extract_last_word_with_prefix("123"), None);
    }
}
