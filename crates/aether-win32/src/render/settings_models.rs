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
        _title_format: IDWriteTextFormat,
        _text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        let mut cy = start_y;
        unsafe {
            // 卡片布局最小宽度：当 AI 侧边栏过宽、设置区被压窄时，卡片不再继续压缩，
            // 而是保持该最小宽度并向右溢出；溢出部分会被随后绘制的 AI 侧边栏覆盖，
            // 从而避免模型卡片被越挤越扁、文字互相重叠。
            let eff_width = width.max(500.0);
            let content_w = eff_width - margin * 2.0;

            // 说明
            let info_text =
                "已添加的模型（点击卡片切换当前使用的模型，可编辑 / 删除）；点击下方「添加模型」新增配置。";
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
                right: x + eff_width - margin,
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
            let card_h = 84.0f32;
            let card_gap = 10.0f32;
            let _card_radius = 6.0f32;
            let models_clone: Vec<_> = self.settings_panel.models.to_vec();
            let active_id = self.settings_panel.active_model_id.clone();

            // "当前使用"标记用的右对齐文本格式（放在名称行右侧，避免与变长名称重叠）
            let cur_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_TRAILING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();
            let name_color = color_f(0.90, 0.90, 0.90, 1.0);
            let name_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &name_color)
                .unwrap();
            let active_name_color = color_f(0.45, 0.74, 1.0, 1.0);
            let active_name_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &active_name_color)
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

            // 空列表提示
            if models_clone.is_empty() {
                let empty_text: Vec<u16> = "还没有模型。点击下方「＋ 添加模型」创建一个模型配置。"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                target.DrawText(
                    &empty_text,
                    &label_format,
                    &D2D_RECT_F {
                        left: x + margin,
                        top: cy + 8.0,
                        right: x + eff_width - margin,
                        bottom: cy + 32.0,
                    },
                    &desc_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                cy += 40.0;
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
                    right: x + eff_width - margin,
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
                let border_w = if is_active { 2.0 } else { 1.0 };
                target.DrawRectangle(&card_rect, &card_border_brush, border_w, None);
                // 激活卡片左侧强调色条
                if is_active {
                    let accent_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &color_f(0.0, 0.55, 0.95, 1.0))
                        .unwrap();
                    target.FillRectangle(
                        &D2D_RECT_F {
                            left: x + margin,
                            top: cy,
                            right: x + margin + 4.0,
                            bottom: cy + card_h,
                        },
                        &accent_brush,
                    );
                }

                // 预计算右侧操作区几何（先定位，便于左侧信息区自动避让，避免重叠）
                let row_cy = cy + card_h / 2.0;
                let op_right = x + eff_width - margin - 16.0;
                let toggle_w = 46.0f32;
                let toggle_h = 24.0f32;
                let toggle_x = op_right - toggle_w;
                let state_w = 44.0f32;
                let state_x = toggle_x - 8.0 - state_w;
                let del_w = 56.0f32;
                let del_x = state_x - 14.0 - del_w;
                let edit_w = 56.0f32;
                let edit_x = del_x - 10.0 - edit_w;
                let act_h = 28.0f32;
                let act_y = row_cy - act_h / 2.0;
                let toggle_y = row_cy - toggle_h / 2.0;
                // 左侧信息区右边界：避让右侧操作区
                let left_area_right = (edit_x - 16.0).max(x + margin + 130.0);

                // 模型名称（左侧，第一行；激活时用蓝色）
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
                        left: x + margin + 20.0,
                        top: cy + 16.0,
                        right: left_area_right,
                        bottom: cy + 40.0,
                    },
                    if is_active {
                        &active_name_brush
                    } else {
                        &name_brush
                    },
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                // "● 当前使用"标记（名称行右侧、右对齐，避免与变长名称重叠）
                if is_active {
                    let cur_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &color_f(0.40, 0.72, 1.0, 1.0))
                        .unwrap();
                    let cur_wide: Vec<u16> = "● 当前使用".encode_utf16().chain(Some(0)).collect();
                    target.DrawText(
                        &cur_wide,
                        &cur_format,
                        &D2D_RECT_F {
                            left: x + margin + 20.0,
                            top: cy + 16.0,
                            right: left_area_right,
                            bottom: cy + 40.0,
                        },
                        &cur_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }

                // 服务商 · 模型（第二行，灰色副信息）
                let provider_label = match model.provider.as_str() {
                    "deepseek" => "DeepSeek",
                    "kimi" => "Kimi",
                    _ => model.provider.as_str(),
                };
                let sub_line = if model.name.is_empty() {
                    format!("{} · （未设置模型）", provider_label)
                } else {
                    format!("{} · {}", provider_label, model.name)
                };
                let sub_wide: Vec<u16> = sub_line.encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &sub_wide,
                    &label_format,
                    &D2D_RECT_F {
                        left: x + margin + 20.0,
                        top: cy + 48.0,
                        right: left_area_right,
                        bottom: cy + 70.0,
                    },
                    &provider_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // ---- 右侧操作区：编辑 / 删除 文本按钮 + 启用开关（几何已在上方预计算）----

                // 启用/禁用开关（最右侧）
                let toggle_rect = D2D_RECT_F {
                    left: toggle_x,
                    top: toggle_y,
                    right: toggle_x + toggle_w,
                    bottom: toggle_y + toggle_h,
                };
                let toggle_bg = if model.enabled {
                    color_f(0.20, 0.72, 0.40, 1.0)
                } else {
                    color_f(0.34, 0.34, 0.37, 1.0)
                };
                let toggle_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &toggle_bg)
                    .unwrap();
                target.FillRectangle(&toggle_rect, &toggle_bg_brush);
                let knob_r = 8.0f32;
                let knob_y = toggle_y + toggle_h / 2.0;
                let knob_x = if model.enabled {
                    toggle_x + toggle_w - knob_r - 4.0
                } else {
                    toggle_x + knob_r + 4.0
                };
                let knob_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &color_f(1.0, 1.0, 1.0, 1.0))
                    .unwrap();
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
                    toggle_y,
                    toggle_w,
                    toggle_h,
                );
                // 开关状态文字（开关左侧）
                let state_text = if model.enabled {
                    "已启用"
                } else {
                    "已停用"
                };
                let state_color = if model.enabled {
                    color_f(0.42, 0.80, 0.54, 1.0)
                } else {
                    color_f(0.60, 0.60, 0.62, 1.0)
                };
                let state_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &state_color)
                    .unwrap();
                let state_wide: Vec<u16> = state_text.encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &state_wide,
                    &label_format,
                    &D2D_RECT_F {
                        left: state_x,
                        top: row_cy - 10.0,
                        right: state_x + state_w,
                        bottom: row_cy + 10.0,
                    },
                    &state_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // 删除按钮（描边文本，悬停变红）
                let is_del_hover = self.settings_panel.hover_model_button
                    == Some(crate::settings::ModelButton::Delete)
                    && self.settings_panel.hover_model_button_id.as_ref() == Some(&model.id);
                let del_rect = D2D_RECT_F {
                    left: del_x,
                    top: act_y,
                    right: del_x + del_w,
                    bottom: act_y + act_h,
                };
                let del_bg = if is_del_hover {
                    color_f(0.34, 0.16, 0.16, 1.0)
                } else {
                    color_f(0.16, 0.16, 0.18, 1.0)
                };
                let del_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &del_bg)
                    .unwrap();
                target.FillRectangle(&del_rect, &del_bg_brush);
                let del_border = if is_del_hover {
                    color_f(0.78, 0.36, 0.36, 1.0)
                } else {
                    color_f(0.40, 0.32, 0.32, 1.0)
                };
                let del_border_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &del_border)
                    .unwrap();
                target.DrawRectangle(&del_rect, &del_border_brush, 1.0, None);
                let del_txt_color = if is_del_hover {
                    color_f(1.0, 0.74, 0.74, 1.0)
                } else {
                    color_f(0.84, 0.62, 0.62, 1.0)
                };
                let del_txt_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &del_txt_color)
                    .unwrap();
                let del_text: Vec<u16> = "删除".encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &del_text,
                    &button_format,
                    &del_rect,
                    &del_txt_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                self.settings_panel.add_model_button_region(
                    crate::settings::ModelButton::Delete,
                    del_x,
                    act_y,
                    del_w,
                    act_h,
                );

                // 编辑按钮（描边文本，悬停高亮蓝）
                let is_edit_hover = self.settings_panel.hover_model_button
                    == Some(crate::settings::ModelButton::Edit)
                    && self.settings_panel.hover_model_button_id.as_ref() == Some(&model.id);
                let edit_rect = D2D_RECT_F {
                    left: edit_x,
                    top: act_y,
                    right: edit_x + edit_w,
                    bottom: act_y + act_h,
                };
                let edit_bg = if is_edit_hover {
                    color_f(0.18, 0.28, 0.40, 1.0)
                } else {
                    color_f(0.16, 0.16, 0.18, 1.0)
                };
                let edit_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &edit_bg)
                    .unwrap();
                target.FillRectangle(&edit_rect, &edit_bg_brush);
                let edit_border = if is_edit_hover {
                    color_f(0.28, 0.56, 0.86, 1.0)
                } else {
                    color_f(0.34, 0.34, 0.37, 1.0)
                };
                let edit_border_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &edit_border)
                    .unwrap();
                target.DrawRectangle(&edit_rect, &edit_border_brush, 1.0, None);
                let edit_txt_color = if is_edit_hover {
                    color_f(0.82, 0.90, 1.0, 1.0)
                } else {
                    color_f(0.84, 0.86, 0.90, 1.0)
                };
                let edit_txt_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &edit_txt_color)
                    .unwrap();
                let edit_text: Vec<u16> = "编辑".encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &edit_text,
                    &button_format,
                    &edit_rect,
                    &edit_txt_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                self.settings_panel.add_model_button_region(
                    crate::settings::ModelButton::Edit,
                    edit_x,
                    act_y,
                    edit_w,
                    act_h,
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

            // 「＋ 添加模型」按钮：位于模型列表下方（主强调色）
            let add_btn_w = 132.0f32;
            let add_btn_h = 34.0f32;
            let add_btn_x = x + margin;
            let add_btn_y = cy + 4.0;
            let is_add_hover =
                self.settings_panel.hover_model_button == Some(crate::settings::ModelButton::Add);
            let add_bg = if is_add_hover {
                color_f(0.0, 0.55, 0.95, 1.0)
            } else {
                color_f(0.0, 0.47, 0.83, 1.0)
            };
            let add_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &add_bg)
                .unwrap();
            let add_rect = D2D_RECT_F {
                left: add_btn_x,
                top: add_btn_y,
                right: add_btn_x + add_btn_w,
                bottom: add_btn_y + add_btn_h,
            };
            target.FillRectangle(&add_rect, &add_bg_brush);
            let add_text_color = color_f(1.0, 1.0, 1.0, 1.0);
            let add_text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &add_text_color)
                .unwrap();
            let add_text: Vec<u16> = "＋ 添加模型".encode_utf16().chain(Some(0)).collect();
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
                add_btn_y,
                add_btn_w,
                add_btn_h,
            );
        }
    }

    /// 渲染模型编辑视图顶部的「← 返回模型列表」按钮，返回其占用高度。
    pub(super) fn render_model_edit_back_button(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        button_format: &IDWriteTextFormat,
    ) -> f32 {
        unsafe {
            let btn_w = 132.0f32;
            let btn_h = 28.0f32;
            let bg = color_f(0.18, 0.18, 0.20, 1.0);
            let bg_brush = self.render_ctx.brush_cache.get_brush(target, &bg).unwrap();
            let rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + btn_w,
                bottom: y + btn_h,
            };
            target.FillRectangle(&rect, &bg_brush);
            let border = color_f(0.35, 0.35, 0.37, 1.0);
            let border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &border)
                .unwrap();
            target.DrawRectangle(&rect, &border_brush, 1.0, None);
            let tc = color_f(0.85, 0.85, 0.85, 1.0);
            let tb = self.render_ctx.brush_cache.get_brush(target, &tc).unwrap();
            let txt: Vec<u16> = "← 返回模型列表".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &txt,
                button_format,
                &rect,
                &tb,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_button_region(
                crate::settings::SettingsButton::BackToModels,
                x,
                y,
                btn_w,
                btn_h,
            );
            btn_h
        }
    }
}
