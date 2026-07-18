use super::*;

impl EditorState {
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
