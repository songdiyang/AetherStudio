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
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "c" | "h" | "cpp" | "hpp" | "cc" | "cxx" => Language::C,
            "rs" => Language::Rust,
            "py" | "pyw" | "pyi" => Language::Python,
            "js" | "jsx" | "mjs" | "cjs" => Language::JavaScript,
            "ts" | "tsx" | "mts" | "cts" => Language::TypeScript,
            "json" => Language::Json,
            "md" | "markdown" => Language::Markdown,
            "toml" => Language::Toml,
            "html" | "htm" => Language::Html,
            "css" => Language::Css,
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "svg" => Language::Image,
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
            Language::Css => Box::new(PlainTextLexer::new()),
            Language::PlainText => Box::new(PlainTextLexer::new()),
            Language::Image => Box::new(PlainTextLexer::new()),
        }
    }
}

pub mod c_lexer;
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
