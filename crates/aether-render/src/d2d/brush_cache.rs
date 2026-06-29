use std::collections::HashMap;
use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget;
use windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING,
};

/// 预存画笔槽位数量（覆盖最常用的主题颜色）
const PRECOMPUTED_BRUSH_SLOTS: usize = 16;

/// P4-5: 回退 HashMap 最大条目数，超出时清空重建避免无界增长
const MAX_BRUSH_CACHE_ENTRIES: usize = 64;

/// P4-5: TextFormatCache 回退 HashMap 最大条目数
const MAX_TEXT_FORMAT_CACHE_ENTRIES: usize = 64;

/// 画刷缓存 - 避免每帧创建 COM 对象
///
/// 优化策略：
/// - 预存常用颜色画笔（小数组线性扫描，命中率高时比 HashMap 快）
/// - 未命中的颜色回退到 HashMap
pub struct BrushCache {
    /// 预存常用画笔（key + brush 对，线性扫描）
    precomputed: Vec<(u32, ID2D1SolidColorBrush)>,
    /// 回退 HashMap（不常用颜色）
    brushes: HashMap<u32, ID2D1SolidColorBrush>,
}

impl BrushCache {
    pub fn new() -> Self {
        Self {
            precomputed: Vec::with_capacity(PRECOMPUTED_BRUSH_SLOTS),
            brushes: HashMap::new(),
        }
    }

    /// 预初始化常用颜色画笔（在渲染目标就绪后调用一次）
    ///
    /// 建议传入主题中最常用的 ~10 种颜色，使渲染帧内直接命中预存数组
    pub fn init_common_brushes(&mut self, target: &ID2D1HwndRenderTarget, colors: &[D2D1_COLOR_F]) {
        self.precomputed.clear();
        for color in colors.iter().take(PRECOMPUTED_BRUSH_SLOTS) {
            let key = color_key(color);
            // 跳过已存在的 key
            if self.precomputed.iter().any(|(k, _)| *k == key) {
                continue;
            }
            if let Ok(brush) = unsafe { target.CreateSolidColorBrush(color, None) } {
                self.precomputed.push((key, brush));
            }
        }
    }

    /// 获取或创建指定颜色的画刷
    ///
    /// 查找顺序：预存数组 → HashMap → 新建
    pub fn get_brush(
        &mut self,
        target: &ID2D1HwndRenderTarget,
        color: &D2D1_COLOR_F,
    ) -> Result<ID2D1SolidColorBrush> {
        let key = color_key(color);

        // 1. 先查预存数组（通常 < 16 项，线性扫描比 HashMap 快）
        for (k, brush) in &self.precomputed {
            if *k == key {
                return Ok(brush.clone());
            }
        }

        // 2. 查 HashMap
        if let Some(brush) = self.brushes.get(&key) {
            return Ok(brush.clone());
        }

        // 3. 新建并缓存到 HashMap
        // P4-5: 超过最大条目数时清空回退缓存（简单 LRU 替代方案）
        if self.brushes.len() >= MAX_BRUSH_CACHE_ENTRIES {
            self.brushes.clear();
        }
        let brush = unsafe { target.CreateSolidColorBrush(color, None)? };
        let result = brush.clone();
        self.brushes.insert(key, brush);
        Ok(result)
    }

    /// 清空缓存（设备丢失时调用）
    pub fn clear(&mut self) {
        self.precomputed.clear();
        self.brushes.clear();
    }
}

/// 文本格式缓存 - 避免每帧创建 DirectWrite 格式对象
///
/// 优化策略：
/// - 预存最常用的 3 种格式（code / line_number / center）
/// - 其他格式回退到 HashMap
pub struct TextFormatCache {
    dwrite_factory: IDWriteFactory,
    /// 预存常用格式（key + format 对，线性扫描）
    precomputed: Vec<(TextFormatKey, IDWriteTextFormat)>,
    /// 回退 HashMap（不常用格式）
    formats: HashMap<TextFormatKey, IDWriteTextFormat>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TextFormatKey {
    font_size: u32, // 缩放为整数避免浮点精度问题
    font_weight: u32,
    alignment: u8,
    paragraph_alignment: u8,
}

impl TextFormatCache {
    pub fn new() -> Result<Self> {
        unsafe {
            let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            Ok(Self {
                dwrite_factory,
                precomputed: Vec::with_capacity(8),
                formats: HashMap::new(),
            })
        }
    }

    /// 预初始化常用文本格式（在字体大小确定后调用一次）
    ///
    /// 预创建 code（左对齐）、line_number（右对齐）、center（居中）三种常用格式
    pub fn init_common_formats(&mut self, font_size: f32) {
        self.precomputed.clear();

        // Code 格式：左对齐 + 顶部
        if let Ok(fmt) = self.create_format_internal(
            font_size,
            DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
            DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
            DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
        ) {
            let key = TextFormatKey {
                font_size: (font_size * 10.0) as u32,
                font_weight: DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                alignment: DWRITE_TEXT_ALIGNMENT_LEADING.0 as u8,
                paragraph_alignment: DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u8,
            };
            self.precomputed.push((key, fmt));
        }

        // 行号格式：右对齐 + 顶部
        if let Ok(fmt) = self.create_format_internal(
            font_size,
            DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
            DWRITE_TEXT_ALIGNMENT_TRAILING.0 as u32,
            DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
        ) {
            let key = TextFormatKey {
                font_size: (font_size * 10.0) as u32,
                font_weight: DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                alignment: DWRITE_TEXT_ALIGNMENT_TRAILING.0 as u8,
                paragraph_alignment: DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u8,
            };
            self.precomputed.push((key, fmt));
        }

        // 居中格式
        if let Ok(fmt) = self.create_format_internal(
            font_size,
            DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
            DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
        ) {
            let key = TextFormatKey {
                font_size: (font_size * 10.0) as u32,
                font_weight: DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                alignment: DWRITE_TEXT_ALIGNMENT_CENTER.0 as u8,
                paragraph_alignment: DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u8,
            };
            self.precomputed.push((key, fmt));
        }
    }

    /// 内部格式创建（不缓存）
    fn create_format_internal(
        &self,
        font_size: f32,
        font_weight: u32,
        text_alignment: u32,
        paragraph_alignment: u32,
    ) -> Result<IDWriteTextFormat> {
        unsafe {
            let format = self.dwrite_factory.CreateTextFormat(
                windows::core::w!("Consolas"),
                None,
                windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT(font_weight as i32),
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font_size,
                windows::core::w!("zh-CN"),
            )?;
            let _ = format.SetTextAlignment(std::mem::transmute::<
                u32,
                windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT,
            >(text_alignment));
            let _ = format.SetParagraphAlignment(std::mem::transmute::<
                u32,
                windows::Win32::Graphics::DirectWrite::DWRITE_PARAGRAPH_ALIGNMENT,
            >(paragraph_alignment));
            Ok(format)
        }
    }

    /// 获取或创建文本格式
    ///
    /// 查找顺序：预存数组 → HashMap → 新建
    pub fn get_format(
        &mut self,
        font_size: f32,
        font_weight: u32,
        text_alignment: u32,
        paragraph_alignment: u32,
    ) -> Result<IDWriteTextFormat> {
        let key = TextFormatKey {
            font_size: (font_size * 10.0) as u32,
            font_weight,
            alignment: text_alignment as u8,
            paragraph_alignment: paragraph_alignment as u8,
        };

        // 1. 先查预存数组（通常 ≤ 3 项，线性扫描极快）
        for (k, format) in &self.precomputed {
            if *k == key {
                return Ok(format.clone());
            }
        }

        // 2. 查 HashMap
        if let Some(format) = self.formats.get(&key) {
            return Ok(format.clone());
        }

        // 3. 新建并缓存到 HashMap
        // P4-5: 超过最大条目数时清空回退缓存（简单 LRU 替代方案）
        if self.formats.len() >= MAX_TEXT_FORMAT_CACHE_ENTRIES {
            self.formats.clear();
        }
        let format = self.create_format_internal(
            font_size,
            font_weight,
            text_alignment,
            paragraph_alignment,
        )?;
        let result = format.clone();
        self.formats.insert(key, format);
        Ok(result)
    }

    /// 获取代码文本格式（左对齐，顶部）
    pub fn get_code_format(&mut self, font_size: f32) -> Result<IDWriteTextFormat> {
        self.get_format(
            font_size,
            DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
            DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
            DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
        )
    }

    /// 获取行号格式（右对齐，顶部）
    pub fn get_line_number_format(&mut self, font_size: f32) -> Result<IDWriteTextFormat> {
        self.get_format(
            font_size,
            DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
            DWRITE_TEXT_ALIGNMENT_TRAILING.0 as u32,
            DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
        )
    }

    /// 获取居中格式
    pub fn get_center_format(
        &mut self,
        font_size: f32,
        font_weight: u32,
    ) -> Result<IDWriteTextFormat> {
        self.get_format(
            font_size,
            font_weight,
            DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
        )
    }

    /// 清空缓存
    pub fn clear(&mut self) {
        self.precomputed.clear();
        self.formats.clear();
    }
}

/// 将颜色转换为缓存键
/// 使用 round() 避免浮点精度问题（如 0.47 * 255 = 119.85 截断为 119，round 为 120）
fn color_key(color: &D2D1_COLOR_F) -> u32 {
    let r = (color.r * 255.0).round() as u32;
    let g = (color.g * 255.0).round() as u32;
    let b = (color.b * 255.0).round() as u32;
    let a = (color.a * 255.0).round() as u32;
    (r << 24) | (g << 16) | (b << 8) | a
}
