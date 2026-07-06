use super::common::{
    skip_block_comment, skip_line_comment, skip_quoted, skip_whitespace,
};
use super::{LexemeSpan, Lexer, TokenKind};

/// C语言词法分析器 — 基于确定性有限自动机(DFA)
pub struct CLexer;

impl CLexer {
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
                        b'*' => {
                            let end = skip_block_comment(bytes, pos);
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
                let end = skip_preprocessor(bytes, pos);
                (
                    LexemeSpan {
                        start: pos,
                        len: end - pos,
                        kind: TokenKind::Preprocessor,
                        flags: 0,
                    },
                    end,
                )
            }
            b'"' => {
                let end = skip_quoted(bytes, pos, b'"');
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
                let end = skip_quoted(bytes, pos, b'\'');
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
                let kind = if is_keyword_bytes(&bytes[pos..end]) {
                    TokenKind::Keyword
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
            b'+' | b'-' | b'*' | b'%' | b'=' | b'!' | b'<' | b'>' | b'&' | b'|' | b'^' | b'~' => {
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
            b'(' | b')' | b'{' | b'}' | b'[' | b']' | b',' | b';' | b':' | b'.' | b'?' => (
                LexemeSpan {
                    start: pos,
                    len: 1,
                    kind: TokenKind::Punctuation,
                    flags: 0,
                },
                pos + 1,
            ),
            _ => {
                let len = crate::lexer::utf8_char_len(bytes[pos]);
                (
                    LexemeSpan {
                        start: pos,
                        len,
                        kind: TokenKind::Unknown,
                        flags: 0,
                    },
                    pos + len,
                )
            }
        }
    }
}

impl Lexer for CLexer {
    fn lex_full(&self, text: &str) -> Vec<LexemeSpan> {
        let mut tokens = Vec::with_capacity(text.len() / 4 + 1);
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

impl Default for CLexer {
    fn default() -> Self {
        Self::new()
    }
}

fn is_keyword_bytes(bytes: &[u8]) -> bool {
    matches!(
        bytes,
        b"auto" | b"break" | b"case" | b"char" | b"const" | b"continue" | b"default"
            | b"do" | b"double" | b"else" | b"enum" | b"extern" | b"float" | b"for"
            | b"goto" | b"if" | b"inline" | b"int" | b"long" | b"register" | b"restrict"
            | b"return" | b"short" | b"signed" | b"sizeof" | b"static" | b"struct"
            | b"switch" | b"typedef" | b"union" | b"unsigned" | b"void" | b"volatile"
            | b"while" | b"_Alignas" | b"_Alignof" | b"_Atomic" | b"_Bool" | b"_Complex"
            | b"_Generic" | b"_Imaginary" | b"_Noreturn" | b"_Static_assert"
            | b"_Thread_local"
    )
}

fn skip_preprocessor(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            i += 2; // 续行
        } else if bytes[i] == b'\n' {
            return i + 1;
        } else {
            i += 1;
        }
    }
    bytes.len()
}

fn skip_number(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    let mut dot_count = 0;
    let mut exponent_seen = false;

    // 前缀：0x / 0X / 0b / 0B
    if i + 1 < bytes.len() && bytes[i] == b'0' && matches!(bytes[i + 1], b'x' | b'X' | b'b' | b'B') {
        i += 2;
        while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
            i += 1;
        }
        // 整数后缀
        while i < bytes.len() && matches!(bytes[i], b'u' | b'U' | b'l' | b'L') {
            i += 1;
        }
        return i;
    }

    while i < bytes.len() {
        let ch = bytes[i];
        if ch.is_ascii_digit() {
            i += 1;
        } else if ch == b'.' {
            // 阻止 1..2 被合并为一个数字：第二个 . 或 后无数字时停止
            if dot_count > 0 || (i + 1 < bytes.len() && bytes[i + 1] == b'.') {
                break;
            }
            dot_count += 1;
            i += 1;
        } else if matches!(ch, b'e' | b'E') && !exponent_seen {
            exponent_seen = true;
            i += 1;
            if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
                i += 1;
            }
        } else if matches!(ch, b'+' | b'-') {
            // 只允许作为指数符号出现；若不在指数上下文中则停止
            break;
        } else if matches!(ch, b'f' | b'F' | b'l' | b'L' | b'u' | b'U') {
            // 浮点/整数后缀
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
                    if i < bytes.len() && bytes[i] == b'=' {
                        i += 1;
                    }
                }
            }
            b'>' => {
                if next == b'=' || next == b'>' {
                    i += 1;
                    if i < bytes.len() && bytes[i] == b'=' {
                        i += 1;
                    }
                }
            }
            b'&' => match next {
                b'&' | b'=' => i += 1,
                _ => {}
            },
            b'|' => match next {
                b'|' | b'=' => i += 1,
                _ => {}
            },
            _ => {}
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keywords() {
        let lexer = CLexer::new();
        let tokens = lexer.lex_full("int main() { return 0; }");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&TokenKind::Keyword));
        assert!(kinds.contains(&TokenKind::NumberLiteral));
    }

    #[test]
    fn test_comments() {
        let lexer = CLexer::new();
        let tokens = lexer.lex_full("// line comment\n/* block */");
        assert!(tokens.iter().any(|t| t.kind == TokenKind::LineComment));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::BlockComment));
    }

    #[test]
    fn test_operators() {
        let lexer = CLexer::new();
        let tokens =
            lexer.lex_full("a + b - c * d / e % f == g != h <= i >= j && k || l << m >> n");
        let op_count = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Operator)
            .count();
        assert!(op_count > 5);
    }

    #[test]
    fn test_preprocessor() {
        let lexer = CLexer::new();
        let tokens = lexer.lex_full("#include <stdio.h>\n#define MAX 100");
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Preprocessor));
    }

    #[test]
    fn test_c_empty() {
        assert!(CLexer::new().lex_full("").is_empty());
    }

    #[test]
    fn test_c_doc_comment() {
        let tokens = CLexer::new().lex_full("/** doc */\n/*/ not doc */");
        let docs = tokens.iter().filter(|t| t.kind == TokenKind::DocComment).count();
        assert_eq!(docs, 1);
    }

    #[test]
    fn test_c_strings_and_chars() {
        let tokens = CLexer::new().lex_full(r#""str" 'c' "#);
        assert_eq!(tokens.iter().filter(|t| t.kind == TokenKind::StringLiteral).count(), 1);
        assert_eq!(tokens.iter().filter(|t| t.kind == TokenKind::CharLiteral).count(), 1);
    }

    #[test]
    fn test_c_numbers() {
        let tokens = CLexer::new().lex_full("0x1F 0b10 3.14f 1e10L 123u");
        assert_eq!(tokens.iter().filter(|t| t.kind == TokenKind::NumberLiteral).count(), 5);
    }

    #[test]
    fn test_c_operators() {
        let tokens = CLexer::new().lex_full("++ -- -> == != <= >= << >> && ||");
        assert!(tokens.iter().filter(|t| t.kind == TokenKind::Operator).count() >= 10);
    }

    #[test]
    fn test_c_divide_assignment() {
        let tokens = CLexer::new().lex_full("a /= b");
        assert_eq!(tokens.iter().filter(|t| t.kind == TokenKind::Operator).count(), 1);
    }

    #[test]
    fn test_c_preprocessor_continuation() {
        let tokens = CLexer::new().lex_full("#define FOO \\\n  bar");
        assert_eq!(tokens.iter().filter(|t| t.kind == TokenKind::Preprocessor).count(), 1);
    }

    #[test]
    fn test_c_unknown_utf8() {
        let tokens = CLexer::new().lex_full("中文");
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Unknown && t.len == 3));
    }
}
