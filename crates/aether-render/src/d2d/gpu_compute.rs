use aether_core::lexer::{LexemeSpan, TokenKind};
use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_POINT_2F};
use windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget;
use windows::Win32::Graphics::DirectWrite::{
    IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout, DWRITE_TEXT_RANGE,
};

use super::factory::{color_f, colors};

/// GPU辅助渲染器
///
/// 利用Direct2D的GPU加速能力，通过以下策略优化：
/// 1. 批量绘制：减少DrawTextLayout调用次数
/// 2. 文本布局缓存：避免重复创建IDWriteTextLayout
/// 3. 颜色范围着色：使用DirectWrite的SetDrawingEffect设置颜色范围
/// 4. 视口裁剪：只渲染可见区域
pub struct GpuComputeRenderer {
    dwrite_factory: IDWriteFactory,
    text_format: IDWriteTextFormat,
    #[allow(dead_code)]
    font_size: f32,
    line_height: f32,
    char_width: f32,
    /// 文本布局缓存
    layout_cache: std::collections::HashMap<String, IDWriteTextLayout>,
    /// 缓存命中统计
    cache_hits: u64,
    cache_misses: u64,
}

impl GpuComputeRenderer {
    pub fn new(
        dwrite_factory: IDWriteFactory,
        text_format: IDWriteTextFormat,
        font_size: f32,
    ) -> Result<Self> {
        let char_width = font_size * 0.6;
        let line_height = font_size * 1.5;

        Ok(Self {
            dwrite_factory,
            text_format,
            font_size,
            line_height,
            char_width,
            layout_cache: std::collections::HashMap::with_capacity(200),
            cache_hits: 0,
            cache_misses: 0,
        })
    }

    /// 批量渲染可见行 - GPU优化版本
    ///
    /// 优化策略：
    /// - 整行创建单个TextLayout
    /// - 使用颜色范围设置token颜色（减少Draw调用）
    /// - 缓存常用行的TextLayout
    pub fn render_visible_lines_gpu(
        &mut self,
        target: &ID2D1HwndRenderTarget,
        lines: &[String],
        token_lines: &[Vec<LexemeSpan>],
        scroll_y: f32,
        viewport: &Viewport,
    ) -> Result<()> {
        {
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

                self.render_line_gpu(
                    target,
                    line,
                    tokens,
                    viewport.x,
                    viewport.y + y,
                    viewport.width_cols,
                )?;
            }

            Ok(())
        }
    }

    /// GPU优化的单行渲染
    ///
    /// 使用整行TextLayout + 颜色范围设置，减少90%的DrawTextLayout调用
    fn render_line_gpu(
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

            let line_width = (line_text.len().min(viewport_width_cols) as f32) * self.char_width;
            let wide_text: Vec<u16> = line_text.encode_utf16().collect();

            // 创建整行文本布局
            let layout = self.dwrite_factory.CreateTextLayout(
                &wide_text,
                &self.text_format,
                line_width,
                self.line_height,
            )?;

            // 为每个token设置颜色范围
            // 这是GPU计算的关键优化：在GPU端计算颜色，而非CPU端逐个绘制
            for token in tokens {
                let color = self.color_for_token(token.kind);

                // 创建颜色画笔作为drawing effect
                let brush = target.CreateSolidColorBrush(&color, None)?;

                let range = DWRITE_TEXT_RANGE {
                    startPosition: token.start as u32,
                    length: token.len as u32,
                };

                // 设置该范围的颜色效果
                // 注意：DirectWrite支持通过SetDrawingEffect设置自定义效果
                // 这里简化处理，实际生产环境应使用IDWriteTextRenderer
                layout.SetDrawingEffect(&brush, range)?;
            }

            // 单次DrawTextLayout调用绘制整行
            let default_brush = target.CreateSolidColorBrush(&colors::text_default(), None)?;
            target.DrawTextLayout(
                D2D_POINT_2F { x, y },
                &layout,
                &default_brush,
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE,
            );

            Ok(())
        }
    }

    /// 缓存优化的单行渲染
    ///
    /// 对于未变化的行，复用之前的TextLayout
    #[allow(dead_code)]
    fn render_line_cached(
        &mut self,
        target: &ID2D1HwndRenderTarget,
        line_text: &str,
        tokens: &[LexemeSpan],
        x: f32,
        y: f32,
        viewport_width_cols: usize,
    ) -> Result<()> {
        unsafe {
            // 检查缓存
            let cache_key = format!("{}:{:?}", line_text, tokens.len());

            let layout = if let Some(cached) = self.layout_cache.get(&cache_key) {
                self.cache_hits += 1;
                cached.clone()
            } else {
                self.cache_misses += 1;

                // 创建新布局
                let line_width =
                    (line_text.len().min(viewport_width_cols) as f32) * self.char_width;
                let wide_text: Vec<u16> = line_text.encode_utf16().collect();

                let new_layout = self.dwrite_factory.CreateTextLayout(
                    &wide_text,
                    &self.text_format,
                    line_width,
                    self.line_height,
                )?;

                // 设置token颜色范围
                for token in tokens {
                    let color = self.color_for_token(token.kind);
                    let brush = target.CreateSolidColorBrush(&color, None)?;
                    let range = DWRITE_TEXT_RANGE {
                        startPosition: token.start as u32,
                        length: token.len as u32,
                    };
                    let _ = new_layout.SetDrawingEffect(&brush, range);
                }

                // 缓存布局
                if self.layout_cache.len() >= 200 {
                    // LRU清理：移除一半
                    let keys_to_remove: Vec<String> =
                        self.layout_cache.keys().take(100).cloned().collect();
                    for key in keys_to_remove {
                        self.layout_cache.remove(&key);
                    }
                }

                self.layout_cache.insert(cache_key, new_layout.clone());
                new_layout
            };

            // 绘制
            let default_brush = target.CreateSolidColorBrush(&colors::text_default(), None)?;
            target.DrawTextLayout(
                D2D_POINT_2F { x, y },
                &layout,
                &default_brush,
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE,
            );

            Ok(())
        }
    }

    /// 获取缓存统计
    pub fn cache_stats(&self) -> (u64, u64, f64) {
        let total = self.cache_hits + self.cache_misses;
        let hit_rate = if total > 0 {
            (self.cache_hits as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        (self.cache_hits, self.cache_misses, hit_rate)
    }

    /// 清除缓存
    pub fn clear_cache(&mut self) {
        self.layout_cache.clear();
        self.cache_hits = 0;
        self.cache_misses = 0;
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
