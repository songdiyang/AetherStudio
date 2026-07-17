use super::*;

impl EditorState {
    pub(super) fn render_source_control_sidebar(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        _text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        unsafe {
            let ui_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let bold_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let mono_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    11.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();

            let text_color = color_f(0.9, 0.9, 0.9, 1.0);
            let text_br2 = self
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
            let sel_color = color_f(0.0, 0.47, 0.83, 1.0);
            let sel_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sel_color)
                .unwrap();
            let hover_color = color_f(0.2, 0.2, 0.2, 1.0);
            let hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_color)
                .unwrap();
            let sep_color = color_f(0.2, 0.2, 0.2, 1.0);
            let sep_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sep_color)
                .unwrap();
            let green_color = color_f(0.2, 0.8, 0.3, 1.0);
            let green_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &green_color)
                .unwrap();
            let yellow_color = color_f(0.9, 0.7, 0.2, 1.0);
            let _yellow_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &yellow_color)
                .unwrap();
            let red_color = color_f(0.9, 0.2, 0.2, 1.0);
            let _red_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &red_color)
                .unwrap();
            let btn_bg_color = color_f(0.2, 0.2, 0.2, 1.0);
            let btn_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_bg_color)
                .unwrap();
            let btn_hover_color = color_f(0.3, 0.3, 0.3, 1.0);
            let btn_hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_hover_color)
                .unwrap();

            let mut current_y = y + 10.0 - self.git.scroll_y;

            // 标题
            let title: Vec<u16> = "源代码管理".encode_utf16().chain(Some(0)).collect();
            let title_rect = D2D_RECT_F {
                left: x + 10.0,
                top: current_y,
                right: x + width - 10.0,
                bottom: current_y + 20.0,
            };
            target.DrawText(
                &title,
                &bold_format,
                &title_rect,
                &text_br2,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            current_y += 24.0;

            if !self.git.is_repo() {
                let msg: Vec<u16> = "当前文件夹不是 Git 仓库"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let msg_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: current_y,
                    right: x + width - 10.0,
                    bottom: current_y + 20.0,
                };
                target.DrawText(
                    &msg,
                    &ui_format,
                    &msg_rect,
                    &dim_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                return;
            }

            // 分支名称
            if let Some(branch) = self.git.current_branch_name() {
                let branch_text: Vec<u16> = format!("{} {}", "🌿", branch)
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let branch_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: current_y,
                    right: x + width - 10.0,
                    bottom: current_y + 20.0,
                };
                target.DrawText(
                    &branch_text,
                    &ui_format,
                    &branch_rect,
                    &green_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
            current_y += 22.0;

            // 分隔线
            let sep_rect = D2D_RECT_F {
                left: x + 10.0,
                top: current_y,
                right: x + width - 10.0,
                bottom: current_y + 1.0,
            };
            target.FillRectangle(&sep_rect, &sep_brush);
            current_y += 6.0;

            // Commit 消息输入框
            let input_bg = D2D_RECT_F {
                left: x + 10.0,
                top: current_y,
                right: x + width - 10.0,
                bottom: current_y + 24.0,
            };
            let input_bg_color = color_f(0.18, 0.18, 0.18, 1.0);
            let input_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &input_bg_color)
                .unwrap();
            target.FillRectangle(&input_bg, &input_bg_brush);
            let msg_label = if self.git.commit_message.is_empty() {
                "输入提交消息..."
            } else {
                &self.git.commit_message
            };
            let msg_color = if self.git.commit_message.is_empty() {
                dim_brush.clone()
            } else {
                text_br2.clone()
            };
            let msg_text: Vec<u16> = msg_label.encode_utf16().chain(Some(0)).collect();
            let msg_rect = D2D_RECT_F {
                left: x + 14.0,
                top: current_y + 3.0,
                right: x + width - 14.0,
                bottom: current_y + 21.0,
            };
            target.DrawText(
                &msg_text,
                &mono_format,
                &msg_rect,
                &msg_color,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            current_y += 30.0;

            // 按钮：Commit 和 Refresh
            let btn_y = current_y;
            let btn_h = 24.0;
            let btn_w = 60.0;

            // Commit 按钮
            let commit_btn_rect = D2D_RECT_F {
                left: x + 10.0,
                top: btn_y,
                right: x + 10.0 + btn_w,
                bottom: btn_y + btn_h,
            };
            let is_commit_hover = self
                .git
                .hover_button
                .as_ref()
                .map(|s| s == "commit")
                .unwrap_or(false);
            target.FillRectangle(
                &commit_btn_rect,
                if is_commit_hover {
                    &btn_hover_brush
                } else {
                    &btn_bg_brush
                },
            );
            let commit_text: Vec<u16> = "提交".encode_utf16().chain(Some(0)).collect();
            let commit_text_rect = D2D_RECT_F {
                left: x + 10.0,
                top: btn_y + 3.0,
                right: x + 10.0 + btn_w,
                bottom: btn_y + btn_h - 2.0,
            };
            target.DrawText(
                &commit_text,
                &ui_format,
                &commit_text_rect,
                &text_br2,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // Refresh 按钮
            let refresh_btn_rect = D2D_RECT_F {
                left: x + 80.0,
                top: btn_y,
                right: x + 80.0 + btn_w,
                bottom: btn_y + btn_h,
            };
            let is_refresh_hover = self
                .git
                .hover_button
                .as_ref()
                .map(|s| s == "refresh")
                .unwrap_or(false);
            target.FillRectangle(
                &refresh_btn_rect,
                if is_refresh_hover {
                    &btn_hover_brush
                } else {
                    &btn_bg_brush
                },
            );
            let refresh_text: Vec<u16> = "刷新".encode_utf16().chain(Some(0)).collect();
            let refresh_text_rect = D2D_RECT_F {
                left: x + 80.0,
                top: btn_y + 3.0,
                right: x + 80.0 + btn_w,
                bottom: btn_y + btn_h - 2.0,
            };
            target.DrawText(
                &refresh_text,
                &ui_format,
                &refresh_text_rect,
                &text_br2,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            current_y += 36.0;

            // 分隔线
            let sep2_rect = D2D_RECT_F {
                left: x + 10.0,
                top: current_y,
                right: x + width - 10.0,
                bottom: current_y + 1.0,
            };
            target.FillRectangle(&sep2_rect, &sep_brush);
            current_y += 6.0;

            let item_h = 22.0;
            let section_header_h = 20.0;

            // Staged Changes
            let staged = self.git.staged_files();
            if !staged.is_empty() {
                let header_text: Vec<u16> = format!("已暂存的更改 ({})", staged.len())
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let header_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: current_y,
                    right: x + width - 10.0,
                    bottom: current_y + section_header_h,
                };
                target.DrawText(
                    &header_text,
                    &bold_format,
                    &header_rect,
                    &text_br2,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                current_y += section_header_h;

                for (file, status) in &staged {
                    if current_y + item_h > y + height {
                        break;
                    }
                    if current_y + item_h >= y {
                        let is_selected = self.git.selected_file.as_ref() == Some(file);
                        let is_hover = self.git.hover_file.as_ref() == Some(file);
                        let file_rect = D2D_RECT_F {
                            left: x + 10.0,
                            top: current_y,
                            right: x + width - 30.0,
                            bottom: current_y + item_h,
                        };
                        if is_selected {
                            target.FillRectangle(&file_rect, &sel_brush);
                        } else if is_hover {
                            target.FillRectangle(&file_rect, &hover_brush);
                        }

                        let icon = crate::git::GitRepository::status_icon(*status);
                        let icon_color = crate::git::GitRepository::status_color(*status);
                        let icon_brush = self
                            .render_ctx
                            .brush_cache
                            .get_brush(
                                target,
                                &color_f(icon_color.0, icon_color.1, icon_color.2, 1.0),
                            )
                            .unwrap();
                        let icon_text: Vec<u16> = icon.encode_utf16().chain(Some(0)).collect();
                        let icon_rect = D2D_RECT_F {
                            left: x + 14.0,
                            top: current_y + 2.0,
                            right: x + 30.0,
                            bottom: current_y + item_h - 2.0,
                        };
                        target.DrawText(
                            &icon_text,
                            &mono_format,
                            &icon_rect,
                            &icon_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        let file_name: Vec<u16> = file.encode_utf16().chain(Some(0)).collect();
                        let file_name_rect = D2D_RECT_F {
                            left: x + 32.0,
                            top: current_y + 2.0,
                            right: x + width - 40.0,
                            bottom: current_y + item_h - 2.0,
                        };
                        target.DrawText(
                            &file_name,
                            &mono_format,
                            &file_name_rect,
                            &text_br2,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        // 取消暂存按钮 (-)
                        let minus_rect = D2D_RECT_F {
                            left: x + width - 28.0,
                            top: current_y + 4.0,
                            right: x + width - 10.0,
                            bottom: current_y + item_h - 4.0,
                        };
                        let minus_text: Vec<u16> = "−".encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &minus_text,
                            &ui_format,
                            &minus_rect,
                            &dim_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                    current_y += item_h;
                }
                current_y += 6.0;
            }

            // Changes (unstaged)
            let unstaged = self.git.unstaged_files();
            if !unstaged.is_empty() {
                let header_text: Vec<u16> = format!("更改 ({})", unstaged.len())
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let header_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: current_y,
                    right: x + width - 10.0,
                    bottom: current_y + section_header_h,
                };
                target.DrawText(
                    &header_text,
                    &bold_format,
                    &header_rect,
                    &text_br2,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                current_y += section_header_h;

                for (file, status) in &unstaged {
                    if current_y + item_h > y + height {
                        break;
                    }
                    if current_y + item_h >= y {
                        let is_selected = self.git.selected_file.as_ref() == Some(file);
                        let is_hover = self.git.hover_file.as_ref() == Some(file);
                        let file_rect = D2D_RECT_F {
                            left: x + 10.0,
                            top: current_y,
                            right: x + width - 30.0,
                            bottom: current_y + item_h,
                        };
                        if is_selected {
                            target.FillRectangle(&file_rect, &sel_brush);
                        } else if is_hover {
                            target.FillRectangle(&file_rect, &hover_brush);
                        }

                        let icon = crate::git::GitRepository::status_icon(*status);
                        let icon_color = crate::git::GitRepository::status_color(*status);
                        let icon_brush = self
                            .render_ctx
                            .brush_cache
                            .get_brush(
                                target,
                                &color_f(icon_color.0, icon_color.1, icon_color.2, 1.0),
                            )
                            .unwrap();
                        let icon_text: Vec<u16> = icon.encode_utf16().chain(Some(0)).collect();
                        let icon_rect = D2D_RECT_F {
                            left: x + 14.0,
                            top: current_y + 2.0,
                            right: x + 30.0,
                            bottom: current_y + item_h - 2.0,
                        };
                        target.DrawText(
                            &icon_text,
                            &mono_format,
                            &icon_rect,
                            &icon_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        let file_name: Vec<u16> = file.encode_utf16().chain(Some(0)).collect();
                        let file_name_rect = D2D_RECT_F {
                            left: x + 32.0,
                            top: current_y + 2.0,
                            right: x + width - 40.0,
                            bottom: current_y + item_h - 2.0,
                        };
                        target.DrawText(
                            &file_name,
                            &mono_format,
                            &file_name_rect,
                            &text_br2,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        // 暂存按钮 (+)
                        let plus_rect = D2D_RECT_F {
                            left: x + width - 28.0,
                            top: current_y + 4.0,
                            right: x + width - 10.0,
                            bottom: current_y + item_h - 4.0,
                        };
                        let plus_text: Vec<u16> = "+".encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &plus_text,
                            &ui_format,
                            &plus_rect,
                            &green_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                    current_y += item_h;
                }
                current_y += 6.0;
            }

            // Untracked Files
            let untracked = self.git.untracked_files();
            if !untracked.is_empty() {
                let header_text: Vec<u16> = format!("未跟踪的文件 ({})", untracked.len())
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let header_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: current_y,
                    right: x + width - 10.0,
                    bottom: current_y + section_header_h,
                };
                target.DrawText(
                    &header_text,
                    &bold_format,
                    &header_rect,
                    &text_br2,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                current_y += section_header_h;

                for file in &untracked {
                    if current_y + item_h > y + height {
                        break;
                    }
                    if current_y + item_h >= y {
                        let is_selected = self.git.selected_file.as_ref() == Some(file);
                        let is_hover = self.git.hover_file.as_ref() == Some(file);
                        let file_rect = D2D_RECT_F {
                            left: x + 10.0,
                            top: current_y,
                            right: x + width - 30.0,
                            bottom: current_y + item_h,
                        };
                        if is_selected {
                            target.FillRectangle(&file_rect, &sel_brush);
                        } else if is_hover {
                            target.FillRectangle(&file_rect, &hover_brush);
                        }

                        let icon_text: Vec<u16> = "U".encode_utf16().chain(Some(0)).collect();
                        let icon_rect = D2D_RECT_F {
                            left: x + 14.0,
                            top: current_y + 2.0,
                            right: x + 30.0,
                            bottom: current_y + item_h - 2.0,
                        };
                        target.DrawText(
                            &icon_text,
                            &mono_format,
                            &icon_rect,
                            &dim_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        let file_name: Vec<u16> = file.encode_utf16().chain(Some(0)).collect();
                        let file_name_rect = D2D_RECT_F {
                            left: x + 32.0,
                            top: current_y + 2.0,
                            right: x + width - 40.0,
                            bottom: current_y + item_h - 2.0,
                        };
                        target.DrawText(
                            &file_name,
                            &mono_format,
                            &file_name_rect,
                            &text_br2,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        // 暂存按钮 (+)
                        let plus_rect = D2D_RECT_F {
                            left: x + width - 28.0,
                            top: current_y + 4.0,
                            right: x + width - 10.0,
                            bottom: current_y + item_h - 4.0,
                        };
                        let plus_text: Vec<u16> = "+".encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &plus_text,
                            &ui_format,
                            &plus_rect,
                            &green_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                    current_y += item_h;
                }
            }
        }
    }
}
