use lsp_types::*;

/// 解码后的语义令牌
#[derive(Clone, Debug)]
pub struct SemanticToken {
    pub line: u32,
    pub start_char: u32,
    pub length: u32,
    pub token_type: u32,
    pub token_modifiers: u32,
}

/// Semantic Tokens 解析器
/// 将LSP返回的紧凑uinteger数组解码为结构化token列表
pub struct SemanticTokensDecoder;

impl SemanticTokensDecoder {
    /// 解码完整的 semantic tokens 数据
    /// LSP数据格式：每5个uinteger描述一个token
    /// [deltaLine, deltaStartChar, length, tokenType, tokenModifiers, ...]
    pub fn decode(data: &[u32]) -> Vec<SemanticToken> {
        let mut tokens = Vec::with_capacity(data.len() / 5);
        let mut current_line = 0u32;
        let mut current_char = 0u32;

        for chunk in data.chunks_exact(5) {
            let delta_line = chunk[0];
            let delta_start = chunk[1];
            let length = chunk[2];
            let token_type = chunk[3];
            let token_modifiers = chunk[4];

            if delta_line > 0 {
                current_line += delta_line;
                current_char = delta_start;
            } else {
                current_char += delta_start;
            }

            tokens.push(SemanticToken {
                line: current_line,
                start_char: current_char,
                length,
                token_type,
                token_modifiers,
            });
        }

        tokens
    }

    /// 解码 delta 更新数据，合并到现有token列表
    pub fn decode_delta(
        previous_tokens: &[SemanticToken],
        delta: &SemanticTokensDelta,
    ) -> Vec<SemanticToken> {
        // Delta 更新包含编辑操作后的重新编码数据
        // 实际实现：应用edits到之前的token数据
        let mut result = previous_tokens.to_vec();

        for edit in delta.edits.iter().rev() {
            // 删除范围 [start, start + deleteCount)
            let start = edit.start as usize;
            let delete_count = edit.delete_count as usize;
            if start < result.len() {
                let end = (start + delete_count).min(result.len());
                result.drain(start..end);
            }
            // 插入新数据（如果有）
            if let Some(new_tokens) = &edit.data {
                for (i, lsp_token) in new_tokens.iter().enumerate() {
                    let token = SemanticToken {
                        line: lsp_token.delta_line,
                        start_char: lsp_token.delta_start,
                        length: lsp_token.length,
                        token_type: lsp_token.token_type,
                        token_modifiers: lsp_token.token_modifiers_bitset,
                    };
                    result.insert(start + i, token);
                }
            }
        }

        result
    }
}

/// 语义令牌类型映射（LSP标准22种类型）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SemanticTokenTypeKind {
    Namespace,
    Type,
    Class,
    Enum,
    Interface,
    Struct,
    TypeParameter,
    Parameter,
    Variable,
    Property,
    EnumMember,
    Event,
    Function,
    Method,
    Macro,
    Keyword,
    Modifier,
    Comment,
    String,
    Number,
    Regexp,
    Operator,
}

impl SemanticTokenTypeKind {
    /// 从索引获取类型（按LSP标准顺序）
    pub fn from_index(index: u32) -> Option<Self> {
        match index {
            0 => Some(Self::Namespace),
            1 => Some(Self::Type),
            2 => Some(Self::Class),
            3 => Some(Self::Enum),
            4 => Some(Self::Interface),
            5 => Some(Self::Struct),
            6 => Some(Self::TypeParameter),
            7 => Some(Self::Parameter),
            8 => Some(Self::Variable),
            9 => Some(Self::Property),
            10 => Some(Self::EnumMember),
            11 => Some(Self::Event),
            12 => Some(Self::Function),
            13 => Some(Self::Method),
            14 => Some(Self::Macro),
            15 => Some(Self::Keyword),
            16 => Some(Self::Modifier),
            17 => Some(Self::Comment),
            18 => Some(Self::String),
            19 => Some(Self::Number),
            20 => Some(Self::Regexp),
            21 => Some(Self::Operator),
            _ => None,
        }
    }

    /// 获取类型名称
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Namespace => "namespace",
            Self::Type => "type",
            Self::Class => "class",
            Self::Enum => "enum",
            Self::Interface => "interface",
            Self::Struct => "struct",
            Self::TypeParameter => "typeParameter",
            Self::Parameter => "parameter",
            Self::Variable => "variable",
            Self::Property => "property",
            Self::EnumMember => "enumMember",
            Self::Event => "event",
            Self::Function => "function",
            Self::Method => "method",
            Self::Macro => "macro",
            Self::Keyword => "keyword",
            Self::Modifier => "modifier",
            Self::Comment => "comment",
            Self::String => "string",
            Self::Number => "number",
            Self::Regexp => "regexp",
            Self::Operator => "operator",
        }
    }
}

/// 语义令牌修饰符
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SemanticTokenModifierKind {
    Declaration,
    Definition,
    Readonly,
    Static,
    Deprecated,
    Abstract,
    Async,
    Modification,
    Documentation,
    DefaultLibrary,
}

impl SemanticTokenModifierKind {
    /// 从位掩码检查是否包含此修饰符
    pub fn check(modifiers: u32, bit: u8) -> bool {
        modifiers & (1 << bit) != 0
    }

    pub fn bit(&self) -> u8 {
        match self {
            Self::Declaration => 0,
            Self::Definition => 1,
            Self::Readonly => 2,
            Self::Static => 3,
            Self::Deprecated => 4,
            Self::Abstract => 5,
            Self::Async => 6,
            Self::Modification => 7,
            Self::Documentation => 8,
            Self::DefaultLibrary => 9,
        }
    }
}

/// 语义令牌到TokenKind的映射结果
/// 包含类型和修饰符信息，用于精确着色
#[derive(Clone, Debug)]
pub struct SemanticTokenMapping {
    pub token_type: SemanticTokenTypeKind,
    pub modifiers: Vec<SemanticTokenModifierKind>,
    pub line: u32,
    pub start_char: u32,
    pub length: u32,
}

/// 将解码后的token映射为渲染可用的信息
pub fn map_tokens(
    tokens: &[SemanticToken],
    _token_types: &[SemanticTokenType],
    _token_modifiers: &[SemanticTokenModifier],
) -> Vec<SemanticTokenMapping> {
    tokens
        .iter()
        .filter_map(|t| {
            let type_kind = SemanticTokenTypeKind::from_index(t.token_type)?;
            let mut modifiers = Vec::new();
            let modifier_bits = t.token_modifiers;

            // 检查每个修饰符位
            for i in 0..10u8 {
                if SemanticTokenModifierKind::check(modifier_bits, i) {
                    let modifier = match i {
                        0 => SemanticTokenModifierKind::Declaration,
                        1 => SemanticTokenModifierKind::Definition,
                        2 => SemanticTokenModifierKind::Readonly,
                        3 => SemanticTokenModifierKind::Static,
                        4 => SemanticTokenModifierKind::Deprecated,
                        5 => SemanticTokenModifierKind::Abstract,
                        6 => SemanticTokenModifierKind::Async,
                        7 => SemanticTokenModifierKind::Modification,
                        8 => SemanticTokenModifierKind::Documentation,
                        9 => SemanticTokenModifierKind::DefaultLibrary,
                        _ => continue,
                    };
                    modifiers.push(modifier);
                }
            }

            Some(SemanticTokenMapping {
                token_type: type_kind,
                modifiers,
                line: t.line,
                start_char: t.start_char,
                length: t.length,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantic_tokens_decoder_basic() {
        // [deltaLine, deltaStartChar, length, tokenType, tokenModifiers]
        let data = vec![
            0, 0, 5, 0, 0, // line 0, char 0, length 5, type 0, mods 0
            0, 6, 4, 1, 0, // line 0, char 6, length 4, type 1
            1, 0, 3, 2, 0, // line 1, char 0, length 3, type 2
        ];
        let tokens = SemanticTokensDecoder::decode(&data);
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].line, 0);
        assert_eq!(tokens[0].start_char, 0);
        assert_eq!(tokens[1].start_char, 6);
        assert_eq!(tokens[2].line, 1);
        assert_eq!(tokens[2].start_char, 0);
    }

    #[test]
    fn test_semantic_tokens_decoder_incomplete_chunk_ignored() {
        let data = vec![0, 0, 5];
        let tokens = SemanticTokensDecoder::decode(&data);
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_decode_delta_basic() {
        let previous = vec![
            SemanticToken {
                line: 0,
                start_char: 0,
                length: 5,
                token_type: 0,
                token_modifiers: 0,
            },
            SemanticToken {
                line: 0,
                start_char: 6,
                length: 4,
                token_type: 1,
                token_modifiers: 0,
            },
        ];
        let delta = SemanticTokensDelta {
            result_id: Some("1".to_string()),
            edits: vec![SemanticTokensEdit {
                start: 1,
                delete_count: 1,
                data: Some(vec![lsp_types::SemanticToken {
                    delta_line: 0,
                    delta_start: 10,
                    length: 3,
                    token_type: 2,
                    token_modifiers_bitset: 0,
                }]),
            }],
        };
        let result = SemanticTokensDecoder::decode_delta(&previous, &delta);
        assert_eq!(result.len(), 2);
        assert_eq!(result[1].length, 3);
    }

    #[test]
    fn test_semantic_token_type_kind_from_index() {
        assert_eq!(
            SemanticTokenTypeKind::from_index(0).unwrap(),
            SemanticTokenTypeKind::Namespace
        );
        assert_eq!(
            SemanticTokenTypeKind::from_index(21).unwrap(),
            SemanticTokenTypeKind::Operator
        );
        assert!(SemanticTokenTypeKind::from_index(22).is_none());
    }

    #[test]
    fn test_semantic_token_type_kind_as_str() {
        assert_eq!(SemanticTokenTypeKind::Function.as_str(), "function");
        assert_eq!(SemanticTokenTypeKind::Keyword.as_str(), "keyword");
        assert_eq!(
            SemanticTokenTypeKind::TypeParameter.as_str(),
            "typeParameter"
        );
    }

    #[test]
    fn test_semantic_token_modifier_kind_bit_and_check() {
        assert_eq!(SemanticTokenModifierKind::Declaration.bit(), 0);
        assert_eq!(SemanticTokenModifierKind::Readonly.bit(), 2);

        let modifiers = (1 << 0) | (1 << 2);
        assert!(SemanticTokenModifierKind::check(modifiers, 0));
        assert!(SemanticTokenModifierKind::check(modifiers, 2));
        assert!(!SemanticTokenModifierKind::check(modifiers, 1));
    }

    #[test]
    fn test_map_tokens() {
        let tokens = vec![SemanticToken {
            line: 0,
            start_char: 0,
            length: 4,
            token_type: 8,                        // Variable
            token_modifiers: (1 << 0) | (1 << 1), // Declaration + Definition
        }];
        let mappings = map_tokens(&tokens, &[], &[]);
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].token_type, SemanticTokenTypeKind::Variable);
        assert_eq!(mappings[0].modifiers.len(), 2);
        assert!(mappings[0]
            .modifiers
            .contains(&SemanticTokenModifierKind::Declaration));
        assert!(mappings[0]
            .modifiers
            .contains(&SemanticTokenModifierKind::Definition));
    }

    #[test]
    fn test_map_tokens_invalid_type_ignored() {
        let tokens = vec![SemanticToken {
            line: 0,
            start_char: 0,
            length: 1,
            token_type: 99,
            token_modifiers: 0,
        }];
        let mappings = map_tokens(&tokens, &[], &[]);
        assert!(mappings.is_empty());
    }

    #[test]
    fn test_decode_multiline_and_same_line() {
        // 行内连续 token: [0,5] 和 [0,9]
        let data = vec![0, 0, 5, 0, 0, 0, 4, 4, 1, 0, 2, 3, 2, 2, 0];
        let tokens = SemanticTokensDecoder::decode(&data);
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].start_char, 0);
        assert_eq!(tokens[1].start_char, 4); // 同行为前一个 char 0 + delta_start 4
        assert_eq!(tokens[2].line, 2);
        assert_eq!(tokens[2].start_char, 3);
    }

    #[test]
    fn test_decode_empty_data() {
        assert!(SemanticTokensDecoder::decode(&[]).is_empty());
        assert!(SemanticTokensDecoder::decode(&[0, 0, 1]).is_empty());
    }

    #[test]
    fn test_decode_delta_edge_cases() {
        let previous = vec![
            SemanticToken {
                line: 0,
                start_char: 0,
                length: 1,
                token_type: 0,
                token_modifiers: 0,
            },
            SemanticToken {
                line: 0,
                start_char: 2,
                length: 1,
                token_type: 1,
                token_modifiers: 0,
            },
            SemanticToken {
                line: 0,
                start_char: 4,
                length: 1,
                token_type: 2,
                token_modifiers: 0,
            },
        ];

        // start 超出范围: 应被忽略
        let delta = SemanticTokensDelta {
            result_id: Some("1".to_string()),
            edits: vec![SemanticTokensEdit {
                start: 10,
                delete_count: 1,
                data: None,
            }],
        };
        assert_eq!(
            SemanticTokensDecoder::decode_delta(&previous, &delta).len(),
            3
        );

        // delete_count 超过长度
        let delta = SemanticTokensDelta {
            result_id: Some("2".to_string()),
            edits: vec![SemanticTokensEdit {
                start: 1,
                delete_count: 100,
                data: None,
            }],
        };
        let result = SemanticTokensDecoder::decode_delta(&previous, &delta);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].token_type, 0);

        // 多个 edit 按反向顺序应用
        let delta = SemanticTokensDelta {
            result_id: Some("3".to_string()),
            edits: vec![
                SemanticTokensEdit {
                    start: 0,
                    delete_count: 1,
                    data: Some(vec![lsp_types::SemanticToken {
                        delta_line: 0,
                        delta_start: 0,
                        length: 9,
                        token_type: 9,
                        token_modifiers_bitset: 0,
                    }]),
                },
                SemanticTokensEdit {
                    start: 2,
                    delete_count: 1,
                    data: None,
                },
            ],
        };
        let result = SemanticTokensDecoder::decode_delta(&previous, &delta);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].token_type, 9);
        assert_eq!(result[1].token_type, 1);
    }

    #[test]
    fn test_map_tokens_all_modifiers() {
        let tokens = vec![SemanticToken {
            line: 1,
            start_char: 2,
            length: 3,
            token_type: 8,          // Variable
            token_modifiers: 0x3FF, // 10 位全 1
        }];
        let mappings = map_tokens(&tokens, &[], &[]);
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].modifiers.len(), 10);
    }

    #[test]
    fn test_token_type_kind_full_coverage() {
        for i in 0..=21 {
            assert!(SemanticTokenTypeKind::from_index(i).is_some());
        }
        assert!(SemanticTokenTypeKind::from_index(22).is_none());

        let kinds = [
            SemanticTokenTypeKind::Namespace,
            SemanticTokenTypeKind::Type,
            SemanticTokenTypeKind::Class,
            SemanticTokenTypeKind::Enum,
            SemanticTokenTypeKind::Interface,
            SemanticTokenTypeKind::Struct,
            SemanticTokenTypeKind::TypeParameter,
            SemanticTokenTypeKind::Parameter,
            SemanticTokenTypeKind::Variable,
            SemanticTokenTypeKind::Property,
            SemanticTokenTypeKind::EnumMember,
            SemanticTokenTypeKind::Event,
            SemanticTokenTypeKind::Function,
            SemanticTokenTypeKind::Method,
            SemanticTokenTypeKind::Macro,
            SemanticTokenTypeKind::Keyword,
            SemanticTokenTypeKind::Modifier,
            SemanticTokenTypeKind::Comment,
            SemanticTokenTypeKind::String,
            SemanticTokenTypeKind::Number,
            SemanticTokenTypeKind::Regexp,
            SemanticTokenTypeKind::Operator,
        ];
        for kind in &kinds {
            assert!(!kind.as_str().is_empty());
        }
    }

    #[test]
    fn test_modifier_check_all_bits() {
        for i in 0..10u8 {
            assert!(SemanticTokenModifierKind::check(0x3FF, i));
        }
        for i in 0..10u8 {
            assert!(!SemanticTokenModifierKind::check(0u32, i));
        }
    }
}
