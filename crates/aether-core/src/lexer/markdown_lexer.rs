use super::{LexemeSpan, Lexer, TokenKind};

/// Markdown 词法分析器
pub struct MarkdownLexer;

impl MarkdownLexer {
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

        // 处理换行符
        if ch == b'\n' {
            return (
                LexemeSpan {
                    start: pos,
                    len: 1,
                    kind: TokenKind::Newline,
                    flags: 0,
                },
                pos + 1,
            );
        }

        // 检测标题
        if ch == b'#' {
            let mut i = pos;
            while i < bytes.len() && bytes[i] == b'#' {
                i += 1;
            }
            let level = i - pos;
            if level <= 6 && (i >= bytes.len() || bytes[i] == b' ' || bytes[i] == b'\n') {
                let end = skip_to_line_end(bytes, i);
                return (
                    LexemeSpan {
                        start: pos,
                        len: end - pos,
                        kind: TokenKind::MdHeading,
                        flags: level as u8,
                    },
                    end,
                );
            }
        }

        // 检测代码块标记 ```
        if ch == b'`' && pos + 2 < bytes.len() && bytes[pos + 1] == b'`' && bytes[pos + 2] == b'`' {
            let end = skip_to_line_end(bytes, pos);
            return (
                LexemeSpan {
                    start: pos,
                    len: end - pos,
                    kind: TokenKind::MdCode,
                    flags: 0,
                },
                end,
            );
        }

        // 检测行内代码 `
        if ch == b'`' {
            let end = skip_inline_code(bytes, pos);
            return (
                LexemeSpan {
                    start: pos,
                    len: end - pos,
                    kind: TokenKind::MdCode,
                    flags: 0,
                },
                end,
            );
        }

        // 检测链接 [text](url)
        if ch == b'[' {
            let end = skip_link(bytes, pos);
            if end > pos + 1 {
                return (
                    LexemeSpan {
                        start: pos,
                        len: end - pos,
                        kind: TokenKind::MdLink,
                        flags: 0,
                    },
                    end,
                );
            }
        }

        // 检测强调 **text** 或 *text* 或 __text__ 或 _text_
        if ch == b'*' || ch == b'_' {
            let mut i = pos + 1;
            let mut count = 1;
            while i < bytes.len() && bytes[i] == ch {
                count += 1;
                i += 1;
            }
            if count <= 3 {
                let end = skip_emphasis(bytes, pos, ch, count);
                if end > pos + count * 2 {
                    let kind = if count >= 2 {
                        TokenKind::MdEmphasis
                    } else {
                        TokenKind::MdEmphasis
                    };
                    return (
                        LexemeSpan {
                            start: pos,
                            len: end - pos,
                            kind,
                            flags: count as u8,
                        },
                        end,
                    );
                }
            }
        }

        // 检测无序列表 - 或 * 或 +
        if (ch == b'-' || ch == b'*' || ch == b'+')
            && pos + 1 < bytes.len()
            && bytes[pos + 1] == b' '
        {
            let end = skip_to_line_end(bytes, pos);
            return (
                LexemeSpan {
                    start: pos,
                    len: end - pos,
                    kind: TokenKind::Punctuation,
                    flags: 0,
                },
                end,
            );
        }

        // 检测有序列表 1. 2. 等
        if ch.is_ascii_digit() {
            let mut i = pos;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1] == b' ' {
                let end = skip_to_line_end(bytes, pos);
                return (
                    LexemeSpan {
                        start: pos,
                        len: end - pos,
                        kind: TokenKind::Punctuation,
                        flags: 0,
                    },
                    end,
                );
            }
        }

        // 检测 HTML 标签
        if ch == b'<' {
            let end = skip_html_tag(bytes, pos);
            if end > pos + 1 {
                return (
                    LexemeSpan {
                        start: pos,
                        len: end - pos,
                        kind: TokenKind::MdCode,
                        flags: 0,
                    },
                    end,
                );
            }
        }

        // 默认：普通文本
        let end = skip_plain_text(bytes, pos);
        (
            LexemeSpan {
                start: pos,
                len: end - pos,
                kind: TokenKind::Unknown,
                flags: 0,
            },
            end,
        )
    }
}

impl Lexer for MarkdownLexer {
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

impl Default for MarkdownLexer {
    fn default() -> Self {
        Self::new()
    }
}

fn skip_to_line_end(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

fn skip_inline_code(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 1;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn skip_link(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 1;
    // 跳过 [text]
    while i < bytes.len() && bytes[i] != b']' {
        i += 1;
    }
    if i >= bytes.len() {
        return pos + 1;
    }
    i += 1; // skip ]
            // 检测 (url)
    if i < bytes.len() && bytes[i] == b'(' {
        i += 1;
        while i < bytes.len() && bytes[i] != b')' {
            i += 1;
        }
        if i < bytes.len() {
            i += 1;
        }
    }
    i
}

fn skip_emphasis(bytes: &[u8], pos: usize, marker: u8, count: usize) -> usize {
    let mut i = pos + count;
    while i + count <= bytes.len() {
        if bytes[i] == marker {
            let mut match_count = 1;
            let mut j = i + 1;
            while j < bytes.len() && match_count < count && bytes[j] == marker {
                match_count += 1;
                j += 1;
            }
            if match_count == count {
                return j;
            }
        }
        i += 1;
    }
    bytes.len()
}

fn skip_html_tag(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 1;
    if i < bytes.len() && bytes[i] == b'/' {
        i += 1;
    }
    while i < bytes.len()
        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_')
    {
        i += 1;
    }
    while i < bytes.len() && bytes[i] != b'>' {
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    if i < bytes.len() {
        i += 1;
    }
    i
}

fn skip_plain_text(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    while i < bytes.len() {
        let ch = bytes[i];
        if ch == b'#'
            || ch == b'`'
            || ch == b'['
            || ch == b'*'
            || ch == b'_'
            || ch == b'<'
            || ch == b'-'
            || ch == b'+'
            || ch.is_ascii_digit()
        {
            break;
        }
        i += 1;
    }
    if i == pos {
        i + 1
    } else {
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md_headings() {
        let lexer = MarkdownLexer::new();
        let tokens = lexer.lex_full("# Title\n## Subtitle");
        let heading_count = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::MdHeading)
            .count();
        assert_eq!(heading_count, 2);
    }

    #[test]
    fn test_md_code() {
        let lexer = MarkdownLexer::new();
        let tokens = lexer.lex_full("`code` and more");
        let code_count = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::MdCode)
            .count();
        assert_eq!(code_count, 1);
    }

    #[test]
    fn test_md_link() {
        let lexer = MarkdownLexer::new();
        let tokens = lexer.lex_full("[link](https://example.com)");
        assert!(tokens.iter().any(|t| t.kind == TokenKind::MdLink));
    }
}
