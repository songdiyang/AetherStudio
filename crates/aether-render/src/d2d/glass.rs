//! Glass effect drawing helpers
//!
//! Provides functions for drawing translucent panels, soft borders,
//! glow effects, and shadow layers to create an Acrylic/Glass UI.

use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget;

use crate::d2d::brush_cache::BrushCache;

/// Draw a glass panel with background fill and optional soft border
pub fn draw_glass_panel(
    target: &ID2D1HwndRenderTarget,
    brush_cache: &mut BrushCache,
    rect: &D2D_RECT_F,
    bg_color: &D2D1_COLOR_F,
    border_color: &D2D1_COLOR_F,
    border_width: f32,
) -> Result<()> {
    unsafe {
        let bg_brush = brush_cache.get_brush(target, bg_color)?;
        target.FillRectangle(rect, &bg_brush);

        if border_width > 0.0 {
            let border_brush = brush_cache.get_brush(target, border_color)?;
            // Top border
            let top = D2D_RECT_F {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.top + border_width,
            };
            target.FillRectangle(&top, &border_brush);
            // Bottom border
            let bottom = D2D_RECT_F {
                left: rect.left,
                top: rect.bottom - border_width,
                right: rect.right,
                bottom: rect.bottom,
            };
            target.FillRectangle(&bottom, &border_brush);
            // Left border
            let left = D2D_RECT_F {
                left: rect.left,
                top: rect.top,
                right: rect.left + border_width,
                bottom: rect.bottom,
            };
            target.FillRectangle(&left, &border_brush);
            // Right border
            let right = D2D_RECT_F {
                left: rect.right - border_width,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            };
            target.FillRectangle(&right, &border_brush);
        }
    }
    Ok(())
}

/// Draw a soft glow selection highlight (simulated by drawing a slightly larger
/// rect with lower opacity behind the main selection)
pub fn draw_glow_selection(
    target: &ID2D1HwndRenderTarget,
    brush_cache: &mut BrushCache,
    rect: &D2D_RECT_F,
    glow_color: &D2D1_COLOR_F,
    glow_radius: f32,
) -> Result<()> {
    unsafe {
        // Outer glow (larger, more transparent)
        let outer_glow = D2D1_COLOR_F {
            r: glow_color.r,
            g: glow_color.g,
            b: glow_color.b,
            a: glow_color.a * 0.3,
        };
        let outer_brush = brush_cache.get_brush(target, &outer_glow)?;
        let outer_rect = D2D_RECT_F {
            left: rect.left - glow_radius,
            top: rect.top - glow_radius,
            right: rect.right + glow_radius,
            bottom: rect.bottom + glow_radius,
        };
        target.FillRectangle(&outer_rect, &outer_brush);

        // Inner glow
        let inner_brush = brush_cache.get_brush(target, glow_color)?;
        target.FillRectangle(rect, &inner_brush);
    }
    Ok(())
}

/// Draw a subtle drop shadow beneath a panel
pub fn draw_panel_shadow(
    target: &ID2D1HwndRenderTarget,
    brush_cache: &mut BrushCache,
    rect: &D2D_RECT_F,
    shadow_color: &D2D1_COLOR_F,
    shadow_height: f32,
) -> Result<()> {
    unsafe {
        let shadow_brush = brush_cache.get_brush(target, shadow_color)?;
        let shadow_rect = D2D_RECT_F {
            left: rect.left,
            top: rect.bottom,
            right: rect.right,
            bottom: rect.bottom + shadow_height,
        };
        target.FillRectangle(&shadow_rect, &shadow_brush);
    }
    Ok(())
}

/// Draw a rounded-corner-like panel by using a slightly smaller inner rect
/// with brighter color to simulate corner highlights
pub fn draw_rounded_panel(
    target: &ID2D1HwndRenderTarget,
    brush_cache: &mut BrushCache,
    rect: &D2D_RECT_F,
    bg_color: &D2D1_COLOR_F,
    border_color: &D2D1_COLOR_F,
    corner_highlight: &D2D1_COLOR_F,
) -> Result<()> {
    unsafe {
        // Main background
        let bg_brush = brush_cache.get_brush(target, bg_color)?;
        target.FillRectangle(rect, &bg_brush);

        // Soft border
        let border_brush = brush_cache.get_brush(target, border_color)?;
        let top = D2D_RECT_F {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.top + 1.0,
        };
        target.FillRectangle(&top, &border_brush);

        // Corner highlight (subtle top gradient feel)
        let corner_brush = brush_cache.get_brush(target, corner_highlight)?;
        let corner_rect = D2D_RECT_F {
            left: rect.left + 2.0,
            top: rect.top + 1.0,
            right: rect.right - 2.0,
            bottom: rect.top + 2.0,
        };
        target.FillRectangle(&corner_rect, &corner_brush);
    }
    Ok(())
}
