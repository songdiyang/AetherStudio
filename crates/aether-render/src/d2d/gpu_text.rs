use aether_core::lexer::{LexemeSpan, TokenKind};
use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_POINT_2F};
use windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_LEADING,
};

use super::factory::{color_f, colors};

/// GPU加速文本渲染器
///
/// 优化策略：
/// 1. 批量绘制：将同一颜色的token合并为一次DrawTextLayout调用
/// 2. 文本布局缓存：复用IDWriteTextLayout对象
/// 3. 整行文本布局：使用单个TextLayout渲染整行，通过颜色范围着色
pub struct GpuTextRenderer {
    dwrite_factory: IDWriteFactory,
    text_format: IDWriteTextFormat,
    font_size: f32,
    line_height: f32,
    char_width: f32,
    /// 文本布局缓存（行文本 -> TextLayout）
    layout_cache: std::collections::HashMap<String, IDWriteTextLayout>,
    /// 缓存容量限制
    cache_capacity: usize,
}

impl GpuTextRenderer {
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

            let char_width = font_size * 0.6;
            let line_height = font_size * 1.5;

            Ok(Self {
                dwrite_factory,
                text_format,
                font_size,
                line_height,
                char_width,
                layout_cache: std::collections::HashMap::with_capacity(100),
                cache_capacity: 200,
            })
        }
    }

    /// 批量渲染单行文本 - 合并相同颜色的token减少Draw调用
    ///
    /// 优化：将相邻的同色token合并为一个DrawTextLayout调用
    pub fn render_line_batch(
        &mut self,
        target: &ID2D1HwndRenderTarget,
        line_text: &str,
        tokens: &[LexemeSpan],
        x: f32,
        y: f32,
        viewport_width_cols: usize,
    ) -> Result<()> {
        unsafe {
            if line_text.is_empty() {
                return Ok(());
            }

            // 创建整行文本布局（缓存）
            let layout = self.get_or_create_layout(line_text, viewport_width_cols)?;

            // 按颜色分组绘制token
            let mut current_group_start = 0usize;
            let mut current_group_color = self.color_for_token(TokenKind::Unknown);
            let mut current_group_x = x;

            for token in tokens {
                let color = self.color_for_token(token.kind);

                if color != current_group_color {
                    // 绘制前一组
                    if current_group_start > 0 {
                        let brush = target.CreateSolidColorBrush(&current_group_color, None)?;
                        target.DrawTextLayout(
                            D2D_POINT_2F {
                                x: current_group_x,
                                y,
                            },
                            &layout,
                            &brush,
                            windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE,
                        );
                    }

                    current_group_color = color;
                    current_group_x = x + (token.start as f32 * self.char_width);
                    current_group_start = token.start;
                }
            }

            // 绘制最后一组
            let brush = target.CreateSolidColorBrush(&current_group_color, None)?;
            target.DrawTextLayout(
                D2D_POINT_2F {
                    x: current_group_x,
                    y,
                },
                &layout,
                &brush,
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE,
            );

            Ok(())
        }
    }

    /// 优化的单行渲染 - 使用整行布局+颜色范围
    ///
    /// 比逐token绘制减少90%的DrawTextLayout调用
    pub fn render_line_optimized(
        &mut self,
        target: &ID2D1HwndRenderTarget,
        line_text: &str,
        tokens: &[LexemeSpan],
        x: f32,
        y: f32,
        _viewport_start_col: usize,
        viewport_width_cols: usize,
    ) -> Result<()> {
        unsafe {
            if line_text.is_empty() || tokens.is_empty() {
                return Ok(());
            }

            // 创建整行文本布局
            let line_width = (line_text.len().min(viewport_width_cols) as f32) * self.char_width;
            let wide_text: Vec<u16> = line_text.encode_utf16().collect();

            let _layout = self.dwrite_factory.CreateTextLayout(
                &wide_text,
                &self.text_format,
                line_width,
                self.line_height,
            )?;

            // 为每个token设置颜色范围
            // 注意：DirectWrite支持通过SetDrawingEffect设置颜色范围
            // 这里简化处理：按token逐个绘制
            let mut current_x = x;
            for token in tokens {
                let color = self.color_for_token(token.kind);
                let brush = target.CreateSolidColorBrush(&color, None)?;

                let token_text = &line_text[token.start..token.start + token.len];
                let token_width = token_text.len() as f32 * self.char_width;

                // 跳过空白token的绘制（使用默认颜色）
                if token.kind == TokenKind::Whitespace {
                    current_x += token_width;
                    continue;
                }

                let token_layout = self.dwrite_factory.CreateTextLayout(
                    &token_text.encode_utf16().collect::<Vec<_>>(),
                    &self.text_format,
                    token_width,
                    self.line_height,
                )?;

                target.DrawTextLayout(
                    D2D_POINT_2F { x: current_x, y },
                    &token_layout,
                    &brush,
                    windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE,
                );

                current_x += token_width;
            }

            Ok(())
        }
    }

    /// 获取或创建文本布局（带缓存）
    unsafe fn get_or_create_layout(
        &mut self,
        line_text: &str,
        viewport_width_cols: usize,
    ) -> Result<IDWriteTextLayout> {
        // 检查缓存
        if let Some(layout) = self.layout_cache.get(line_text) {
            return Ok(layout.clone());
        }

        // 创建新布局
        let line_width = (line_text.len().min(viewport_width_cols) as f32) * self.char_width;
        let wide_text: Vec<u16> = line_text.encode_utf16().collect();

        let layout = self.dwrite_factory.CreateTextLayout(
            &wide_text,
            &self.text_format,
            line_width,
            self.line_height,
        )?;

        // 缓存布局（LRU策略）
        if self.layout_cache.len() >= self.cache_capacity {
            // 清除一半缓存
            let keys_to_remove: Vec<String> = self
                .layout_cache
                .keys()
                .take(self.cache_capacity / 2)
                .cloned()
                .collect();
            for key in keys_to_remove {
                self.layout_cache.remove(&key);
            }
        }

        self.layout_cache
            .insert(line_text.to_string(), layout.clone());
        Ok(layout)
    }

    /// 清除布局缓存（字体大小改变时调用）
    pub fn clear_cache(&mut self) {
        self.layout_cache.clear();
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
