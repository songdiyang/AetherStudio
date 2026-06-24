use tree_sitter::Language;

/// 根据语言ID获取 Tree-sitter Language
/// 注意: HTML 不在此列表中，因为 tree-sitter-html crate 版本冲突
/// HTML 高亮由 aether-core 的自定义 html_lexer 处理
pub fn get_language(language_id: &str) -> Option<Language> {
    match language_id {
        "rust" | "rs" => Some(tree_sitter_rust::language()),
        "javascript" | "js" | "jsx" | "mjs" | "cjs" => Some(tree_sitter_javascript::language()),
        "typescript" | "ts" | "tsx" | "mts" | "cts" => {
            Some(tree_sitter_typescript::language_typescript())
        }
        "python" | "py" | "pyw" | "pyi" => Some(tree_sitter_python::language()),
        "c" | "h" => Some(tree_sitter_c::language()),
        "cpp" | "hpp" | "cc" | "cxx" | "c++" => Some(tree_sitter_cpp::language()),
        "json" => Some(tree_sitter_json::language()),
        "toml" => Some(tree_sitter_toml::language()),
        _ => None,
    }
}
