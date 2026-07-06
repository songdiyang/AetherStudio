//! 各语言 lexer 共享的基础跳过/扫描工具函数
//!
//! 这些函数仅依赖字节切片，不耦合任何特定语言的语义，因此可以安全复用。

/// 跳过空白字符（空格、制表符、回车），返回第一个非空白位置
pub fn skip_whitespace(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r') {
        i += 1;
    }
    i
}

/// 跳过从 `//` 开始的行注释，返回行尾或文本末尾位置
///
/// 调用者应确保 `bytes[pos..]` 以 `//` 开头
pub fn skip_line_comment(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 2;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

/// 跳过从 `/*` 开始的块注释，返回注释结束后的位置
///
/// 调用者应确保 `bytes[pos..]` 以 `/*` 开头。支持嵌套注释的 lexer 需自行处理。
pub fn skip_block_comment(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            return i + 2;
        }
        i += 1;
    }
    bytes.len()
}

/// 跳过由 `quote` 字符包围的字符串/字符字面量
///
/// 正确处理末尾反斜杠，避免越界。遇到不匹配的结束引号则吞到文本末尾。
pub fn skip_quoted(bytes: &[u8], pos: usize, quote: u8) -> usize {
    let mut i = pos + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            // 安全跳过转义：反斜杠在末尾时只前进 1
            i += if i + 1 < bytes.len() { 2 } else { 1 };
        } else if bytes[i] == quote {
            return i + 1;
        } else {
            i += 1;
        }
    }
    bytes.len()
}

/// 跳过标识符：ASCII 字母、数字、下划线
pub fn skip_identifier_ascii(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
        i += 1;
    }
    i
}

/// 跳过标识符：ASCII 字母、数字、下划线，以及额外允许的字符（如 JS 的 `$`）
pub fn skip_identifier_with(bytes: &[u8], pos: usize, extra: &[u8]) -> usize {
    let mut i = pos;
    while i < bytes.len()
        && (bytes[i].is_ascii_alphanumeric()
            || bytes[i] == b'_'
            || extra.contains(&bytes[i]))
    {
        i += 1;
    }
    i
}

/// 跳过数字字面量的通用框架
///
/// `is_valid` 回调用于判断当前字节是否应被纳入数字。遇到不满足条件或边界情况时停止。
pub fn skip_number_generic(bytes: &[u8], pos: usize, mut is_valid: impl FnMut(u8) -> bool) -> usize {
    let mut i = pos;
    while i < bytes.len() && is_valid(bytes[i]) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skip_whitespace() {
        assert_eq!(skip_whitespace(b"   hello", 0), 3);
        assert_eq!(skip_whitespace(b"\t\r  text", 0), 4);
        assert_eq!(skip_whitespace(b"hello", 0), 0);
        assert_eq!(skip_whitespace(b"   ", 0), 3);
        assert_eq!(skip_whitespace(b"x   y", 1), 4);
    }

    #[test]
    fn test_skip_line_comment() {
        assert_eq!(skip_line_comment(b"// hello\nworld", 0), 8);
        assert_eq!(skip_line_comment(b"// no newline", 0), 13);
        assert_eq!(skip_line_comment(b"x// comment", 1), 11);
    }

    #[test]
    fn test_skip_block_comment() {
        assert_eq!(skip_block_comment(b"/* hello */world", 0), 11);
        assert_eq!(skip_block_comment(b"/* unclosed", 0), 11);
        assert_eq!(skip_block_comment(b"/* a\nb\nc */", 0), 11);
    }

    #[test]
    fn test_skip_quoted() {
        assert_eq!(skip_quoted(br#""hello"world"#, 0, b'"'), 7);
        assert_eq!(skip_quoted(br#""he\"llo"world"#, 0, b'"'), 9);
        assert_eq!(skip_quoted(br#""unclosed\"#, 0, b'"'), 10);
        assert_eq!(skip_quoted(b"'a'bc", 0, b'\''), 3);
        assert_eq!(skip_quoted(b"\"\\", 0, b'"'), 2); // 末尾反斜杠，无闭合引号
    }

    #[test]
    fn test_skip_identifier_ascii() {
        assert_eq!(skip_identifier_ascii(b"abc123", 0), 6);
        assert_eq!(skip_identifier_ascii(b"_underscore", 0), 0); // 下划线不在范围内
        assert_eq!(skip_identifier_ascii(b"abc def", 0), 3);
        assert_eq!(skip_identifier_ascii(b"", 0), 0);
    }

    #[test]
    fn test_skip_identifier_with() {
        assert_eq!(skip_identifier_with(b"$var_name", 0, b"$"), 9);
        assert_eq!(skip_identifier_with(b"abc$def", 0, b"$"), 7);
        assert_eq!(skip_identifier_with(b"abc", 0, b"$"), 3);
    }

    #[test]
    fn test_skip_number_generic() {
        let is_digit = |b: u8| b.is_ascii_digit();
        assert_eq!(skip_number_generic(b"123abc", 0, is_digit), 3);
        assert_eq!(skip_number_generic(b"abc", 0, is_digit), 0);
        assert_eq!(skip_number_generic(b"12345", 2, is_digit), 5);
    }
}
