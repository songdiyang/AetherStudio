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

impl Lexer for CLexer {
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

impl Default for CLexer {
    fn default() -> Self {
        Self::new()
    }
}

fn is_keyword(text: &str) -> bool {
    matches!(
        text,
        "auto"
            | "break"
            | "case"
            | "char"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "double"
            | "else"
            | "enum"
            | "extern"
            | "float"
            | "for"
            | "goto"
            | "if"
            | "inline"
            | "int"
            | "long"
            | "register"
            | "restrict"
            | "return"
            | "short"
            | "signed"
            | "sizeof"
            | "static"
            | "struct"
            | "switch"
            | "typedef"
            | "union"
            | "unsigned"
            | "void"
            | "volatile"
            | "while"
            | "_Alignas"
            | "_Alignof"
            | "_Atomic"
            | "_Bool"
            | "_Complex"
            | "_Generic"
            | "_Imaginary"
            | "_Noreturn"
            | "_Static_assert"
            | "_Thread_local"
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
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            return i + 2;
        }
        i += 1;
    }
    bytes.len()
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

fn skip_string(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2; // 转义字符
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

fn skip_number(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    while i < bytes.len()
        && (bytes[i].is_ascii_digit()
            || bytes[i] == b'.'
            || bytes[i] == b'e'
            || bytes[i] == b'E'
            || bytes[i] == b'+'
            || bytes[i] == b'-'
            || bytes[i] == b'x'
            || bytes[i] == b'X'
            || bytes[i] == b'a'
            || bytes[i] == b'A'
            || bytes[i] == b'b'
            || bytes[i] == b'B'
            || bytes[i] == b'c'
            || bytes[i] == b'C'
            || bytes[i] == b'd'
            || bytes[i] == b'D'
            || bytes[i] == b'f'
            || bytes[i] == b'F'
            || bytes[i] == b'l'
            || bytes[i] == b'L'
            || bytes[i] == b'u'
            || bytes[i] == b'U')
    {
        i += 1;
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
}
