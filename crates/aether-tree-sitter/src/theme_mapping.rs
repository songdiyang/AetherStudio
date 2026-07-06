/// Tree-sitter capture name 到 TextMate scope 的映射
/// 这是连接 Tree-sitter 高亮系统和 VS Code 主题生态的关键层
use std::collections::HashMap;

/// 获取 capture name 对应的 TextMate scope
pub fn capture_to_textmate_scope(capture: &str) -> &'static str {
    match capture {
        // 变量
        "variable" => "variable.other.readwrite",
        "variable.parameter" => "variable.parameter",
        "variable.builtin" => "variable.language",
        "variable.member" => "variable.other.property",

        // 常量
        "constant" => "constant.other",
        "constant.builtin" => "constant.language",
        "constant.macro" => "constant.other",

        // 模块/命名空间
        "module" => "entity.name.namespace",
        "namespace" => "entity.name.namespace",

        // 类型
        "type" => "entity.name.type",
        "type.builtin" => "support.type",
        "type.definition" => "entity.name.type.definition",

        // 类/接口/枚举/结构体
        "class" => "entity.name.class",
        "class.definition" => "entity.name.class.definition",
        "interface" => "entity.name.interface",
        "enum" => "entity.name.enum",
        "struct" => "entity.name.struct",
        "union" => "entity.name.union",

        // 函数/方法
        "function" => "entity.name.function",
        "function.builtin" => "support.function",
        "function.macro" => "entity.name.function.macro",
        "function.definition" => "entity.name.function.definition",
        "method" => "entity.name.function.member",
        "method.definition" => "entity.name.function.member.definition",
        "constructor" => "entity.name.function.constructor",
        "call" => "entity.name.function.call",

        // 属性
        "property" => "entity.name.property",
        "property.definition" => "entity.name.property.definition",
        "field" => "entity.name.field",

        // 关键字
        "keyword" => "keyword.control",
        "keyword.conditional" => "keyword.control.conditional",
        "keyword.repeat" => "keyword.control.repeat",
        "keyword.return" => "keyword.control.return",
        "keyword.import" => "keyword.control.import",
        "keyword.exception" => "keyword.control.exception",
        "keyword.operator" => "keyword.operator",
        "keyword.directive" => "keyword.control.directive",
        "keyword.function" => "keyword.declaration.function",
        "keyword.type" => "keyword.declaration.type",
        "keyword.storage" => "storage.type",
        "keyword.modifier" => "storage.modifier",

        // 运算符
        "operator" => "keyword.operator",
        "arithmetic" => "keyword.operator.arithmetic",
        "logical" => "keyword.operator.logical",
        "comparison" => "keyword.operator.comparison",
        "assignment" => "keyword.operator.assignment",

        // 注释
        "comment" => "comment",
        "comment.line" => "comment.line",
        "comment.block" => "comment.block",
        "comment.documentation" => "comment.block.documentation",
        "doc" => "comment.block.documentation",

        // 字符串
        "string" => "string.quoted",
        "string.special" => "string.special",
        "string.escape" => "constant.character.escape",
        "string.regexp" => "string.regexp",
        "character" => "string.quoted.single",

        // 数字
        "number" => "constant.numeric",
        "float" => "constant.numeric.float",
        "integer" => "constant.numeric.integer",
        "boolean" => "constant.language.boolean",

        // 标签（HTML/XML）
        "tag" => "entity.name.tag",
        "tag.builtin" => "entity.name.tag.builtin",
        "tag.delimiter" => "punctuation.definition.tag",
        "attribute" => "entity.other.attribute-name",

        // 标点
        "punctuation" => "punctuation",
        "punctuation.delimiter" => "punctuation.separator",
        "punctuation.bracket" => "punctuation.section",
        "punctuation.special" => "punctuation.definition",

        // 标签（Rust 生命周期等）
        "label" => "entity.name.label",
        "lifetime" => "storage.modifier.lifetime",

        // 宏
        "macro" => "entity.name.function.macro",

        // 包含
        "include" => "keyword.control.import",

        // 异常
        "exception" => "keyword.control.exception",

        // 默认回退
        _ => "source",
    }
}

/// 构建完整的映射表
pub fn build_theme_mapping() -> HashMap<String, String> {
    let mut map = HashMap::new();

    let captures = [
        "variable",
        "variable.parameter",
        "variable.builtin",
        "variable.member",
        "constant",
        "constant.builtin",
        "constant.macro",
        "module",
        "namespace",
        "type",
        "type.builtin",
        "type.definition",
        "class",
        "class.definition",
        "interface",
        "enum",
        "struct",
        "union",
        "function",
        "function.builtin",
        "function.macro",
        "function.definition",
        "method",
        "method.definition",
        "constructor",
        "call",
        "property",
        "property.definition",
        "field",
        "keyword",
        "keyword.conditional",
        "keyword.repeat",
        "keyword.return",
        "keyword.import",
        "keyword.exception",
        "keyword.operator",
        "keyword.directive",
        "keyword.function",
        "keyword.type",
        "keyword.storage",
        "keyword.modifier",
        "operator",
        "arithmetic",
        "logical",
        "comparison",
        "assignment",
        "comment",
        "comment.line",
        "comment.block",
        "comment.documentation",
        "doc",
        "string",
        "string.special",
        "string.escape",
        "string.regexp",
        "character",
        "number",
        "float",
        "integer",
        "boolean",
        "tag",
        "tag.builtin",
        "tag.delimiter",
        "attribute",
        "punctuation",
        "punctuation.delimiter",
        "punctuation.bracket",
        "punctuation.special",
        "label",
        "lifetime",
        "macro",
        "include",
        "exception",
    ];

    for capture in &captures {
        map.insert(
            capture.to_string(),
            capture_to_textmate_scope(capture).to_string(),
        );
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_to_textmate_scope_variables() {
        assert_eq!(capture_to_textmate_scope("variable"), "variable.other.readwrite");
        assert_eq!(capture_to_textmate_scope("variable.parameter"), "variable.parameter");
        assert_eq!(capture_to_textmate_scope("variable.builtin"), "variable.language");
        assert_eq!(capture_to_textmate_scope("variable.member"), "variable.other.property");
    }

    #[test]
    fn test_capture_to_textmate_scope_constants() {
        assert_eq!(capture_to_textmate_scope("constant"), "constant.other");
        assert_eq!(capture_to_textmate_scope("constant.builtin"), "constant.language");
        assert_eq!(capture_to_textmate_scope("constant.macro"), "constant.other");
    }

    #[test]
    fn test_capture_to_textmate_scope_modules_and_types() {
        assert_eq!(capture_to_textmate_scope("module"), "entity.name.namespace");
        assert_eq!(capture_to_textmate_scope("namespace"), "entity.name.namespace");
        assert_eq!(capture_to_textmate_scope("type"), "entity.name.type");
        assert_eq!(capture_to_textmate_scope("type.builtin"), "support.type");
        assert_eq!(capture_to_textmate_scope("type.definition"), "entity.name.type.definition");
    }

    #[test]
    fn test_capture_to_textmate_scope_classes_and_structs() {
        assert_eq!(capture_to_textmate_scope("class"), "entity.name.class");
        assert_eq!(capture_to_textmate_scope("class.definition"), "entity.name.class.definition");
        assert_eq!(capture_to_textmate_scope("interface"), "entity.name.interface");
        assert_eq!(capture_to_textmate_scope("enum"), "entity.name.enum");
        assert_eq!(capture_to_textmate_scope("struct"), "entity.name.struct");
        assert_eq!(capture_to_textmate_scope("union"), "entity.name.union");
    }

    #[test]
    fn test_capture_to_textmate_scope_functions() {
        assert_eq!(capture_to_textmate_scope("function"), "entity.name.function");
        assert_eq!(capture_to_textmate_scope("function.builtin"), "support.function");
        assert_eq!(capture_to_textmate_scope("function.macro"), "entity.name.function.macro");
        assert_eq!(capture_to_textmate_scope("function.definition"), "entity.name.function.definition");
        assert_eq!(capture_to_textmate_scope("method"), "entity.name.function.member");
        assert_eq!(capture_to_textmate_scope("method.definition"), "entity.name.function.member.definition");
        assert_eq!(capture_to_textmate_scope("constructor"), "entity.name.function.constructor");
        assert_eq!(capture_to_textmate_scope("call"), "entity.name.function.call");
    }

    #[test]
    fn test_capture_to_textmate_scope_properties() {
        assert_eq!(capture_to_textmate_scope("property"), "entity.name.property");
        assert_eq!(capture_to_textmate_scope("property.definition"), "entity.name.property.definition");
        assert_eq!(capture_to_textmate_scope("field"), "entity.name.field");
    }

    #[test]
    fn test_capture_to_textmate_scope_keywords() {
        assert_eq!(capture_to_textmate_scope("keyword"), "keyword.control");
        assert_eq!(capture_to_textmate_scope("keyword.conditional"), "keyword.control.conditional");
        assert_eq!(capture_to_textmate_scope("keyword.repeat"), "keyword.control.repeat");
        assert_eq!(capture_to_textmate_scope("keyword.return"), "keyword.control.return");
        assert_eq!(capture_to_textmate_scope("keyword.import"), "keyword.control.import");
        assert_eq!(capture_to_textmate_scope("keyword.exception"), "keyword.control.exception");
        assert_eq!(capture_to_textmate_scope("keyword.operator"), "keyword.operator");
        assert_eq!(capture_to_textmate_scope("keyword.directive"), "keyword.control.directive");
        assert_eq!(capture_to_textmate_scope("keyword.function"), "keyword.declaration.function");
        assert_eq!(capture_to_textmate_scope("keyword.type"), "keyword.declaration.type");
        assert_eq!(capture_to_textmate_scope("keyword.storage"), "storage.type");
        assert_eq!(capture_to_textmate_scope("keyword.modifier"), "storage.modifier");
    }

    #[test]
    fn test_capture_to_textmate_scope_operators() {
        assert_eq!(capture_to_textmate_scope("operator"), "keyword.operator");
        assert_eq!(capture_to_textmate_scope("arithmetic"), "keyword.operator.arithmetic");
        assert_eq!(capture_to_textmate_scope("logical"), "keyword.operator.logical");
        assert_eq!(capture_to_textmate_scope("comparison"), "keyword.operator.comparison");
        assert_eq!(capture_to_textmate_scope("assignment"), "keyword.operator.assignment");
    }

    #[test]
    fn test_capture_to_textmate_scope_comments() {
        assert_eq!(capture_to_textmate_scope("comment"), "comment");
        assert_eq!(capture_to_textmate_scope("comment.line"), "comment.line");
        assert_eq!(capture_to_textmate_scope("comment.block"), "comment.block");
        assert_eq!(capture_to_textmate_scope("comment.documentation"), "comment.block.documentation");
        assert_eq!(capture_to_textmate_scope("doc"), "comment.block.documentation");
    }

    #[test]
    fn test_capture_to_textmate_scope_strings() {
        assert_eq!(capture_to_textmate_scope("string"), "string.quoted");
        assert_eq!(capture_to_textmate_scope("string.special"), "string.special");
        assert_eq!(capture_to_textmate_scope("string.escape"), "constant.character.escape");
        assert_eq!(capture_to_textmate_scope("string.regexp"), "string.regexp");
        assert_eq!(capture_to_textmate_scope("character"), "string.quoted.single");
    }

    #[test]
    fn test_capture_to_textmate_scope_numbers() {
        assert_eq!(capture_to_textmate_scope("number"), "constant.numeric");
        assert_eq!(capture_to_textmate_scope("float"), "constant.numeric.float");
        assert_eq!(capture_to_textmate_scope("integer"), "constant.numeric.integer");
        assert_eq!(capture_to_textmate_scope("boolean"), "constant.language.boolean");
    }

    #[test]
    fn test_capture_to_textmate_scope_tags_and_attributes() {
        assert_eq!(capture_to_textmate_scope("tag"), "entity.name.tag");
        assert_eq!(capture_to_textmate_scope("tag.builtin"), "entity.name.tag.builtin");
        assert_eq!(capture_to_textmate_scope("tag.delimiter"), "punctuation.definition.tag");
        assert_eq!(capture_to_textmate_scope("attribute"), "entity.other.attribute-name");
    }

    #[test]
    fn test_capture_to_textmate_scope_punctuation() {
        assert_eq!(capture_to_textmate_scope("punctuation"), "punctuation");
        assert_eq!(capture_to_textmate_scope("punctuation.delimiter"), "punctuation.separator");
        assert_eq!(capture_to_textmate_scope("punctuation.bracket"), "punctuation.section");
        assert_eq!(capture_to_textmate_scope("punctuation.special"), "punctuation.definition");
    }

    #[test]
    fn test_capture_to_textmate_scope_misc() {
        assert_eq!(capture_to_textmate_scope("label"), "entity.name.label");
        assert_eq!(capture_to_textmate_scope("lifetime"), "storage.modifier.lifetime");
        assert_eq!(capture_to_textmate_scope("macro"), "entity.name.function.macro");
        assert_eq!(capture_to_textmate_scope("include"), "keyword.control.import");
        assert_eq!(capture_to_textmate_scope("exception"), "keyword.control.exception");
    }

    #[test]
    fn test_capture_to_textmate_scope_default() {
        assert_eq!(capture_to_textmate_scope("unknown"), "source");
        assert_eq!(capture_to_textmate_scope(""), "source");
    }

    #[test]
    fn test_build_theme_mapping() {
        let map = build_theme_mapping();
        assert!(!map.is_empty());
        assert_eq!(map.get("variable"), Some(&"variable.other.readwrite".to_string()));
        assert_eq!(map.get("keyword"), Some(&"keyword.control".to_string()));
        assert_eq!(map.get("string"), Some(&"string.quoted".to_string()));
        assert_eq!(map.get("number"), Some(&"constant.numeric".to_string()));
        assert_eq!(map.get("comment"), Some(&"comment".to_string()));
        assert_eq!(map.get("function"), Some(&"entity.name.function".to_string()));
        assert_eq!(map.get("type"), Some(&"entity.name.type".to_string()));
        assert_eq!(map.get("unknown_capture"), None);
    }
}
