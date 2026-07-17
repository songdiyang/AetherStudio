use super::*;

impl EditorState {
    pub(super) fn render_ai_assistant_sidebar(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        unsafe {
            // 防御性检查：面板太小则跳过渲染
            if width < 20.0 || height < 20.0 {
                return;
            }

            // 确保矢量图标几何已创建（AI 面板工具栏图标）
            self.icons.ensure_created_from_target(target);

            // 安全获取文本格式，失败时跳过渲染
            let bold_format = match self.render_ctx.text_format_cache.get_format(
                13.0,
                DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
            ) {
                Ok(f) => f,
                Err(_) => return,
            };
            let msg_format = match self.render_ctx.text_format_cache.get_format(
                11.0,
                DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
            ) {
                Ok(f) => f,
                Err(_) => return,
            };
            let small_format = match self.render_ctx.text_format_cache.get_format(
                10.0,
                DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
            ) {
                Ok(f) => f,
                Err(_) => return,
            };

            // 安全获取画刷，失败时返回
            let title_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &color_f(0.9, 0.9, 0.9, 1.0))
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let dim_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &color_f(0.5, 0.5, 0.5, 1.0))
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let user_bg_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &color_f(0.18, 0.18, 0.2, 1.0))
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let assistant_bg_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &color_f(0.15, 0.15, 0.17, 1.0))
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let input_bg_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &color_f(0.12, 0.12, 0.12, 1.0))
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let sep_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &color_f(0.2, 0.2, 0.2, 1.0))
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let accent_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &color_f(0.0, 0.47, 0.83, 1.0))
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let green_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &color_f(0.2, 0.8, 0.3, 1.0))
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let yellow_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &color_f(0.9, 0.7, 0.2, 1.0))
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let code_bg_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &color_f(0.08, 0.08, 0.09, 1.0))
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let code_text_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &color_f(0.85, 0.85, 0.85, 1.0))
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let white_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &color_f(1.0, 1.0, 1.0, 1.0))
            {
                Ok(b) => b,
                Err(_) => return,
            };

            let margin = 10.0f32;
            let mut cy = y + margin;

            // ===== 标题区域 =====
            let title: Vec<u16> = "AI 助手".encode_utf16().chain(Some(0)).collect();
            let title_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 22.0,
            };
            target.DrawText(
                &title,
                &bold_format,
                &title_rect,
                &title_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 26.0;

            // 分隔线
            let sep_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 1.0,
            };
            target.FillRectangle(&sep_rect, &sep_brush);
            cy += 10.0;

            // 清空命中区域（每帧重建）
            self.ai_panel.clear_hit_regions();

            // ===== 欢迎页/空工作区提示 =====
            let has_workspace = self.current_folder.is_some() || self.content.file_path.is_some();
            if !has_workspace {
                let hint_bg_color = color_f(0.15, 0.15, 0.17, 1.0);
                let hint_bg_brush = match self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &hint_bg_color)
                {
                    Ok(b) => b,
                    Err(_) => return,
                };
                let hint_bg_rect = D2D_RECT_F {
                    left: x + margin,
                    top: cy,
                    right: x + width - margin,
                    bottom: cy + 70.0,
                };
                target.FillRectangle(&hint_bg_rect, &hint_bg_brush);

                let hint_text: Vec<u16> = "当前工作区为空，请打开一个文件夹以继续。"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let hint_rect = D2D_RECT_F {
                    left: x + margin + 8.0,
                    top: cy + 10.0,
                    right: x + width - margin - 8.0,
                    bottom: cy + 28.0,
                };
                target.DrawText(
                    &hint_text,
                    &msg_format,
                    &hint_rect,
                    &dim_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // "浏览并选择文件夹" 按钮
                let open_btn_w = 120.0f32;
                let open_btn_h = 28.0f32;
                let open_btn_x = x + margin + 8.0;
                let open_btn_y = cy + 32.0;
                let open_btn_rect = D2D_RECT_F {
                    left: open_btn_x,
                    top: open_btn_y,
                    right: open_btn_x + open_btn_w,
                    bottom: open_btn_y + open_btn_h,
                };
                let open_btn_brush = match self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &color_f(0.0, 0.47, 0.83, 1.0))
                {
                    Ok(b) => b,
                    Err(_) => return,
                };
                target.FillRectangle(&open_btn_rect, &open_btn_brush);
                let open_btn_text: Vec<u16> =
                    "浏览并选择文件夹".encode_utf16().chain(Some(0)).collect();
                let open_btn_text_rect = D2D_RECT_F {
                    left: open_btn_x,
                    top: open_btn_y + 5.0,
                    right: open_btn_x + open_btn_w,
                    bottom: open_btn_y + open_btn_h - 3.0,
                };
                target.DrawText(
                    &open_btn_text,
                    &small_format,
                    &open_btn_text_rect,
                    &white_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                cy += 80.0;

                // 分隔线
                let sep3_rect = D2D_RECT_F {
                    left: x + margin,
                    top: cy,
                    right: x + width - margin,
                    bottom: cy + 1.0,
                };
                target.FillRectangle(&sep3_rect, &sep_brush);
                cy += 10.0;
            }

            // ===== 聊天消息区域 =====
            let chat_top = cy;
            let chat_bottom = y + height - 52.0;
            // 消息区域（自动换行 + 完整显示 + 代码块分段，不再按 80 字符截断）
            let content_left = x + margin;
            let content_right = x + width - margin;
            let seg_pad = 6.0f32;
            let label_h = 14.0f32;
            let msg_gap = 12.0f32;
            let seg_gap = 4.0f32;
            // 自动滚到底：吸附底部时对齐到最新消息（用上一帧的最大滚动量）
            if self.ai_panel.stick_to_bottom {
                self.ai_panel.scroll_y = self.ai_panel.content_height;
            }
            let dwrite = self.text_renderer.dwrite_factory();
            let content_start_y = chat_top - self.ai_panel.scroll_y;
            let mut msg_y = content_start_y;

            for msg in &self.ai_panel.messages {
                if msg.role == crate::ai_panel::AiRole::System {
                    continue;
                }
                let is_user = msg.role == crate::ai_panel::AiRole::User;

                // 角色标签
                let label = if is_user { "你" } else { "AI" };
                let label_color: &ID2D1SolidColorBrush =
                    if is_user { &accent_brush } else { &green_brush };
                if msg_y + label_h >= chat_top && msg_y <= chat_bottom {
                    let label_wide: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                    let label_rect = D2D_RECT_F {
                        left: content_left + 4.0,
                        top: msg_y,
                        right: content_right,
                        bottom: msg_y + label_h,
                    };
                    target.DrawText(
                        &label_wide,
                        &small_format,
                        &label_rect,
                        label_color,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
                msg_y += label_h;

                // 按 ``` 代码围栏拆分为普通段 / 代码段
                let mut segments: Vec<(bool, String)> = Vec::new();
                {
                    let mut in_code = false;
                    let mut buf: Vec<&str> = Vec::new();
                    for line in msg.content.lines() {
                        if line.trim_start().starts_with("```") {
                            if !buf.is_empty() {
                                segments.push((in_code, buf.join("\n")));
                                buf.clear();
                            }
                            in_code = !in_code;
                            continue;
                        }
                        buf.push(line);
                    }
                    if !buf.is_empty() {
                        segments.push((in_code, buf.join("\n")));
                    }
                }
                if segments.is_empty() {
                    segments.push((false, String::new()));
                }

                for (is_code, seg_text) in &segments {
                    let inner_w = if *is_code {
                        (content_right - content_left - seg_pad * 2.0 - 8.0).max(20.0)
                    } else {
                        (content_right - content_left - seg_pad * 2.0).max(20.0)
                    };

                    // 普通段解析轻量 Markdown；代码段保持原文
                    #[allow(clippy::type_complexity)]
                    let (layout_wide, bolds, headings): (
                        Vec<u16>,
                        Vec<(u32, u32)>,
                        Vec<(u32, u32, f32)>,
                    ) = if *is_code {
                        (seg_text.encode_utf16().collect(), Vec::new(), Vec::new())
                    } else {
                        crate::ai_panel::parse_markdown_segment(seg_text)
                    };
                    let layout =
                        match dwrite.CreateTextLayout(&layout_wide, &msg_format, inner_w, 100000.0)
                        {
                            Ok(l) => l,
                            Err(_) => {
                                msg_y += 14.0 + seg_pad * 2.0 + seg_gap;
                                continue;
                            }
                        };
                    if !*is_code {
                        for (bs, bl) in &bolds {
                            let _ = layout.SetFontWeight(
                                DWRITE_FONT_WEIGHT_BOLD,
                                windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_RANGE {
                                    startPosition: *bs,
                                    length: *bl,
                                },
                            );
                        }
                        for (hs, hl, hsize) in &headings {
                            let r = windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_RANGE {
                                startPosition: *hs,
                                length: *hl,
                            };
                            let _ = layout.SetFontSize(*hsize, r);
                            let _ = layout.SetFontWeight(DWRITE_FONT_WEIGHT_BOLD, r);
                        }
                    }
                    let mut m =
                        windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_METRICS::default();
                    let text_h = if layout.GetMetrics(&mut m).is_ok() {
                        m.height.max(14.0)
                    } else {
                        14.0
                    };
                    let seg_h = text_h + seg_pad * 2.0;

                    // 完全在视口外：仅累加高度，跳过绘制
                    if msg_y + seg_h < chat_top || msg_y > chat_bottom {
                        msg_y += seg_h + seg_gap;
                        continue;
                    }

                    let seg_left = if *is_code {
                        content_left + 4.0
                    } else {
                        content_left
                    };
                    let seg_bg: &ID2D1SolidColorBrush = if *is_code {
                        &code_bg_brush
                    } else if is_user {
                        &user_bg_brush
                    } else {
                        &assistant_bg_brush
                    };
                    let seg_rect = D2D_RECT_F {
                        left: seg_left,
                        top: msg_y,
                        right: content_right,
                        bottom: msg_y + seg_h,
                    };
                    target.FillRectangle(&seg_rect, seg_bg);

                    let seg_fg: &ID2D1SolidColorBrush = if *is_code {
                        &code_text_brush
                    } else {
                        text_brush
                    };
                    let origin = windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                        x: seg_left + seg_pad,
                        y: msg_y + seg_pad,
                    };
                    target.DrawTextLayout(origin, &layout, seg_fg, D2D1_DRAW_TEXT_OPTIONS_NONE);

                    // 代码块添加"保存为文件"按钮
                    if *is_code && !is_user && !seg_text.is_empty() {
                        let save_btn_w = 60.0f32;
                        let save_btn_h = 18.0f32;
                        let save_btn_x = content_right - save_btn_w - 4.0;
                        let save_btn_y = msg_y + 2.0;
                        let save_btn_rect = D2D_RECT_F {
                            left: save_btn_x,
                            top: save_btn_y,
                            right: save_btn_x + save_btn_w,
                            bottom: save_btn_y + save_btn_h,
                        };
                        let save_bg = color_f(0.2, 0.5, 0.3, 1.0);
                        if let Ok(save_brush) =
                            self.render_ctx.brush_cache.get_brush(target, &save_bg)
                        {
                            target.FillRectangle(&save_btn_rect, &save_brush);
                        }
                        let save_text: Vec<u16> = "保存".encode_utf16().chain(Some(0)).collect();
                        let save_text_rect = D2D_RECT_F {
                            left: save_btn_x,
                            top: save_btn_y + 1.0,
                            right: save_btn_x + save_btn_w,
                            bottom: save_btn_y + save_btn_h - 1.0,
                        };
                        target.DrawText(
                            &save_text,
                            &small_format,
                            &save_text_rect,
                            &white_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        // 注册保存按钮区域（简化：只存储 y 范围，点击时通过内容匹配）
                        // 实际文件名从消息内容中解析
                    }

                    msg_y += seg_h + seg_gap;
                }

                msg_y += msg_gap;
            }

            // 记录内容高度与最大滚动量（供滚轮/滚动条），并绘制滚动条
            let viewport_h = (chat_bottom - chat_top).max(1.0);
            let total_content = (msg_y - content_start_y).max(0.0);
            self.ai_panel.content_height = (total_content - viewport_h).max(0.0);
            if self.ai_panel.content_height > 0.0 {
                let track_h = viewport_h;
                let total = total_content.max(viewport_h);
                let thumb_h = (viewport_h / total * track_h).max(24.0);
                let denom = self.ai_panel.content_height.max(1.0);
                let scroll_ratio = (self.ai_panel.scroll_y / denom).clamp(0.0, 1.0);
                let thumb_y = chat_top + scroll_ratio * (track_h - thumb_h);
                let track_x = x + width - 6.0;
                let thumb_rect = D2D_RECT_F {
                    left: track_x,
                    top: thumb_y,
                    right: track_x + 4.0,
                    bottom: thumb_y + thumb_h,
                };
                if let Ok(sb) = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &color_f(0.4, 0.4, 0.45, 0.85))
                {
                    target.FillRectangle(&thumb_rect, &sb);
                }
            }

            // 正在生成指示器（带动画点）
            if self.ai_panel.is_generating && msg_y < chat_bottom && msg_y + 16.0 > chat_top {
                let typing_text = format!(
                    "AI 正在思考{}",
                    ".".repeat((self.ai_panel.messages.len() % 3) + 1)
                );
                let typing: Vec<u16> = typing_text.encode_utf16().chain(Some(0)).collect();
                let typing_rect = D2D_RECT_F {
                    left: x + margin + 4.0,
                    top: msg_y,
                    right: x + width - margin,
                    bottom: msg_y + 16.0,
                };
                target.DrawText(
                    &typing,
                    &small_format,
                    &typing_rect,
                    &yellow_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            // ===== Apply 按钮区域 =====
            let has_code = self.ai_panel.extract_last_code_block().is_some();
            if has_code && !self.ai_panel.is_generating {
                let apply_y = y + height - 78.0;
                let apply_btn_w = 90.0f32;
                let apply_btn_h = 26.0f32;
                let apply_btn_x = x + width - margin - apply_btn_w;
                let apply_btn_rect = D2D_RECT_F {
                    left: apply_btn_x,
                    top: apply_y,
                    right: apply_btn_x + apply_btn_w,
                    bottom: apply_y + apply_btn_h,
                };
                let apply_bg_color = if self.ai_panel.hover_apply_button {
                    color_f(0.0, 0.55, 0.95, 1.0)
                } else {
                    color_f(0.0, 0.47, 0.83, 1.0)
                };
                let apply_bg_brush = match self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &apply_bg_color)
                {
                    Ok(b) => b,
                    Err(_) => return,
                };
                target.FillRectangle(&apply_btn_rect, &apply_bg_brush);
                let apply_text: Vec<u16> = "应用代码".encode_utf16().chain(Some(0)).collect();
                let apply_text_rect = D2D_RECT_F {
                    left: apply_btn_x,
                    top: apply_y + 4.0,
                    right: apply_btn_x + apply_btn_w,
                    bottom: apply_y + apply_btn_h - 2.0,
                };
                target.DrawText(
                    &apply_text,
                    &small_format,
                    &apply_text_rect,
                    &white_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            // ===== 停止 / 复制 / 重新生成 按钮（浮层行，左侧） =====
            let act_y = y + height - 78.0;
            let act_h = 26.0f32;
            if self.ai_panel.is_generating {
                let stop_w = 96.0f32;
                let stop_x = x + margin;
                let stop_rect = D2D_RECT_F {
                    left: stop_x,
                    top: act_y,
                    right: stop_x + stop_w,
                    bottom: act_y + act_h,
                };
                if let Ok(b) = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &color_f(0.6, 0.2, 0.2, 1.0))
                {
                    target.FillRectangle(&stop_rect, &b);
                }
                let t: Vec<u16> = "■ 停止生成".encode_utf16().chain(Some(0)).collect();
                let tr = D2D_RECT_F {
                    left: stop_x,
                    top: act_y + 4.0,
                    right: stop_x + stop_w,
                    bottom: act_y + act_h - 2.0,
                };
                target.DrawText(
                    &t,
                    &small_format,
                    &tr,
                    &white_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            // ===== 变更列表 + Diff 预览（Edit/Agent 模式） =====
            if self.ai_panel.show_diff_view && !self.ai_panel.diff_view.is_empty() {
                let input_top = y + height - 44.0;
                let changes_y = (y + height - 340.0).max(chat_top + 4.0);
                let changes_h = 150.0f32;

                // 面板背景，避免与聊天内容视觉重叠
                let panel_bg_brush = match self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &color_f(0.10, 0.10, 0.12, 1.0))
                {
                    Ok(b) => b,
                    Err(_) => return,
                };
                let panel_bg_rect = D2D_RECT_F {
                    left: x + margin - 2.0,
                    top: changes_y - 4.0,
                    right: x + width - margin + 2.0,
                    bottom: input_top - 6.0,
                };
                target.FillRectangle(&panel_bg_rect, &panel_bg_brush);

                // 标题
                let ch_title: Vec<u16> = "待确认变更".encode_utf16().chain(Some(0)).collect();
                let ch_title_rect = D2D_RECT_F {
                    left: x + margin,
                    top: changes_y,
                    right: x + width - margin,
                    bottom: changes_y + 16.0,
                };
                target.DrawText(
                    &ch_title,
                    &small_format,
                    &ch_title_rect,
                    &yellow_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // "全部接受" / "全部拒绝" 按钮（idx = usize::MAX 表示批量操作）
                let accept_all_w = 60.0f32;
                let reject_all_w = 60.0f32;
                let btn_h2 = 20.0f32;
                let accept_x = x + width - margin - reject_all_w - accept_all_w - 8.0;
                let reject_x = x + width - margin - reject_all_w;
                let accept_rect = D2D_RECT_F {
                    left: accept_x,
                    top: changes_y,
                    right: accept_x + accept_all_w,
                    bottom: changes_y + btn_h2,
                };
                let reject_rect = D2D_RECT_F {
                    left: reject_x,
                    top: changes_y,
                    right: reject_x + reject_all_w,
                    bottom: changes_y + btn_h2,
                };
                let accept_brush = match self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &color_f(0.0, 0.55, 0.3, 1.0))
                {
                    Ok(b) => b,
                    Err(_) => return,
                };
                let reject_brush = match self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &color_f(0.6, 0.2, 0.2, 1.0))
                {
                    Ok(b) => b,
                    Err(_) => return,
                };
                target.FillRectangle(&accept_rect, &accept_brush);
                target.FillRectangle(&reject_rect, &reject_brush);
                let accept_t: Vec<u16> = "全部接受".encode_utf16().chain(Some(0)).collect();
                let reject_t: Vec<u16> = "全部拒绝".encode_utf16().chain(Some(0)).collect();
                let accept_tr = D2D_RECT_F {
                    left: accept_x,
                    top: changes_y + 3.0,
                    right: accept_x + accept_all_w,
                    bottom: changes_y + btn_h2 - 1.0,
                };
                let reject_tr = D2D_RECT_F {
                    left: reject_x,
                    top: changes_y + 3.0,
                    right: reject_x + reject_all_w,
                    bottom: changes_y + btn_h2 - 1.0,
                };
                target.DrawText(
                    &accept_t,
                    &small_format,
                    &accept_tr,
                    &white_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                target.DrawText(
                    &reject_t,
                    &small_format,
                    &reject_tr,
                    &white_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                self.ai_panel.change_action_regions.push((
                    usize::MAX,
                    1,
                    accept_x,
                    changes_y,
                    accept_all_w,
                    btn_h2,
                ));
                self.ai_panel.change_action_regions.push((
                    usize::MAX,
                    2,
                    reject_x,
                    changes_y,
                    reject_all_w,
                    btn_h2,
                ));

                // 文件列表（最多显示 4 个）
                let list_y = changes_y + 24.0;
                let mut item_y = list_y;
                let selected_idx = self.ai_panel.diff_view.selected_index;
                let max_files_shown = 4usize;
                for (idx, file) in self
                    .ai_panel
                    .diff_view
                    .files
                    .iter()
                    .enumerate()
                    .take(max_files_shown)
                {
                    if item_y + 20.0 > changes_y + changes_h {
                        break;
                    }
                    // 选中行高亮
                    if idx == selected_idx {
                        let sel_rect = D2D_RECT_F {
                            left: x + margin,
                            top: item_y - 1.0,
                            right: x + width - margin,
                            bottom: item_y + 17.0,
                        };
                        if let Ok(sel_brush) = self
                            .render_ctx
                            .brush_cache
                            .get_brush(target, &color_f(0.18, 0.20, 0.26, 1.0))
                        {
                            target.FillRectangle(&sel_rect, &sel_brush);
                        }
                    }
                    let (del, ins) = file.change_count();
                    let file_name = file
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let status = if file.accepted {
                        "✓"
                    } else if file.rejected {
                        "✗"
                    } else {
                        "○"
                    };
                    let line = format!("{} {} (+{} -{})", status, file_name, ins, del);
                    let line_wide: Vec<u16> = line.encode_utf16().chain(Some(0)).collect();
                    let line_rect = D2D_RECT_F {
                        left: x + margin + 4.0,
                        top: item_y,
                        right: x + width - margin - 130.0,
                        bottom: item_y + 16.0,
                    };
                    target.DrawText(
                        &line_wide,
                        &small_format,
                        &line_rect,
                        if file.accepted {
                            &green_brush
                        } else if file.rejected {
                            &dim_brush
                        } else {
                            text_brush
                        },
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    // 预览/接受/拒绝 三个小按钮
                    let act_w = 36.0f32;
                    let act_gap = 4.0;
                    let act_start = x + width - margin - (act_w * 3.0 + act_gap * 2.0);
                    for (ai, label) in ["预览", "接受", "拒绝"].iter().enumerate() {
                        let ax = act_start + ai as f32 * (act_w + act_gap);
                        let arect = D2D_RECT_F {
                            left: ax,
                            top: item_y,
                            right: ax + act_w,
                            bottom: item_y + 16.0,
                        };
                        let acolor = match ai {
                            0 => color_f(0.2, 0.2, 0.25, 1.0),
                            1 => color_f(0.0, 0.45, 0.25, 1.0),
                            _ => color_f(0.45, 0.15, 0.15, 1.0),
                        };
                        let abrush = match self.render_ctx.brush_cache.get_brush(target, &acolor) {
                            Ok(b) => b,
                            Err(_) => continue,
                        };
                        target.FillRectangle(&arect, &abrush);
                        let at: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                        let atr = D2D_RECT_F {
                            left: ax,
                            top: item_y + 2.0,
                            right: ax + act_w,
                            bottom: item_y + 14.0,
                        };
                        target.DrawText(
                            &at,
                            &small_format,
                            &atr,
                            &white_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        self.ai_panel
                            .change_action_regions
                            .push((idx, ai as u8, ax, item_y, act_w, 16.0));
                    }
                    item_y += 18.0;
                }
                if self.ai_panel.diff_view.files.len() > max_files_shown {
                    let more = format!(
                        "… 其余 {} 个文件",
                        self.ai_panel.diff_view.files.len() - max_files_shown
                    );
                    let more_wide: Vec<u16> = more.encode_utf16().chain(Some(0)).collect();
                    let more_rect = D2D_RECT_F {
                        left: x + margin + 4.0,
                        top: item_y,
                        right: x + width - margin,
                        bottom: item_y + 16.0,
                    };
                    target.DrawText(
                        &more_wide,
                        &small_format,
                        &more_rect,
                        &dim_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    item_y += 18.0;
                }

                // ===== 选中文件的 Diff 预览行 =====
                let preview_top = item_y + 4.0;
                let preview_bottom = input_top - 8.0;
                if preview_bottom - preview_top > 16.0 {
                    if let Some(file) = self.ai_panel.diff_view.files.get(selected_idx) {
                        let dl_h = 13.0f32;
                        let mut ly = preview_top;
                        for dline in &file.lines {
                            if ly + dl_h > preview_bottom {
                                break;
                            }
                            let (bg_color, fg_color, prefix) = match dline.kind {
                                crate::diff_view::DiffLineKind::Delete => (
                                    Some(color_f(0.32, 0.12, 0.12, 1.0)),
                                    color_f(0.95, 0.6, 0.6, 1.0),
                                    "-",
                                ),
                                crate::diff_view::DiffLineKind::Insert => (
                                    Some(color_f(0.10, 0.30, 0.14, 1.0)),
                                    color_f(0.7, 0.95, 0.7, 1.0),
                                    "+",
                                ),
                                crate::diff_view::DiffLineKind::Context => {
                                    (None, color_f(0.6, 0.6, 0.6, 1.0), " ")
                                }
                            };
                            if let Some(bc) = bg_color {
                                if let Ok(lb) = self.render_ctx.brush_cache.get_brush(target, &bc) {
                                    let lr = D2D_RECT_F {
                                        left: x + margin,
                                        top: ly,
                                        right: x + width - margin,
                                        bottom: ly + dl_h,
                                    };
                                    target.FillRectangle(&lr, &lb);
                                }
                            }
                            let fg_brush =
                                match self.render_ctx.brush_cache.get_brush(target, &fg_color) {
                                    Ok(b) => b,
                                    Err(_) => break,
                                };
                            let raw = dline.text.trim_end_matches(['\n', '\r']);
                            let shown: String = if raw.chars().count() > 120 {
                                raw.chars().take(120).collect()
                            } else {
                                raw.to_string()
                            };
                            let dtext = format!("{}{}", prefix, shown);
                            let dwide: Vec<u16> = dtext.encode_utf16().chain(Some(0)).collect();
                            let dr = D2D_RECT_F {
                                left: x + margin + 4.0,
                                top: ly,
                                right: x + width - margin - 2.0,
                                bottom: ly + dl_h,
                            };
                            target.DrawText(
                                &dwide,
                                &small_format,
                                &dr,
                                &fg_brush,
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );
                            ly += dl_h;
                        }
                    }
                }
            }

            // ===== 输入框区域（新设计：参考图样式） =====
            let input_area_h = 80.0f32; // 输入区域总高度（去掉提示文字后缩小）
            let input_y = y + height - input_area_h;
            let input_margin = 8.0f32;

            // 输入框卡片背景（圆角矩形效果用纯色填充）
            let card_rect = D2D_RECT_F {
                left: x + margin,
                top: input_y,
                right: x + width - margin,
                bottom: input_y + input_area_h,
            };
            target.FillRectangle(&card_rect, &input_bg_brush);

            // 卡片边框
            let card_border_color = color_f(0.22, 0.22, 0.25, 1.0);
            let card_border_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &card_border_color)
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let card_border_top = D2D_RECT_F {
                left: x + margin,
                top: input_y,
                right: x + width - margin,
                bottom: input_y + 1.0,
            };
            let card_border_bottom = D2D_RECT_F {
                left: x + margin,
                top: input_y + input_area_h - 1.0,
                right: x + width - margin,
                bottom: input_y + input_area_h,
            };
            target.FillRectangle(&card_border_top, &card_border_brush);
            target.FillRectangle(&card_border_bottom, &card_border_brush);

            // 2. 中间输入区域
            let text_input_y = input_y + 6.0;
            let text_input_h = 36.0f32;
            let text_input_rect = D2D_RECT_F {
                left: x + margin + input_margin,
                top: text_input_y,
                right: x + width - margin - input_margin,
                bottom: text_input_y + text_input_h,
            };

            let input_text = if self.ai_panel.input.is_empty() {
                "输入问题..."
            } else {
                &self.ai_panel.input
            };
            let input_color: &ID2D1SolidColorBrush = if self.ai_panel.input.is_empty() {
                &dim_brush
            } else {
                text_brush
            };
            let input_wide: Vec<u16> = input_text.encode_utf16().chain(Some(0)).collect();
            let input_text_rect = D2D_RECT_F {
                left: text_input_rect.left + 4.0,
                top: text_input_y + 8.0,
                right: text_input_rect.right - 4.0,
                bottom: text_input_y + text_input_h - 4.0,
            };
            target.DrawText(
                &input_wide,
                &msg_format,
                &input_text_rect,
                input_color,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // IME 合成串（pre-edit text）显示在光标位置之后
            if let Some(comp) = &self.ai_panel.composition {
                if !comp.is_empty() {
                    let comp_text: Vec<u16> = comp.encode_utf16().collect();
                    // 合成串定位到光标处（光标前文本宽度），而非整段输入末尾
                    let caret_prefix = if self.ai_panel.caret_pos <= self.ai_panel.input.len() {
                        &self.ai_panel.input[..self.ai_panel.caret_pos]
                    } else {
                        self.ai_panel.input.as_str()
                    };
                    let input_width = self
                        .render_ctx
                        .text_format_cache
                        .measure_text_width(caret_prefix, 11.0, DWRITE_FONT_WEIGHT_NORMAL.0 as u32)
                        .unwrap_or(0.0);
                    let comp_x = text_input_rect.left + 4.0 + input_width;
                    let comp_rect = D2D_RECT_F {
                        left: comp_x,
                        top: text_input_y + 8.0,
                        right: text_input_rect.right - 4.0,
                        bottom: text_input_y + text_input_h - 4.0,
                    };
                    let comp_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &color_f(1.0, 0.9, 0.4, 1.0))
                        .unwrap();
                    target.DrawText(
                        &comp_text,
                        &msg_format,
                        &comp_rect,
                        &comp_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    let comp_width = self
                        .render_ctx
                        .text_format_cache
                        .measure_text_width(comp, 11.0, DWRITE_FONT_WEIGHT_NORMAL.0 as u32)
                        .unwrap_or(0.0);
                    let underline_rect = D2D_RECT_F {
                        left: comp_x,
                        top: text_input_y + text_input_h - 10.0,
                        right: comp_x + comp_width,
                        bottom: text_input_y + text_input_h - 9.0,
                    };
                    target.FillRectangle(&underline_rect, &comp_brush);
                }
            }

            // 输入框光标（聚焦且 caret_visible 时闪烁）
            if self.ai_panel.input_focused && self.ai_panel.caret_visible {
                let caret_x = if self.ai_panel.input.is_empty() {
                    text_input_rect.left + 4.0
                } else {
                    // 根据 caret_pos 计算光标位置
                    let text_before_caret = if self.ai_panel.caret_pos <= self.ai_panel.input.len()
                    {
                        &self.ai_panel.input[..self.ai_panel.caret_pos]
                    } else {
                        &self.ai_panel.input
                    };
                    let tw = self
                        .render_ctx
                        .text_format_cache
                        .measure_text_width(
                            text_before_caret,
                            11.0,
                            DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                        )
                        .unwrap_or(0.0);
                    text_input_rect.left + 4.0 + tw
                };
                let caret_rect = D2D_RECT_F {
                    left: caret_x,
                    top: text_input_y + 10.0,
                    right: caret_x + 1.5,
                    bottom: text_input_y + text_input_h - 10.0,
                };
                target.FillRectangle(&caret_rect, text_brush);
            }

            // 3. 底部分隔线
            let toolbar_sep_y = input_y + input_area_h - 34.0;
            let toolbar_sep = D2D_RECT_F {
                left: x + margin + input_margin,
                top: toolbar_sep_y,
                right: x + width - margin - input_margin,
                bottom: toolbar_sep_y + 1.0,
            };
            target.FillRectangle(&toolbar_sep, &sep_brush);

            // 4. 底部工具栏
            let toolbar_y = toolbar_sep_y + 4.0;
            let toolbar_h = 26.0f32;
            let btn_bg = color_f(0.18, 0.18, 0.20, 1.0);
            let btn_bg_brush = match self.render_ctx.brush_cache.get_brush(target, &btn_bg) {
                Ok(b) => b,
                Err(_) => return,
            };
            let btn_hover_bg = color_f(0.25, 0.25, 0.28, 1.0);
            let _btn_hover_brush =
                match self.render_ctx.brush_cache.get_brush(target, &btn_hover_bg) {
                    Ok(b) => b,
                    Err(_) => return,
                };

            // 左侧：智能体下拉按钮
            let agent_btn_w = 80.0f32;
            let agent_btn_x = x + margin + input_margin;
            let agent_btn_rect = D2D_RECT_F {
                left: agent_btn_x,
                top: toolbar_y,
                right: agent_btn_x + agent_btn_w,
                bottom: toolbar_y + toolbar_h,
            };
            target.FillRectangle(&agent_btn_rect, &btn_bg_brush);
            let agent_text: Vec<u16> = "∞ 智能体 ▼".encode_utf16().chain(Some(0)).collect();
            let agent_text_rect = D2D_RECT_F {
                left: agent_btn_x + 6.0,
                top: toolbar_y + 4.0,
                right: agent_btn_x + agent_btn_w - 4.0,
                bottom: toolbar_y + toolbar_h - 2.0,
            };
            target.DrawText(
                &agent_text,
                &small_format,
                &agent_text_rect,
                &dim_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 中间：模型选择下拉按钮
            let model_btn_w = 140.0f32;
            let model_btn_x = agent_btn_x + agent_btn_w + 6.0;
            let model_btn_rect = D2D_RECT_F {
                left: model_btn_x,
                top: toolbar_y,
                right: model_btn_x + model_btn_w,
                bottom: toolbar_y + toolbar_h,
            };
            target.FillRectangle(&model_btn_rect, &btn_bg_brush);
            // 获取当前激活模型名称
            let active_ai = self.app_settings.active_ai_settings();
            let model_label = if active_ai.model.is_empty() {
                "未配置模型".to_string()
            } else {
                active_ai.model.clone()
            };
            let model_text: Vec<u16> = format!("{} ▼", model_label)
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let model_text_rect = D2D_RECT_F {
                left: model_btn_x + 6.0,
                top: toolbar_y + 4.0,
                right: model_btn_x + model_btn_w - 4.0,
                bottom: toolbar_y + toolbar_h - 2.0,
            };
            target.DrawText(
                &model_text,
                &small_format,
                &model_text_rect,
                &dim_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 当前模型下拉弹层（点击模型按钮展开，向上弹出，列出所有已启用模型）
            if self.ai_panel.model_menu_open {
                let models: Vec<(String, String, bool)> = self
                    .app_settings
                    .ai_models
                    .iter()
                    .filter(|m| m.enabled)
                    .map(|m| {
                        let label = if !m.display_name.is_empty() {
                            m.display_name.clone()
                        } else if !m.model.is_empty() {
                            m.model.clone()
                        } else {
                            "(未命名模型)".to_string()
                        };
                        let is_active =
                            self.app_settings.active_model_id.as_deref() == Some(m.id.as_str());
                        (m.id.clone(), label, is_active)
                    })
                    .collect();
                if !models.is_empty() {
                    let item_h = 30.0f32;
                    let menu_w = model_btn_w.max(200.0);
                    let menu_x = model_btn_x;
                    let menu_bottom = toolbar_y - 4.0;
                    let menu_h = models.len() as f32 * item_h + 8.0;
                    let menu_top = menu_bottom - menu_h;
                    // 弹层背景 + 边框
                    let menu_bg = color_f(0.15, 0.15, 0.17, 1.0);
                    if let Ok(menu_bg_brush) =
                        self.render_ctx.brush_cache.get_brush(target, &menu_bg)
                    {
                        target.FillRectangle(
                            &D2D_RECT_F {
                                left: menu_x,
                                top: menu_top,
                                right: menu_x + menu_w,
                                bottom: menu_bottom,
                            },
                            &menu_bg_brush,
                        );
                    }
                    let menu_border = color_f(0.32, 0.32, 0.36, 1.0);
                    if let Ok(menu_border_brush) =
                        self.render_ctx.brush_cache.get_brush(target, &menu_border)
                    {
                        target.DrawRectangle(
                            &D2D_RECT_F {
                                left: menu_x,
                                top: menu_top,
                                right: menu_x + menu_w,
                                bottom: menu_bottom,
                            },
                            &menu_border_brush,
                            1.0,
                            None,
                        );
                    }
                    let sel_bg = color_f(0.16, 0.30, 0.46, 1.0);
                    let sel_bg_brush = self.render_ctx.brush_cache.get_brush(target, &sel_bg).ok();
                    for (i, (_id, label, is_active)) in models.iter().enumerate() {
                        let iy = menu_top + 4.0 + i as f32 * item_h;
                        if *is_active {
                            if let Some(b) = &sel_bg_brush {
                                target.FillRectangle(
                                    &D2D_RECT_F {
                                        left: menu_x + 2.0,
                                        top: iy,
                                        right: menu_x + menu_w - 2.0,
                                        bottom: iy + item_h,
                                    },
                                    b,
                                );
                            }
                        }
                        let item_str = if *is_active {
                            format!("● {}", label)
                        } else {
                            format!("    {}", label)
                        };
                        let item_wide: Vec<u16> =
                            item_str.encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &item_wide,
                            &small_format,
                            &D2D_RECT_F {
                                left: menu_x + 10.0,
                                top: iy + 6.0,
                                right: menu_x + menu_w - 10.0,
                                bottom: iy + item_h,
                            },
                            if *is_active { &white_brush } else { text_brush },
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                }
            }

            // 右侧功能按钮区域
            let right_btn_area_x = x + width - margin - input_margin;

            // 发送按钮（蓝色背景）
            let send_btn_size = 24.0f32;
            let send_btn_x = right_btn_area_x - send_btn_size;
            let send_btn_y = toolbar_y + 1.0;
            let send_btn_rect = D2D_RECT_F {
                left: send_btn_x,
                top: send_btn_y,
                right: send_btn_x + send_btn_size,
                bottom: send_btn_y + send_btn_size,
            };
            let send_bg = color_f(0.0, 0.47, 0.83, 1.0);
            let send_bg_brush = match self.render_ctx.brush_cache.get_brush(target, &send_bg) {
                Ok(b) => b,
                Err(_) => return,
            };
            target.FillRectangle(&send_btn_rect, &send_bg_brush);
            // 使用 SVG 图标绘制发送箭头
            self.icons.draw(
                target,
                crate::icons::IconKind::Send,
                send_btn_x + 2.0,
                send_btn_y + 2.0,
                send_btn_size - 4.0,
                send_btn_size - 4.0,
                &white_brush,
            );

            // 麦克风按钮
            let mic_btn_size = 24.0f32;
            let mic_btn_x = send_btn_x - mic_btn_size - 4.0;
            let mic_btn_rect = D2D_RECT_F {
                left: mic_btn_x,
                top: send_btn_y,
                right: mic_btn_x + mic_btn_size,
                bottom: send_btn_y + send_btn_size,
            };
            target.FillRectangle(&mic_btn_rect, &btn_bg_brush);
            // 使用 SVG 图标绘制麦克风
            self.icons.draw(
                target,
                crate::icons::IconKind::Mic,
                mic_btn_x + 2.0,
                send_btn_y + 2.0,
                mic_btn_size - 4.0,
                send_btn_size - 4.0,
                &dim_brush,
            );

            // 快捷按钮（星星）
            let star_btn_size = 24.0f32;
            let star_btn_x = mic_btn_x - star_btn_size - 4.0;
            let star_btn_rect = D2D_RECT_F {
                left: star_btn_x,
                top: send_btn_y,
                right: star_btn_x + star_btn_size,
                bottom: send_btn_y + star_btn_size,
            };
            target.FillRectangle(&star_btn_rect, &btn_bg_brush);
            // 使用 SVG 图标绘制闪光/星星
            self.icons.draw(
                target,
                crate::icons::IconKind::Sparkles,
                star_btn_x + 2.0,
                send_btn_y + 2.0,
                star_btn_size - 4.0,
                send_btn_size - 4.0,
                &dim_brush,
            );

            // 菜单按钮（列表图标）
            let menu_btn_size = 24.0f32;
            let menu_btn_x = star_btn_x - menu_btn_size - 4.0;
            let menu_btn_rect = D2D_RECT_F {
                left: menu_btn_x,
                top: send_btn_y,
                right: menu_btn_x + menu_btn_size,
                bottom: send_btn_y + send_btn_size,
            };
            target.FillRectangle(&menu_btn_rect, &btn_bg_brush);
            // 使用 SVG 图标绘制列表/菜单
            self.icons.draw(
                target,
                crate::icons::IconKind::List,
                menu_btn_x + 2.0,
                send_btn_y + 2.0,
                menu_btn_size - 4.0,
                send_btn_size - 4.0,
                &dim_brush,
            );
        }
    }
}
