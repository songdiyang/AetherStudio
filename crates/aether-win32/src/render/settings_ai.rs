use super::*;

impl EditorState {
    /// 渲染 AI 接口设置字段（provider / key / url / model / 保存 / 测试连接）
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_ai_settings_fields(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        width: f32,
        start_y: f32,
        margin: f32,
        label_h: f32,
        input_h: f32,
        gap: f32,
        label_format: IDWriteTextFormat,
        input_format: IDWriteTextFormat,
        button_format: IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        avail_h: f32,
    ) {
        // 居中表单容器：重定义局部 x/width/input_w 指向居中列，
        // 使后续所有字段自动居中并占满表单宽度（不再固定 460 贴左）。
        let content_left = x;
        let content_width = width;
        let form_w = width.min(560.0);
        let x = x + ((width - form_w) / 2.0).max(0.0);
        let width = form_w;
        let input_w = form_w;
        let scroll = self.settings_panel.scroll_offset;
        let mut cy = start_y - scroll;
        unsafe {
            // 裁剪到可视内容区：滚动后超出上下边界的内容不会绘制到标题栏/边界外
            let clip_rect = D2D_RECT_F {
                left: content_left,
                top: start_y,
                right: content_left + content_width,
                bottom: start_y + avail_h,
            };
            target.PushAxisAlignedClip(&clip_rect, D2D1_ANTIALIAS_MODE_ALIASED);

            // 信息卡片：左侧强调色条 + 说明文字（对比度提高，两行自适应）
            let card_h = 56.0_f32;
            let card_bg = color_f(0.16, 0.18, 0.22, 1.0);
            let card_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &card_bg)
                .unwrap();
            let card_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + input_w,
                bottom: cy + card_h,
            };
            target.FillRectangle(&card_rect, &card_bg_brush);
            let accent = color_f(0.0, 0.47, 0.83, 1.0);
            let accent_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &accent)
                .unwrap();
            let accent_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + 3.0,
                bottom: cy + card_h,
            };
            target.FillRectangle(&accent_rect, &accent_brush);
            let info_text = "配置 API 密钥后，AI 助手可在 Agent 模式下新建、修改、删除文件。建议先点击「测试连接」验证密钥有效性，再保存。";
            let info_color = color_f(0.72, 0.74, 0.78, 1.0);
            let info_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &info_color)
                .unwrap();
            let info_wide: Vec<u16> = info_text.encode_utf16().chain(Some(0)).collect();
            let info_rect = D2D_RECT_F {
                left: x + margin + 14.0,
                top: cy + 8.0,
                right: x + margin + input_w - 12.0,
                bottom: cy + card_h - 8.0,
            };
            target.DrawText(
                &info_wide,
                &label_format,
                &info_rect,
                &info_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += card_h + gap;

            // 当前编辑模型指示（AI 页编辑的是当前激活模型；在「模型」页可切换/新建）
            let model_hint = format!("正在编辑：{}", self.settings_panel.active_model_display());
            let hint_wide: Vec<u16> = model_hint.encode_utf16().chain(Some(0)).collect();
            let hint_color = color_f(0.60, 0.78, 0.95, 1.0);
            let hint_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hint_color)
                .unwrap();
            target.DrawText(
                &hint_wide,
                &label_format,
                &D2D_RECT_F {
                    left: x + margin,
                    top: cy,
                    right: x + margin + input_w,
                    bottom: cy + label_h,
                },
                &hint_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += label_h + 4.0;

            // 厂商下拉
            let provider_label_text = self.settings_panel.provider_display_label();
            let provider_items: Vec<String> =
                crate::settings::SettingsPanel::provider_dropdown_options()
                    .into_iter()
                    .map(|(_, name)| name.to_string())
                    .collect();
            cy = self.render_settings_dropdown(
                target,
                x,
                cy,
                margin,
                input_w,
                label_h,
                input_h,
                gap,
                "厂商",
                &provider_label_text,
                true,
                crate::settings::SettingsDropdownKind::Provider,
                provider_items,
                &label_format,
                &input_format,
                text_brush,
            );

            // API 密钥（必填）——带显示/隐藏切换
            let apikey_label: Vec<u16> = "API 密钥 *".encode_utf16().chain(Some(0)).collect();
            let apikey_label_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + label_h,
            };
            target.DrawText(
                &apikey_label,
                &label_format,
                &apikey_label_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += label_h;
            let apikey_focused =
                self.settings_panel.active_field == Some(crate::settings::SettingsField::ApiKey);
            let apikey_bg = color_f(0.18, 0.18, 0.18, 1.0);
            let apikey_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &apikey_bg)
                .unwrap();
            let apikey_border = if apikey_focused {
                color_f(0.0, 0.47, 0.83, 1.0)
            } else {
                color_f(0.3, 0.3, 0.3, 1.0)
            };
            let apikey_border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &apikey_border)
                .unwrap();
            let apikey_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + input_w,
                bottom: cy + input_h,
            };
            target.FillRectangle(&apikey_rect, &apikey_bg_brush);
            draw_input_borders(
                target,
                x + margin,
                cy,
                input_w,
                input_h,
                &apikey_border_brush,
            );
            // 眼睛按钮（右侧）：切换明文 / 掩码
            let eye_w = 34.0_f32;
            let eye_x = x + margin + input_w - eye_w;
            let eye_color = if self.settings_panel.hover_api_key_toggle {
                color_f(0.85, 0.85, 0.85, 1.0)
            } else {
                color_f(0.55, 0.55, 0.55, 1.0)
            };
            let eye_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &eye_color)
                .unwrap();
            // Segoe MDL2：0xE7B3 = 显示，0xED1A = 隐藏
            let eye_glyph = if self.settings_panel.show_api_key {
                "\u{ED1A}"
            } else {
                "\u{E7B3}"
            };
            let eye_wide: Vec<u16> = eye_glyph.encode_utf16().chain(Some(0)).collect();
            let eye_rect = D2D_RECT_F {
                left: eye_x,
                top: cy,
                right: eye_x + eye_w,
                bottom: cy + input_h,
            };
            target.DrawText(
                &eye_wide,
                &button_format,
                &eye_rect,
                &eye_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.api_key_toggle_region = Some((eye_x, cy, eye_w, input_h));
            // 密钥文本或占位符
            let key_empty = self.settings_panel.api_key.is_empty();
            let display_key = if key_empty {
                "sk-...（粘贴你的密钥）".to_string()
            } else {
                self.settings_panel.display_api_key()
            };
            let apikey_text: Vec<u16> = display_key.encode_utf16().chain(Some(0)).collect();
            let apikey_text_rect = D2D_RECT_F {
                left: x + margin + 8.0,
                top: cy,
                right: eye_x - 6.0,
                bottom: cy + input_h,
            };
            let key_text_color = if key_empty {
                color_f(0.5, 0.5, 0.5, 1.0)
            } else {
                color_f(0.9, 0.9, 0.9, 1.0)
            };
            let key_text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &key_text_color)
                .unwrap();
            target.DrawText(
                &apikey_text,
                &input_format,
                &apikey_text_rect,
                &key_text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_field_region(
                crate::settings::SettingsField::ApiKey,
                x + margin,
                cy,
                input_w - eye_w,
                input_h,
            );
            cy += input_h + gap;

            // 判断是否为自定义模式（预制模式自动填充 base_url 和 model）
            let is_custom = self.settings_panel.provider == "custom";

            // Base URL（仅自定义模式显示，预制模式自动填充）
            if is_custom {
                let baseurl_label: Vec<u16> = "基础地址".encode_utf16().chain(Some(0)).collect();
                let baseurl_label_rect = D2D_RECT_F {
                    left: x + margin,
                    top: cy,
                    right: x + width - margin,
                    bottom: cy + label_h,
                };
                target.DrawText(
                    &baseurl_label,
                    &label_format,
                    &baseurl_label_rect,
                    text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                cy += label_h;
                let baseurl_bg = color_f(0.18, 0.18, 0.18, 1.0);
                let baseurl_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &baseurl_bg)
                    .unwrap();
                let baseurl_border = if self.settings_panel.active_field
                    == Some(crate::settings::SettingsField::BaseUrl)
                {
                    color_f(0.0, 0.47, 0.83, 1.0)
                } else {
                    color_f(0.3, 0.3, 0.3, 1.0)
                };
                let baseurl_border_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &baseurl_border)
                    .unwrap();
                let baseurl_rect = D2D_RECT_F {
                    left: x + margin,
                    top: cy,
                    right: x + margin + input_w,
                    bottom: cy + input_h,
                };
                target.FillRectangle(&baseurl_rect, &baseurl_bg_brush);
                let border_top = D2D_RECT_F {
                    left: x + margin,
                    top: cy,
                    right: x + margin + input_w,
                    bottom: cy + 1.0,
                };
                let border_bottom = D2D_RECT_F {
                    left: x + margin,
                    top: cy + input_h - 1.0,
                    right: x + margin + input_w,
                    bottom: cy + input_h,
                };
                let border_left = D2D_RECT_F {
                    left: x + margin,
                    top: cy,
                    right: x + margin + 1.0,
                    bottom: cy + input_h,
                };
                let border_right = D2D_RECT_F {
                    left: x + margin + input_w - 1.0,
                    top: cy,
                    right: x + margin + input_w,
                    bottom: cy + input_h,
                };
                target.FillRectangle(&border_top, &baseurl_border_brush);
                target.FillRectangle(&border_bottom, &baseurl_border_brush);
                target.FillRectangle(&border_left, &baseurl_border_brush);
                target.FillRectangle(&border_right, &baseurl_border_brush);
                let baseurl_text: Vec<u16> = self
                    .settings_panel
                    .base_url
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let baseurl_text_rect = D2D_RECT_F {
                    left: x + margin + 6.0,
                    top: cy,
                    right: x + margin + input_w - 6.0,
                    bottom: cy + input_h,
                };
                target.DrawText(
                    &baseurl_text,
                    &input_format,
                    &baseurl_text_rect,
                    text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                self.settings_panel.add_field_region(
                    crate::settings::SettingsField::BaseUrl,
                    x + margin,
                    cy,
                    input_w,
                    input_h,
                );
                cy += input_h + gap;
            } // end if is_custom

            // Model 下拉（仅自定义模式显示，预制模式自动填充）
            if is_custom {
                let model_value = if self.settings_panel.model.is_empty() {
                    "选择模型".to_string()
                } else {
                    self.settings_panel.model.clone()
                };
                let model_items: Vec<String> = self
                    .settings_panel
                    .model_dropdown_options()
                    .into_iter()
                    .map(|(_id, name)| name)
                    .collect();
                cy = self.render_settings_dropdown(
                    target,
                    x,
                    cy,
                    margin,
                    input_w,
                    label_h,
                    input_h,
                    gap,
                    "模型",
                    &model_value,
                    true,
                    crate::settings::SettingsDropdownKind::Model,
                    model_items,
                    &label_format,
                    &input_format,
                    text_brush,
                );
            } // end if is_custom

            // 温度：滑块（0.0 - 2.0，步进 0.1）——比裸文本框更直观，且天然合法
            let temp_val = self
                .settings_panel
                .temperature
                .trim()
                .parse::<f32>()
                .unwrap_or(0.7)
                .clamp(0.0, 2.0);
            let temp_label_str = format!("温度  {:.1}   （越低越严谨，越高越发散）", temp_val);
            let temp_label: Vec<u16> = temp_label_str.encode_utf16().chain(Some(0)).collect();
            let temp_label_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + label_h,
            };
            target.DrawText(
                &temp_label,
                &label_format,
                &temp_label_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += label_h + 6.0;
            let track_h = 4.0_f32;
            let track_x = x + margin + 8.0;
            let track_w = (input_w - 16.0).max(1.0);
            let track_y = cy + 8.0;
            let ratio = (temp_val / 2.0).clamp(0.0, 1.0);
            let knob_cx = track_x + track_w * ratio;
            let track_bg = color_f(0.30, 0.30, 0.33, 1.0);
            let track_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &track_bg)
                .unwrap();
            target.FillRectangle(
                &D2D_RECT_F {
                    left: track_x,
                    top: track_y,
                    right: track_x + track_w,
                    bottom: track_y + track_h,
                },
                &track_bg_brush,
            );
            let track_fill = color_f(0.0, 0.47, 0.83, 1.0);
            let track_fill_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &track_fill)
                .unwrap();
            target.FillRectangle(
                &D2D_RECT_F {
                    left: track_x,
                    top: track_y,
                    right: knob_cx,
                    bottom: track_y + track_h,
                },
                &track_fill_brush,
            );
            let knob_r = 8.0_f32;
            let knob_color = color_f(0.95, 0.95, 0.95, 1.0);
            let knob_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &knob_color)
                .unwrap();
            target.FillEllipse(
                &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                    point: windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                        x: knob_cx,
                        y: track_y + track_h / 2.0,
                    },
                    radiusX: knob_r,
                    radiusY: knob_r,
                },
                &knob_brush,
            );
            self.settings_panel.temp_slider_region = Some((track_x, track_y, track_w, track_h));
            cy += 24.0 + gap;

            // Max Tokens（正整数）——带合法性校验
            let maxtok_valid = self.settings_panel.max_tokens_valid();
            let maxtok_label: Vec<u16> = "Max Tokens".encode_utf16().chain(Some(0)).collect();
            let maxtok_label_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + label_h,
            };
            target.DrawText(
                &maxtok_label,
                &label_format,
                &maxtok_label_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += label_h;
            let maxtok_focused =
                self.settings_panel.active_field == Some(crate::settings::SettingsField::MaxTokens);
            let maxtok_bg = color_f(0.18, 0.18, 0.18, 1.0);
            let maxtok_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &maxtok_bg)
                .unwrap();
            let maxtok_border = if !maxtok_valid {
                color_f(0.85, 0.30, 0.30, 1.0)
            } else if maxtok_focused {
                color_f(0.0, 0.47, 0.83, 1.0)
            } else {
                color_f(0.3, 0.3, 0.3, 1.0)
            };
            let maxtok_border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &maxtok_border)
                .unwrap();
            let maxtok_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + input_w,
                bottom: cy + input_h,
            };
            target.FillRectangle(&maxtok_rect, &maxtok_bg_brush);
            draw_input_borders(
                target,
                x + margin,
                cy,
                input_w,
                input_h,
                &maxtok_border_brush,
            );
            let maxtok_empty = self.settings_panel.max_tokens.is_empty();
            let maxtok_display = if maxtok_empty {
                "如 2048".to_string()
            } else {
                self.settings_panel.max_tokens.clone()
            };
            let maxtok_text: Vec<u16> = maxtok_display.encode_utf16().chain(Some(0)).collect();
            let maxtok_text_rect = D2D_RECT_F {
                left: x + margin + 8.0,
                top: cy,
                right: x + margin + input_w - 8.0,
                bottom: cy + input_h,
            };
            let maxtok_text_color = if maxtok_empty {
                color_f(0.5, 0.5, 0.5, 1.0)
            } else {
                color_f(0.9, 0.9, 0.9, 1.0)
            };
            let maxtok_text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &maxtok_text_color)
                .unwrap();
            target.DrawText(
                &maxtok_text,
                &input_format,
                &maxtok_text_rect,
                &maxtok_text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_field_region(
                crate::settings::SettingsField::MaxTokens,
                x + margin,
                cy,
                input_w,
                input_h,
            );
            cy += input_h;
            if !maxtok_valid {
                let warn_color = color_f(0.90, 0.45, 0.45, 1.0);
                let warn_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &warn_color)
                    .unwrap();
                let warn_text: Vec<u16> = "请输入 1 到 1000000 之间的整数"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let warn_rect = D2D_RECT_F {
                    left: x + margin,
                    top: cy + 2.0,
                    right: x + margin + input_w,
                    bottom: cy + 18.0,
                };
                target.DrawText(
                    &warn_text,
                    &label_format,
                    &warn_rect,
                    &warn_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                cy += 18.0;
            }
            cy += gap;

            // System Prompt
            let sysp_label: Vec<u16> = "系统提示词（可选）".encode_utf16().chain(Some(0)).collect();
            let sysp_label_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + label_h,
            };
            target.DrawText(
                &sysp_label,
                &label_format,
                &sysp_label_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += label_h;
            let sysp_bg = color_f(0.18, 0.18, 0.18, 1.0);
            let sysp_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sysp_bg)
                .unwrap();
            let sysp_border = if self.settings_panel.active_field
                == Some(crate::settings::SettingsField::SystemPrompt)
            {
                color_f(0.0, 0.47, 0.83, 1.0)
            } else {
                color_f(0.3, 0.3, 0.3, 1.0)
            };
            let sysp_border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sysp_border)
                .unwrap();
            let sysp_h = input_h * 2.0;
            let sysp_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + input_w,
                bottom: cy + sysp_h,
            };
            target.FillRectangle(&sysp_rect, &sysp_bg_brush);
            draw_input_borders(target, x + margin, cy, input_w, sysp_h, &sysp_border_brush);
            let sysp_display: String = if self.settings_panel.system_prompt.is_empty() {
                "（留空使用默认）".to_string()
            } else {
                self.settings_panel.system_prompt.clone()
            };
            let sysp_text: Vec<u16> = sysp_display.encode_utf16().chain(Some(0)).collect();
            let sysp_text_rect = D2D_RECT_F {
                left: x + margin + 6.0,
                top: cy + 4.0,
                right: x + margin + input_w - 6.0,
                bottom: cy + sysp_h - 4.0,
            };
            let sysp_text_color = if self.settings_panel.system_prompt.is_empty() {
                color_f(0.5, 0.5, 0.5, 1.0)
            } else {
                color_f(0.85, 0.85, 0.85, 1.0)
            };
            let sysp_text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sysp_text_color)
                .unwrap();
            target.DrawText(
                &sysp_text,
                &input_format,
                &sysp_text_rect,
                &sysp_text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_field_region(
                crate::settings::SettingsField::SystemPrompt,
                x + margin,
                cy,
                input_w,
                sysp_h,
            );
            cy += sysp_h + gap + 8.0;

            // 未保存更改提示
            if self.settings_panel.is_dirty() {
                let dot_color = color_f(0.95, 0.65, 0.20, 1.0);
                let dot_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &dot_color)
                    .unwrap();
                target.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                            x: x + margin + 4.0,
                            y: cy + 9.0,
                        },
                        radiusX: 4.0,
                        radiusY: 4.0,
                    },
                    &dot_brush,
                );
                let dirty_text: Vec<u16> = "有未保存的更改，点击「保存设置」生效"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let dirty_rect = D2D_RECT_F {
                    left: x + margin + 16.0,
                    top: cy,
                    right: x + margin + input_w,
                    bottom: cy + 18.0,
                };
                target.DrawText(
                    &dirty_text,
                    &label_format,
                    &dirty_rect,
                    &dot_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                cy += 24.0;
            }

            // 操作按钮：并排 [测试连接(次)] [保存设置(主)]
            let btn_h = 34.0_f32;
            let btn_gap = 12.0_f32;
            let is_testing = self.settings_panel.is_testing;
            let test_btn_w = (input_w * 0.4).clamp(120.0, (input_w - 120.0).max(120.0));
            let test_x = x + margin;
            let save_x = test_x + test_btn_w + btn_gap;
            let save_btn_w = (x + margin + input_w - save_x).max(1.0);

            // 测试连接（描边次按钮）
            let test_hover = self.settings_panel.hover_button
                == Some(crate::settings::SettingsButton::TestConnection);
            let test_bg = if is_testing {
                color_f(0.14, 0.14, 0.14, 1.0)
            } else if test_hover {
                color_f(0.25, 0.25, 0.25, 1.0)
            } else {
                color_f(0.18, 0.18, 0.18, 1.0)
            };
            let test_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &test_bg)
                .unwrap();
            let test_rect = D2D_RECT_F {
                left: test_x,
                top: cy,
                right: test_x + test_btn_w,
                bottom: cy + btn_h,
            };
            target.FillRectangle(&test_rect, &test_bg_brush);
            let test_border_color = color_f(0.3, 0.3, 0.3, 1.0);
            let test_border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &test_border_color)
                .unwrap();
            draw_input_borders(target, test_x, cy, test_btn_w, btn_h, &test_border_brush);
            let test_label = if is_testing {
                "测试中…"
            } else {
                "测试连接"
            };
            let test_text: Vec<u16> = test_label.encode_utf16().chain(Some(0)).collect();
            let test_text_color = if is_testing {
                color_f(0.6, 0.6, 0.6, 1.0)
            } else {
                color_f(0.85, 0.85, 0.85, 1.0)
            };
            let test_text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &test_text_color)
                .unwrap();
            target.DrawText(
                &test_text,
                &button_format,
                &test_rect,
                &test_text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_button_region(
                crate::settings::SettingsButton::TestConnection,
                test_x,
                cy,
                test_btn_w,
                btn_h,
            );

            // 保存设置（主按钮；保存时会自动先测试密钥有效性）
            let save_hover =
                self.settings_panel.hover_button == Some(crate::settings::SettingsButton::Save);
            let save_bg = if is_testing {
                color_f(0.0, 0.30, 0.52, 1.0)
            } else if save_hover {
                color_f(0.0, 0.55, 0.95, 1.0)
            } else {
                color_f(0.0, 0.47, 0.83, 1.0)
            };
            let save_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &save_bg)
                .unwrap();
            let save_rect = D2D_RECT_F {
                left: save_x,
                top: cy,
                right: save_x + save_btn_w,
                bottom: cy + btn_h,
            };
            target.FillRectangle(&save_rect, &save_bg_brush);
            let save_label = if is_testing {
                "验证并保存中…"
            } else {
                "保存设置"
            };
            let save_text: Vec<u16> = save_label.encode_utf16().chain(Some(0)).collect();
            let btn_text_color = color_f(1.0, 1.0, 1.0, 1.0);
            let btn_text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_text_color)
                .unwrap();
            target.DrawText(
                &save_text,
                &button_format,
                &save_rect,
                &btn_text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_button_region(
                crate::settings::SettingsButton::Save,
                save_x,
                cy,
                save_btn_w,
                btn_h,
            );
            cy += btn_h + 12.0;

            // 状态消息卡片
            if !self.settings_panel.test_status.is_empty() {
                let (status_bg, status_fg) = if self.settings_panel.is_testing {
                    (
                        color_f(0.20, 0.20, 0.12, 1.0),
                        color_f(0.90, 0.85, 0.40, 1.0),
                    )
                } else if self.settings_panel.test_status.starts_with('✓') {
                    (
                        color_f(0.12, 0.22, 0.14, 1.0),
                        color_f(0.40, 0.85, 0.45, 1.0),
                    )
                } else {
                    (
                        color_f(0.24, 0.14, 0.14, 1.0),
                        color_f(0.95, 0.50, 0.50, 1.0),
                    )
                };
                let status_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &status_bg)
                    .unwrap();
                let status_h = 34.0_f32;
                let status_rect = D2D_RECT_F {
                    left: x + margin,
                    top: cy,
                    right: x + margin + input_w,
                    bottom: cy + status_h,
                };
                target.FillRectangle(&status_rect, &status_bg_brush);
                let status_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &status_fg)
                    .unwrap();
                let status_format = self
                    .render_ctx
                    .text_format_cache
                    .get_format(
                        12.0,
                        DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                        DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                        DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                    )
                    .unwrap();
                let status_text: Vec<u16> = self
                    .settings_panel
                    .test_status
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let status_text_rect = D2D_RECT_F {
                    left: x + margin + 10.0,
                    top: cy,
                    right: x + margin + input_w - 10.0,
                    bottom: cy + status_h,
                };
                target.DrawText(
                    &status_text,
                    &status_format,
                    &status_text_rect,
                    &status_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                cy += status_h;
            }
            cy += 8.0;

            // 结束裁剪，计算可滚动高度并绘制滚动条
            target.PopAxisAlignedClip();
            let total_content = (cy + scroll) - start_y;
            let max_scroll = (total_content - avail_h).max(0.0);
            self.settings_panel.content_height = max_scroll;
            if self.settings_panel.scroll_offset > max_scroll {
                self.settings_panel.scroll_offset = max_scroll;
            }
            if max_scroll > 0.0 && total_content > 0.0 {
                let sb_w = 6.0_f32;
                let sb_x = content_left + content_width - sb_w - 2.0;
                let visible_ratio = (avail_h / total_content).clamp(0.1, 1.0);
                let thumb_h = (avail_h * visible_ratio).max(30.0);
                let scroll_ratio = (self.settings_panel.scroll_offset / max_scroll).clamp(0.0, 1.0);
                let thumb_y = start_y + (avail_h - thumb_h) * scroll_ratio;
                let thumb_color = color_f(0.4, 0.4, 0.45, 1.0);
                let thumb_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &thumb_color)
                    .unwrap();
                target.FillRectangle(
                    &D2D_RECT_F {
                        left: sb_x,
                        top: thumb_y,
                        right: sb_x + sb_w,
                        bottom: thumb_y + thumb_h,
                    },
                    &thumb_brush,
                );
            }
        }
    }

    /// 渲染设置面板主编辑区的下拉字段（厂商 / 模型）
    /// 下拉项列表由调用方传入（settings_panel 上有多个下拉，items 集合各异）。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_settings_dropdown(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        cy: f32,
        margin: f32,
        input_w: f32,
        label_h: f32,
        input_h: f32,
        gap: f32,
        label: &str,
        value: &str,
        required: bool,
        kind: crate::settings::SettingsDropdownKind,
        items: Vec<String>,
        label_format: &IDWriteTextFormat,
        input_format: &IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) -> f32 {
        unsafe {
            let is_open = self.settings_panel.open_dropdown == Some(kind);
            // 标签
            let label_color = if required {
                color_f(0.92, 0.30, 0.30, 1.0)
            } else {
                color_f(0.85, 0.85, 0.85, 1.0)
            };
            let label_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &label_color)
                .unwrap();
            let prefix: Vec<u16> = "*".encode_utf16().chain(Some(0)).collect();
            let label_text: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
            let label_y = cy;
            if required {
                target.DrawText(
                    &prefix,
                    label_format,
                    &D2D_RECT_F {
                        left: x + margin,
                        top: label_y,
                        right: x + margin + 10.0,
                        bottom: label_y + label_h,
                    },
                    &label_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
            target.DrawText(
                &label_text,
                label_format,
                &D2D_RECT_F {
                    left: x + margin + (if required { 12.0 } else { 0.0 }),
                    top: label_y,
                    right: x + margin + input_w,
                    bottom: label_y + label_h,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let cy = cy + label_h + 4.0;

            // 下拉框背景
            let input_bg = color_f(0.18, 0.18, 0.18, 1.0);
            let input_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &input_bg)
                .unwrap();
            let input_border = if is_open {
                color_f(0.0, 0.47, 0.83, 1.0)
            } else {
                color_f(0.3, 0.3, 0.3, 1.0)
            };
            let input_border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &input_border)
                .unwrap();
            let input_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + input_w,
                bottom: cy + input_h,
            };
            target.FillRectangle(&input_rect, &input_bg_brush);
            for (b_left, b_top, b_right, b_bottom) in [
                (x + margin, cy, x + margin + input_w, cy + 1.0),
                (
                    x + margin,
                    cy + input_h - 1.0,
                    x + margin + input_w,
                    cy + input_h,
                ),
                (x + margin, cy, x + margin + 1.0, cy + input_h),
                (
                    x + margin + input_w - 1.0,
                    cy,
                    x + margin + input_w,
                    cy + input_h,
                ),
            ] {
                target.FillRectangle(
                    &D2D_RECT_F {
                        left: b_left,
                        top: b_top,
                        right: b_right,
                        bottom: b_bottom,
                    },
                    &input_border_brush,
                );
            }
            // 文本
            let value_color = if value.is_empty() || value.starts_with("选择") {
                color_f(0.55, 0.55, 0.55, 1.0)
            } else {
                color_f(0.95, 0.95, 0.95, 1.0)
            };
            let value_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &value_color)
                .unwrap();
            let value_wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &value_wide,
                input_format,
                &D2D_RECT_F {
                    left: x + margin + 10.0,
                    top: cy,
                    right: x + margin + input_w - 28.0,
                    bottom: cy + input_h,
                },
                &value_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            // 箭头
            let arrow = if is_open { "▴" } else { "▾" };
            let arrow_wide: Vec<u16> = arrow.encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &arrow_wide,
                input_format,
                &D2D_RECT_F {
                    left: x + margin + input_w - 24.0,
                    top: cy,
                    right: x + margin + input_w - 6.0,
                    bottom: cy + input_h,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 保存触发区域
            self.settings_panel.dropdown_trigger_regions.push((
                kind,
                x + margin,
                cy,
                input_w,
                input_h,
            ));

            let mut next_cy = cy + input_h + gap;

            // 如果展开，渲染下拉项
            if is_open {
                let item_h = 28.0f32;
                let item_bg = color_f(0.22, 0.22, 0.24, 1.0);
                let item_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &item_bg)
                    .unwrap();
                let hover_color = color_f(0.30, 0.30, 0.32, 1.0);
                let hover_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &hover_color)
                    .unwrap();
                let selected_color = color_f(0.20, 0.20, 0.22, 1.0);
                let selected_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &selected_color)
                    .unwrap();
                for (i, item_label) in items.iter().enumerate() {
                    let iy = next_cy + i as f32 * item_h;
                    let is_hover = self.settings_panel.hover_dropdown == Some(kind)
                        && self.settings_panel.hover_dropdown_index == Some(i);
                    let is_selected = match kind {
                        crate::settings::SettingsDropdownKind::Provider => {
                            // dropdown_items() 顺序：DeepSeek, Kimi, 自定义
                            matches!(
                                (self.settings_panel.current_provider_button(), i),
                                (Some(ProviderTemplateButton::DeepSeek), 0)
                                    | (Some(ProviderTemplateButton::Kimi), 1)
                                    | (Some(ProviderTemplateButton::Custom), 2)
                            )
                        }
                        crate::settings::SettingsDropdownKind::Model => {
                            self.settings_panel.model == *item_label
                        }
                    };
                    let brush = if is_hover {
                        &hover_brush
                    } else if is_selected {
                        &selected_brush
                    } else {
                        &item_bg_brush
                    };
                    let item_rect = D2D_RECT_F {
                        left: x + margin,
                        top: iy,
                        right: x + margin + input_w,
                        bottom: iy + item_h,
                    };
                    target.FillRectangle(&item_rect, brush);
                    let item_wide: Vec<u16> = item_label.encode_utf16().chain(Some(0)).collect();
                    target.DrawText(
                        &item_wide,
                        input_format,
                        &D2D_RECT_F {
                            left: x + margin + 14.0,
                            top: iy,
                            right: x + margin + input_w - 14.0,
                            bottom: iy + item_h,
                        },
                        text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    self.settings_panel.dropdown_item_regions.push((
                        kind,
                        i,
                        x + margin,
                        iy,
                        input_w,
                        item_h,
                    ));
                }
                next_cy += items.len() as f32 * item_h + gap;
            }
            next_cy
        }
    }
}
