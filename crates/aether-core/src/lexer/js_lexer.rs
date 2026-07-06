use super::common::{
    skip_block_comment, skip_line_comment, skip_quoted, skip_whitespace,
};
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
                // 按完整 UTF-8 字符推进，避免中文/emoji 被拆散导致高亮错位
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

impl Lexer for JsLexer {
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

impl Default for JsLexer {
    fn default() -> Self {
        Self::new()
    }
}

fn is_keyword_bytes(bytes: &[u8]) -> bool {
    matches!(
        bytes,
        b"break" | b"case" | b"catch" | b"class" | b"const" | b"continue"
            | b"debugger" | b"default" | b"delete" | b"do" | b"else" | b"export"
            | b"extends" | b"finally" | b"for" | b"function" | b"if" | b"import"
            | b"in" | b"instanceof" | b"let" | b"new" | b"return" | b"super"
            | b"switch" | b"this" | b"throw" | b"try" | b"typeof" | b"var"
            | b"void" | b"while" | b"with" | b"yield" | b"async" | b"await"
            | b"static" | b"get" | b"set" | b"of" | b"from" | b"as" | b"enum"
            | b"implements" | b"interface" | b"package" | b"private" | b"protected"
            | b"public" | b"abstract" | b"boolean" | b"byte" | b"char" | b"double"
            | b"final" | b"float" | b"goto" | b"int" | b"long" | b"native"
            | b"short" | b"synchronized" | b"throws" | b"transient" | b"volatile"
            | b"null" | b"true" | b"false" | b"undefined"
    )
}

fn is_builtin_bytes(bytes: &[u8]) -> bool {
    matches!(
        bytes,
        b"Array" | b"Object" | b"String" | b"Number" | b"Boolean" | b"Date"
            | b"RegExp" | b"Function" | b"Symbol" | b"Error" | b"Map" | b"Set"
            | b"WeakMap" | b"WeakSet" | b"Promise" | b"Proxy" | b"Reflect" | b"JSON"
            | b"Math" | b"console" | b"window" | b"document" | b"globalThis"
            | b"require" | b"module" | b"exports" | b"Buffer" | b"process"
            | b"EventEmitter" | b"string" | b"number" | b"boolean" | b"any"
            | b"unknown" | b"never" | b"void" | b"object" | b"Record" | b"Partial"
            | b"Required" | b"Pick" | b"Omit" | b"Exclude" | b"Extract" | b"ReturnType"
            | b"Parameters" | b"Readonly" | b"interface" | b"type" | b"namespace"
            | b"declare" | b"global" | b"infer" | b"keyof" | b"unique" | b"symbol"
            | b"bigint" | b"asserts"
    )
}

fn skip_template_string(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            // 安全跳过转义：反斜杠在末尾时只前进 1，避免越界
            i += if i + 1 < bytes.len() { 2 } else { 1 };
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

fn skip_regex(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 1;
    let mut in_class = false;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            // 安全跳过转义
            i += if i + 1 < bytes.len() { 2 } else { 1 };
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

    // 前缀：0x / 0X / 0o / 0O / 0b / 0B
    if i + 1 < bytes.len()
        && bytes[i] == b'0'
        && matches!(bytes[i + 1], b'x' | b'X' | b'o' | b'O' | b'b' | b'B')
    {
        let base = bytes[i + 1].to_ascii_lowercase();
        i += 2;
        while i < bytes.len() {
            let ch = bytes[i];
            let valid = ch == b'_'
                || ch.is_ascii_digit()
                || (base == b'x' && ch.is_ascii_hexdigit());
            if !valid {
                break;
            }
            i += 1;
        }
        return i;
    }

    let mut dot_count = 0;
    let mut exponent_seen = false;
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
        } else if ch == b'n' && i > pos && bytes[i - 1].is_ascii_digit() {
            // BigInt 后缀 n
            i += 1;
        } else {
            break;
        }
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
                    // C-23: **= 四字符运算符
                    if i < bytes.len() && bytes[i] == b'=' {
                        i += 1;
                    }
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
                if next == b'=' {
                    i += 1;
                    // C-23: === 三字符运算符
                    if i < bytes.len() && bytes[i] == b'=' {
                        i += 1;
                    }
                } else if next == b'>' {
                    i += 1;
                }
            }
            b'!' => {
                if next == b'=' {
                    i += 1;
                    // C-23: !== 三字符运算符
                    if i < bytes.len() && bytes[i] == b'=' {
                        i += 1;
                    }
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
                    // C-23: >>> 三字符运算符
                    if i < bytes.len() && bytes[i] == b'>' {
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
            b'^' => {
                if next == b'=' {
                    i += 1;
                }
            }
            // C-23: ?? 和 ??= 运算符
            b'?' => {
                if next == b'?' {
                    i += 1;
                    if i < bytes.len() && bytes[i] == b'=' {
                        i += 1;
                    }
                } else if next == b'.' {
                    // C-23: ?. 可选链运算符
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

    #[test]
    fn test_js_empty_and_whitespace() {
        assert!(JsLexer::new().lex_full("").is_empty());
        let tokens = JsLexer::new().lex_full("   \n\t");
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Whitespace));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Newline));
    }

    #[test]
    fn test_js_comments() {
        let tokens = JsLexer::new().lex_full("// line\n/* block */");
        assert!(tokens.iter().any(|t| t.kind == TokenKind::LineComment));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::BlockComment));
    }

    #[test]
    fn test_js_strings_and_chars() {
        let tokens = JsLexer::new().lex_full(r#""a" 'b' "#);
        let strings: Vec<_> = tokens.iter().filter(|t| t.kind == TokenKind::StringLiteral).collect();
        assert_eq!(strings.len(), 2);
    }

    #[test]
    fn test_js_numbers() {
        let tokens = JsLexer::new().lex_full("0x1F 0b10 0o7 1.5e2 1_000n");
        assert_eq!(
            tokens.iter().filter(|t| t.kind == TokenKind::NumberLiteral).count(),
            5
        );
    }

    #[test]
    fn test_js_operators() {
        let tokens = JsLexer::new().lex_full("=== !== **= ??= ?. >>> <<=");
        let ops = tokens.iter().filter(|t| t.kind == TokenKind::Operator).count();
        assert_eq!(ops, 7);
    }

    #[test]
    fn test_js_punctuation_and_unknown() {
        let tokens = JsLexer::new().lex_full("(){}[],;:?");
        let puncs = tokens.iter().filter(|t| t.kind == TokenKind::Punctuation).count();
        assert_eq!(puncs, 10);
        let tokens = JsLexer::new().lex_full("中文");
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Unknown && t.len == 3));
    }

    #[test]
    fn test_js_builtins_and_keywords() {
        let tokens = JsLexer::new().lex_full("const Array = true;");
        let ks: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert!(ks.contains(&TokenKind::Keyword));
        assert!(ks.contains(&TokenKind::TypeName));
    }

    #[test]
    fn test_js_slash_as_operator() {
        // 在标识符后 / 应识别为运算符，而非正则
        let tokens = JsLexer::new().lex_full("a / b");
        assert_eq!(tokens.iter().filter(|t| t.kind == TokenKind::Operator).count(), 1);
        assert!(tokens.iter().all(|t| t.kind != TokenKind::RegexLiteral));
    }

    #[test]
    fn test_js_regex_with_class() {
        let tokens = JsLexer::new().lex_full("const re = /[a-z]+/g;");
        assert_eq!(tokens.iter().filter(|t| t.kind == TokenKind::RegexLiteral).count(), 1);
    }

    #[test]
    fn test_js_template_with_nesting() {
        let tokens = JsLexer::new().lex_full("`outer ${`inner` + 1}`");
        assert_eq!(tokens.iter().filter(|t| t.kind == TokenKind::FormatString).count(), 1);
    }
}
