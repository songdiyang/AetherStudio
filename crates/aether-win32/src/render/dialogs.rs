use super::*;

impl EditorState {
    pub(super) fn render_new_project_dialog(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
    ) {
        unsafe {
            let scale = self.dpi_scale.max(1.0);
            let width = 460.0f32 / scale;
            let height = 220.0f32 / scale;
            let x = (self.window_width as f32 / scale - width) / 2.0;
            let y = (self.window_height as f32 / scale - height) / 2.0;

            let bg_color = color_f(0.18, 0.18, 0.18, 1.0);
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bg_color)
                .unwrap();
            let border_color = color_f(0.3, 0.3, 0.3, 1.0);
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
            let btn_bg_color = color_f(0.0, 0.47, 0.83, 1.0);
            let btn_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_bg_color)
                .unwrap();
            let btn_hover_color = color_f(0.0, 0.55, 0.95, 1.0);
            let btn_hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_hover_color)
                .unwrap();
            let overlay_color = color_f(0.0, 0.0, 0.0, 0.5);
            let overlay_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &overlay_color)
                .unwrap();

            let format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let title_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    14.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let small_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    11.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();

            // 遮罩层
            let overlay_rect = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: self.window_width as f32,
                bottom: self.window_height as f32,
            };
            target.FillRectangle(&overlay_rect, &overlay_brush);

            // 对话框背景
            let dialog_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&dialog_rect, &bg_brush);
            target.DrawRectangle(&dialog_rect, &border_brush, 1.0, None);

            let mut cy = y + 16.0;

            // 标题
            let title: Vec<u16> = "新建项目".encode_utf16().chain(Some(0)).collect();
            let title_rect = D2D_RECT_F {
                left: x + 16.0,
                top: cy,
                right: x + width - 16.0,
                bottom: cy + 22.0,
            };
            target.DrawText(
                &title,
                &title_format,
                &title_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 36.0;

            // 基础路径显示
            let base_label: Vec<u16> = "项目位置:".encode_utf16().chain(Some(0)).collect();
            let base_label_rect = D2D_RECT_F {
                left: x + 16.0,
                top: cy,
                right: x + 80.0,
                bottom: cy + 18.0,
            };
            target.DrawText(
                &base_label,
                &format,
                &base_label_rect,
                &dim_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let base_path_text: Vec<u16> = self
                .new_project_dialog
                .base_path
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let base_path_rect = D2D_RECT_F {
                left: x + 80.0,
                top: cy,
                right: x + width - 16.0,
                bottom: cy + 18.0,
            };
            target.DrawText(
                &base_path_text,
                &small_format,
                &base_path_rect,
                &dim_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 28.0;

            // 项目名称输入
            let label_text: Vec<u16> = "项目名称:".encode_utf16().chain(Some(0)).collect();
            let label_rect = D2D_RECT_F {
                left: x + 16.0,
                top: cy,
                right: x + 80.0,
                bottom: cy + 18.0,
            };
            target.DrawText(
                &label_text,
                &format,
                &label_rect,
                &dim_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            let input_rect = D2D_RECT_F {
                left: x + 80.0,
                top: cy - 2.0,
                right: x + width - 16.0,
                bottom: cy + 20.0,
            };
            target.FillRectangle(&input_rect, &input_bg_brush);
            let val_text: Vec<u16> = self
                .new_project_dialog
                .project_name
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let val_rect = D2D_RECT_F {
                left: x + 84.0,
                top: cy,
                right: x + width - 20.0,
                bottom: cy + 18.0,
            };
            target.DrawText(
                &val_text,
                &format,
                &val_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            if self.new_project_dialog.focus_field == 0 {
                // 绘制输入框光标
                if self.new_project_dialog.caret_visible {
                    let char_width = self.text_renderer.char_width();
                    let text_width: f32 = self
                        .new_project_dialog
                        .project_name
                        .chars()
                        .map(|ch| char_width * unicode_char_width(ch) as f32)
                        .sum();
                    let caret_x = (x + 84.0 + text_width).min(x + width - 22.0);
                    let caret_rect = D2D_RECT_F {
                        left: caret_x,
                        top: cy + 1.0,
                        right: caret_x + 1.0,
                        bottom: cy + 17.0,
                    };
                    let caret_color = color_f(0.9, 0.9, 0.9, 1.0);
                    let caret_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &caret_color)
                        .unwrap();
                    target.FillRectangle(&caret_rect, &caret_brush);
                }

                let focus_rect = D2D_RECT_F {
                    left: x + 80.0,
                    top: cy - 2.0,
                    right: x + width - 16.0,
                    bottom: cy + 20.0,
                };
                let focus_color = color_f(0.0, 0.47, 0.83, 1.0);
                let focus_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &focus_color)
                    .unwrap();
                target.DrawRectangle(&focus_rect, &focus_brush, 1.0, None);
            }
            cy += 40.0;

            // 错误消息
            if let Some(err) = &self.new_project_dialog.error_message {
                let err_text: Vec<u16> = err.encode_utf16().chain(Some(0)).collect();
                let err_rect = D2D_RECT_F {
                    left: x + 16.0,
                    top: cy,
                    right: x + width - 16.0,
                    bottom: cy + 18.0,
                };
                let err_color = color_f(0.9, 0.2, 0.2, 1.0);
                let err_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &err_color)
                    .unwrap();
                target.DrawText(
                    &err_text,
                    &format,
                    &err_rect,
                    &err_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                cy += 22.0;
            }

            cy += 8.0;

            // 按钮：确认 和 取消
            let btn_w = 80.0;
            let btn_h = 28.0;

            let confirm_btn_rect = D2D_RECT_F {
                left: x + width - 16.0 - btn_w * 2.0 - 8.0,
                top: cy,
                right: x + width - 16.0 - btn_w - 8.0,
                bottom: cy + btn_h,
            };
            let is_confirm_hover = self.new_project_dialog.hover_button == Some(0);
            target.FillRectangle(
                &confirm_btn_rect,
                if is_confirm_hover {
                    &btn_hover_brush
                } else {
                    &btn_bg_brush
                },
            );
            let confirm_text: Vec<u16> = "确认".encode_utf16().chain(Some(0)).collect();
            let confirm_text_rect = D2D_RECT_F {
                left: confirm_btn_rect.left,
                top: cy + 4.0,
                right: confirm_btn_rect.right,
                bottom: cy + btn_h - 2.0,
            };
            target.DrawText(
                &confirm_text,
                &format,
                &confirm_text_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            let cancel_btn_rect = D2D_RECT_F {
                left: x + width - 16.0 - btn_w,
                top: cy,
                right: x + width - 16.0,
                bottom: cy + btn_h,
            };
            let cancel_bg_color = color_f(0.25, 0.25, 0.25, 1.0);
            let cancel_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &cancel_bg_color)
                .unwrap();
            let cancel_hover_color = color_f(0.35, 0.35, 0.35, 1.0);
            let cancel_hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &cancel_hover_color)
                .unwrap();
            let is_cancel_hover = self.new_project_dialog.hover_button == Some(1);
            target.FillRectangle(
                &cancel_btn_rect,
                if is_cancel_hover {
                    &cancel_hover_brush
                } else {
                    &cancel_bg_brush
                },
            );
            let cancel_text: Vec<u16> = "取消".encode_utf16().chain(Some(0)).collect();
            let cancel_text_rect = D2D_RECT_F {
                left: cancel_btn_rect.left,
                top: cy + 4.0,
                right: cancel_btn_rect.right,
                bottom: cy + btn_h - 2.0,
            };
            target.DrawText(
                &cancel_text,
                &format,
                &cancel_text_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 存储区域用于点击检测
            self.new_project_dialog.input_rect = Some(crate::layout::Region::new(
                input_rect.left,
                input_rect.top,
                input_rect.right - input_rect.left,
                input_rect.bottom - input_rect.top,
            ));
            self.new_project_dialog.confirm_btn_rect = Some(crate::layout::Region::new(
                confirm_btn_rect.left,
                confirm_btn_rect.top,
                confirm_btn_rect.right - confirm_btn_rect.left,
                confirm_btn_rect.bottom - confirm_btn_rect.top,
            ));
            self.new_project_dialog.cancel_btn_rect = Some(crate::layout::Region::new(
                cancel_btn_rect.left,
                cancel_btn_rect.top,
                cancel_btn_rect.right - cancel_btn_rect.left,
                cancel_btn_rect.bottom - cancel_btn_rect.top,
            ));
        }
    }

    /// 渲染图片预览
    pub(super) fn render_image_preview(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        unsafe {
            // 背景
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.editor_bg)
                .unwrap();
            let bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&bg_rect, &bg_brush);

            let title_format = self
                .render_ctx
                .text_format_cache
                .get_center_format(20.0, DWRITE_FONT_WEIGHT_BOLD.0 as u32)
                .unwrap();
            let info_format = self
                .render_ctx
                .text_format_cache
                .get_center_format(14.0, DWRITE_FONT_WEIGHT_NORMAL.0 as u32)
                .unwrap();

            let title_color = color_f(0.83, 0.83, 0.83, 1.0);
            let title_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &title_color)
                .unwrap();
            let info_color = color_f(0.5, 0.5, 0.5, 1.0);
            let info_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &info_color)
                .unwrap();
            let icon_color = color_f(0.3, 0.7, 1.0, 1.0);
            let icon_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &icon_color)
                .unwrap();

            let center_y = y + height / 2.0;

            // 图片图标
            let icon_text: Vec<u16> = "🖼️".encode_utf16().chain(Some(0)).collect();
            let icon_rect = D2D_RECT_F {
                left: x,
                top: center_y - 60.0,
                right: x + width,
                bottom: center_y - 20.0,
            };
            target.DrawText(
                &icon_text,
                &title_format,
                &icon_rect,
                &icon_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 标题
            let title = "图片预览";
            let title_wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
            let title_rect = D2D_RECT_F {
                left: x,
                top: center_y - 20.0,
                right: x + width,
                bottom: center_y + 10.0,
            };
            target.DrawText(
                &title_wide,
                &title_format,
                &title_rect,
                &title_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 文件路径
            if let Some(path) = &self.content.file_path {
                let path_text = format!("{}", path.display());
                let path_wide: Vec<u16> = path_text.encode_utf16().chain(Some(0)).collect();
                let path_rect = D2D_RECT_F {
                    left: x + 20.0,
                    top: center_y + 20.0,
                    right: x + width - 20.0,
                    bottom: center_y + 50.0,
                };
                target.DrawText(
                    &path_wide,
                    &info_format,
                    &path_rect,
                    &info_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }
}
