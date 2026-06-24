/// HTML 词法分析器
pub struct HtmlLexer;

impl HtmlLexer {
    pub fn new() -> Self {
        Self
    }
}

impl super::Lexer for HtmlLexer {
    fn lex_full(&self, text: &str) -> Vec<super::LexemeSpan> {
        let mut spans = Vec::new();
        let bytes = text.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            // HTML 注释 <!-- ... -->
            if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"<!--" {
                let start = i;
                i += 4;
                while i + 3 <= bytes.len() {
                    if &bytes[i..i + 3] == b"-->" {
                        i += 3;
                        break;
                    }
                    i += 1;
                }
                spans.push(super::LexemeSpan {
                    start,
                    len: i - start,
                    kind: super::TokenKind::BlockComment,
                    flags: 0,
                });
                continue;
            }

            // HTML 标签 <...>
            if bytes[i] == b'<' {
                let start = i;
                i += 1;

                // 检查是否是结束标签 </
                if i < bytes.len() && bytes[i] == b'/' {
                    i += 1;
                }

                // 标签名
                let tag_name_start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric()
                        || bytes[i] == b'-'
                        || bytes[i] == b'_'
                        || bytes[i] == b':')
                {
                    i += 1;
                }
                if i > tag_name_start {
                    spans.push(super::LexemeSpan {
                        start,
                        len: i - start,
                        kind: super::TokenKind::Keyword,
                        flags: 0,
                    });
                } else {
                    // 不是有效的标签名，回退
                    i = start + 1;
                    spans.push(super::LexemeSpan {
                        start,
                        len: 1,
                        kind: super::TokenKind::Punctuation,
                        flags: 0,
                    });
                    continue;
                }

                // 标签属性
                while i < bytes.len() && bytes[i] != b'>' && bytes[i] != b'/' {
                    // 跳过空白
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if i >= bytes.len() || bytes[i] == b'>' || bytes[i] == b'/' {
                        break;
                    }

                    // 属性名
                    let attr_start = i;
                    while i < bytes.len()
                        && bytes[i] != b'='
                        && bytes[i] != b'>'
                        && bytes[i] != b'/'
                        && !bytes[i].is_ascii_whitespace()
                    {
                        i += 1;
                    }
                    if i > attr_start {
                        spans.push(super::LexemeSpan {
                            start: attr_start,
                            len: i - attr_start,
                            kind: super::TokenKind::Attribute,
                            flags: 0,
                        });
                    }

                    // 等号
                    if i < bytes.len() && bytes[i] == b'=' {
                        spans.push(super::LexemeSpan {
                            start: i,
                            len: 1,
                            kind: super::TokenKind::Operator,
                            flags: 0,
                        });
                        i += 1;

                        // 跳过空白
                        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                            i += 1;
                        }

                        // 属性值
                        if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                            let quote = bytes[i];
                            let val_start = i;
                            i += 1;
                            while i < bytes.len() && bytes[i] != quote {
                                i += 1;
                            }
                            if i < bytes.len() {
                                i += 1; // 包含引号
                            }
                            spans.push(super::LexemeSpan {
                                start: val_start,
                                len: i - val_start,
                                kind: super::TokenKind::StringLiteral,
                                flags: 0,
                            });
                        } else {
                            // 无引号属性值
                            let val_start = i;
                            while i < bytes.len()
                                && bytes[i] != b'>'
                                && bytes[i] != b'/'
                                && !bytes[i].is_ascii_whitespace()
                            {
                                i += 1;
                            }
                            if i > val_start {
                                spans.push(super::LexemeSpan {
                                    start: val_start,
                                    len: i - val_start,
                                    kind: super::TokenKind::StringLiteral,
                                    flags: 0,
                                });
                            }
                        }
                    }
                }

                // 自闭合标签 / 或结束标签 >
                if i < bytes.len() && bytes[i] == b'/' {
                    spans.push(super::LexemeSpan {
                        start: i,
                        len: 1,
                        kind: super::TokenKind::Punctuation,
                        flags: 0,
                    });
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'>' {
                    spans.push(super::LexemeSpan {
                        start: i,
                        len: 1,
                        kind: super::TokenKind::Punctuation,
                        flags: 0,
                    });
                    i += 1;
                }
                continue;
            }

            // 实体引用 &...;
            if bytes[i] == b'&' {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b';' && bytes[i] != b' ' && bytes[i] != b'<' {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b';' {
                    i += 1;
                }
                spans.push(super::LexemeSpan {
                    start,
                    len: i - start,
                    kind: super::TokenKind::Identifier,
                    flags: 0,
                });
                continue;
            }

            // 普通文本（收集连续的非标签字符）
            let start = i;
            while i < bytes.len() && bytes[i] != b'<' && bytes[i] != b'&' {
                // 检查是否是注释开始
                if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"<!--" {
                    break;
                }
                i += 1;
            }
            if i > start {
                spans.push(super::LexemeSpan {
                    start,
                    len: i - start,
                    kind: super::TokenKind::Unknown,
                    flags: 0,
                });
            }
        }

        spans
    }
}

impl Default for HtmlLexer {
    fn default() -> Self {
        Self::new()
    }
}
