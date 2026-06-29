use std::collections::HashMap;
use tree_sitter::{Language, Parser, Tree};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use aether_core::lexer::{LexemeSpan, TokenKind};

/// Tree-sitter 增量语法高亮器
/// 与现有 Lexer 框架并存，提供更精确的语法高亮
pub struct TreeSitterHighlighter {
    highlighter: Highlighter,
    rust_config: Option<HighlightConfiguration>,
    js_config: Option<HighlightConfiguration>,
    ts_config: Option<HighlightConfiguration>,
    python_config: Option<HighlightConfiguration>,
    c_config: Option<HighlightConfiguration>,
    cpp_config: Option<HighlightConfiguration>,
    json_config: Option<HighlightConfiguration>,
    toml_config: Option<HighlightConfiguration>,
    /// 缓存每文档的语法树，用于增量解析
    /// key: 文档标识 (如文件路径), value: (语言, 语法树)
    pub tree_cache: HashMap<String, (String, Tree)>,
    /// 缓存每文档的 Parser
    parser_cache: HashMap<String, Parser>,
}

/// P4-6: 文档缓存最大条目数，避免长时间运行后无限增长
const MAX_HIGHLIGHTER_DOCS: usize = 32;

impl TreeSitterHighlighter {
    pub fn new() -> Self {
        let mut highlighter = Self {
            highlighter: Highlighter::new(),
            rust_config: None,
            js_config: None,
            ts_config: None,
            python_config: None,
            c_config: None,
            cpp_config: None,
            json_config: None,
            toml_config: None,
            tree_cache: HashMap::new(),
            parser_cache: HashMap::new(),
        };
        highlighter.init_configs();
        highlighter
    }

    fn init_configs(&mut self) {
        // Rust
        if let Ok(config) = HighlightConfiguration::new(
            tree_sitter_rust::language(),
            tree_sitter_rust::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            self.rust_config = Some(config);
        }

        // JavaScript
        if let Ok(config) = HighlightConfiguration::new(
            tree_sitter_javascript::language(),
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            self.js_config = Some(config);
        }

        // TypeScript
        if let Ok(config) = HighlightConfiguration::new(
            tree_sitter_typescript::language_typescript(),
            tree_sitter_typescript::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            self.ts_config = Some(config);
        }

        // Python
        if let Ok(config) = HighlightConfiguration::new(
            tree_sitter_python::language(),
            tree_sitter_python::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            self.python_config = Some(config);
        }

        // C
        if let Ok(config) = HighlightConfiguration::new(
            tree_sitter_c::language(),
            tree_sitter_c::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            self.c_config = Some(config);
        }

        // C++
        if let Ok(config) = HighlightConfiguration::new(
            tree_sitter_cpp::language(),
            tree_sitter_cpp::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            self.cpp_config = Some(config);
        }

        // JSON
        if let Ok(config) = HighlightConfiguration::new(
            tree_sitter_json::language(),
            tree_sitter_json::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            self.json_config = Some(config);
        }

        // TOML - 使用内联的基本高亮查询
        let toml_query = r#"
          (table_header (key) @type)
          (key) @attribute
          (string) @string
          (integer) @number
          (float) @number
          (boolean) @number
          (comment) @comment
        "#;
        if let Ok(config) =
            HighlightConfiguration::new(tree_sitter_toml::language(), toml_query, "", "")
        {
            self.toml_config = Some(config);
        }
    }

    /// 对单行文本进行高亮
    /// 返回 LexemeSpan 列表，与现有 Lexer 接口兼容
    pub fn highlight_line(&mut self, text: &str, language: &str) -> Vec<LexemeSpan> {
        // 安全地分离结构体字段的借用：
        // 先获取 config 的不可变指针，然后获取 highlighter 的可变引用
        // 这是安全的因为 config 和 highlighter 是结构体中不同的字段
        let config_ptr: *const HighlightConfiguration = match language {
            "rust" => self
                .rust_config
                .as_ref()
                .map(|c| c as *const HighlightConfiguration)
                .unwrap_or(std::ptr::null()),
            "javascript" | "js" => self
                .js_config
                .as_ref()
                .map(|c| c as *const HighlightConfiguration)
                .unwrap_or(std::ptr::null()),
            "typescript" | "ts" | "tsx" => self
                .ts_config
                .as_ref()
                .map(|c| c as *const HighlightConfiguration)
                .unwrap_or(std::ptr::null()),
            "python" | "py" => self
                .python_config
                .as_ref()
                .map(|c| c as *const HighlightConfiguration)
                .unwrap_or(std::ptr::null()),
            "c" => self
                .c_config
                .as_ref()
                .map(|c| c as *const HighlightConfiguration)
                .unwrap_or(std::ptr::null()),
            "cpp" | "c++" | "cxx" => self
                .cpp_config
                .as_ref()
                .map(|c| c as *const HighlightConfiguration)
                .unwrap_or(std::ptr::null()),
            "json" => self
                .json_config
                .as_ref()
                .map(|c| c as *const HighlightConfiguration)
                .unwrap_or(std::ptr::null()),
            "toml" => self
                .toml_config
                .as_ref()
                .map(|c| c as *const HighlightConfiguration)
                .unwrap_or(std::ptr::null()),
            _ => std::ptr::null(),
        };

        let config = match unsafe { config_ptr.as_ref() } {
            Some(c) => c,
            None => return Vec::new(),
        };

        let mut spans = Vec::new();
        let mut current_start = 0usize;
        let mut current_kind = TokenKind::Unknown;
        let mut in_highlight = false;

        match self
            .highlighter
            .highlight(config, text.as_bytes(), None, |_| None)
        {
            Ok(events) => {
                for event in events {
                    match event {
                        Ok(HighlightEvent::Source { start, end: _ }) => {
                            if in_highlight && start > current_start {
                                spans.push(LexemeSpan {
                                    start: current_start,
                                    len: start - current_start,
                                    kind: current_kind,
                                    flags: 0,
                                });
                            }
                            current_start = start;
                        }
                        Ok(HighlightEvent::HighlightStart(s)) => {
                            current_kind = capture_to_token_kind(s.0);
                            in_highlight = true;
                        }
                        Ok(HighlightEvent::HighlightEnd) => {
                            in_highlight = false;
                        }
                        Err(_) => {}
                    }
                }
            }
            Err(_) => {}
        }

        spans
    }

    /// 增量解析：更新文档的语法树
    pub fn parse_document(&mut self, doc_id: &str, language: &str, text: &str) -> Option<&Tree> {
        let lang = self.get_language(language)?;

        // P4-6: 在解析前检查缓存上限，避免无界增长。
        // 若已达上限且当前 doc 不在缓存中，清空 tree_cache（parser_cache 保留，
        // 因 Parser 对象较轻量且可复用）。这相当于"全量淘汰"策略。
        if self.tree_cache.len() >= MAX_HIGHLIGHTER_DOCS && !self.tree_cache.contains_key(doc_id) {
            self.tree_cache.clear();
        }

        let parser = self
            .parser_cache
            .entry(doc_id.to_string())
            .or_insert_with(|| {
                let mut p = Parser::new();
                let _ = p.set_language(lang);
                p
            });

        let tree = if let Some((_, old_tree)) = self.tree_cache.get(doc_id) {
            parser.parse(text, Some(old_tree))
        } else {
            parser.parse(text, None)
        };

        if let Some(tree) = tree {
            self.tree_cache
                .insert(doc_id.to_string(), (language.to_string(), tree));
            self.tree_cache.get(doc_id).map(|(_, t)| t)
        } else {
            None
        }
    }

    pub fn get_tree(&self, doc_id: &str) -> Option<&Tree> {
        self.tree_cache.get(doc_id).map(|(_, t)| t)
    }

    pub fn remove_document(&mut self, doc_id: &str) {
        self.tree_cache.remove(doc_id);
        self.parser_cache.remove(doc_id);
    }

    fn get_language(&self, language: &str) -> Option<Language> {
        match language {
            "rust" => Some(tree_sitter_rust::language()),
            "javascript" | "js" => Some(tree_sitter_javascript::language()),
            "typescript" | "ts" => Some(tree_sitter_typescript::language_typescript()),
            "python" | "py" => Some(tree_sitter_python::language()),
            "c" => Some(tree_sitter_c::language()),
            "cpp" | "c++" | "cxx" => Some(tree_sitter_cpp::language()),
            "json" => Some(tree_sitter_json::language()),
            "toml" => Some(tree_sitter_toml::language()),
            _ => None,
        }
    }

    fn get_config(&self, language: &str) -> Option<&HighlightConfiguration> {
        match language {
            "rust" => self.rust_config.as_ref(),
            "javascript" | "js" => self.js_config.as_ref(),
            "typescript" | "ts" | "tsx" => self.ts_config.as_ref(),
            "python" | "py" => self.python_config.as_ref(),
            "c" => self.c_config.as_ref(),
            "cpp" | "c++" | "cxx" => self.cpp_config.as_ref(),
            "json" => self.json_config.as_ref(),
            "toml" => self.toml_config.as_ref(),
            _ => None,
        }
    }

    pub fn supports_language(&self, language: &str) -> bool {
        self.get_config(language).is_some()
    }
}

impl Default for TreeSitterHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

/// 独立的 capture index 到 TokenKind 转换函数
fn capture_to_token_kind(capture_index: usize) -> TokenKind {
    match capture_index {
        0 => TokenKind::Keyword,
        1 => TokenKind::StringLiteral,
        2 => TokenKind::NumberLiteral,
        3 => TokenKind::LineComment,
        4 => TokenKind::Function,
        5 => TokenKind::TypeName,
        6 => TokenKind::Operator,
        7 => TokenKind::Identifier,
        8 => TokenKind::Preprocessor,
        9 => TokenKind::Attribute,
        _ => TokenKind::Unknown,
    }
}
