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
                    let end = skip_string(bytes, pos, b'"');
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
                    let end = skip_string(bytes, pos, b'\'');
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
                    let end = skip_string(bytes, pos + 1, quote);
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
                    let text = std::str::from_utf8(&bytes[pos..end]).unwrap_or("");
                    let kind = if is_keyword(text) {
                        TokenKind::Keyword
                    } else if is_builtin(text) {
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

impl Lexer for PythonLexer {
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

impl Default for PythonLexer {
    fn default() -> Self {
        Self::new()
    }
}

fn is_keyword(text: &str) -> bool {
    matches!(
        text,
        "False"
            | "None"
            | "True"
            | "and"
            | "as"
            | "assert"
            | "async"
            | "await"
            | "break"
            | "class"
            | "continue"
            | "def"
            | "del"
            | "elif"
            | "else"
            | "except"
            | "finally"
            | "for"
            | "from"
            | "global"
            | "if"
            | "import"
            | "in"
            | "is"
            | "lambda"
            | "nonlocal"
            | "not"
            | "or"
            | "pass"
            | "raise"
            | "return"
            | "try"
            | "while"
            | "with"
            | "yield"
    )
}

fn is_builtin(text: &str) -> bool {
    matches!(
        text,
        "int"
            | "float"
            | "str"
            | "bool"
            | "list"
            | "dict"
            | "tuple"
            | "set"
            | "frozenset"
            | "bytes"
            | "bytearray"
            | "memoryview"
            | "object"
            | "type"
            | "range"
            | "enumerate"
            | "zip"
            | "map"
            | "filter"
            | "len"
            | "print"
            | "input"
            | "open"
            | "super"
            | "self"
            | "Exception"
            | "BaseException"
            | "ValueError"
            | "TypeError"
            | "KeyError"
            | "IndexError"
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

fn skip_string(bytes: &[u8], pos: usize, quote: u8) -> usize {
    let mut i = pos + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == quote {
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
            || bytes[i] == b'j'
            || bytes[i] == b'J'
            || bytes[i] == b'_')
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
            b'|' => {
                if next == b'=' {
                    i += 1;
                }
            }
            b'^' => {
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
