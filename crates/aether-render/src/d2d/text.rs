use aether_core::lexer::{LexemeSpan, TokenKind};
use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_POINT_2F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_LEADING,
};

use super::factory::{color_f, colors};

/// 文本渲染器
pub struct TextRenderer {
    dwrite_factory: IDWriteFactory,
    text_format: IDWriteTextFormat,
    font_size: f32,
    line_height: f32,
    char_width: f32,
}

impl TextRenderer {
    pub fn new() -> Result<Self> {
        unsafe {
            let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            let font_size = 14.0;
            let text_format = dwrite_factory.CreateTextFormat(
                windows::core::w!("Consolas"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font_size,
                windows::core::w!("zh-CN"),
            )?;

            text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
            text_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;

            // 估算字符宽度和行高
            let char_width = font_size * 0.6;
            let line_height = font_size * 1.5;

            Ok(Self {
                dwrite_factory,
                text_format,
                font_size,
                line_height,
                char_width,
            })
        }
    }

    /// 渲染单行文本
    pub fn render_line(
        &self,
        target: &ID2D1HwndRenderTarget,
        line_text: &str,
        tokens: &[LexemeSpan],
        x: f32,
        y: f32,
        _viewport_start_col: usize,
        viewport_width_cols: usize,
    ) -> Result<()> {
        unsafe {
            // 绘制行背景（当前行高亮等）
            let _bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + (viewport_width_cols as f32 * self.char_width),
                bottom: y + self.line_height,
            };

            // 根据token类型绘制带颜色的文本
            let mut current_x = x;
            for token in tokens {
                let color = self.color_for_token(token.kind);
                let brush = target.CreateSolidColorBrush(&color, None)?;

                let token_text = &line_text[token.start..token.start + token.len];
                let token_width = token_text.len() as f32 * self.char_width;

                let layout = self.dwrite_factory.CreateTextLayout(
                    &token_text.encode_utf16().collect::<Vec<_>>(),
                    &self.text_format,
                    token_width,
                    self.line_height,
                )?;

                target.DrawTextLayout(
                    D2D_POINT_2F { x: current_x, y },
                    &layout,
                    &brush,
                    windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE,
                );

                current_x += token_width;
            }

            Ok(())
        }
    }

    /// 渲染可见区域的所有文本行
    pub fn render_visible_lines(
        &self,
        target: &ID2D1HwndRenderTarget,
        lines: &[String],
        token_lines: &[Vec<LexemeSpan>],
        scroll_y: f32,
        viewport: &Viewport,
    ) -> Result<()> {
        let start_line = (scroll_y / self.line_height) as usize;
        let end_line = ((viewport.height + scroll_y) / self.line_height) as usize + 1;
        let end_line = end_line.min(lines.len());

        for (i, line) in lines[start_line..end_line].iter().enumerate() {
            let line_idx = start_line + i;
            let y = i as f32 * self.line_height - (scroll_y % self.line_height);
            let tokens = token_lines
                .get(line_idx)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            self.render_line(
                target,
                line,
                tokens,
                viewport.x,
                viewport.y + y,
                0,
                viewport.width_cols,
            )?;
        }

        Ok(())
    }

    /// 获取token对应的颜色
    fn color_for_token(&self, kind: TokenKind) -> D2D1_COLOR_F {
        match kind {
            TokenKind::Keyword => colors::keyword(),
            TokenKind::Identifier => colors::variable(),
            TokenKind::StringLiteral | TokenKind::CharLiteral => colors::string(),
            TokenKind::NumberLiteral => colors::number(),
            TokenKind::LineComment | TokenKind::BlockComment | TokenKind::DocComment => {
                colors::comment()
            }
            TokenKind::Operator | TokenKind::Punctuation => colors::operator(),
            TokenKind::Preprocessor => colors::preprocessor(),
            TokenKind::Attribute => color_f(0.8, 0.6, 0.3, 1.0),
            TokenKind::TypeName => colors::type_name(),
            TokenKind::Function => colors::function(),
            TokenKind::Macro => color_f(0.6, 0.4, 0.8, 1.0),
            TokenKind::Lifetime => color_f(0.5, 0.7, 0.9, 1.0),
            TokenKind::Generic => colors::type_name(),
            TokenKind::RegexLiteral => color_f(0.8, 0.5, 0.3, 1.0),
            TokenKind::FormatString => color_f(0.8, 0.6, 0.4, 1.0),
            TokenKind::MdHeading => color_f(0.3, 0.6, 0.9, 1.0),
            TokenKind::MdLink => color_f(0.3, 0.5, 0.9, 1.0),
            TokenKind::MdCode => color_f(0.7, 0.5, 0.3, 1.0),
            TokenKind::MdEmphasis => color_f(0.9, 0.7, 0.4, 1.0),
            TokenKind::JsonKey => color_f(0.6, 0.8, 0.9, 1.0),
            TokenKind::TomlTable => color_f(0.8, 0.5, 0.3, 1.0),
            TokenKind::Whitespace | TokenKind::Newline | TokenKind::Unknown | TokenKind::EOF => {
                colors::text_default()
            }
        }
    }

    pub fn line_height(&self) -> f32 {
        self.line_height
    }

    pub fn char_width(&self) -> f32 {
        self.char_width
    }

    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    pub fn dwrite_factory(&self) -> &IDWriteFactory {
        &self.dwrite_factory
    }
}

/// 视口定义
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub width_cols: usize,
}

impl Viewport {
    pub fn new(x: f32, y: f32, width: f32, height: f32, char_width: f32) -> Self {
        let width_cols = (width / char_width) as usize;
        Self {
            x,
            y,
            width,
            height,
            width_cols,
        }
    }
}
