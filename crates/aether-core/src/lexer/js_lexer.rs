use super::{LexemeSpan, Lexer, TokenKind};

/// JavaScript/TypeScript 词法分析器
pub struct JsLexer;

impl JsLexer {
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
                            (
                                LexemeSpan {
                                    start: pos,
                                    len: end - pos,
                                    kind: TokenKind::BlockComment,
                                    flags: 0,
                                },
                                end,
                            )
                        }
                        _ => {
                            // 正则表达式检测：向前查找最近的非空白字符
                            let is_regex_context = if pos > 0 {
                                let prev = bytes[..pos].iter().rev().find(|&&b| {
                                    b != b' ' && b != b'\t' && b != b'\r' && b != b'\n'
                                });
                                match prev {
                                    Some(&b) => matches!(
                                        b,
                                        b'(' | b'['
                                            | b','
                                            | b'='
                                            | b':'
                                            | b';'
                                            | b'!'
                                            | b'&'
                                            | b'|'
                                            | b'?'
                                            | b'{'
                                            | b'}'
                                            | b'\n'
                                            | b'~'
                                    ),
                                    None => true,
                                }
                            } else {
                                true
                            };
                            if is_regex_context {
                                let end = skip_regex(bytes, pos);
                                if end > pos + 1 {
                                    (
                                        LexemeSpan {
                                            start: pos,
                                            len: end - pos,
                                            kind: TokenKind::RegexLiteral,
                                            flags: 0,
                                        },
                                        end,
                                    )
                                } else {
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
                            } else {
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
                        }
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
            b'`' => {
                let end = skip_template_string(bytes, pos);
                (
                    LexemeSpan {
                        start: pos,
                        len: end - pos,
                        kind: TokenKind::FormatString,
                        flags: 0,
                    },
                    end,
                )
            }
            b'"' => {
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
            b'\'' => {
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
            b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' => {
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

impl Lexer for JsLexer {
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

impl Default for JsLexer {
    fn default() -> Self {
        Self::new()
    }
}

fn is_keyword(text: &str) -> bool {
    matches!(
        text,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "export"
            | "extends"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "let"
            | "new"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
            | "async"
            | "await"
            | "static"
            | "get"
            | "set"
            | "of"
            | "from"
            | "as"
            | "enum"
            | "implements"
            | "interface"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "abstract"
            | "boolean"
            | "byte"
            | "char"
            | "double"
            | "final"
            | "float"
            | "goto"
            | "int"
            | "long"
            | "native"
            | "short"
            | "synchronized"
            | "throws"
            | "transient"
            | "volatile"
            | "null"
            | "true"
            | "false"
            | "undefined"
    )
}

fn is_builtin(text: &str) -> bool {
    matches!(
        text,
        "Array"
            | "Object"
            | "String"
            | "Number"
            | "Boolean"
            | "Date"
            | "RegExp"
            | "Function"
            | "Symbol"
            | "Error"
            | "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "Promise"
            | "Proxy"
            | "Reflect"
            | "JSON"
            | "Math"
            | "console"
            | "window"
            | "document"
            | "globalThis"
            | "require"
            | "module"
            | "exports"
            | "Buffer"
            | "process"
            | "EventEmitter"
            | "string"
            | "number"
            | "boolean"
            | "any"
            | "unknown"
            | "never"
            | "void"
            | "object"
            | "Record"
            | "Partial"
            | "Required"
            | "Pick"
            | "Omit"
            | "Exclude"
            | "Extract"
            | "ReturnType"
            | "Parameters"
            | "Readonly"
            | "interface"
            | "type"
            | "namespace"
            | "declare"
            | "global"
            | "infer"
            | "keyof"
            | "unique"
            | "symbol"
            | "bigint"
            | "asserts"
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

fn skip_template_string(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == b'`' {
            return i + 1;
        } else if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            // 跳过 ${...} 中的内容
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if bytes[i] == b'{' {
                    depth += 1;
                } else if bytes[i] == b'}' {
                    depth -= 1;
                }
                i += 1;
            }
        } else {
            i += 1;
        }
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

fn skip_regex(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 1;
    let mut in_class = false;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == b'[' {
            in_class = true;
            i += 1;
        } else if bytes[i] == b']' {
            in_class = false;
            i += 1;
        } else if bytes[i] == b'/' && !in_class {
            // 跳过标志
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            return i;
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
            || bytes[i] == b'o'
            || bytes[i] == b'O'
            || bytes[i] == b'b'
            || bytes[i] == b'B'
            || bytes[i] == b'n'
            || bytes[i] == b'_')
    {
        i += 1;
    }
    i
}

fn skip_identifier(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    while i < bytes.len()
        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
    {
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
                if next == b'-' || next == b'=' {
                    i += 1;
                }
            }
            b'*' => {
                if next == b'=' || next == b'*' {
                    i += 1;
                }
            }
            b'/' => {
                if next == b'=' {
                    i += 1;
                }
            }
            b'%' => {
                if next == b'=' {
                    i += 1;
                }
            }
            b'=' => {
                if next == b'=' || next == b'>' {
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
                if next == b'&' || next == b'=' {
                    i += 1;
                }
            }
            b'|' => {
                if next == b'|' || next == b'=' {
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
    fn test_js_keywords() {
        let lexer = JsLexer::new();
        let tokens = lexer.lex_full("const x = 42;");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&TokenKind::Keyword));
        assert!(kinds.contains(&TokenKind::NumberLiteral));
    }

    #[test]
    fn test_js_template_string() {
        let lexer = JsLexer::new();
        let tokens = lexer.lex_full("`Hello ${name}!`");
        let kind = tokens.iter().find(|t| t.start == 0).map(|t| t.kind);
        assert_eq!(kind, Some(TokenKind::FormatString));
    }

    #[test]
    fn test_js_regex() {
        let lexer = JsLexer::new();
        let tokens = lexer.lex_full("const re = /abc/gi;");
        let regex_count = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::RegexLiteral)
            .count();
        assert_eq!(regex_count, 1);
    }
}
