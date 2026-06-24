use windows::core::Result;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory1, ID2D1HwndRenderTarget, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_FACTORY_OPTIONS, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_HARDWARE,
};

/// Direct2D工厂管理器
pub struct D2DFactory {
    factory: ID2D1Factory1,
}

impl D2DFactory {
    pub fn new() -> Result<Self> {
        unsafe {
            let options = D2D1_FACTORY_OPTIONS::default();
            let factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&options))?;
            Ok(Self { factory })
        }
    }

    pub fn factory(&self) -> &ID2D1Factory1 {
        &self.factory
    }

    /// 创建HWND渲染目标
    /// pixel_width/pixel_height 为物理像素，dpi 为实际显示器 DPI
    pub fn create_hwnd_render_target(
        &self,
        hwnd: HWND,
        pixel_width: u32,
        pixel_height: u32,
        dpi: f32,
    ) -> Result<ID2D1HwndRenderTarget> {
        unsafe {
            let render_target_props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_HARDWARE,
                dpiX: dpi,
                dpiY: dpi,
                ..Default::default()
            };

            let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd,
                pixelSize: windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U {
                    width: pixel_width,
                    height: pixel_height,
                },
                presentOptions: D2D1_PRESENT_OPTIONS(0),
            };

            self.factory
                .CreateHwndRenderTarget(&render_target_props, &hwnd_props)
        }
    }
}

/// 渲染目标管理
pub struct RenderTarget {
    target: ID2D1HwndRenderTarget,
    width: u32,
    height: u32,
    dpi: f32,
}

impl RenderTarget {
    pub fn new(
        factory: &D2DFactory,
        hwnd: HWND,
        pixel_width: u32,
        pixel_height: u32,
        dpi: f32,
    ) -> Result<Self> {
        let target = factory.create_hwnd_render_target(hwnd, pixel_width, pixel_height, dpi)?;
        Ok(Self {
            target,
            width: pixel_width,
            height: pixel_height,
            dpi,
        })
    }

    pub fn begin_draw(&self) {
        unsafe {
            self.target.BeginDraw();
        }
    }

    pub fn end_draw(&self) -> Result<()> {
        unsafe { self.target.EndDraw(None, None) }
    }

    pub fn clear(&self, color: &D2D1_COLOR_F) {
        unsafe {
            self.target.Clear(Some(color));
        }
    }

    pub fn resize(&mut self, pixel_width: u32, pixel_height: u32) -> Result<()> {
        unsafe {
            let size = windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U {
                width: pixel_width,
                height: pixel_height,
            };
            self.target.Resize(&size)?;
            self.width = pixel_width;
            self.height = pixel_height;
            Ok(())
        }
    }

    /// 更新渲染目标 DPI（显示器切换时调用）
    pub fn set_dpi(&mut self, dpi: f32) {
        unsafe {
            self.target.SetDpi(dpi, dpi);
        }
        self.dpi = dpi;
    }

    pub fn target(&self) -> &ID2D1HwndRenderTarget {
        &self.target
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn dpi(&self) -> f32 {
        self.dpi
    }

    /// 设置轴对齐裁剪区域（用于脏矩形局部重绘）
    pub fn push_clip(&self, x: f32, y: f32, width: f32, height: f32) {
        unsafe {
            let rect = windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            let _ = self
                .target
                .PushAxisAlignedClip(&rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
    }

    /// 弹出裁剪区域
    pub fn pop_clip(&self) {
        unsafe {
            self.target.PopAxisAlignedClip();
        }
    }

    /// 检查点是否在裁剪区域内（用于快速剔除）
    pub fn is_point_in_clip(&self, _x: f32, _y: f32) -> bool {
        // Direct2D 自动处理裁剪，这里总是返回 true
        // 实际裁剪由 GPU 在渲染时执行
        true
    }
}

/// 颜色工具函数
pub fn color_f(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

/// 深色主题默认颜色
pub mod colors {
    use super::color_f;
    use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

    pub fn editor_bg() -> D2D1_COLOR_F {
        color_f(0.118, 0.118, 0.118, 1.0)
    } // #1E1E1E
    pub fn line_highlight() -> D2D1_COLOR_F {
        color_f(0.15, 0.15, 0.15, 1.0)
    } // #262626
    pub fn line_number_fg() -> D2D1_COLOR_F {
        color_f(0.52, 0.52, 0.52, 1.0)
    } // #858585
    pub fn line_number_bg() -> D2D1_COLOR_F {
        color_f(0.118, 0.118, 0.118, 1.0)
    } // #1E1E1E
    pub fn selection_bg() -> D2D1_COLOR_F {
        color_f(0.18, 0.36, 0.55, 1.0)
    } // #2E638C
    pub fn cursor() -> D2D1_COLOR_F {
        color_f(0.8, 0.8, 0.8, 1.0)
    } // #CCCCCC
    pub fn text_default() -> D2D1_COLOR_F {
        color_f(0.83, 0.83, 0.83, 1.0)
    } // #D4D4D4
    pub fn sidebar_bg() -> D2D1_COLOR_F {
        color_f(0.145, 0.145, 0.149, 1.0)
    } // #252526
    pub fn statusbar_bg() -> D2D1_COLOR_F {
        color_f(0.0, 0.47, 0.83, 1.0)
    } // #0078D4
    pub fn tab_active() -> D2D1_COLOR_F {
        color_f(0.118, 0.118, 0.118, 1.0)
    } // #1E1E1E
    pub fn tab_inactive() -> D2D1_COLOR_F {
        color_f(0.145, 0.145, 0.149, 1.0)
    } // #252526

    // 语法高亮颜色
    pub fn keyword() -> D2D1_COLOR_F {
        color_f(0.77, 0.52, 0.75, 1.0)
    } // #C586C0
    pub fn string() -> D2D1_COLOR_F {
        color_f(0.81, 0.57, 0.47, 1.0)
    } // #CE9178
    pub fn number() -> D2D1_COLOR_F {
        color_f(0.71, 0.81, 0.66, 1.0)
    } // #B5CEA8
    pub fn comment() -> D2D1_COLOR_F {
        color_f(0.42, 0.60, 0.33, 1.0)
    } // #6A9955
    pub fn function() -> D2D1_COLOR_F {
        color_f(0.86, 0.86, 0.67, 1.0)
    } // #DCDCAA
    pub fn type_name() -> D2D1_COLOR_F {
        color_f(0.31, 0.79, 0.69, 1.0)
    } // #4EC9B0
    pub fn operator() -> D2D1_COLOR_F {
        color_f(0.83, 0.83, 0.83, 1.0)
    } // #D4D4D4
    pub fn variable() -> D2D1_COLOR_F {
        color_f(0.61, 0.74, 1.0, 1.0)
    } // #9CDCFE
    pub fn preprocessor() -> D2D1_COLOR_F {
        color_f(0.50, 0.50, 0.50, 1.0)
    } // #808080
}
