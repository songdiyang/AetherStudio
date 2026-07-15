//! PNG 位图加载器：使用 `image` crate 解码 PNG，再创建 ID2D1Bitmap。
//!
//! 用于欢迎页/空占位页显示 logo 图片。
//! 避免依赖系统 WIC PNG 解码器（某些精简 Windows 环境可能缺少）。

use windows::Win32::Graphics::Direct2D::{
    Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_SIZE_U},
    ID2D1RenderTarget, D2D1_BITMAP_PROPERTIES,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

/// 将 PNG 字节数据解码为 ID2D1Bitmap。
///
/// 使用 image crate 解码 PNG 为 RGBA8，再转换为预乘 alpha 的 BGRA8，
/// 最后通过 D2D CreateBitmap 从内存创建位图。
///
/// 注意：D2D CreateBitmap 要求 PREMULTIPLIED alpha 模式。
pub fn load_png_to_bitmap(
    target: &ID2D1RenderTarget,
    png_bytes: &[u8],
) -> Result<windows::Win32::Graphics::Direct2D::ID2D1Bitmap, String> {
    let img = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)
        .map_err(|e| format!("解码 PNG 失败: {}", e))?;
    let rgba = img.to_rgba8();
    let width = rgba.width();
    let height = rgba.height();

    // Direct2D 要求 BGRA8 + 预乘 alpha
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
    for chunk in rgba.chunks_exact(4) {
        let r = chunk[0];
        let g = chunk[1];
        let b = chunk[2];
        let a = chunk[3];
        let af = a as f32 / 255.0;
        bgra.push((b as f32 * af).round() as u8); // B 预乘
        bgra.push((g as f32 * af).round() as u8); // G 预乘
        bgra.push((r as f32 * af).round() as u8); // R 预乘
        bgra.push(a); // A 不变
    }

    let pixel_format = D2D1_PIXEL_FORMAT {
        format: DXGI_FORMAT_B8G8R8A8_UNORM,
        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
    };
    let props = D2D1_BITMAP_PROPERTIES {
        pixelFormat: pixel_format,
        dpiX: 96.0,
        dpiY: 96.0,
    };
    let size = D2D_SIZE_U { width, height };
    let pitch = width * 4;

    unsafe {
        match target.CreateBitmap(size, Some(bgra.as_ptr() as *const _), pitch, &props) {
            Ok(bmp) => {
                tracing::info!("D2D CreateBitmap 成功 (BGRA8 PREMULTIPLIED)");
                Ok(bmp)
            }
            Err(e) => {
                tracing::warn!(error = ?e, "BGRA8 PREMULTIPLIED 失败，尝试默认属性");
                let default_props = D2D1_BITMAP_PROPERTIES::default();
                target
                    .CreateBitmap(
                        size,
                        Some(bgra.as_ptr() as *const _),
                        pitch,
                        &default_props,
                    )
                    .map_err(|e2| {
                        tracing::error!(error = ?e2, "D2D CreateBitmap 默认属性也失败");
                        format!("D2D CreateBitmap 失败: {:?}", e2)
                    })
            }
        }
    }
}
