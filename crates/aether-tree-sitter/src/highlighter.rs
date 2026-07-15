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
    go_config: Option<HighlightConfiguration>,
    java_config: Option<HighlightConfiguration>,
    /// 缓存每文档的语法树，用于增量解析
    /// key: 文档标识 (如文件路径), value: (语言, 语法树)
    pub tree_cache: HashMap<String, (String, Tree)>,
    /// 缓存每文档的 Parser
    parser_cache: HashMap<String, Parser>,
}

/// P4-6: 文档缓存最大条目数，避免长时间运行后无限增长
const MAX_HIGHLIGHTER_DOCS: usize = 32;

/// 启用并固定 capture 索引，使 `capture_to_token_kind` 的映射生效。
/// 索引 0-9 分别对应 Keyword、StringLiteral、NumberLiteral、LineComment、
/// Function、TypeName、Operator、Identifier、Preprocessor、Attribute。
const HIGHLIGHT_NAMES: &[&str] = &[
    "keyword",
    "string",
    "number",
    "comment",
    "function",
    "type",
    "operator",
    "identifier",
    "preprocessor",
    "attribute",
];

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
            go_config: None,
            java_config: None,
            tree_cache: HashMap::new(),
            parser_cache: HashMap::new(),
        };
        highlighter.init_configs();
        highlighter
    }

    fn init_configs(&mut self) {
        // Rust
        if let Ok(mut config) = HighlightConfiguration::new(
            tree_sitter_rust::language(),
            tree_sitter_rust::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            config.configure(HIGHLIGHT_NAMES);
            self.rust_config = Some(config);
        }

        // JavaScript
        if let Ok(mut config) = HighlightConfiguration::new(
            tree_sitter_javascript::language(),
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            config.configure(HIGHLIGHT_NAMES);
            self.js_config = Some(config);
        }

        // TypeScript
        if let Ok(mut config) = HighlightConfiguration::new(
            tree_sitter_typescript::language_typescript(),
            tree_sitter_typescript::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            config.configure(HIGHLIGHT_NAMES);
            self.ts_config = Some(config);
        }

        // Python
        if let Ok(mut config) = HighlightConfiguration::new(
            tree_sitter_python::language(),
            tree_sitter_python::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            config.configure(HIGHLIGHT_NAMES);
            self.python_config = Some(config);
        }

        // C
        if let Ok(mut config) = HighlightConfiguration::new(
            tree_sitter_c::language(),
            tree_sitter_c::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            config.configure(HIGHLIGHT_NAMES);
            self.c_config = Some(config);
        }

        // C++
        if let Ok(mut config) = HighlightConfiguration::new(
            tree_sitter_cpp::language(),
            tree_sitter_cpp::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            config.configure(HIGHLIGHT_NAMES);
            self.cpp_config = Some(config);
        }

        // JSON
        if let Ok(mut config) = HighlightConfiguration::new(
            tree_sitter_json::language(),
            tree_sitter_json::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            config.configure(HIGHLIGHT_NAMES);
            self.json_config = Some(config);
        }

        // TOML - 使用 crate 自带的高亮查询
        if let Ok(mut config) = HighlightConfiguration::new(
            tree_sitter_toml::language(),
            tree_sitter_toml::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            config.configure(HIGHLIGHT_NAMES);
            self.toml_config = Some(config);
        }

        // Go
        if let Ok(mut config) = HighlightConfiguration::new(
            tree_sitter_go::language(),
            tree_sitter_go::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            config.configure(HIGHLIGHT_NAMES);
            self.go_config = Some(config);
        }

        // Java
        if let Ok(mut config) = HighlightConfiguration::new(
            tree_sitter_java::language(),
            tree_sitter_java::HIGHLIGHT_QUERY,
            "",
            "",
        ) {
            config.configure(HIGHLIGHT_NAMES);
            self.java_config = Some(config);
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
            "go" => self
                .go_config
                .as_ref()
                .map(|c| c as *const HighlightConfiguration)
                .unwrap_or(std::ptr::null()),
            "java" => self
                .java_config
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

        if let Ok(events) = self
            .highlighter
            .highlight(config, text.as_bytes(), None, |_| None)
        {
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
                        // H-16: 按 capture 名称而非索引映射 TokenKind，
                        // 因为不同语言的 highlight query 定义不同的 capture 顺序
                        let name = config.names().get(s.0).map(|s| s.as_str()).unwrap_or("");
                        current_kind = capture_name_to_token_kind(name);
                        in_highlight = true;
                    }
                    Ok(HighlightEvent::HighlightEnd) => {
                        in_highlight = false;
                    }
                    Err(_) => {}
                }
            }
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
            "go" => Some(tree_sitter_go::language()),
            "java" => Some(tree_sitter_java::language()),
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
            "go" => self.go_config.as_ref(),
            "java" => self.java_config.as_ref(),
            _ => None,
        }
    }

    pub fn supports_language(&self, language: &str) -> bool {
        self.get_config(language).is_some()
    }

    /// 获取 config 的原始指针（用于绕过借用冲突）
    fn get_config_ptr(&self, language: &str) -> *const HighlightConfiguration {
        match language {
            "rust" => self
                .rust_config
                .as_ref()
                .map(|c| c as *const _)
                .unwrap_or(std::ptr::null()),
            "javascript" | "js" => self
                .js_config
                .as_ref()
                .map(|c| c as *const _)
                .unwrap_or(std::ptr::null()),
            "typescript" | "ts" | "tsx" => self
                .ts_config
                .as_ref()
                .map(|c| c as *const _)
                .unwrap_or(std::ptr::null()),
            "python" | "py" => self
                .python_config
                .as_ref()
                .map(|c| c as *const _)
                .unwrap_or(std::ptr::null()),
            "c" => self
                .c_config
                .as_ref()
                .map(|c| c as *const _)
                .unwrap_or(std::ptr::null()),
            "cpp" | "c++" | "cxx" => self
                .cpp_config
                .as_ref()
                .map(|c| c as *const _)
                .unwrap_or(std::ptr::null()),
            "json" => self
                .json_config
                .as_ref()
                .map(|c| c as *const _)
                .unwrap_or(std::ptr::null()),
            "toml" => self
                .toml_config
                .as_ref()
                .map(|c| c as *const _)
                .unwrap_or(std::ptr::null()),
            "go" => self
                .go_config
                .as_ref()
                .map(|c| c as *const _)
                .unwrap_or(std::ptr::null()),
            "java" => self
                .java_config
                .as_ref()
                .map(|c| c as *const _)
                .unwrap_or(std::ptr::null()),
            _ => std::ptr::null(),
        }
    }

    /// 对整个文档进行高亮，返回每行的 token 列表
    ///
    /// 一次解析全文并高亮，比逐行 `highlight_line` 更正确（支持多行构造如块注释、
    /// 三引号字符串、宏等）且更高效（避免逐行重复初始化解析器）。
    ///
    /// 注意：`tree-sitter-highlight` 0.20.x 的 `Highlighter::highlight` 第三参数为
    /// `cancellation_flag: Option<&AtomicUsize>`（取消标志），而非旧语法树。
    /// `Highlighter` 内部维护自己的 `Parser`，每次调用做完整解析。
    /// 真正的增量解析需要升级到 `tree-sitter-highlight` 0.22+ 或手动遍历语法树。
    pub fn highlight_document(
        &mut self,
        doc_id: &str,
        language: &str,
        full_text: &str,
    ) -> Vec<Vec<LexemeSpan>> {
        // 预计算行数和行起始偏移
        let mut line_starts: Vec<usize> = vec![0];
        for (i, b) in full_text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        let line_count = line_starts.len();
        let mut result: Vec<Vec<LexemeSpan>> = vec![Vec::new(); line_count];

        // 获取 config（raw pointer 避免与 self.highlighter 的借用冲突）
        let config_ptr = self.get_config_ptr(language);
        let config = match unsafe { config_ptr.as_ref() } {
            Some(c) => c,
            None => return result,
        };

        // 更新语法树缓存（供代码折叠、结构导航等功能使用）
        self.parse_document(doc_id, language, full_text);

        // 调用 highlighter（第三参数为 cancellation_flag，传 None 表示不可取消）
        let events = self
            .highlighter
            .highlight(config, full_text.as_bytes(), None, |_| None);

        if let Ok(events) = events {
            let mut current_start = 0usize;
            let mut current_kind = TokenKind::Unknown;
            let mut in_highlight = false;

            for event in events {
                match event {
                    Ok(HighlightEvent::Source { start, end: _ }) => {
                        if in_highlight && start > current_start {
                            assign_segment_to_lines(
                                current_start,
                                start,
                                current_kind,
                                &line_starts,
                                &mut result,
                            );
                        }
                        current_start = start;
                    }
                    Ok(HighlightEvent::HighlightStart(s)) => {
                        let name = config.names().get(s.0).map(|s| s.as_str()).unwrap_or("");
                        current_kind = capture_name_to_token_kind(name);
                        in_highlight = true;
                    }
                    Ok(HighlightEvent::HighlightEnd) => {
                        in_highlight = false;
                    }
                    Err(_) => {}
                }
            }
        }

        result
    }
}

/// 将字节区间 [byte_start, byte_end) 分配到对应的行
fn assign_segment_to_lines(
    byte_start: usize,
    byte_end: usize,
    kind: TokenKind,
    line_starts: &[usize],
    result: &mut [Vec<LexemeSpan>],
) {
    // 二分查找 byte_start 所在行
    let line_idx = line_starts
        .binary_search(&byte_start)
        .unwrap_or_else(|i| i.saturating_sub(1));

    let mut current_line = line_idx;
    let mut offset = byte_start;

    while offset < byte_end && current_line < line_starts.len() {
        let l_start = line_starts[current_line];
        let l_end = line_starts
            .get(current_line + 1)
            .copied()
            .unwrap_or(usize::MAX)
            .min(byte_end);

        if l_end > offset {
            result[current_line].push(LexemeSpan {
                start: offset - l_start,
                len: l_end - offset,
                kind,
                flags: 0,
            });
        }
        offset = l_end;
        current_line += 1;
    }
}

impl Default for TreeSitterHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

/// 按 capture 索引映射 TokenKind（与 HIGHLIGHT_NAMES 顺序对应）
#[cfg(test)]
fn capture_to_token_kind(index: usize) -> TokenKind {
    match index {
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

/// H-16: 按 capture 名称映射 TokenKind，兼容不同语言的 highlight query
///
/// tree-sitter-highlight 的 capture 名称遵循 tree-sitter 标准 highlight 规范
/// (https://tree-sitter.github.io/tree-sitter/syntax-highlighting#captures)
/// 不同语言定义不同的 capture 顺序，因此必须按名称而非索引映射。
fn capture_name_to_token_kind(name: &str) -> TokenKind {
    match name {
        "keyword" => TokenKind::Keyword,
        "string" | "string.special" => TokenKind::StringLiteral,
        "number" => TokenKind::NumberLiteral,
        "comment" => TokenKind::LineComment,
        "function" | "function.call" | "function.builtin" | "method" | "method.call" => {
            TokenKind::Function
        }
        "type" | "type.builtin" | "constructor" => TokenKind::TypeName,
        "operator" => TokenKind::Operator,
        "constant" | "constant.builtin" => TokenKind::NumberLiteral,
        "variable" | "variable.builtin" | "variable.parameter" | "identifier" => {
            TokenKind::Identifier
        }
        "preproc" => TokenKind::Preprocessor,
        "attribute" => TokenKind::Attribute,
        _ => TokenKind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlighter_new() {
        let highlighter = TreeSitterHighlighter::new();
        assert!(highlighter.supports_language("rust"));
        assert!(highlighter.supports_language("javascript"));
        assert!(highlighter.supports_language("js"));
        assert!(highlighter.supports_language("typescript"));
        assert!(highlighter.supports_language("ts"));
        assert!(highlighter.supports_language("tsx"));
        assert!(highlighter.supports_language("python"));
        assert!(highlighter.supports_language("py"));
        assert!(highlighter.supports_language("c"));
        assert!(highlighter.supports_language("cpp"));
        assert!(highlighter.supports_language("c++"));
        assert!(highlighter.supports_language("cxx"));
        assert!(highlighter.supports_language("json"));
        assert!(highlighter.supports_language("toml"));
        assert!(highlighter.supports_language("go"));
        assert!(highlighter.supports_language("java"));
        assert!(highlighter.tree_cache.is_empty());
        assert!(highlighter.parser_cache.is_empty());
    }

    #[test]
    fn test_highlighter_default() {
        let highlighter = TreeSitterHighlighter::default();
        assert!(highlighter.supports_language("rust"));
        assert!(highlighter.supports_language("json"));
    }

    #[test]
    fn test_supports_language_unsupported() {
        let highlighter = TreeSitterHighlighter::new();
        assert!(!highlighter.supports_language("html"));
        assert!(!highlighter.supports_language("markdown"));
        assert!(!highlighter.supports_language("md"));
        assert!(!highlighter.supports_language("unknown"));
        assert!(!highlighter.supports_language(""));
    }

    #[test]
    fn test_highlight_line_rust() {
        let mut highlighter = TreeSitterHighlighter::new();
        let spans = highlighter.highlight_line("fn main() {}", "rust");
        assert!(!spans.is_empty(), "Rust 简单代码应产生高亮 span");
    }

    #[test]
    fn test_highlight_line_python() {
        let mut highlighter = TreeSitterHighlighter::new();
        // 使用内联字符串，不依赖外部文件
        let spans = highlighter.highlight_line("def hello():\n    pass", "python");
        assert!(!spans.is_empty(), "Python 简单代码应产生高亮 span");
    }

    #[test]
    fn test_highlight_line_javascript() {
        let mut highlighter = TreeSitterHighlighter::new();
        let spans = highlighter.highlight_line("function foo() { return 42; }", "javascript");
        assert!(!spans.is_empty(), "JavaScript 简单代码应产生高亮 span");
    }

    #[test]
    fn test_highlight_line_typescript() {
        let mut highlighter = TreeSitterHighlighter::new();
        let spans = highlighter.highlight_line(
            "function add(x: number): number { return x; }",
            "typescript",
        );
        assert!(!spans.is_empty(), "TypeScript 简单代码应产生高亮 span");
    }

    #[test]
    fn test_highlight_line_c() {
        let mut highlighter = TreeSitterHighlighter::new();
        let spans = highlighter.highlight_line("int main(void) { return 0; }", "c");
        assert!(!spans.is_empty(), "C 简单代码应产生高亮 span");
    }

    #[test]
    fn test_highlight_line_cpp() {
        let mut highlighter = TreeSitterHighlighter::new();
        // C++ 查询将 using / namespace 视为 keyword
        let spans = highlighter.highlight_line("using namespace std;", "cpp");
        assert!(!spans.is_empty(), "C++ 简单代码应产生高亮 span");
    }

    #[test]
    fn test_highlight_line_json() {
        let mut highlighter = TreeSitterHighlighter::new();
        let spans = highlighter.highlight_line(r#"{ "key": "value" }"#, "json");
        assert!(!spans.is_empty(), "JSON 简单代码应产生高亮 span");
    }

    #[test]
    fn test_highlight_line_toml() {
        let mut highlighter = TreeSitterHighlighter::new();
        let spans = highlighter.highlight_line("key = \"value\"", "toml");
        assert!(!spans.is_empty(), "TOML 简单代码应产生高亮 span");
    }

    #[test]
    fn test_highlight_line_go() {
        let mut highlighter = TreeSitterHighlighter::new();
        let spans = highlighter.highlight_line("func main() { return }", "go");
        assert!(!spans.is_empty(), "Go 简单代码应产生高亮 span");
    }

    #[test]
    fn test_highlight_line_java() {
        let mut highlighter = TreeSitterHighlighter::new();
        let spans = highlighter.highlight_line("class Foo { void bar() { return; } }", "java");
        assert!(!spans.is_empty(), "Java 简单代码应产生高亮 span");
    }

    #[test]
    fn test_highlight_line_unsupported_language() {
        let mut highlighter = TreeSitterHighlighter::new();
        let spans = highlighter.highlight_line("hello", "unknown");
        assert!(spans.is_empty());
        let spans_html = highlighter.highlight_line("<div></div>", "html");
        assert!(spans_html.is_empty());
    }

    #[test]
    fn test_highlight_line_language_alias() {
        let mut highlighter = TreeSitterHighlighter::new();
        let via_rust = highlighter.highlight_line("fn main() {}", "rust");
        let via_rs = highlighter.highlight_line("fn main() {}", "rs");
        assert!(
            via_rs.is_empty(),
            "highlighter 仅识别标准语言名，别名 rs 不在 highlight_line 匹配中"
        );
        assert!(!via_rust.is_empty());
    }

    #[test]
    fn test_parse_document_rust() {
        let mut highlighter = TreeSitterHighlighter::new();
        let tree = highlighter.parse_document("doc1", "rust", "fn main() {}");
        assert!(tree.is_some(), "Rust 文档应能成功解析");
        assert!(highlighter.get_tree("doc1").is_some());
    }

    #[test]
    fn test_parse_document_python() {
        let mut highlighter = TreeSitterHighlighter::new();
        let tree = highlighter.parse_document("doc_py", "python", "def hello():\n    pass");
        assert!(tree.is_some());
    }

    #[test]
    fn test_parse_document_javascript() {
        let mut highlighter = TreeSitterHighlighter::new();
        let tree = highlighter.parse_document("doc_js", "javascript", "function foo() {}");
        assert!(tree.is_some());
    }

    #[test]
    fn test_parse_document_typescript() {
        let mut highlighter = TreeSitterHighlighter::new();
        let tree = highlighter.parse_document(
            "doc_ts",
            "typescript",
            "function foo(): number { return 1; }",
        );
        assert!(tree.is_some());
    }

    #[test]
    fn test_parse_document_c() {
        let mut highlighter = TreeSitterHighlighter::new();
        let tree = highlighter.parse_document("doc_c", "c", "int main(void) { return 0; }");
        assert!(tree.is_some());
    }

    #[test]
    fn test_parse_document_cpp() {
        let mut highlighter = TreeSitterHighlighter::new();
        let tree = highlighter.parse_document("doc_cpp", "cpp", "int main() { return 0; }");
        assert!(tree.is_some());
    }

    #[test]
    fn test_parse_document_json() {
        let mut highlighter = TreeSitterHighlighter::new();
        let tree = highlighter.parse_document("doc_json", "json", r#"{ "a": 1 }"#);
        assert!(tree.is_some());
    }

    #[test]
    fn test_parse_document_toml() {
        let mut highlighter = TreeSitterHighlighter::new();
        let tree = highlighter.parse_document("doc_toml", "toml", "key = \"value\"");
        assert!(tree.is_some());
    }

    #[test]
    fn test_parse_document_go() {
        let mut highlighter = TreeSitterHighlighter::new();
        let tree = highlighter.parse_document("doc_go", "go", "package main\nfunc main() {}");
        assert!(tree.is_some());
    }

    #[test]
    fn test_parse_document_java() {
        let mut highlighter = TreeSitterHighlighter::new();
        let tree = highlighter.parse_document("doc_java", "java", "class Foo {}");
        assert!(tree.is_some());
    }

    #[test]
    fn test_parse_document_unsupported_language() {
        let mut highlighter = TreeSitterHighlighter::new();
        let tree = highlighter.parse_document("doc_html", "html", "<div></div>");
        assert!(tree.is_none());
        let tree_md = highlighter.parse_document("doc_md", "markdown", "# Title");
        assert!(tree_md.is_none());
        let tree_unknown = highlighter.parse_document("doc_x", "unknown", "hello");
        assert!(tree_unknown.is_none());
    }

    #[test]
    fn test_get_tree_missing_document() {
        let highlighter = TreeSitterHighlighter::new();
        assert!(highlighter.get_tree("missing").is_none());
    }

    #[test]
    fn test_remove_document() {
        let mut highlighter = TreeSitterHighlighter::new();
        highlighter.parse_document("doc1", "rust", "fn main() {}");
        assert!(highlighter.get_tree("doc1").is_some());
        highlighter.remove_document("doc1");
        assert!(highlighter.get_tree("doc1").is_none());
        assert!(!highlighter.parser_cache.contains_key("doc1"));
    }

    #[test]
    fn test_tree_cache_eviction() {
        let mut highlighter = TreeSitterHighlighter::new();
        for i in 0..MAX_HIGHLIGHTER_DOCS {
            highlighter.parse_document(&format!("doc{}", i), "rust", "fn main() {}");
        }
        assert_eq!(highlighter.tree_cache.len(), MAX_HIGHLIGHTER_DOCS);

        // 第 33 个文档触发全量淘汰
        highlighter.parse_document("doc_overflow", "rust", "fn main() {}");
        assert_eq!(
            highlighter.tree_cache.len(),
            1,
            "超过缓存上限后应清空并只保留最新文档"
        );
        assert!(highlighter.get_tree("doc_overflow").is_some());
    }

    #[test]
    fn test_capture_to_token_kind() {
        assert_eq!(capture_to_token_kind(0), TokenKind::Keyword);
        assert_eq!(capture_to_token_kind(1), TokenKind::StringLiteral);
        assert_eq!(capture_to_token_kind(2), TokenKind::NumberLiteral);
        assert_eq!(capture_to_token_kind(3), TokenKind::LineComment);
        assert_eq!(capture_to_token_kind(4), TokenKind::Function);
        assert_eq!(capture_to_token_kind(5), TokenKind::TypeName);
        assert_eq!(capture_to_token_kind(6), TokenKind::Operator);
        assert_eq!(capture_to_token_kind(7), TokenKind::Identifier);
        assert_eq!(capture_to_token_kind(8), TokenKind::Preprocessor);
        assert_eq!(capture_to_token_kind(9), TokenKind::Attribute);
        assert_eq!(capture_to_token_kind(10), TokenKind::Unknown);
        assert_eq!(capture_to_token_kind(usize::MAX), TokenKind::Unknown);
    }
}
