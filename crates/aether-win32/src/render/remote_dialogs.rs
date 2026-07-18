use super::*;

impl EditorState {
    pub(super) fn render_ssh_dialog(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
    ) {
        unsafe {
            let width = 400.0f32;
            let height = 420.0f32;
            let x = (self.window_width as f32 - width) / 2.0;
            let y = (self.window_height as f32 - height) / 2.0;

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
            let border_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.DrawRectangle(&border_rect, &border_brush, 1.0, None);

            let mut cy = y + 16.0;

            // 标题
            let title: Vec<u16> = "SSH 连接".encode_utf16().chain(Some(0)).collect();
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
            cy += 32.0;

            // 错误消息
            if let Some(err) = &self.ssh_dialog.error_message {
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

            // 字段标签和输入框
            let fields = vec![
                ("主机:", &self.ssh_dialog.host, 0),
                ("端口:", &self.ssh_dialog.port, 1),
                ("用户名:", &self.ssh_dialog.username, 2),
            ];

            for (label, value, idx) in &fields {
                let label_text: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
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
                let val_text: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
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

                // 焦点指示器
                if self.ssh_dialog.focus_field == *idx {
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

                cy += 28.0;
            }

            // 认证类型
            let auth_label: Vec<u16> = "认证:".encode_utf16().chain(Some(0)).collect();
            let auth_label_rect = D2D_RECT_F {
                left: x + 16.0,
                top: cy,
                right: x + 80.0,
                bottom: cy + 18.0,
            };
            target.DrawText(
                &auth_label,
                &format,
                &auth_label_rect,
                &dim_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            let auth_text = match self.ssh_dialog.auth_type {
                crate::ssh::SshAuthType::Password => "密码",
                crate::ssh::SshAuthType::Key => "私钥",
                crate::ssh::SshAuthType::Agent => "SSH Agent",
            };
            let auth_val: Vec<u16> = auth_text.encode_utf16().chain(Some(0)).collect();
            let auth_val_rect = D2D_RECT_F {
                left: x + 80.0,
                top: cy,
                right: x + width - 16.0,
                bottom: cy + 18.0,
            };
            target.DrawText(
                &auth_val,
                &format,
                &auth_val_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 28.0;

            // 根据认证类型显示不同字段
            match self.ssh_dialog.auth_type {
                crate::ssh::SshAuthType::Password => {
                    let label_text: Vec<u16> = "密码:".encode_utf16().chain(Some(0)).collect();
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
                    // H-22: 使用 char 计数而非字节计数，避免多字节 UTF-8 泄漏长度
                    let hidden: String = self.ssh_dialog.password.chars().map(|_| '*').collect();
                    let val_text: Vec<u16> = hidden.encode_utf16().chain(Some(0)).collect();
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

                    if self.ssh_dialog.focus_field == 3 {
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
                    cy += 28.0;
                }
                crate::ssh::SshAuthType::Key => {
                    let label_text: Vec<u16> = "密钥路径:".encode_utf16().chain(Some(0)).collect();
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
                        .ssh_dialog
                        .key_path
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

                    if self.ssh_dialog.focus_field == 3 {
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
                    cy += 28.0;

                    let label2_text: Vec<u16> = "密码短语:".encode_utf16().chain(Some(0)).collect();
                    let label2_rect = D2D_RECT_F {
                        left: x + 16.0,
                        top: cy,
                        right: x + 80.0,
                        bottom: cy + 18.0,
                    };
                    target.DrawText(
                        &label2_text,
                        &format,
                        &label2_rect,
                        &dim_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    let input2_rect = D2D_RECT_F {
                        left: x + 80.0,
                        top: cy - 2.0,
                        right: x + width - 16.0,
                        bottom: cy + 20.0,
                    };
                    target.FillRectangle(&input2_rect, &input_bg_brush);
                    let hidden2 = "*".repeat(self.ssh_dialog.key_passphrase.len());
                    let val2_text: Vec<u16> = hidden2.encode_utf16().chain(Some(0)).collect();
                    let val2_rect = D2D_RECT_F {
                        left: x + 84.0,
                        top: cy,
                        right: x + width - 20.0,
                        bottom: cy + 18.0,
                    };
                    target.DrawText(
                        &val2_text,
                        &format,
                        &val2_rect,
                        &text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    if self.ssh_dialog.focus_field == 4 {
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
                    cy += 28.0;
                }
                crate::ssh::SshAuthType::Agent => {}
            }

            cy += 16.0;

            // 按钮：Connect 和 Cancel
            let btn_w = 80.0;
            let btn_h = 28.0;

            // Connect 按钮
            let connect_btn_rect = D2D_RECT_F {
                left: x + width - 16.0 - btn_w * 2.0 - 8.0,
                top: cy,
                right: x + width - 16.0 - btn_w - 8.0,
                bottom: cy + btn_h,
            };
            let is_connect_hover = self.ssh_dialog.hover_button == Some(0);
            target.FillRectangle(
                &connect_btn_rect,
                if is_connect_hover {
                    &btn_hover_brush
                } else {
                    &btn_bg_brush
                },
            );
            let connect_text: Vec<u16> = "连接".encode_utf16().chain(Some(0)).collect();
            let connect_text_rect = D2D_RECT_F {
                left: connect_btn_rect.left,
                top: cy + 4.0,
                right: connect_btn_rect.right,
                bottom: cy + btn_h - 2.0,
            };
            target.DrawText(
                &connect_text,
                &format,
                &connect_text_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // Cancel 按钮
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
            let is_cancel_hover = self.ssh_dialog.hover_button == Some(1);
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

            // 存储按钮区域用于点击检测
            self.ssh_dialog.connect_btn_rect = Some(crate::layout::Region::new(
                connect_btn_rect.left,
                connect_btn_rect.top,
                connect_btn_rect.right - connect_btn_rect.left,
                connect_btn_rect.bottom - connect_btn_rect.top,
            ));
            self.ssh_dialog.cancel_btn_rect = Some(crate::layout::Region::new(
                cancel_btn_rect.left,
                cancel_btn_rect.top,
                cancel_btn_rect.right - cancel_btn_rect.left,
                cancel_btn_rect.bottom - cancel_btn_rect.top,
            ));
        }
    }

    pub(super) fn render_clone_dialog(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
    ) {
        unsafe {
            let width = 400.0f32;
            let height = 200.0f32;
            let x = (self.window_width as f32 - width) / 2.0;
            let y = (self.window_height as f32 - height) / 2.0;

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
            let title: Vec<u16> = "克隆仓库".encode_utf16().chain(Some(0)).collect();
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
            cy += 32.0;

            // URL 输入
            let label_text: Vec<u16> = "仓库 URL:".encode_utf16().chain(Some(0)).collect();
            let label_rect = D2D_RECT_F {
                left: x + 16.0,
                top: cy,
                right: x + 90.0,
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
                left: x + 90.0,
                top: cy - 2.0,
                right: x + width - 16.0,
                bottom: cy + 20.0,
            };
            target.FillRectangle(&input_rect, &input_bg_brush);
            let val_text: Vec<u16> = self
                .clone_dialog
                .url
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let val_rect = D2D_RECT_F {
                left: x + 94.0,
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

            if self.clone_dialog.focus_field == 0 {
                let focus_rect = D2D_RECT_F {
                    left: x + 90.0,
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
            cy += 36.0;

            // 错误消息
            if let Some(err) = &self.clone_dialog.error_message {
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

            cy += 16.0;

            // 按钮：Clone 和 Cancel
            let btn_w = 80.0;
            let btn_h = 28.0;

            let clone_btn_rect = D2D_RECT_F {
                left: x + width - 16.0 - btn_w * 2.0 - 8.0,
                top: cy,
                right: x + width - 16.0 - btn_w - 8.0,
                bottom: cy + btn_h,
            };
            let is_clone_hover = self.clone_dialog.hover_button == Some(0);
            target.FillRectangle(
                &clone_btn_rect,
                if is_clone_hover {
                    &btn_hover_brush
                } else {
                    &btn_bg_brush
                },
            );
            let clone_text: Vec<u16> = "克隆".encode_utf16().chain(Some(0)).collect();
            let clone_text_rect = D2D_RECT_F {
                left: clone_btn_rect.left,
                top: cy + 4.0,
                right: clone_btn_rect.right,
                bottom: cy + btn_h - 2.0,
            };
            target.DrawText(
                &clone_text,
                &format,
                &clone_text_rect,
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
            let is_cancel_hover = self.clone_dialog.hover_button == Some(1);
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

            // 存储按钮区域用于点击检测
            self.clone_dialog.clone_btn_rect = Some(crate::layout::Region::new(
                clone_btn_rect.left,
                clone_btn_rect.top,
                clone_btn_rect.right - clone_btn_rect.left,
                clone_btn_rect.bottom - clone_btn_rect.top,
            ));
            self.clone_dialog.cancel_btn_rect = Some(crate::layout::Region::new(
                cancel_btn_rect.left,
                cancel_btn_rect.top,
                cancel_btn_rect.right - cancel_btn_rect.left,
                cancel_btn_rect.bottom - cancel_btn_rect.top,
            ));
        }
    }
}
