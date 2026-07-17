use super::*;

impl EditorState {
    pub(super) fn render_find_replace(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
    ) {
        unsafe {
            let bg_color = if self.theme.glass_enabled {
                color_f(0.18, 0.18, 0.18, 0.95)
            } else {
                color_f(0.18, 0.18, 0.18, 1.0)
            };
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bg_color)
                .unwrap();
            let border_color = color_f(0.0, 0.47, 0.83, 1.0);
            let border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &border_color)
                .unwrap();
            let text_color = color_f(0.9, 0.9, 0.9, 1.0);
            let text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &text_color)
                .unwrap();
            let dim_color = color_f(0.5, 0.5, 0.5, 1.0);
            let dim_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &dim_color)
                .unwrap();
            let input_bg_color = color_f(0.12, 0.12, 0.12, 1.0);
            let input_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &input_bg_color)
                .unwrap();
            let match_color = color_f(0.2, 0.8, 0.3, 1.0);
            let match_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &match_color)
                .unwrap();
            let btn_bg_color = color_f(0.25, 0.25, 0.25, 1.0);
            let _btn_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_bg_color)
                .unwrap();
            let btn_hover_color = color_f(0.35, 0.35, 0.35, 1.0);
            let _btn_hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_hover_color)
                .unwrap();

            let label_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let input_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();

            let panel_height = if self.replace_visible { 72.0 } else { 40.0 };
            let panel_width = width.min(600.0);
            let panel_x = x + width - panel_width - 10.0;

            let panel_rect = D2D_RECT_F {
                left: panel_x,
                top: y,
                right: panel_x + panel_width,
                bottom: y + panel_height,
            };
            target.FillRectangle(&panel_rect, &bg_brush);
            let border_rect = D2D_RECT_F {
                left: panel_x,
                top: y,
                right: panel_x + panel_width,
                bottom: y + 1.0,
            };
            target.FillRectangle(&border_rect, &border_brush);

            let mut cy = y + 8.0;
            let input_h = 24.0;
            let input_w = panel_width - 120.0;

            // 查找标签
            let find_label: Vec<u16> = "查找:".encode_utf16().chain(Some(0)).collect();
            let find_label_rect = D2D_RECT_F {
                left: panel_x + 10.0,
                top: cy,
                right: panel_x + 50.0,
                bottom: cy + input_h,
            };
            target.DrawText(
                &find_label,
                &label_format,
                &find_label_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 查找输入框
            let find_input_rect = D2D_RECT_F {
                left: panel_x + 50.0,
                top: cy,
                right: panel_x + 50.0 + input_w,
                bottom: cy + input_h,
            };
            target.FillRectangle(&find_input_rect, &input_bg_brush);
            // 焦点边框
            if self.find_focus == crate::editor::FindReplaceFocus::FindQuery {
                let focus_border = D2D_RECT_F {
                    left: panel_x + 50.0,
                    top: cy,
                    right: panel_x + 50.0 + input_w,
                    bottom: cy + 1.0,
                };
                target.FillRectangle(&focus_border, &border_brush);
                let focus_border2 = D2D_RECT_F {
                    left: panel_x + 50.0,
                    top: cy + input_h - 1.0,
                    right: panel_x + 50.0 + input_w,
                    bottom: cy + input_h,
                };
                target.FillRectangle(&focus_border2, &border_brush);
            }
            let find_text = if self.find_query.is_empty() {
                "输入查找内容..."
            } else {
                &self.find_query
            };
            let find_text_color = if self.find_query.is_empty() {
                &dim_brush
            } else {
                &text_brush
            };
            let find_wide: Vec<u16> = find_text.encode_utf16().chain(Some(0)).collect();
            let find_text_rect = D2D_RECT_F {
                left: panel_x + 54.0,
                top: cy + 2.0,
                right: panel_x + 46.0 + input_w,
                bottom: cy + input_h - 2.0,
            };
            target.DrawText(
                &find_wide,
                &input_format,
                &find_text_rect,
                find_text_color,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 匹配计数
            let match_text = if !self.find_results.is_empty() {
                format!("{}/{}", self.find_active_index + 1, self.find_results.len())
            } else if !self.find_query.is_empty() {
                "0/0".to_string()
            } else {
                String::new()
            };
            if !match_text.is_empty() {
                let match_wide: Vec<u16> = match_text.encode_utf16().chain(Some(0)).collect();
                let match_rect = D2D_RECT_F {
                    left: panel_x + 52.0 + input_w,
                    top: cy,
                    right: panel_x + panel_width - 10.0,
                    bottom: cy + input_h,
                };
                target.DrawText(
                    &match_wide,
                    &label_format,
                    &match_rect,
                    &match_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            cy += input_h + 8.0;

            // 替换输入框（如果可见）
            if self.replace_visible {
                let replace_label: Vec<u16> = "替换:".encode_utf16().chain(Some(0)).collect();
                let replace_label_rect = D2D_RECT_F {
                    left: panel_x + 10.0,
                    top: cy,
                    right: panel_x + 50.0,
                    bottom: cy + input_h,
                };
                target.DrawText(
                    &replace_label,
                    &label_format,
                    &replace_label_rect,
                    &text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                let replace_input_rect = D2D_RECT_F {
                    left: panel_x + 50.0,
                    top: cy,
                    right: panel_x + 50.0 + input_w,
                    bottom: cy + input_h,
                };
                target.FillRectangle(&replace_input_rect, &input_bg_brush);
                // 焦点边框
                if self.find_focus == crate::editor::FindReplaceFocus::ReplaceText {
                    let focus_border = D2D_RECT_F {
                        left: panel_x + 50.0,
                        top: cy,
                        right: panel_x + 50.0 + input_w,
                        bottom: cy + 1.0,
                    };
                    target.FillRectangle(&focus_border, &border_brush);
                    let focus_border2 = D2D_RECT_F {
                        left: panel_x + 50.0,
                        top: cy + input_h - 1.0,
                        right: panel_x + 50.0 + input_w,
                        bottom: cy + input_h,
                    };
                    target.FillRectangle(&focus_border2, &border_brush);
                }
                let replace_text = if self.replace_text.is_empty() {
                    "输入替换内容..."
                } else {
                    &self.replace_text
                };
                let replace_text_color = if self.replace_text.is_empty() {
                    &dim_brush
                } else {
                    &text_brush
                };
                let replace_wide: Vec<u16> = replace_text.encode_utf16().chain(Some(0)).collect();
                let replace_text_rect = D2D_RECT_F {
                    left: panel_x + 54.0,
                    top: cy + 2.0,
                    right: panel_x + 46.0 + input_w,
                    bottom: cy + input_h - 2.0,
                };
                target.DrawText(
                    &replace_wide,
                    &input_format,
                    &replace_text_rect,
                    replace_text_color,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }
}
