use super::*;

impl EditorState {
    /// 渲染设置面板：左侧导航 + 右侧内容
    pub(super) fn render_settings_sidebar(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        unsafe {
            // 公共文本格式
            let nav_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
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
            let title_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    18.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let button_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();

            // 整体背景（右侧内容区）
            let content_bg = color_f(0.12, 0.12, 0.12, 1.0);
            let content_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &content_bg)
                .unwrap();
            let content_bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&content_bg_rect, &content_bg_brush);

            // 左侧导航栏布局（宽度可由用户拖拽调整）
            let nav_w = self.settings_panel.nav_width;
            let nav_x = x;
            let nav_y = y;
            let nav_h = height;

            // 导航栏背景（稍亮，与右侧区分）
            let nav_bg = color_f(0.10, 0.10, 0.10, 1.0);
            let nav_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &nav_bg)
                .unwrap();
            let nav_bg_rect = D2D_RECT_F {
                left: nav_x,
                top: nav_y,
                right: nav_x + nav_w,
                bottom: nav_y + nav_h,
            };
            target.FillRectangle(&nav_bg_rect, &nav_bg_brush);

            // 右侧分隔线
            let sep_color = color_f(0.2, 0.2, 0.2, 1.0);
            let sep_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sep_color)
                .unwrap();
            let sep_rect = D2D_RECT_F {
                left: nav_x + nav_w,
                top: nav_y,
                right: nav_x + nav_w + 1.0,
                bottom: nav_y + nav_h,
            };
            target.FillRectangle(&sep_rect, &sep_brush);

            // 调整手柄：悬停或拖拽时高亮
            if self.settings_panel.hover_nav_resize || self.settings_panel.nav_resizing {
                let handle_color = color_f(0.0, 0.47, 0.83, 1.0);
                let handle_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &handle_color)
                    .unwrap();
                let handle_rect = D2D_RECT_F {
                    left: nav_x + nav_w - 1.0,
                    top: nav_y,
                    right: nav_x + nav_w + 1.0,
                    bottom: nav_y + nav_h,
                };
                target.FillRectangle(&handle_rect, &handle_brush);
            }

            // 导航标题
            let nav_title: Vec<u16> = "设置".encode_utf16().chain(Some(0)).collect();
            let nav_title_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    16.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();
            let nav_title_rect = D2D_RECT_F {
                left: nav_x,
                top: nav_y + 16.0,
                right: nav_x + nav_w,
                bottom: nav_y + 48.0,
            };
            target.DrawText(
                &nav_title,
                &nav_title_format,
                &nav_title_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 导航项
            self.settings_panel.clear_regions();
            let tabs = crate::settings::SettingsTab::ALL;
            let nav_item_h = 36.0;
            let nav_item_start_y = nav_y + 60.0;
            for (i, tab) in tabs.iter().enumerate() {
                let item_y = nav_item_start_y + i as f32 * nav_item_h;
                let is_active = self.settings_panel.active_tab == *tab;
                let is_hover = self.settings_panel.hover_tab == Some(*tab);

                let item_bg = if is_active {
                    color_f(0.18, 0.30, 0.45, 1.0)
                } else if is_hover {
                    color_f(0.20, 0.20, 0.22, 1.0)
                } else {
                    color_f(0.10, 0.10, 0.10, 0.0)
                };
                let item_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &item_bg)
                    .unwrap();
                let item_rect = D2D_RECT_F {
                    left: nav_x,
                    top: item_y,
                    right: nav_x + nav_w,
                    bottom: item_y + nav_item_h,
                };
                target.FillRectangle(&item_rect, &item_bg_brush);

                // 激活状态左侧高亮条
                if is_active {
                    let accent = color_f(0.0, 0.47, 0.83, 1.0);
                    let accent_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &accent)
                        .unwrap();
                    let accent_rect = D2D_RECT_F {
                        left: nav_x,
                        top: item_y,
                        right: nav_x + 3.0,
                        bottom: item_y + nav_item_h,
                    };
                    target.FillRectangle(&accent_rect, &accent_brush);
                }

                let item_text_color = if is_active {
                    color_f(1.0, 1.0, 1.0, 1.0)
                } else {
                    color_f(0.75, 0.75, 0.75, 1.0)
                };
                let item_text_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &item_text_color)
                    .unwrap();
                let item_text: Vec<u16> = tab.label().encode_utf16().chain(Some(0)).collect();
                let item_text_rect = D2D_RECT_F {
                    left: nav_x + 20.0,
                    top: item_y,
                    right: nav_x + nav_w - 8.0,
                    bottom: item_y + nav_item_h,
                };
                target.DrawText(
                    &item_text,
                    &nav_format,
                    &item_text_rect,
                    &item_text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                self.settings_panel
                    .add_tab_region(*tab, nav_x, item_y, nav_w, nav_item_h);
            }

            // 右侧内容区域
            let content_x = nav_x + nav_w + 1.0;
            let content_y = nav_y;
            let content_w = width - nav_w - 1.0;
            let content_h = height;

            // 标题栏
            let page_title = match self.settings_panel.active_tab {
                crate::settings::SettingsTab::Account => "账号",
                crate::settings::SettingsTab::General => "通用",
                crate::settings::SettingsTab::Models => "模型",
                crate::settings::SettingsTab::Ai => "AI",
                crate::settings::SettingsTab::Appearance => "外观",
                crate::settings::SettingsTab::Remote => "远程",
            };
            let page_title_wide: Vec<u16> = page_title.encode_utf16().chain(Some(0)).collect();
            let page_title_rect = D2D_RECT_F {
                left: content_x + 24.0,
                top: content_y + 24.0,
                right: content_x + content_w - 24.0,
                bottom: content_y + 56.0,
            };
            target.DrawText(
                &page_title_wide,
                &title_format,
                &page_title_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 标题下方分隔线
            let title_sep_rect = D2D_RECT_F {
                left: content_x + 24.0,
                top: content_y + 64.0,
                right: content_x + content_w - 24.0,
                bottom: content_y + 65.0,
            };
            target.FillRectangle(&title_sep_rect, &sep_brush);

            // 渲染当前激活页面的内容
            let page_x = content_x + 24.0;
            let page_y = content_y + 80.0;
            let page_w = content_w - 48.0;

            match self.settings_panel.active_tab {
                crate::settings::SettingsTab::Account => {
                    self.render_account_page(
                        target,
                        page_x,
                        page_w,
                        page_y,
                        content_h - 80.0,
                        title_format,
                        label_format,
                        text_brush,
                    );
                }
                crate::settings::SettingsTab::General => {
                    self.render_general_settings(
                        target,
                        page_x,
                        page_w,
                        page_y,
                        0.0,
                        label_format,
                        text_brush,
                    );
                }
                crate::settings::SettingsTab::Models => {
                    let label_format_clone = label_format.clone();
                    let input_format_clone = input_format.clone();
                    let button_format_clone = button_format.clone();
                    let title_format_clone = title_format.clone();
                    self.render_models_management(
                        target,
                        page_x,
                        page_w,
                        page_y,
                        0.0,
                        label_format_clone,
                        input_format_clone,
                        button_format_clone,
                        title_format_clone,
                        text_brush,
                    );
                }
                crate::settings::SettingsTab::Ai => {
                    self.render_ai_settings_fields(
                        target,
                        page_x,
                        page_w,
                        page_y,
                        0.0,
                        20.0,
                        32.0,
                        12.0,
                        label_format,
                        input_format,
                        button_format,
                        text_brush,
                        content_h - 80.0,
                    );
                }
                _ => {}
            }
        }
    }

    /// 渲染"通用"标签页内容（主题 / 字体大小 / 自动保存等只读概览）
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_general_settings(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        width: f32,
        start_y: f32,
        margin: f32,
        label_format: IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        unsafe {
            let mut cy = start_y;

            // 主题
            let theme_label = if self.app_settings.ui.theme.is_empty() {
                "默认深色".to_string()
            } else {
                self.app_settings.ui.theme.clone()
            };
            let theme_text: Vec<u16> = format!("主题：{}", theme_label)
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let theme_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 20.0,
            };
            target.DrawText(
                &theme_text,
                &label_format,
                &theme_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 30.0;

            // 字体大小
            let font_size = if self.app_settings.ui.font_size == 0 {
                14
            } else {
                self.app_settings.ui.font_size
            };
            let font_text: Vec<u16> = format!("编辑器字体大小：{} px", font_size)
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let font_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 20.0,
            };
            target.DrawText(
                &font_text,
                &label_format,
                &font_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 30.0;

            // 自动保存
            let auto_save = &self.app_settings.auto_save;
            let auto_save_text: Vec<u16> = format!(
                "自动保存：{}（防抖 {} ms）",
                if auto_save.enabled {
                    "已启用"
                } else {
                    "已禁用"
                },
                auto_save.debounce_ms
            )
            .encode_utf16()
            .chain(Some(0))
            .collect();
            let auto_save_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 20.0,
            };
            target.DrawText(
                &auto_save_text,
                &label_format,
                &auto_save_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 30.0;

            // 失焦保存
            let focus_loss = if auto_save.focus_loss_save {
                "是"
            } else {
                "否"
            };
            let focus_text: Vec<u16> = format!("失焦自动保存：{}", focus_loss)
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let focus_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 20.0,
            };
            target.DrawText(
                &focus_text,
                &label_format,
                &focus_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 40.0;

            // 分隔线
            let sep_color = color_f(0.2, 0.2, 0.2, 1.0);
            let sep_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sep_color)
                .unwrap();
            let sep_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 1.0,
            };
            target.FillRectangle(&sep_rect, &sep_brush);
            cy += 16.0;

            // 提示
            let hint_text: Vec<u16> = "更多通用选项（主题切换、字体调整等）将在后续版本提供"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let hint_color = color_f(0.55, 0.55, 0.55, 1.0);
            let hint_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hint_color)
                .unwrap();
            let hint_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 18.0,
            };
            target.DrawText(
                &hint_text,
                &label_format,
                &hint_rect,
                &hint_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }
}
