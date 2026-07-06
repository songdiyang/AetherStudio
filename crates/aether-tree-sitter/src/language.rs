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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_language_rust() {
        assert!(get_language("rust").is_some());
        assert!(get_language("rs").is_some());
    }

    #[test]
    fn test_get_language_javascript() {
        assert!(get_language("javascript").is_some());
        assert!(get_language("js").is_some());
        assert!(get_language("jsx").is_some());
        assert!(get_language("mjs").is_some());
        assert!(get_language("cjs").is_some());
    }

    #[test]
    fn test_get_language_typescript() {
        assert!(get_language("typescript").is_some());
        assert!(get_language("ts").is_some());
        assert!(get_language("tsx").is_some());
        assert!(get_language("mts").is_some());
        assert!(get_language("cts").is_some());
    }

    #[test]
    fn test_get_language_python() {
        assert!(get_language("python").is_some());
        assert!(get_language("py").is_some());
        assert!(get_language("pyw").is_some());
        assert!(get_language("pyi").is_some());
    }

    #[test]
    fn test_get_language_c() {
        assert!(get_language("c").is_some());
        assert!(get_language("h").is_some());
    }

    #[test]
    fn test_get_language_cpp() {
        assert!(get_language("cpp").is_some());
        assert!(get_language("hpp").is_some());
        assert!(get_language("cc").is_some());
        assert!(get_language("cxx").is_some());
        assert!(get_language("c++").is_some());
    }

    #[test]
    fn test_get_language_json() {
        assert!(get_language("json").is_some());
    }

    #[test]
    fn test_get_language_toml() {
        assert!(get_language("toml").is_some());
    }

    #[test]
    fn test_get_language_unsupported() {
        // HTML 与 Markdown 当前未集成 tree-sitter grammar，应返回 None
        assert!(get_language("html").is_none());
        assert!(get_language("markdown").is_none());
        assert!(get_language("md").is_none());
        assert!(get_language("unknown").is_none());
        assert!(get_language("").is_none());
    }
}
