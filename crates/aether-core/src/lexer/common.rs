/// 各语言 lexer 共享的基础跳过/扫描工具函数
///
/// 这些函数仅依赖字节切片，不耦合任何特定语言的语义，因此可以安全复用。

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
