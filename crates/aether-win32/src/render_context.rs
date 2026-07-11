use aether_render::d2d::brush_cache::{BrushCache, TextFormatCache, TextLayoutCache};
use aether_render::d2d::factory::{D2DFactory, RenderTarget};
use windows::core::Result;
use windows::Win32::Foundation::HWND;

/// 渲染上下文 — 封装所有与 Direct2D 渲染相关的资源
///
/// 将渲染目标、画刷缓存、文本格式缓存从 EditorState 中分离出来，
/// 消除渲染函数调用时的借用冲突（&mut self vs &render_target）。
pub struct RenderContext {
    /// Direct2D 渲染目标
    pub target: Option<RenderTarget>,
    /// D2D 画刷缓存
    pub brush_cache: BrushCache,
    /// DirectWrite 文本格式缓存
    pub text_format_cache: TextFormatCache,
    /// DirectWrite TextLayout 缓存（避免每帧重复创建 COM 对象）
    pub text_layout_cache: TextLayoutCache,
}

impl RenderContext {
    pub fn new() -> Self {
        let text_format_cache = TextFormatCache::new()
            .expect("无法初始化 DirectWrite 文本格式缓存，请确认 DirectX 已安装");
        Self {
            target: None,
            brush_cache: BrushCache::new(),
            text_layout_cache: TextLayoutCache::new(text_format_cache.dwrite_factory()),
            text_format_cache,
        }
    }

    /// 初始化 HWND 渲染目标
    pub fn init_render_target(
        &mut self,
        d2d_factory: &D2DFactory,
        hwnd: HWND,
        phys_width: u32,
        phys_height: u32,
        dpi_scale: f32,
    ) -> Result<()> {
        let dpi = dpi_scale * 96.0;
        let target = RenderTarget::new(d2d_factory, hwnd, phys_width, phys_height, dpi)?;
        self.target = Some(target);
        Ok(())
    }

    /// 调整渲染目标尺寸
    pub fn resize(&mut self, phys_width: u32, phys_height: u32) {
        if let Some(rt) = &mut self.target {
            let _ = rt.resize(phys_width, phys_height);
        }
    }

    /// 获取渲染目标引用（用于渲染函数）
    pub fn target_ref(&self) -> Option<&RenderTarget> {
        self.target.as_ref()
    }

    /// 获取 D2D1HwndRenderTarget 克隆（传递给渲染函数）
    pub fn d2d_target(&self) -> Option<windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget> {
        self.target.as_ref().map(|rt| rt.target().clone())
    }

    /// 开始绘制
    pub fn begin_draw(&self) {
        if let Some(rt) = &self.target {
            rt.begin_draw();
        }
    }

    /// 结束绘制
    pub fn end_draw(&self) -> Result<()> {
        if let Some(rt) = &self.target {
            rt.end_draw()
        } else {
            Ok(())
        }
    }

    /// 清除画布
    pub fn clear(&self, color: &windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F) {
        if let Some(rt) = &self.target {
            rt.clear(color);
        }
    }

    /// 设置裁剪区域
    pub fn push_clip(&self, x: f32, y: f32, width: f32, height: f32) {
        if let Some(rt) = &self.target {
            rt.push_clip(x, y, width, height);
        }
    }

    /// 弹出裁剪区域
    pub fn pop_clip(&self) {
        if let Some(rt) = &self.target {
            rt.pop_clip();
        }
    }

    /// REQ-P3-03: 设置多个独立矩形的并集裁剪区域
    ///
    /// 返回 true 表示使用了 PushLayer（多矩形路径），返回 false 表示走了
    /// PushAxisAlignedClip 单矩形快路径或空路径。调用方需在 pop_multi_clip
    /// 时传入相同的标志以选择 PopLayer 或 PopAxisAlignedClip。
    pub fn push_multi_clip(
        &self,
        factory: &windows::Win32::Graphics::Direct2D::ID2D1Factory1,
        rects: &[(f32, f32, f32, f32)],
    ) -> bool {
        if let Some(rt) = &self.target {
            // 单矩形走快路径
            if rects.len() == 1 {
                let (x, y, w, h) = rects[0];
                rt.push_clip(x, y, w, h);
                return false;
            }
            if rects.is_empty() {
                return false;
            }
            match rt.push_multi_clip(factory, rects) {
                Ok(()) => true,
                Err(_) => {
                    // 失败时回退到合并包围盒的 AxisAlignedClip
                    let mut min_x = f32::MAX;
                    let mut min_y = f32::MAX;
                    let mut max_x = f32::MIN;
                    let mut max_y = f32::MIN;
                    for &(x, y, w, h) in rects {
                        if w <= 0.0 || h <= 0.0 {
                            continue;
                        }
                        min_x = min_x.min(x);
                        min_y = min_y.min(y);
                        max_x = max_x.max(x + w);
                        max_y = max_y.max(y + h);
                    }
                    if max_x > min_x && max_y > min_y {
                        rt.push_clip(min_x, min_y, max_x - min_x, max_y - min_y);
                    }
                    false
                }
            }
        } else {
            false
        }
    }

    /// REQ-P3-03: 弹出多矩形裁剪区域
    pub fn pop_multi_clip(&self, use_layer: bool) {
        if let Some(rt) = &self.target {
            rt.pop_multi_clip(use_layer);
        }
    }

    /// 填充指定矩形区域（用于局部背景清除）
    pub fn fill_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: &windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
    ) {
        use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
        if let Some(rt) = &self.target {
            let rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            unsafe {
                if let Ok(brush) = self.brush_cache.get_brush(rt.target(), color) {
                    rt.target().FillRectangle(&rect, &brush);
                }
            }
        }
    }

    /// 设置 DPI
    pub fn set_dpi(&mut self, dpi: f32) {
        if let Some(rt) = &mut self.target {
            rt.set_dpi(dpi);
        }
    }

    /// 预初始化常用画刷和文本格式（渲染目标就绪后调用）
    pub fn init_common_resources(&mut self, theme: &aether_render::theme::Theme, font_size: f32) {
        if let Some(rt) = &self.target {
            let target = rt.target().clone();
            let common_colors = [
                theme.editor_bg,
                theme.line_number_bg,
                theme.line_number_fg,
                theme.line_highlight_bg,
                theme.selection_bg,
                theme.cursor_color,
                theme.sidebar_bg,
                theme.statusbar_bg,
                theme.text_default,
                theme.tab_active_bg,
                theme.tab_inactive_bg,
                theme.titlebar_bg,
                theme.activity_bar_bg,
                theme.panel_border,
                theme.shadow,
                theme.glow_selection,
                theme.command_palette_bg,
                theme.submenu_bg,
            ];
            self.brush_cache
                .init_common_brushes(&target, &common_colors);
            self.text_format_cache.init_common_formats(font_size);
        }
    }

    /// 处理设备丢失 — 清除所有资源并重建
    pub fn handle_device_lost(&mut self) {
        self.target = None;
        self.brush_cache.clear();
        self.text_format_cache.clear();
    }
}
