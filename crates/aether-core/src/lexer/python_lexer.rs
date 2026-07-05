use super::common::{skip_quoted, skip_whitespace};
use super::{LexemeSpan, Lexer, TokenKind};

/// Python 词法分析器
pub struct PythonLexer;

impl PythonLexer {
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
            b'#' => {
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
            b'"' => {
                if pos + 2 < bytes.len() && bytes[pos + 1] == b'"' && bytes[pos + 2] == b'"' {
                    let end = skip_triple_quoted(bytes, pos, b'"');
                    // CORE-H05: Python f-string 前缀在引号之前: f"""...""", 不是 """f...
                    let kind = if pos > 0 && (bytes[pos - 1] == b'f' || bytes[pos - 1] == b'F') {
                        TokenKind::FormatString
                    } else {
                        TokenKind::StringLiteral
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
            }
            b'\'' => {
                if pos + 2 < bytes.len() && bytes[pos + 1] == b'\'' && bytes[pos + 2] == b'\'' {
                    let end = skip_triple_quoted(bytes, pos, b'\'');
                    (
                        LexemeSpan {
                            start: pos,
                            len: end - pos,
                            kind: TokenKind::StringLiteral,
                            flags: 0,
                        },
                        end,
                    )
                } else {
                    let end = skip_quoted(bytes, pos, b'\'');
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
                // 检测 f-string: f"..." 或 f'...'
                if (bytes[pos] == b'f' || bytes[pos] == b'F')
                    && pos + 1 < bytes.len()
                    && (bytes[pos + 1] == b'"' || bytes[pos + 1] == b'\'')
                {
                    let quote = bytes[pos + 1];
                    let end = skip_quoted(bytes, pos + 1, quote);
                    (
                        LexemeSpan {
                            start: pos,
                            len: end - pos,
                            kind: TokenKind::FormatString,
                            flags: 0,
                        },
                        end,
                    )
                } else {
                    let end = skip_identifier(bytes, pos);
                    let ident = &bytes[pos..end];
                    let kind = if is_keyword_bytes(ident) {
                        TokenKind::Keyword
                    } else if is_builtin_bytes(ident) {
                        TokenKind::TypeName
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
            }
            b'+' | b'-' | b'*' | b'/' | b'%' | b'=' | b'!' | b'<' | b'>' | b'&' | b'|' | b'^'
            | b'~' => {
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
            b'(' | b')' | b'{' | b'}' | b'[' | b']' | b',' | b';' | b':' | b'.' | b'?' | b'@' => (
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

impl Lexer for PythonLexer {
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

impl Default for PythonLexer {
    fn default() -> Self {
        Self::new()
    }
}

fn is_keyword_bytes(bytes: &[u8]) -> bool {
    match bytes {
        b"False" | b"None" | b"True" | b"and" | b"as" | b"assert" | b"async" | b"await"
        | b"break" | b"class" | b"continue" | b"def" | b"del" | b"elif" | b"else"
        | b"except" | b"finally" | b"for" | b"from" | b"global" | b"if" | b"import"
        | b"in" | b"is" | b"lambda" | b"nonlocal" | b"not" | b"or" | b"pass" | b"raise"
        | b"return" | b"try" | b"while" | b"with" | b"yield" => true,
        _ => false,
    }
}

fn is_builtin_bytes(bytes: &[u8]) -> bool {
    match bytes {
        b"int" | b"float" | b"str" | b"bool" | b"list" | b"dict" | b"tuple" | b"set"
        | b"frozenset" | b"bytes" | b"bytearray" | b"memoryview" | b"object" | b"type"
        | b"range" | b"enumerate" | b"zip" | b"map" | b"filter" | b"len" | b"print"
        | b"input" | b"open" | b"super" | b"self" | b"Exception" | b"BaseException"
        | b"ValueError" | b"TypeError" | b"KeyError" | b"IndexError" => true,
        _ => false,
    }
}

fn skip_line_comment(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 1;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

fn skip_triple_quoted(bytes: &[u8], pos: usize, quote: u8) -> usize {
    let mut i = pos + 3;
    while i + 2 < bytes.len() {
        if bytes[i] == quote && bytes[i + 1] == quote && bytes[i + 2] == quote {
            return i + 3;
        }
        i += 1;
    }
    bytes.len()
}

fn skip_number(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    let mut dot_count = 0;
    let mut exponent_seen = false;
    let mut imaginary = false;

    while i < bytes.len() {
        let ch = bytes[i];
        if ch.is_ascii_digit() || ch == b'_' {
            i += 1;
        } else if ch == b'.' {
            // 阻止 1..2 被合并
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
        } else if matches!(ch, b'j' | b'J') && !imaginary {
            imaginary = true;
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

#[allow(clippy::collapsible_match)]
fn skip_operator(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    let ch = bytes[pos];
    i += 1;
    if i < bytes.len() {
        let next = bytes[i];
        match ch {
            b'+' => {
                if next == b'=' {
                    i += 1;
                }
            }
            b'-' => {
                if next == b'=' || next == b'>' {
                    i += 1;
                }
            }
            b'*' => {
                if next == b'=' || next == b'*' {
                    i += 1;
                    if i < bytes.len() && bytes[i] == b'=' {
                        i += 1;
                    }
                }
            }
            b'/' => {
                if next == b'=' || next == b'/' {
                    i += 1;
                }
            }
            b'%' => {
                if next == b'=' {
                    i += 1;
                }
            }
            b'=' => {
                if next == b'=' {
                    i += 1;
                }
            }
            b'!' => {
                if next == b'=' {
                    i += 1;
                }
            }
            b'<' => {
                if next == b'=' || next == b'<' {
                    i += 1;
                }
            }
            b'>' => {
                if next == b'=' || next == b'>' {
                    i += 1;
                }
            }
            b'&' => {
                if next == b'=' {
                    i += 1;
                }
            }
            b'|' | b'^' => {
                if next == b'=' {
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
    fn test_python_keywords() {
        let lexer = PythonLexer::new();
        let tokens = lexer.lex_full("def hello():\n    return 42");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&TokenKind::Keyword));
        assert!(kinds.contains(&TokenKind::NumberLiteral));
    }

    #[test]
    fn test_python_decorators() {
        let lexer = PythonLexer::new();
        let tokens = lexer.lex_full("@property\n@staticmethod");
        let attr_count = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Punctuation && t.len == 1)
            .count();
        assert!(attr_count >= 2);
    }

    #[test]
    fn test_python_fstring() {
        let lexer = PythonLexer::new();
        let tokens = lexer.lex_full("f'Hello {name}'");
        let kind = tokens.iter().find(|t| t.start == 0).map(|t| t.kind);
        assert_eq!(kind, Some(TokenKind::FormatString));
    }
}
