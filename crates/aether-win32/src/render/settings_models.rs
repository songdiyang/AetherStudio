use super::*;

impl EditorState {
    /// 渲染模型管理区（标题、说明、添加模型按钮、已有模型列表）
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_models_management(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        width: f32,
        start_y: f32,
        margin: f32,
        label_format: IDWriteTextFormat,
        input_format: IDWriteTextFormat,
        button_format: IDWriteTextFormat,
        title_format: IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        let mut cy = start_y;
        unsafe {
            let content_w = width - margin * 2.0;

            // 标题行：左侧"模型"，右侧"+ 添加"按钮
            let title_text = "模型";
            let title_wide: Vec<u16> = title_text.encode_utf16().chain(Some(0)).collect();
            let title_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin - 80.0,
                bottom: cy + 28.0,
            };
            target.DrawText(
                &title_wide,
                &title_format,
                &title_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // + 添加 按钮（右上角）
            let add_btn_w = 80.0f32;
            let add_btn_h = 28.0f32;
            let add_btn_x = x + width - margin - add_btn_w;
            let is_add_hover =
                self.settings_panel.hover_model_button == Some(crate::settings::ModelButton::Add);
            let add_bg = if is_add_hover {
                color_f(0.30, 0.30, 0.32, 1.0)
            } else {
                color_f(0.22, 0.22, 0.24, 1.0)
            };
            let add_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &add_bg)
                .unwrap();
            let add_rect = D2D_RECT_F {
                left: add_btn_x,
                top: cy,
                right: add_btn_x + add_btn_w,
                bottom: cy + add_btn_h,
            };
            target.FillRectangle(&add_rect, &add_bg_brush);
            // 按钮边框
            let add_border_color = color_f(0.35, 0.35, 0.37, 1.0);
            let add_border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &add_border_color)
                .unwrap();
            target.DrawRectangle(&add_rect, &add_border_brush, 1.0, None);
            let add_text_color = color_f(0.85, 0.85, 0.85, 1.0);
            let add_text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &add_text_color)
                .unwrap();
            let add_text: Vec<u16> = "+ 新建".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &add_text,
                &button_format,
                &add_rect,
                &add_text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_model_button_region(
                crate::settings::ModelButton::Add,
                add_btn_x,
                cy,
                add_btn_w,
                add_btn_h,
            );
            cy += 36.0;

            // 说明
            let info_text = "管理多个模型配置；点击卡片切换当前使用的模型，可新建 / 编辑 / 删除。";
            let info_color = color_f(0.55, 0.55, 0.55, 1.0);
            let info_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &info_color)
                .unwrap();
            let info_wide: Vec<u16> = info_text.encode_utf16().chain(Some(0)).collect();
            let info_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 20.0,
            };
            target.DrawText(
                &info_wide,
                &label_format,
                &info_rect,
                &info_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 28.0;

            // 卡片式模型列表
            let card_h = 72.0f32;
            let card_gap = 8.0f32;
            let _card_radius = 6.0f32;
            let models_clone: Vec<_> = self.settings_panel.models.to_vec();
            let active_id = self.settings_panel.active_model_id.clone();

            let name_color = color_f(0.90, 0.90, 0.90, 1.0);
            let name_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &name_color)
                .unwrap();
            let desc_color = color_f(0.55, 0.55, 0.55, 1.0);
            let desc_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &desc_color)
                .unwrap();
            let provider_color = color_f(0.65, 0.65, 0.65, 1.0);
            let provider_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &provider_color)
                .unwrap();
            let op_hover_color = color_f(0.70, 0.70, 0.70, 1.0);
            let op_hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &op_hover_color)
                .unwrap();
            let op_normal_color = color_f(0.45, 0.45, 0.45, 1.0);
            let op_normal_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &op_normal_color)
                .unwrap();

            // 空列表提示
            if models_clone.is_empty() {
                let empty_text: Vec<u16> = "还没有模型。点击右上角「+ 新建」创建一个模型配置。"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                target.DrawText(
                    &empty_text,
                    &label_format,
                    &D2D_RECT_F {
                        left: x + margin,
                        top: cy + 8.0,
                        right: x + width - margin,
                        bottom: cy + 32.0,
                    },
                    &desc_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            for (i, model) in models_clone.iter().enumerate() {
                let is_hover = self.settings_panel.hover_model_id.as_ref() == Some(&model.id);
                let is_active = active_id.as_deref() == Some(model.id.as_str());
                let card_bg = if is_hover {
                    color_f(0.22, 0.22, 0.24, 1.0)
                } else {
                    color_f(0.18, 0.18, 0.20, 1.0)
                };
                let card_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &card_bg)
                    .unwrap();
                let card_rect = D2D_RECT_F {
                    left: x + margin,
                    top: cy,
                    right: x + width - margin,
                    bottom: cy + card_h,
                };
                target.FillRectangle(&card_rect, &card_bg_brush);
                // 卡片边框（激活模型高亮）
                let card_border = if is_active {
                    color_f(0.0, 0.47, 0.83, 1.0)
                } else {
                    color_f(0.28, 0.28, 0.30, 1.0)
                };
                let card_border_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &card_border)
                    .unwrap();
                target.DrawRectangle(&card_rect, &card_border_brush, 1.0, None);

                // 模型名称（左侧，大字体）
                let display_name = if model.display_name.is_empty() {
                    &model.name
                } else {
                    &model.display_name
                };
                let name_wide: Vec<u16> = display_name.encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &name_wide,
                    &input_format,
                    &D2D_RECT_F {
                        left: x + margin + 16.0,
                        top: cy + 12.0,
                        right: x + margin + content_w * 0.5,
                        bottom: cy + 32.0,
                    },
                    &name_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // 激活徽标
                if is_active {
                    let badge: Vec<u16> = "● 当前".encode_utf16().chain(Some(0)).collect();
                    let badge_color = color_f(0.0, 0.65, 0.95, 1.0);
                    let badge_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &badge_color)
                        .unwrap();
                    target.DrawText(
                        &badge,
                        &label_format,
                        &D2D_RECT_F {
                            left: x + margin + content_w * 0.5,
                            top: cy + 12.0,
                            right: x + margin + content_w * 0.5 + 90.0,
                            bottom: cy + 32.0,
                        },
                        &badge_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }

                // 描述/标签
                if !model.description.is_empty() {
                    let desc_wide: Vec<u16> =
                        model.description.encode_utf16().chain(Some(0)).collect();
                    target.DrawText(
                        &desc_wide,
                        &label_format,
                        &D2D_RECT_F {
                            left: x + margin + 16.0,
                            top: cy + 36.0,
                            right: x + margin + content_w * 0.5,
                            bottom: cy + 54.0,
                        },
                        &desc_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }

                // 服务商名称（左下角）
                let provider_label = match model.provider.as_str() {
                    "deepseek" => "DeepSeek",
                    "kimi" => "Kimi",
                    _ => &model.provider,
                };
                let provider_wide: Vec<u16> =
                    provider_label.encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &provider_wide,
                    &label_format,
                    &D2D_RECT_F {
                        left: x + margin + 16.0,
                        top: cy + 50.0,
                        right: x + margin + 120.0,
                        bottom: cy + 68.0,
                    },
                    &provider_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // 右侧操作区：编辑 / 删除 / 开关
                let op_y = cy + (card_h - 24.0) / 2.0;
                let op_right = x + width - margin - 16.0;

                // 启用/禁用开关（最右侧）
                let toggle_w = 40.0f32;
                let toggle_h = 22.0f32;
                let toggle_x = op_right - toggle_w;
                let toggle_rect = D2D_RECT_F {
                    left: toggle_x,
                    top: op_y,
                    right: toggle_x + toggle_w,
                    bottom: op_y + toggle_h,
                };
                let toggle_bg = if model.enabled {
                    color_f(0.20, 0.75, 0.40, 1.0) // 绿色
                } else {
                    color_f(0.35, 0.35, 0.37, 1.0) // 灰色
                };
                let toggle_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &toggle_bg)
                    .unwrap();
                target.FillRectangle(&toggle_rect, &toggle_bg_brush);
                // 开关圆角边框
                target.DrawRectangle(&toggle_rect, &toggle_bg_brush, 1.0, None);
                // 开关滑块
                let knob_r = 8.0f32;
                let knob_y = op_y + toggle_h / 2.0;
                let knob_x = if model.enabled {
                    toggle_x + toggle_w - knob_r - 4.0
                } else {
                    toggle_x + knob_r + 4.0
                };
                let knob_color = color_f(1.0, 1.0, 1.0, 1.0);
                let knob_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &knob_color)
                    .unwrap();
                let _knob_rect = D2D_RECT_F {
                    left: knob_x - knob_r,
                    top: knob_y - knob_r,
                    right: knob_x + knob_r,
                    bottom: knob_y + knob_r,
                };
                target.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                            x: knob_x,
                            y: knob_y,
                        },
                        radiusX: knob_r,
                        radiusY: knob_r,
                    },
                    &knob_brush,
                );
                self.settings_panel.add_model_button_region(
                    crate::settings::ModelButton::ToggleEnabled,
                    toggle_x,
                    op_y,
                    toggle_w,
                    toggle_h,
                );

                // 删除按钮
                let del_w = 28.0f32;
                let del_x = toggle_x - del_w - 12.0;
                let is_del_hover = self.settings_panel.hover_model_button
                    == Some(crate::settings::ModelButton::Delete)
                    && self.settings_panel.hover_model_button_id.as_ref() == Some(&model.id);
                let del_text: Vec<u16> = "\u{E74D}".encode_utf16().chain(Some(0)).collect(); // 删除图标
                target.DrawText(
                    &del_text,
                    &button_format,
                    &D2D_RECT_F {
                        left: del_x,
                        top: op_y,
                        right: del_x + del_w,
                        bottom: op_y + 24.0,
                    },
                    if is_del_hover {
                        &op_hover_brush
                    } else {
                        &op_normal_brush
                    },
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                self.settings_panel.add_model_button_region(
                    crate::settings::ModelButton::Delete,
                    del_x,
                    op_y,
                    del_w,
                    24.0,
                );

                // 编辑按钮
                let edit_w = 28.0f32;
                let edit_x = del_x - edit_w - 8.0;
                let is_edit_hover = self.settings_panel.hover_model_button
                    == Some(crate::settings::ModelButton::Edit)
                    && self.settings_panel.hover_model_button_id.as_ref() == Some(&model.id);
                let edit_text: Vec<u16> = "\u{E70F}".encode_utf16().chain(Some(0)).collect(); // 编辑图标
                target.DrawText(
                    &edit_text,
                    &button_format,
                    &D2D_RECT_F {
                        left: edit_x,
                        top: op_y,
                        right: edit_x + edit_w,
                        bottom: op_y + 24.0,
                    },
                    if is_edit_hover {
                        &op_hover_brush
                    } else {
                        &op_normal_brush
                    },
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                self.settings_panel.add_model_button_region(
                    crate::settings::ModelButton::Edit,
                    edit_x,
                    op_y,
                    edit_w,
                    24.0,
                );

                // 注册整个卡片区域为模型项区域（用于悬停检测）
                self.settings_panel.add_model_item_region(
                    model.id.clone(),
                    x + margin,
                    cy,
                    content_w,
                    card_h,
                );

                cy += card_h + card_gap;
                if i >= 9 {
                    // 最多显示 10 张卡片
                    break;
                }
            }
        }
    }
}
