use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::d2d::factory::color_f;
use crate::theme::{SyntaxColors, Theme};

/// VS Code 主题文件解析
/// 支持 `.json` 格式的 VS Code 主题文件
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct VsCodeThemeJson {
    /// 主题名称
    #[serde(default)]
    pub name: String,
    /// 主题类型（dark/light/hc）
    #[serde(rename = "type", default)]
    pub theme_type: String,
    /// UI 颜色映射（editor.background, editor.foreground 等）
    #[serde(default)]
    pub colors: HashMap<String, String>,
    /// Token 颜色规则列表
    #[serde(rename = "tokenColors", default)]
    pub token_colors: Vec<TokenColorRule>,
    /// 语义高亮规则（可选）
    #[serde(rename = "semanticHighlighting", default)]
    pub semantic_highlighting: bool,
    #[serde(rename = "semanticTokenColors", default)]
    pub semantic_token_colors: HashMap<String, String>,
}

/// Token 颜色规则
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct TokenColorRule {
    /// 名称（描述性）
    #[serde(default)]
    pub name: String,
    /// TextMate scope 列表（如 ["keyword.control", "storage.type"]）
    #[serde(default)]
    pub scope: TokenScope,
    /// 颜色设置
    pub settings: TokenSettings,
}

/// Scope 可以是单个字符串或字符串数组
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TokenScope {
    Single(String),
    Multiple(Vec<String>),
}

impl Default for TokenScope {
    fn default() -> Self {
        TokenScope::Single(String::new())
    }
}

impl TokenScope {
    pub fn as_list(&self) -> Vec<&str> {
        match self {
            TokenScope::Single(s) => vec![s.as_str()],
            TokenScope::Multiple(v) => v.iter().map(|s| s.as_str()).collect(),
        }
    }
}

/// Token 颜色设置
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct TokenSettings {
    /// 前景色（十六进制字符串如 "#FF5733"）
    #[serde(default)]
    pub foreground: Option<String>,
    /// 背景色
    #[serde(default)]
    pub background: Option<String>,
    /// 字体样式（bold, italic, underline）
    #[serde(default)]
    pub font_style: Option<String>,
}

/// 主题解析错误
#[derive(Clone, Debug)]
pub enum ThemeError {
    Io(String),
    Parse(String),
    InvalidColor(String),
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThemeError::Io(s) => write!(f, "IO error: {}", s),
            ThemeError::Parse(s) => write!(f, "Parse error: {}", s),
            ThemeError::InvalidColor(s) => write!(f, "Invalid color: {}", s),
        }
    }
}

impl std::error::Error for ThemeError {}

impl Theme {
    /// 从 VS Code JSON 主题文件加载主题
    pub fn from_vscode_json(path: &Path) -> Result<Self, ThemeError> {
        let content = std::fs::read_to_string(path).map_err(|e| ThemeError::Io(e.to_string()))?;
        let vscode_theme: VsCodeThemeJson =
            serde_json::from_str(&content).map_err(|e| ThemeError::Parse(e.to_string()))?;
        Ok(Self::from_vscode(&vscode_theme))
    }

    /// 从 VS Code 主题 JSON 字符串加载
    pub fn from_vscode_json_str(json: &str) -> Result<Self, ThemeError> {
        let vscode_theme: VsCodeThemeJson =
            serde_json::from_str(json).map_err(|e| ThemeError::Parse(e.to_string()))?;
        Ok(Self::from_vscode(&vscode_theme))
    }

    /// 从已解析的 VS Code 主题构建 Theme
    fn from_vscode(vscode: &VsCodeThemeJson) -> Self {
        let mut theme = Self::dark();

        // 解析 UI 颜色
        if let Some(bg) = vscode.colors.get("editor.background") {
            theme.editor_bg = parse_hex_color(bg).unwrap_or(theme.editor_bg);
        }
        if let Some(fg) = vscode.colors.get("editor.foreground") {
            theme.text_default = parse_hex_color(fg).unwrap_or(theme.text_default);
        }
        if let Some(sel) = vscode.colors.get("editor.selectionBackground") {
            theme.selection_bg = parse_hex_color(sel).unwrap_or(theme.selection_bg);
        }
        if let Some(cursor) = vscode.colors.get("editorCursor.foreground") {
            theme.cursor_color = parse_hex_color(cursor).unwrap_or(theme.cursor_color);
        }
        if let Some(ln_bg) = vscode.colors.get("editorLineNumber.foreground") {
            theme.line_number_fg = parse_hex_color(ln_bg).unwrap_or(theme.line_number_fg);
        }
        if let Some(ln_bg) = vscode.colors.get("editor.lineHighlightBackground") {
            theme.line_highlight_bg = parse_hex_color(ln_bg).unwrap_or(theme.line_highlight_bg);
        }
        if let Some(sb) = vscode.colors.get("sideBar.background") {
            theme.sidebar_bg = parse_hex_color(sb).unwrap_or(theme.sidebar_bg);
        }
        if let Some(st) = vscode.colors.get("statusBar.background") {
            theme.statusbar_bg = parse_hex_color(st).unwrap_or(theme.statusbar_bg);
        }
        if let Some(tab_a) = vscode.colors.get("tab.activeBackground") {
            theme.tab_active_bg = parse_hex_color(tab_a).unwrap_or(theme.tab_active_bg);
        }
        if let Some(tab_i) = vscode.colors.get("tab.inactiveBackground") {
            theme.tab_inactive_bg = parse_hex_color(tab_i).unwrap_or(theme.tab_inactive_bg);
        }
        // Glass theme extensions (optional VS Code keys)
        if let Some(tb) = vscode.colors.get("titleBar.activeBackground") {
            theme.titlebar_bg = parse_hex_color(tb).unwrap_or(theme.titlebar_bg);
        }
        if let Some(ab) = vscode.colors.get("activityBar.background") {
            theme.activity_bar_bg = parse_hex_color(ab).unwrap_or(theme.activity_bar_bg);
        }
        if let Some(pb) = vscode.colors.get("sideBar.border") {
            theme.panel_border = parse_hex_color(pb).unwrap_or(theme.panel_border);
        }
        if let Some(cp) = vscode.colors.get("dropdown.background") {
            theme.command_palette_bg = parse_hex_color(cp).unwrap_or(theme.command_palette_bg);
        }
        if let Some(mb) = vscode.colors.get("menu.background") {
            theme.submenu_bg = parse_hex_color(mb).unwrap_or(theme.submenu_bg);
        }

        // 解析 tokenColors 映射到 SyntaxColors
        theme.syntax = parse_syntax_colors(&vscode.token_colors);

        theme
    }
}

/// 解析语法颜色规则
fn parse_syntax_colors(rules: &[TokenColorRule]) -> SyntaxColors {
    let mut colors = SyntaxColors {
        ..SyntaxColors::dark_default()
    };

    for rule in rules {
        let color = match &rule.settings.foreground {
            Some(c) => match parse_hex_color(c) {
                Ok(c) => c,
                Err(_) => continue,
            },
            None => continue,
        };

        for scope in rule.scope.as_list() {
            match scope {
                "keyword" | "keyword.control" | "storage.type" | "storage.modifier" => {
                    colors.keyword = color;
                }
                "string" | "string.quoted" | "string.quoted.double" | "string.quoted.single" => {
                    colors.string = color;
                }
                "comment" | "comment.line" | "comment.block" => {
                    colors.comment = color;
                }
                "constant.numeric" | "constant" => {
                    colors.number = color;
                }
                "entity.name.function" | "support.function" => {
                    colors.function = color;
                }
                "entity.name.type" | "support.type" | "entity.name.class" => {
                    colors.type_name = color;
                }
                "variable" | "variable.other" | "identifier" => {
                    colors.variable = color;
                }
                "entity.name.tag" | "markup.heading" => {
                    colors.md_heading = color;
                }
                "markup.link" => {
                    colors.md_link = color;
                }
                "markup.inline.raw" | "markup.fenced_code.block" => {
                    colors.md_code = color;
                }
                "markup.italic" | "markup.bold" => {
                    colors.md_emphasis = color;
                }
                _ => {}
            }
        }
    }

    colors
}

/// 解析十六进制颜色字符串为 D2D1_COLOR_F
/// 支持格式：#RRGGBB, #RGB, #RRGGBBAA
fn parse_hex_color(hex: &str) -> Result<D2D1_COLOR_F, ThemeError> {
    let hex = hex.trim();
    let hex = hex.strip_prefix('#').unwrap_or(hex);

    let (r, g, b, a) = match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16)
                .map_err(|_| ThemeError::InvalidColor(hex.to_string()))?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16)
                .map_err(|_| ThemeError::InvalidColor(hex.to_string()))?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16)
                .map_err(|_| ThemeError::InvalidColor(hex.to_string()))?;
            (r, g, b, 255u8)
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16)
                .map_err(|_| ThemeError::InvalidColor(hex.to_string()))?;
            let g = u8::from_str_radix(&hex[2..4], 16)
                .map_err(|_| ThemeError::InvalidColor(hex.to_string()))?;
            let b = u8::from_str_radix(&hex[4..6], 16)
                .map_err(|_| ThemeError::InvalidColor(hex.to_string()))?;
            (r, g, b, 255u8)
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16)
                .map_err(|_| ThemeError::InvalidColor(hex.to_string()))?;
            let g = u8::from_str_radix(&hex[2..4], 16)
                .map_err(|_| ThemeError::InvalidColor(hex.to_string()))?;
            let b = u8::from_str_radix(&hex[4..6], 16)
                .map_err(|_| ThemeError::InvalidColor(hex.to_string()))?;
            let a = u8::from_str_radix(&hex[6..8], 16)
                .map_err(|_| ThemeError::InvalidColor(hex.to_string()))?;
            (r, g, b, a)
        }
        _ => return Err(ThemeError::InvalidColor(hex.to_string())),
    };

    Ok(color_f(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ))
}

impl SyntaxColors {
    /// 创建默认暗色语法颜色（与 Theme::dark() 中的默认值一致）
    fn dark_default() -> Self {
        Self {
            keyword: color_f(0.8, 0.4, 0.6, 1.0),
            string: color_f(0.7, 0.8, 0.4, 1.0),
            number: color_f(0.6, 0.8, 0.9, 1.0),
            comment: color_f(0.4, 0.5, 0.4, 1.0),
            function: color_f(0.4, 0.7, 0.9, 1.0),
            type_name: color_f(0.3, 0.8, 0.7, 1.0),
            operator: color_f(0.8, 0.8, 0.8, 1.0),
            variable: color_f(0.8, 0.8, 0.8, 1.0),
            preprocessor: color_f(0.7, 0.5, 0.3, 1.0),
            attribute: color_f(0.8, 0.6, 0.3, 1.0),
            macro_color: color_f(0.6, 0.4, 0.8, 1.0),
            lifetime: color_f(0.5, 0.7, 0.9, 1.0),
            regex: color_f(0.8, 0.5, 0.3, 1.0),
            format_string: color_f(0.8, 0.6, 0.4, 1.0),
            md_heading: color_f(0.3, 0.6, 0.9, 1.0),
            md_link: color_f(0.3, 0.5, 0.9, 1.0),
            md_code: color_f(0.7, 0.5, 0.3, 1.0),
            md_emphasis: color_f(0.9, 0.7, 0.4, 1.0),
            json_key: color_f(0.6, 0.8, 0.9, 1.0),
            toml_table: color_f(0.8, 0.5, 0.3, 1.0),
            find_highlight: color_f(0.8, 0.7, 0.3, 0.6),
            // Semantic token colors - defaults
            semantic_namespace: color_f(0.5, 0.7, 0.9, 1.0),
            semantic_type: color_f(0.3, 0.7, 0.9, 1.0),
            semantic_class: color_f(0.3, 0.6, 0.9, 1.0),
            semantic_enum: color_f(0.3, 0.6, 0.9, 1.0),
            semantic_interface: color_f(0.3, 0.7, 0.8, 1.0),
            semantic_struct: color_f(0.3, 0.6, 0.9, 1.0),
            semantic_type_parameter: color_f(0.4, 0.7, 0.8, 1.0),
            semantic_parameter: color_f(0.7, 0.7, 0.7, 1.0),
            semantic_variable_local: color_f(0.8, 0.8, 0.8, 1.0),
            semantic_variable_global: color_f(0.7, 0.7, 0.8, 1.0),
            semantic_property: color_f(0.7, 0.7, 0.8, 1.0),
            semantic_enum_member: color_f(0.5, 0.7, 0.9, 1.0),
            semantic_event: color_f(0.7, 0.5, 0.7, 1.0),
            semantic_function_declaration: color_f(0.8, 0.6, 0.3, 1.0),
            semantic_function_call: color_f(0.8, 0.6, 0.3, 1.0),
            semantic_method: color_f(0.8, 0.6, 0.3, 1.0),
            semantic_macro: color_f(0.6, 0.4, 0.8, 1.0),
            semantic_keyword_control: color_f(0.5, 0.5, 0.8, 1.0),
            semantic_modifier: color_f(0.5, 0.5, 0.8, 1.0),
            semantic_comment_doc: color_f(0.4, 0.6, 0.4, 1.0),
            semantic_string_format: color_f(0.8, 0.6, 0.4, 1.0),
            semantic_number_hex: color_f(0.6, 0.8, 0.6, 1.0),
            semantic_regexp: color_f(0.8, 0.5, 0.3, 1.0),
            semantic_operator_logical: color_f(0.5, 0.5, 0.8, 1.0),
            semantic_readonly: color_f(0.5, 0.7, 0.9, 1.0),
            semantic_deprecated: color_f(0.5, 0.5, 0.5, 0.7),
            semantic_async: color_f(0.5, 0.5, 0.8, 1.0),
            semantic_static: color_f(0.7, 0.7, 0.7, 1.0),
            semantic_abstract: color_f(0.5, 0.7, 0.8, 1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_color() {
        let c = parse_hex_color("#FF5733").unwrap();
        assert!((c.r - 1.0).abs() < 0.01);
        assert!((c.g - 0.34).abs() < 0.01);
        assert!((c.b - 0.2).abs() < 0.01);
        assert!((c.a - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_hex_color_rgb() {
        let c = parse_hex_color("#F53").unwrap();
        assert!((c.r - 1.0).abs() < 0.01);
        assert!((c.g - 0.33).abs() < 0.01);
        assert!((c.b - 0.2).abs() < 0.01);
    }

    #[test]
    fn test_parse_hex_color_rgba() {
        let c = parse_hex_color("#FF573380").unwrap();
        assert!((c.a - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_vscode_theme_parse() {
        use std::collections::HashMap;
        let mut colors = HashMap::new();
        colors.insert("editor.background".to_string(), "#1e1e1e".to_string());
        colors.insert("editor.foreground".to_string(), "#d4d4d4".to_string());

        let vscode = VsCodeThemeJson {
            name: "Test Theme".to_string(),
            theme_type: "dark".to_string(),
            colors,
            token_colors: vec![
                TokenColorRule {
                    name: "Keyword".to_string(),
                    scope: TokenScope::Single("keyword".to_string()),
                    settings: TokenSettings {
                        foreground: Some("#569cd6".to_string()),
                        background: None,
                        font_style: None,
                    },
                },
                TokenColorRule {
                    name: "String".to_string(),
                    scope: TokenScope::Multiple(vec![
                        "string".to_string(),
                        "string.quoted".to_string(),
                    ]),
                    settings: TokenSettings {
                        foreground: Some("#ce9178".to_string()),
                        background: None,
                        font_style: None,
                    },
                },
            ],
            semantic_highlighting: false,
            semantic_token_colors: HashMap::new(),
        };

        let theme = Theme::from_vscode(&vscode);
        assert!((theme.editor_bg.r - 0.118).abs() < 0.01);
        assert!((theme.syntax.keyword.r - 0.337).abs() < 0.01);
        assert!((theme.syntax.string.r - 0.808).abs() < 0.01);
    }
}
