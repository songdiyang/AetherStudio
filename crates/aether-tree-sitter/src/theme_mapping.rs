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
