use super::{LexemeSpan, Lexer, TokenKind};

/// Rust 词法分析器
pub struct RustLexer;

impl RustLexer {
    pub fn new() -> Self {
        Self
    }

    fn lex_next(&self, bytes: &[u8], pos: usize) -> (LexemeSpan, usize) {
        if pos >= bytes.len() {
            return (
                LexemeSpan {
                    start: pos,
                    len: 0,
                    kind: TokenKind::EOF,
                    flags: 0,
                },
                pos,
            );
        }

        let ch = bytes[pos];

        match ch {
            b' ' | b'\t' | b'\r' => {
                let end = skip_whitespace(bytes, pos);
                (
                    LexemeSpan {
                        start: pos,
                        len: end - pos,
                        kind: TokenKind::Whitespace,
                        flags: 0,
                    },
                    end,
                )
            }
            b'\n' => (
                LexemeSpan {
                    start: pos,
                    len: 1,
                    kind: TokenKind::Newline,
                    flags: 0,
                },
                pos + 1,
            ),
            b'/' => {
                if pos + 1 < bytes.len() {
                    match bytes[pos + 1] {
                        b'/' => {
                            if pos + 2 < bytes.len() && bytes[pos + 2] == b'/' {
                                let end = skip_line_comment(bytes, pos);
                                let kind = if bytes[pos..end].starts_with(b"///")
                                    && !bytes[pos..end].starts_with(b"////")
                                {
                                    TokenKind::DocComment
                                } else {
                                    TokenKind::LineComment
                                };
                                (
                                    LexemeSpan {
                                        start: pos,
                                        len: end - pos,
                                        kind,
                                        flags: 0,
                                    },
                                    end,
                                )
                            } else {
                                let end = skip_line_comment(bytes, pos);
                                (
                                    LexemeSpan {
                                        start: pos,
                                        len: end - pos,
                                        kind: TokenKind::LineComment,
                                        flags: 0,
                                    },
                                    end,
                                )
                            }
                        }
                        b'*' => {
                            let end = skip_block_comment(bytes, pos);
                            // M-10: `/**/`（空块注释）不应被分类为 DocComment。
                            // 与 C 词法分析器保持一致，添加 `!starts_with("/**/")` 守卫。
                            let kind = if bytes[pos..end].starts_with(b"/**")
                                && !bytes[pos..end].starts_with(b"/**/")
                            {
                                TokenKind::DocComment
                            } else {
                                TokenKind::BlockComment
                            };
                            (
                                LexemeSpan {
                                    start: pos,
                                    len: end - pos,
                                    kind,
                                    flags: 0,
                                },
                                end,
                            )
                        }
                        b'=' => (
                            LexemeSpan {
                                start: pos,
                                len: 2,
                                kind: TokenKind::Operator,
                                flags: 0,
                            },
                            pos + 2,
                        ),
                        _ => (
                            LexemeSpan {
                                start: pos,
                                len: 1,
                                kind: TokenKind::Operator,
                                flags: 0,
                            },
                            pos + 1,
                        ),
                    }
                } else {
                    (
                        LexemeSpan {
                            start: pos,
                            len: 1,
                            kind: TokenKind::Operator,
                            flags: 0,
                        },
                        pos + 1,
                    )
                }
            }
            b'#' => {
                // 属性
                let end = skip_attribute(bytes, pos);
                (
                    LexemeSpan {
                        start: pos,
                        len: end - pos,
                        kind: TokenKind::Attribute,
                        flags: 0,
                    },
                    end,
                )
            }
            b'"' => {
                // 字符串字面量（Rust 没有三引号语法）
                let end = skip_string(bytes, pos);
                (
                    LexemeSpan {
                        start: pos,
                        len: end - pos,
                        kind: TokenKind::StringLiteral,
                        flags: 0,
                    },
                    end,
                )
            }
            b'\'' => {
                // 生命周期或字符字面量
                // CORE-H03: 反斜杠后必为转义字符字面量（如 '\n', '\t'），不会误分类为生命周期
                if pos + 1 < bytes.len() && bytes[pos + 1] == b'\\' {
                    let end = skip_char(bytes, pos);
                    (
                        LexemeSpan {
                            start: pos,
                            len: end - pos,
                            kind: TokenKind::CharLiteral,
                            flags: 0,
                        },
                        end,
                    )
                } else if pos + 2 < bytes.len()
                    && bytes[pos + 1] != b'\''
                    && bytes[pos + 2] == b'\''
                {
                    // 单字符字面量: 'a', 'x', 'z'（格式为 'X'）
                    let end = skip_char(bytes, pos);
                    (
                        LexemeSpan {
                            start: pos,
                            len: end - pos,
                            kind: TokenKind::CharLiteral,
                            flags: 0,
                        },
                        end,
                    )
                } else if pos + 1 < bytes.len()
                    && bytes[pos + 1].is_ascii_alphabetic()
                    && bytes[pos + 1].is_ascii_lowercase()
                {
                    // 生命周期: 'a, 'static
                    let end = skip_lifetime(bytes, pos);
                    (
                        LexemeSpan {
                            start: pos,
                            len: end - pos,
                            kind: TokenKind::Lifetime,
                            flags: 0,
                        },
                        end,
                    )
                } else {
                    let end = skip_char(bytes, pos);
                    (
                        LexemeSpan {
                            start: pos,
                            len: end - pos,
                            kind: TokenKind::CharLiteral,
                            flags: 0,
                        },
                        end,
                    )
                }
            }
            b'0'..=b'9' => {
                let end = skip_number(bytes, pos);
                (
                    LexemeSpan {
                        start: pos,
                        len: end - pos,
                        kind: TokenKind::NumberLiteral,
                        flags: 0,
                    },
                    end,
                )
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let end = skip_identifier(bytes, pos);
                let text = std::str::from_utf8(&bytes[pos..end]).unwrap_or("");
                let kind = if is_keyword(text) {
                    TokenKind::Keyword
                } else if is_builtin_type(text) {
                    TokenKind::TypeName
                } else if text.starts_with("macro_") || text == "macro" {
                    TokenKind::Macro
                } else {
                    TokenKind::Identifier
                };
                (
                    LexemeSpan {
                        start: pos,
                        len: end - pos,
                        kind,
                        flags: 0,
                    },
                    end,
                )
            }
            b'!' => {
                // 宏调用检测: ident!
                if pos > 0 {
                    let prev = bytes[pos - 1];
                    if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b')' || prev == b']'
                    {
                        (
                            LexemeSpan {
                                start: pos,
                                len: 1,
                                kind: TokenKind::Macro,
                                flags: 0,
                            },
                            pos + 1,
                        )
                    } else if pos + 1 < bytes.len() && bytes[pos + 1] == b'=' {
                        (
                            LexemeSpan {
                                start: pos,
                                len: 2,
                                kind: TokenKind::Operator,
                                flags: 0,
                            },
                            pos + 2,
                        )
                    } else {
                        (
                            LexemeSpan {
                                start: pos,
                                len: 1,
                                kind: TokenKind::Operator,
                                flags: 0,
                            },
                            pos + 1,
                        )
                    }
                } else {
                    (
                        LexemeSpan {
                            start: pos,
                            len: 1,
                            kind: TokenKind::Operator,
                            flags: 0,
                        },
                        pos + 1,
                    )
                }
            }
            b'+' | b'-' | b'*' | b'%' | b'=' | b'<' | b'>' | b'&' | b'|' | b'^' | b'~' => {
                let end = skip_operator(bytes, pos);
                (
                    LexemeSpan {
                        start: pos,
                        len: end - pos,
                        kind: TokenKind::Operator,
                        flags: 0,
                    },
                    end,
                )
            }
            b'(' | b')' | b'{' | b'}' | b'[' | b']' | b',' | b';' | b':' | b'.' | b'?' | b'@'
            | b'$' => (
                LexemeSpan {
                    start: pos,
                    len: 1,
                    kind: TokenKind::Punctuation,
                    flags: 0,
                },
                pos + 1,
            ),
            _ => (
                LexemeSpan {
                    start: pos,
                    len: 1,
                    kind: TokenKind::Unknown,
                    flags: 0,
                },
                pos + 1,
            ),
        }
    }
}

impl Lexer for RustLexer {
    fn lex_full(&self, text: &str) -> Vec<LexemeSpan> {
        let mut tokens = Vec::new();
        let bytes = text.as_bytes();
        let mut pos = 0;

        while pos < bytes.len() {
            let (token, new_pos) = self.lex_next(bytes, pos);
            tokens.push(token);
            pos = new_pos;
        }

        tokens
    }
}

impl Default for RustLexer {
    fn default() -> Self {
        Self::new()
    }
}

fn is_keyword(text: &str) -> bool {
    matches!(
        text,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "yield"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "try"
            | "union"
    )
}

fn is_builtin_type(text: &str) -> bool {
    matches!(
        text,
        "i8" | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
            | "bool"
            | "char"
            | "str"
            | "String"
            | "Vec"
            | "Option"
            | "Result"
            | "Box"
            | "Rc"
            | "Arc"
            | "HashMap"
            | "BTreeMap"
            | "HashSet"
            | "BTreeSet"
            | "VecDeque"
            | "LinkedList"
            | "BinaryHeap"
            | "Cow"
    )
}

fn skip_whitespace(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\r') {
        i += 1;
    }
    i
}

fn skip_line_comment(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 2;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

fn skip_block_comment(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 2;
    let mut depth = 1;
    while i + 1 < bytes.len() && depth > 0 {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            depth += 1;
            i += 2;
        } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            depth -= 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    // L-01: 未终止的块注释，循环因 i+1 >= len 退出但 i 仍指向倒数第二字节，
    // 导致末尾字节未被消费、后续产生 1 字节残余 token。将 i 推进到末尾。
    if depth > 0 && i < bytes.len() {
        i = bytes.len();
    }
    i
}

fn skip_attribute(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 1;
    // 跳过 !
    if i < bytes.len() && bytes[i] == b'!' {
        i += 1;
    }
    // 跳过 [...]
    if i < bytes.len() && bytes[i] == b'[' {
        let mut depth = 1;
        i += 1;
        while i < bytes.len() && depth > 0 {
            if bytes[i] == b'[' {
                depth += 1;
            } else if bytes[i] == b']' {
                depth -= 1;
            }
            i += 1;
        }
    }
    i
}

fn skip_string(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == b'"' {
            return i + 1;
        } else {
            i += 1;
        }
    }
    bytes.len()
}

fn skip_char(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == b'\'' {
            return i + 1;
        } else {
            i += 1;
        }
    }
    bytes.len()
}

fn skip_lifetime(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    i
}

fn skip_number(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;

    // H-07: 检测进制前缀。十六进制字符 a-f/A-F 仅在 0x 前缀后有效，
    // 避免 `42fn` 被识别为单个数字 token（应为 42 + fn 关键字）。
    let mut is_hex = false;
    if i < bytes.len() && bytes[i] == b'0' && i + 1 < bytes.len() {
        match bytes[i + 1] {
            b'x' | b'X' => {
                is_hex = true;
                i += 2;
            }
            b'b' | b'B' => {
                // 二进制前缀
                i += 2;
            }
            b'o' | b'O' => {
                // 八进制前缀
                i += 2;
            }
            _ => {}
        }
    }

    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_digit()
            || b == b'.'
            || b == b'e'
            || b == b'E'
            || b == b'+'
            || b == b'-'
            || b == b'_'
        {
            i += 1;
        } else if is_hex && ((b >= b'a' && b <= b'f') || (b >= b'A' && b <= b'F')) {
            i += 1;
        } else {
            break;
        }
    }
    i
}

fn skip_identifier(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    i
}

fn skip_operator(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    let ch = bytes[pos];
    i += 1;
    if i < bytes.len() {
        let next = bytes[i];
        match ch {
            b'+' => {
                if next == b'+' || next == b'=' {
                    i += 1;
                }
            }
            b'-' => {
                if next == b'-' || next == b'=' || next == b'>' {
                    i += 1;
                }
            }
            b'*' | b'%' | b'=' | b'^' => {
                if next == b'=' {
                    i += 1;
                }
            }
            b'<' => {
                if next == b'=' || next == b'<' {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'=' {
                    i += 1;
                }
            }
            b'>' => {
                if next == b'=' || next == b'>' {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'=' {
                    i += 1;
                }
            }
            b'&' => {
                if next == b'&' || next == b'=' {
                    i += 1;
                }
            }
            b'|' => {
                if next == b'|' || next == b'=' {
                    i += 1;
                }
            }
            _ => {}
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_keywords() {
        let lexer = RustLexer::new();
        let tokens = lexer.lex_full("fn main() { let x = 42; }");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&TokenKind::Keyword));
        assert!(kinds.contains(&TokenKind::NumberLiteral));
    }

    #[test]
    fn test_rust_lifetimes() {
        let lexer = RustLexer::new();
        let tokens = lexer.lex_full("fn foo<'a>(x: &'a str) -> &'a str {}");
        let lifetime_count = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Lifetime)
            .count();
        assert!(lifetime_count >= 3);
    }

    #[test]
    fn test_rust_attributes() {
        let lexer = RustLexer::new();
        let tokens = lexer.lex_full("#[derive(Debug)]\n#[cfg(test)]");
        let attr_count = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Attribute)
            .count();
        assert_eq!(attr_count, 2);
    }

    #[test]
    fn test_rust_doc_comments() {
        let lexer = RustLexer::new();
        let tokens = lexer.lex_full("/// doc comment\n//! module doc\n/** block doc */");
        assert!(tokens.iter().any(|t| t.kind == TokenKind::DocComment));
    }
}
