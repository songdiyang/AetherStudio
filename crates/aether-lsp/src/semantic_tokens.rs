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
