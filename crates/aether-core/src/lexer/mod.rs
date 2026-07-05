/// 通用词法分析器 trait
pub trait Lexer {
    /// 对单行文本进行全量词法分析
    fn lex_full(&self, text: &str) -> Vec<LexemeSpan>;
}

/// 通用 Token 类型（跨语言统一）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TokenKind {
    // === 通用类别 ===
    // 关键字
    Keyword,
    // 标识符
    Identifier,
    // 字符串字面量
    StringLiteral,
    // 字符字面量
    CharLiteral,
    // 数字字面量
    NumberLiteral,
    // 注释
    LineComment,
    BlockComment,
    DocComment,
    // 运算符
    Operator,
    // 分隔符/标点
    Punctuation,
    // 预处理/指令
    Preprocessor,
    // 属性/注解/装饰器
    Attribute,
    // 类型名
    TypeName,
    // 函数名/方法名
    Function,
    // 宏
    Macro,
    // 生命周期（Rust专用）
    Lifetime,
    // 模板/泛型参数
    Generic,
    // 正则表达式字面量
    RegexLiteral,
    // 格式化字符串
    FormatString,
    // Markdown 标题
    MdHeading,
    // Markdown 链接
    MdLink,
    // Markdown 代码标记
    MdCode,
    // Markdown 强调
    MdEmphasis,
    // JSON 键
    JsonKey,
    // TOML 表头
    TomlTable,
    // 空白
    Whitespace,
    // 换行
    Newline,
    // 未知
    Unknown,
    // 文件结束
    EOF,
}

/// 词法单元跨度
#[derive(Clone, Debug, PartialEq)]
pub struct LexemeSpan {
    pub start: usize,
    pub len: usize,
    pub kind: TokenKind,
    pub flags: u8,
}

/// 语言类型
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    C,
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Json,
    Markdown,
    Toml,
    Html,
    Css,
    PlainText,
    Image,
}

impl Language {
    /// 根据文件扩展名检测语言
    /// 对于没有独立 lexer 的扩展名，尽量归入语义相近的语言（如 vue/wxml 用 HTML lexer），
    /// 完全未知的扩展名统一归为 PlainText，保证任何文本文件都能被查看。
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            // C/C++ 家族
            "c" | "h" | "cpp" | "hpp" | "cc" | "cxx" | "m" | "mm" => Language::C,
            // Rust
            "rs" => Language::Rust,
            // Python
            "py" | "pyw" | "pyi" | "pyx" | "pxd" => Language::Python,
            // JavaScript/TypeScript 及其衍生
            "js" | "jsx" | "mjs" | "cjs" | "es" | "es6" => Language::JavaScript,
            "ts" | "tsx" | "mts" | "cts" => Language::TypeScript,
            // JSON / JSON-like
            "json" | "jsonc" | "jsonl" => Language::Json,
            // Markdown / 文档
            "md" | "markdown" | "mdx" => Language::Markdown,
            // TOML / INI / 配置
            "toml" | "ini" | "cfg" | "conf" | "config" => Language::Toml,
            // HTML / 模板 / 类 XML 标记
            "html" | "htm" | "xhtml" | "vue" | "svelte" | "wxml" | "axml" | "ftl" | "jinja"
            | "j2" | "njk" | "mustache" | "handlebars" | "hbs" | "ejs" | "erb" | "haml"
            | "pug" | "jade" | "liquid" | "razor" | "cshtml" => Language::Html,
            // CSS / 样式
            "css" | "scss" | "sass" | "less" | "styl" | "stylus" | "wxss" | "acss" => {
                Language::Css
            }
            // 图片（仅用于文件树图标/路由，不用于lexer）
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "svg" | "tiff"
            | "tif" | "raw" | "psd" => Language::Image,
            _ => Language::PlainText,
        }
    }

    /// 根据文件路径检测语言
    pub fn from_path(path: &std::path::Path) -> Self {
        path.extension()
            .and_then(|e| e.to_str())
            .map(Language::from_extension)
            .unwrap_or(Language::PlainText)
    }

    /// 创建对应语言的词法分析器
    pub fn create_lexer(&self) -> Box<dyn Lexer> {
        match self {
            Language::C => Box::new(c_lexer::CLexer::new()),
            Language::Rust => Box::new(rust_lexer::RustLexer::new()),
            Language::Python => Box::new(python_lexer::PythonLexer::new()),
            Language::JavaScript | Language::TypeScript => Box::new(js_lexer::JsLexer::new()),
            Language::Json => Box::new(json_lexer::JsonLexer::new()),
            Language::Markdown => Box::new(markdown_lexer::MarkdownLexer::new()),
            Language::Toml => Box::new(toml_lexer::TomlLexer::new()),
            Language::Html => Box::new(html_lexer::HtmlLexer::new()),
            // CSS 暂时没有独立 lexer，复用 HTML lexer 至少能高亮注释、字符串、标签等公共结构
            Language::Css => Box::new(html_lexer::HtmlLexer::new()),
            Language::PlainText => Box::new(PlainTextLexer::new()),
            Language::Image => Box::new(PlainTextLexer::new()),
        }
    }

    /// 直接对指定语言的文本进行词法分析，使用静态分发，无 Box 分配与动态分发开销。
    pub fn lex_full(&self, text: &str) -> Vec<LexemeSpan> {
        match self {
            Language::C => c_lexer::CLexer::new().lex_full(text),
            Language::Rust => rust_lexer::RustLexer::new().lex_full(text),
            Language::Python => python_lexer::PythonLexer::new().lex_full(text),
            Language::JavaScript | Language::TypeScript => js_lexer::JsLexer::new().lex_full(text),
            Language::Json => json_lexer::JsonLexer::new().lex_full(text),
            Language::Markdown => markdown_lexer::MarkdownLexer::new().lex_full(text),
            Language::Toml => toml_lexer::TomlLexer::new().lex_full(text),
            Language::Html => html_lexer::HtmlLexer::new().lex_full(text),
            Language::Css => html_lexer::HtmlLexer::new().lex_full(text),
            Language::PlainText => PlainTextLexer::new().lex_full(text),
            Language::Image => PlainTextLexer::new().lex_full(text),
        }
    }
}

pub mod c_lexer;
pub mod common;
pub mod html_lexer;
pub mod js_lexer;
pub mod json_lexer;
pub mod markdown_lexer;
pub mod python_lexer;
pub mod rust_lexer;
pub mod toml_lexer;

/// 纯文本词法分析器（无高亮）
pub struct PlainTextLexer;

impl PlainTextLexer {
    pub fn new() -> Self {
        Self
    }
}

impl Lexer for PlainTextLexer {
    fn lex_full(&self, text: &str) -> Vec<LexemeSpan> {
        if text.is_empty() {
            return Vec::new();
        }
        vec![LexemeSpan {
            start: 0,
            len: text.len(),
            kind: TokenKind::Unknown,
            flags: 0,
        }]
    }
}

impl Default for PlainTextLexer {
    fn default() -> Self {
        Self::new()
    }
}

/// 根据 UTF-8 首字节推断字符的字节长度。
/// 非法或 ASCII 字节返回 1，保证 lexer 至少能前进一步。
pub(crate) fn utf8_char_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}
