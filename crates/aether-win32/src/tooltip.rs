//! Tooltip 渲染系统
//! 实现 500ms 延迟显示、4px 移动容差的悬停提示

use windows::Win32::Foundation::POINT;

/// Tooltip 状态
#[derive(Clone, Debug, Default)]
pub struct TooltipState {
    /// 当前 hover 的元素 key（用于检测是否切换了元素）
    pub hover_key: Option<String>,
    /// 鼠标按下时的位置（用于移动容差检测）
    pub anchor: POINT,
    /// 计时器开始时间（GetTickCount64 的返回值，毫秒）
    pub timer_start: Option<u64>,
    /// 当前显示的内容（None 表示不显示）
    pub visible_text: Option<String>,
    /// 显示位置（屏幕坐标转客户端坐标后的位置）
    pub show_pos: (f32, f32),
}

/// Tooltip 触发延迟（毫秒）
pub const TOOLTIP_DELAY_MS: u64 = 500;
/// Tooltip 移动容差（逻辑像素）
pub const TOOLTIP_MOVE_TOLERANCE: f32 = 4.0;

impl crate::editor::EditorState {
    /// 渲染 tooltip（在主渲染流程末尾调用，确保在最上层）
    pub(crate) fn render_tooltip(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        text_format: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
    ) {
        use aether_render::d2d::factory::color_f;
        use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
        use windows::Win32::Graphics::Direct2D::{
            ID2D1SolidColorBrush, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ROUNDED_RECT,
        };
        use windows::Win32::Graphics::DirectWrite::{
            DWRITE_MEASURING_MODE_NATURAL, DWRITE_TEXT_METRICS,
        };

        let Some(text) = self.tooltip_state.visible_text.as_ref() else {
            return;
        };
        if text.is_empty() {
            return;
        }

        unsafe {
            // 1. 测量文本：IDWriteTextLayout::GetMetrics
            let dwrite = self.text_renderer.dwrite_factory();
            let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
            let layout = match dwrite.CreateTextLayout(&wide, text_format, 10000.0, 1000.0) {
                Ok(l) => l,
                Err(_) => return,
            };
            let mut metrics = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut metrics).is_err() {
                return;
            }
            let text_width = metrics.width;
            let text_height = metrics.height;

            // 2. 计算 tooltip rect：x = show_pos.0 + 14, y = show_pos.1 + 18
            //    宽度 = text_width + 16, 高度 = text_height + 8
            let offset_x = 14.0_f32;
            let offset_y = 18.0_f32;
            let pad_x = 8.0_f32; // 左右内边距，合计 16
            let pad_y = 4.0_f32; // 上下内边距，合计 8
            let box_w = text_width + 16.0;
            let box_h = text_height + 8.0;

            let mut tx = self.tooltip_state.show_pos.0 + offset_x;
            let ty = self.tooltip_state.show_pos.1 + offset_y;

            // 3. 如果 tooltip 右侧超出窗口，则放在鼠标左侧
            let win_w = self.window_width as f32;
            if tx + box_w > win_w {
                tx = (self.tooltip_state.show_pos.0 - offset_x - box_w).max(0.0);
            }
            // 钳制 y 到窗口范围内
            let win_h = self.window_height as f32;
            let ty = if ty + box_h > win_h {
                (win_h - box_h).max(0.0)
            } else {
                ty
            };

            // 4. 绘制圆角半透明背景：FillRoundedRectangle
            //    bg_color = RGBA(40, 44, 52, 230)，radius = 4.0
            let bg_color = color_f(40.0 / 255.0, 44.0 / 255.0, 52.0 / 255.0, 230.0 / 255.0);
            let bg_brush: ID2D1SolidColorBrush = match target.CreateSolidColorBrush(&bg_color, None)
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let rounded_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: tx,
                    top: ty,
                    right: tx + box_w,
                    bottom: ty + box_h,
                },
                radiusX: 4.0,
                radiusY: 4.0,
            };
            target.FillRoundedRectangle(&rounded_rect, &bg_brush);

            // 5. 绘制文本：DrawText
            //    color = RGBA(220, 220, 220, 255)，使用 D2D1_DRAW_TEXT_OPTIONS_NONE
            let text_color = color_f(220.0 / 255.0, 220.0 / 255.0, 220.0 / 255.0, 1.0);
            let text_brush: ID2D1SolidColorBrush =
                match target.CreateSolidColorBrush(&text_color, None) {
                    Ok(b) => b,
                    Err(_) => return,
                };
            let text_rect = D2D_RECT_F {
                left: tx + pad_x,
                top: ty + pad_y,
                right: tx + box_w - pad_x,
                bottom: ty + box_h - pad_y,
            };
            let utf16: Vec<u16> = text.encode_utf16().collect();
            target.DrawText(
                &utf16,
                text_format,
                &text_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// 根据当前 hover 状态计算 tooltip 的 hover_key。
    ///
    /// 优先级：活动栏 > 标题栏按钮。
    /// 返回 `(hover_key, tooltip_text)`，二者均为 None 表示未 hover 任何可提示元素。
    pub(crate) fn compute_tooltip_hover_key(&self) -> (Option<String>, Option<String>) {
        // 活动栏项
        if let Some(idx) = self.activity_bar.hover_index {
            if let Some(item) = self.activity_bar.items.get(idx) {
                return (
                    Some(format!("activity_{}", idx)),
                    Some(item.tooltip.clone()),
                );
            }
        }
        // 标题栏按钮
        if let Some(btn) = self.titlebar_hover_button {
            let (key, text) = match btn {
                0 => ("title_btn_0", "最小化"),
                1 => ("title_btn_1", "最大化/还原"),
                2 => ("title_btn_2", "关闭"),
                3 => ("title_btn_3", "账户"),
                4 => ("title_btn_4", "设置"),
                5 => ("title_btn_5", "切换右侧面板"),
                6 => ("title_btn_6", "切换底部面板"),
                7 => ("title_btn_7", "切换侧边栏"),
                8 => ("title_btn_8", "前进"),
                9 => ("title_btn_9", "后退"),
                _ => return (None, None),
            };
            return (Some(key.to_string()), Some(text.to_string()));
        }
        (None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tooltip_state_default() {
        let state = TooltipState::default();
        assert!(state.hover_key.is_none());
        assert_eq!(state.anchor.x, 0);
        assert_eq!(state.anchor.y, 0);
        assert!(state.timer_start.is_none());
        assert!(state.visible_text.is_none());
        assert_eq!(state.show_pos, (0.0, 0.0));
    }

    #[test]
    fn test_tooltip_state_clone() {
        let mut state = TooltipState::default();
        state.hover_key = Some("activity_0".to_string());
        state.visible_text = Some("资源管理器".to_string());
        state.show_pos = (10.0, 20.0);
        state.timer_start = Some(12345);
        state.anchor = POINT { x: 5, y: 6 };

        let cloned = state.clone();
        assert_eq!(cloned.hover_key, state.hover_key);
        assert_eq!(cloned.visible_text, state.visible_text);
        assert_eq!(cloned.show_pos, state.show_pos);
        assert_eq!(cloned.timer_start, state.timer_start);
        assert_eq!(cloned.anchor.x, state.anchor.x);
        assert_eq!(cloned.anchor.y, state.anchor.y);
    }

    #[test]
    fn test_tooltip_state_with_values() {
        let mut state = TooltipState::default();
        state.hover_key = Some("title_btn_2".to_string());
        state.anchor = POINT { x: 100, y: 200 };
        state.timer_start = Some(99999);
        state.visible_text = Some("关闭".to_string());
        state.show_pos = (50.5, 60.5);

        assert_eq!(state.hover_key.as_deref(), Some("title_btn_2"));
        assert_eq!(state.anchor.x, 100);
        assert_eq!(state.anchor.y, 200);
        assert_eq!(state.timer_start, Some(99999));
        assert_eq!(state.visible_text.as_deref(), Some("关闭"));
        assert_eq!(state.show_pos, (50.5, 60.5));
    }

    #[test]
    fn test_tooltip_constants() {
        assert_eq!(TOOLTIP_DELAY_MS, 500);
        assert_eq!(TOOLTIP_MOVE_TOLERANCE, 4.0);
    }
}
