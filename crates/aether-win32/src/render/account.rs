use super::*;

impl EditorState {
    /// 渲染"账号"标签页内容（账户信息 / 速通套餐 / 速通用量 / 隐私模式）
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_account_page(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        width: f32,
        start_y: f32,
        _height: f32,
        _title_format: IDWriteTextFormat,
        label_format: IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        unsafe {
            let margin = 24.0_f32;
            let card_x = x + margin;
            let card_w = width - margin * 2.0;
            let mut cy = start_y + 8.0;

            // ============ 账户信息 ============
            let account_label: Vec<u16> = "账户信息".encode_utf16().chain(Some(0)).collect();
            let section_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    14.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            target.DrawText(
                &account_label,
                &section_format,
                &D2D_RECT_F {
                    left: card_x,
                    top: cy,
                    right: card_x + card_w,
                    bottom: cy + 22.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 30.0;

            // 账户信息卡片
            let card_bg = color_f(0.16, 0.16, 0.18, 1.0);
            let card_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &card_bg)
                .unwrap();
            let _card_radius = 6.0_f32;
            let card_h = 168.0_f32;
            let card_rect = D2D_RECT_F {
                left: card_x,
                top: cy,
                right: card_x + card_w,
                bottom: cy + card_h,
            };
            target.FillRectangle(&card_rect, &card_bg_brush);

            // 头像占位（左侧）
            let avatar_size = 40.0_f32;
            let avatar_x = card_x + 20.0;
            let avatar_y = cy + 20.0;
            let avatar_bg = color_f(0.30, 0.30, 0.32, 1.0);
            let avatar_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &avatar_bg)
                .unwrap();
            let avatar_rect = D2D_RECT_F {
                left: avatar_x,
                top: avatar_y,
                right: avatar_x + avatar_size,
                bottom: avatar_y + avatar_size,
            };
            target.FillRectangle(&avatar_rect, &avatar_brush);
            let initial: Vec<u16> = "U".encode_utf16().chain(Some(0)).collect();
            let initial_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    18.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();
            target.DrawText(
                &initial,
                &initial_format,
                &avatar_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 姓名
            let name_x = avatar_x + avatar_size + 12.0;
            let name_y = avatar_y + 2.0;
            let name_text: Vec<u16> = "未登录".encode_utf16().chain(Some(0)).collect();
            let name_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    14.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            target.DrawText(
                &name_text,
                &name_format,
                &D2D_RECT_F {
                    left: name_x,
                    top: name_y,
                    right: card_x + card_w - 200.0,
                    bottom: name_y + 20.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            // 邮箱/手机占位
            let phone_text: Vec<u16> = "未关联手机号".encode_utf16().chain(Some(0)).collect();
            let phone_color = color_f(0.55, 0.55, 0.55, 1.0);
            let phone_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &phone_color)
                .unwrap();
            target.DrawText(
                &phone_text,
                &label_format,
                &D2D_RECT_F {
                    left: name_x,
                    top: name_y + 20.0,
                    right: card_x + card_w - 200.0,
                    bottom: name_y + 40.0,
                },
                &phone_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 右侧按钮：管理账号 / ...
            let btn_h = 28.0_f32;
            let btn_w = 90.0_f32;
            let btn_gap = 8.0_f32;
            let btn_y = avatar_y + (avatar_size - btn_h) / 2.0;
            let manage_btn_x = card_x + card_w - 16.0 - btn_w * 2.0 - btn_gap;
            let manage_btn_rect = D2D_RECT_F {
                left: manage_btn_x,
                top: btn_y,
                right: manage_btn_x + btn_w,
                bottom: btn_y + btn_h,
            };
            let manage_btn_bg = color_f(0.25, 0.25, 0.27, 1.0);
            let manage_btn_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &manage_btn_bg)
                .unwrap();
            target.FillRectangle(&manage_btn_rect, &manage_btn_brush);
            let manage_text: Vec<u16> = "管理账号".encode_utf16().chain(Some(0)).collect();
            let btn_text_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();
            target.DrawText(
                &manage_text,
                &btn_text_format,
                &manage_btn_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            let more_btn_x = manage_btn_x + btn_w + btn_gap;
            let more_btn_rect = D2D_RECT_F {
                left: more_btn_x,
                top: btn_y,
                right: more_btn_x + btn_w,
                bottom: btn_y + btn_h,
            };
            target.FillRectangle(&more_btn_rect, &manage_btn_brush);
            let more_text: Vec<u16> = "···".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &more_text,
                &btn_text_format,
                &more_btn_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 卡片内分隔线
            let sep_y = cy + 76.0;
            let sep_color = color_f(0.22, 0.22, 0.24, 1.0);
            let sep_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sep_color)
                .unwrap();
            let sep_rect = D2D_RECT_F {
                left: card_x + 16.0,
                top: sep_y,
                right: card_x + card_w - 16.0,
                bottom: sep_y + 1.0,
            };
            target.FillRectangle(&sep_rect, &sep_brush);

            // 速通 Pro 行
            let pro_y = sep_y + 10.0;
            let bolt_color = color_f(0.20, 0.80, 0.50, 1.0);
            let bolt_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bolt_color)
                .unwrap();
            let bolt_text: Vec<u16> = "⚡ 速通 Pro".encode_utf16().chain(Some(0)).collect();
            let pro_label_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            target.DrawText(
                &bolt_text,
                &pro_label_format,
                &D2D_RECT_F {
                    left: card_x + 20.0,
                    top: pro_y,
                    right: card_x + 200.0,
                    bottom: pro_y + 20.0,
                },
                &bolt_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let pro_sub: Vec<u16> = "尚未开通 · 享更快 AI 回复"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            target.DrawText(
                &pro_sub,
                &label_format,
                &D2D_RECT_F {
                    left: card_x + 20.0,
                    top: pro_y + 22.0,
                    right: card_x + card_w - 200.0,
                    bottom: pro_y + 42.0,
                },
                &phone_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            let sub_btn_w = 90.0_f32;
            let sub_btn_x = card_x + card_w - 16.0 - sub_btn_w;
            let sub_btn_rect = D2D_RECT_F {
                left: sub_btn_x,
                top: pro_y - 4.0,
                right: sub_btn_x + sub_btn_w,
                bottom: pro_y + 24.0,
            };
            let sub_btn_bg = color_f(0.25, 0.25, 0.27, 1.0);
            let sub_btn_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sub_btn_bg)
                .unwrap();
            target.FillRectangle(&sub_btn_rect, &sub_btn_brush);
            let sub_text: Vec<u16> = "立即订阅".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &sub_text,
                &btn_text_format,
                &sub_btn_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            cy += card_h + 24.0;

            // ============ 速通用量 ============
            let usage_label: Vec<u16> = "速通用量".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &usage_label,
                &section_format,
                &D2D_RECT_F {
                    left: card_x,
                    top: cy,
                    right: card_x + card_w,
                    bottom: cy + 22.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 30.0;

            let usage_card_h = 96.0_f32;
            let usage_card_rect = D2D_RECT_F {
                left: card_x,
                top: cy,
                right: card_x + card_w,
                bottom: cy + usage_card_h,
            };
            target.FillRectangle(&usage_card_rect, &card_bg_brush);

            // 速通可用次数
            let usage_text: Vec<u16> = "⚡ 速通可用 0 次".encode_utf16().chain(Some(0)).collect();
            let usage_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    14.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            target.DrawText(
                &usage_text,
                &usage_format,
                &D2D_RECT_F {
                    left: card_x + 20.0,
                    top: cy + 16.0,
                    right: card_x + card_w - 80.0,
                    bottom: cy + 36.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 右侧刷新按钮
            let refresh_size = 28.0_f32;
            let refresh_x = card_x + card_w - 16.0 - refresh_size;
            let refresh_y = cy + 12.0;
            let refresh_rect = D2D_RECT_F {
                left: refresh_x,
                top: refresh_y,
                right: refresh_x + refresh_size,
                bottom: refresh_y + refresh_size,
            };
            let refresh_bg = color_f(0.25, 0.25, 0.27, 1.0);
            let refresh_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &refresh_bg)
                .unwrap();
            target.FillRectangle(&refresh_rect, &refresh_brush);
            let refresh_text: Vec<u16> = "↻".encode_utf16().chain(Some(0)).collect();
            let refresh_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    14.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();
            target.DrawText(
                &refresh_text,
                &refresh_format,
                &refresh_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 卡片内分隔线
            let usage_sep_y = cy + 50.0;
            let usage_sep_rect = D2D_RECT_F {
                left: card_x + 16.0,
                top: usage_sep_y,
                right: card_x + card_w - 16.0,
                bottom: usage_sep_y + 1.0,
            };
            target.FillRectangle(&usage_sep_rect, &sep_brush);

            // 速通次数行
            let count_text: Vec<u16> = "› 速通次数".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &count_text,
                &usage_format,
                &D2D_RECT_F {
                    left: card_x + 20.0,
                    top: usage_sep_y + 8.0,
                    right: card_x + 200.0,
                    bottom: usage_sep_y + 38.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let count_value: Vec<u16> = "0 次".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &count_value,
                &usage_format,
                &D2D_RECT_F {
                    left: card_x + card_w - 120.0,
                    top: usage_sep_y + 8.0,
                    right: card_x + card_w - 20.0,
                    bottom: usage_sep_y + 38.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            cy += usage_card_h + 24.0;

            // ============ 隐私模式 ============
            let privacy_label: Vec<u16> = "隐私模式".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &privacy_label,
                &section_format,
                &D2D_RECT_F {
                    left: card_x,
                    top: cy,
                    right: card_x + card_w,
                    bottom: cy + 22.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 30.0;

            let privacy_card_h = 56.0_f32;
            let privacy_card_rect = D2D_RECT_F {
                left: card_x,
                top: cy,
                right: card_x + card_w,
                bottom: cy + privacy_card_h,
            };
            target.FillRectangle(&privacy_card_rect, &card_bg_brush);
            let privacy_text: Vec<u16> = "开启后 AI 助手不会保留对话历史"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            target.DrawText(
                &privacy_text,
                &label_format,
                &D2D_RECT_F {
                    left: card_x + 20.0,
                    top: cy + 18.0,
                    right: card_x + card_w - 80.0,
                    bottom: cy + 38.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    pub(super) fn render_user_menu(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
    ) {
        unsafe {
            let menu_width = self.user_menu.menu_width();
            let menu_height = self.user_menu.menu_height();

            // 边界检查：确保菜单不超出窗口右边界
            let max_x = (self.window_width as f32 - menu_width).max(4.0);
            let menu_x = x.min(max_x);

            // 菜单背景
            let bg_color = if self.theme.glass_enabled {
                self.theme.submenu_bg
            } else {
                color_f(0.18, 0.18, 0.18, 1.0)
            };
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bg_color)
                .unwrap();
            let menu_rect = D2D_RECT_F {
                left: menu_x,
                top: y,
                right: menu_x + menu_width,
                bottom: y + menu_height,
            };

            // 绘制阴影（右侧和底部）
            let shadow_color = color_f(0.0, 0.0, 0.0, 0.35);
            let shadow_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &shadow_color)
                .unwrap();
            let shadow_right = D2D_RECT_F {
                left: menu_rect.right,
                top: menu_rect.top + 4.0,
                right: menu_rect.right + 6.0,
                bottom: menu_rect.bottom + 6.0,
            };
            target.FillRectangle(&shadow_right, &shadow_brush);
            let shadow_bottom = D2D_RECT_F {
                left: menu_rect.left + 4.0,
                top: menu_rect.bottom,
                right: menu_rect.right + 6.0,
                bottom: menu_rect.bottom + 6.0,
            };
            target.FillRectangle(&shadow_bottom, &shadow_brush);

            target.FillRectangle(&menu_rect, &bg_brush);

            // 菜单边框
            let border_color = color_f(0.3, 0.3, 0.3, 1.0);
            let border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &border_color)
                .unwrap();
            target.DrawRectangle(&menu_rect, &border_brush, 1.0, None);

            // 保存菜单区域用于点击检测（使用调整后的位置）
            self.user_menu.menu_rect = Some(crate::layout::Region::new(
                menu_x,
                y,
                menu_width,
                menu_height,
            ));

            let text_color = color_f(0.85, 0.85, 0.85, 1.0);
            let text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &text_color)
                .unwrap();
            let hover_bg = color_f(0.0, 0.47, 0.83, 1.0);
            let hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_bg)
                .unwrap();
            let text_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();
            let shortcut_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_TRAILING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();

            // 绘制用户名头部（蓝色背景）
            let header_height = 40.0;
            let header_rect = D2D_RECT_F {
                left: menu_x,
                top: y,
                right: menu_x + menu_width,
                bottom: y + header_height,
            };
            target.FillRectangle(&header_rect, &hover_brush);
            let username_wide: Vec<u16> = self
                .user_menu
                .username
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let username_rect = D2D_RECT_F {
                left: menu_x + 12.0,
                top: y,
                right: menu_x + menu_width - 12.0,
                bottom: y + header_height,
            };
            target.DrawText(
                &username_wide,
                &text_format,
                &username_rect,
                &self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &color_f(1.0, 1.0, 1.0, 1.0))
                    .unwrap(),
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 绘制菜单项
            let item_height = 32.0;
            let separator_height = 9.0;
            let mut current_y = y + header_height;

            for (i, item) in self.user_menu.items.iter().enumerate() {
                if item.is_separator() {
                    // 分隔线
                    let sep_rect = D2D_RECT_F {
                        left: menu_x + 8.0,
                        top: current_y + 4.0,
                        right: menu_x + menu_width - 8.0,
                        bottom: current_y + 5.0,
                    };
                    let sep_color = color_f(0.3, 0.3, 0.3, 1.0);
                    let sep_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &sep_color)
                        .unwrap();
                    target.FillRectangle(&sep_rect, &sep_brush);
                    current_y += separator_height;
                } else {
                    let is_hover = self.user_menu.hover_index == Some(i);
                    if is_hover {
                        let item_rect = D2D_RECT_F {
                            left: menu_x + 4.0,
                            top: current_y,
                            right: menu_x + menu_width - 4.0,
                            bottom: current_y + item_height,
                        };
                        target.FillRectangle(&item_rect, &hover_brush);
                    }

                    let label_wide: Vec<u16> = item.label().encode_utf16().chain(Some(0)).collect();
                    let label_rect = D2D_RECT_F {
                        left: menu_x + 16.0,
                        top: current_y,
                        right: menu_x + menu_width - 80.0,
                        bottom: current_y + item_height,
                    };
                    target.DrawText(
                        &label_wide,
                        &text_format,
                        &label_rect,
                        &text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    // 快捷键
                    if let Some(shortcut) = item.shortcut() {
                        let shortcut_wide: Vec<u16> =
                            shortcut.encode_utf16().chain(Some(0)).collect();
                        let shortcut_rect = D2D_RECT_F {
                            left: menu_x + menu_width - 78.0,
                            top: current_y,
                            right: menu_x + menu_width - 16.0,
                            bottom: current_y + item_height,
                        };
                        let shortcut_color = color_f(0.6, 0.6, 0.6, 1.0);
                        let shortcut_brush = self
                            .render_ctx
                            .brush_cache
                            .get_brush(target, &shortcut_color)
                            .unwrap();
                        target.DrawText(
                            &shortcut_wide,
                            &shortcut_format,
                            &shortcut_rect,
                            &shortcut_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }

                    current_y += item_height;
                }
            }
        }
    }
}
