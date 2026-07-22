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
            // 标题栏右侧：历史记录按钮
            {
                let hb_w = 44.0f32;
                let hb_h = 20.0f32;
                let hb_x = x + width - margin - hb_w;
                let hb_y = cy + 1.0;
                let hb_rect = D2D_RECT_F {
                    left: hb_x,
                    top: hb_y,
                    right: hb_x + hb_w,
                    bottom: hb_y + hb_h,
                };
                let hb_bg = if self.ai_panel.history_open {
                    color_f(0.0, 0.47, 0.83, 1.0)
                } else {
                    color_f(0.20, 0.21, 0.24, 1.0)
                };
                if let Ok(b) = self.render_ctx.brush_cache.get_brush(target, &hb_bg) {
                    target.FillRectangle(&hb_rect, &b);
                }
                let hb_text: Vec<u16> = "历史".encode_utf16().chain(Some(0)).collect();
                let hb_text_rect = D2D_RECT_F {
                    left: hb_x,
                    top: hb_y + 2.0,
                    right: hb_x + hb_w,
                    bottom: hb_y + hb_h - 1.0,
                };
                target.DrawText(
                    &hb_text,
                    &small_format,
                    &hb_text_rect,
                    &white_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                self.ai_panel.history_button_region = Some((hb_x, hb_y, hb_w, hb_h));
            }
            // 标题栏右侧：Playbook 策略库按钮（历史按钮左侧）
            {
                let pb_w = 44.0f32;
                let pb_h = 20.0f32;
                let pb_x = x + width - margin - 44.0 - 6.0 - pb_w;
                let pb_y = cy + 1.0;
                let pb_rect = D2D_RECT_F {
                    left: pb_x,
                    top: pb_y,
                    right: pb_x + pb_w,
                    bottom: pb_y + pb_h,
                };
                let pb_bg = if self.ai_panel.playbook_open {
                    color_f(0.0, 0.47, 0.83, 1.0)
                } else {
                    color_f(0.20, 0.21, 0.24, 1.0)
                };
                if let Ok(b) = self.render_ctx.brush_cache.get_brush(target, &pb_bg) {
                    target.FillRectangle(&pb_rect, &b);
                }
                let pb_text: Vec<u16> = "策略".encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &pb_text,
                    &small_format,
                    &D2D_RECT_F {
                        left: pb_x,
                        top: pb_y + 2.0,
                        right: pb_x + pb_w,
                        bottom: pb_y + pb_h - 1.0,
                    },
                    &white_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                self.ai_panel.playbook_button_region = Some((pb_x, pb_y, pb_w, pb_h));
            }
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

            // ===== 对话标签条（多会话）=====
            {
                let tab_h = 24.0f32;
                let tab_y = cy;
                let gap = 4.0f32;
                let tab_w = 92.0f32;
                let close_w = 16.0f32;
                let plus_w = 24.0f32;
                let strip_right = x + width - margin - plus_w - gap;
                let mut tx = x + margin;
                let n = self.ai_panel.conversations.len();
                for i in 0..n {
                    if tx + tab_w > strip_right {
                        break; // 溢出裁剪：其余会话经"历史"访问
                    }
                    let is_active = i == self.ai_panel.active;
                    let generating = self.ai_panel.conv_is_generating(i);
                    let title = self.ai_panel.conv_title(i).to_string();
                    let tab_rect = D2D_RECT_F {
                        left: tx,
                        top: tab_y,
                        right: tx + tab_w,
                        bottom: tab_y + tab_h,
                    };
                    let bg = if is_active {
                        color_f(0.18, 0.30, 0.48, 1.0)
                    } else {
                        color_f(0.16, 0.17, 0.20, 1.0)
                    };
                    if let Ok(b) = self.render_ctx.brush_cache.get_brush(target, &bg) {
                        target.FillRectangle(&tab_rect, &b);
                    }
                    let mut title_left = tx + 8.0;
                    if generating {
                        if let Ok(gb) = self
                            .render_ctx
                            .brush_cache
                            .get_brush(target, &color_f(0.95, 0.75, 0.2, 1.0))
                        {
                            let dot = D2D_RECT_F {
                                left: tx + 6.0,
                                top: tab_y + tab_h / 2.0 - 3.0,
                                right: tx + 12.0,
                                bottom: tab_y + tab_h / 2.0 + 3.0,
                            };
                            target.FillRectangle(&dot, &gb);
                        }
                        title_left = tx + 16.0;
                    }
                    let tw: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
                    let title_rect = D2D_RECT_F {
                        left: title_left,
                        top: tab_y + 3.0,
                        right: tx + tab_w - close_w - 2.0,
                        bottom: tab_y + tab_h - 2.0,
                    };
                    let tcol: &ID2D1SolidColorBrush =
                        if is_active { &white_brush } else { &dim_brush };
                    target.DrawText(
                        &tw,
                        &small_format,
                        &title_rect,
                        tcol,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    let close_x = tx + tab_w - close_w;
                    let close_rect = D2D_RECT_F {
                        left: close_x,
                        top: tab_y + 3.0,
                        right: tx + tab_w - 2.0,
                        bottom: tab_y + tab_h - 2.0,
                    };
                    let xw: Vec<u16> = "×".encode_utf16().chain(Some(0)).collect();
                    target.DrawText(
                        &xw,
                        &small_format,
                        &close_rect,
                        &dim_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    self.ai_panel
                        .tab_regions
                        .push((i, tx, tab_y, tab_w - close_w, tab_h));
                    self.ai_panel
                        .tab_close_regions
                        .push((i, close_x, tab_y, close_w, tab_h));
                    tx += tab_w + gap;
                }
                // ＋ 新建对话
                let plus_x = tx.min(x + width - margin - plus_w);
                let plus_rect = D2D_RECT_F {
                    left: plus_x,
                    top: tab_y,
                    right: plus_x + plus_w,
                    bottom: tab_y + tab_h,
                };
                if let Ok(b) = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &color_f(0.16, 0.17, 0.20, 1.0))
                {
                    target.FillRectangle(&plus_rect, &b);
                }
                let pw: Vec<u16> = "＋".encode_utf16().chain(Some(0)).collect();
                let plus_text_rect = D2D_RECT_F {
                    left: plus_x,
                    top: tab_y + 2.0,
                    right: plus_x + plus_w,
                    bottom: tab_y + tab_h - 1.0,
                };
                target.DrawText(
                    &pw,
                    &small_format,
                    &plus_text_rect,
                    &white_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                self.ai_panel.new_tab_region = Some((plus_x, tab_y, plus_w, tab_h));

                cy += tab_h + 8.0;
                let sep2 = D2D_RECT_F {
                    left: x + margin,
                    top: cy,
                    right: x + width - margin,
                    bottom: cy + 1.0,
                };
                target.FillRectangle(&sep2, &sep_brush);
                cy += 8.0;
            }

            // ===== 历史记录面板（列表视图 / 详情视图）=====
            if self.ai_panel.history_open {
                let hist_y = cy;
                let item_h = 30.0f32;
                let header_h = 20.0f32;
                let filter_h = 22.0f32;
                let footer_h = 22.0f32;
                let panel_left = x + margin;
                let panel_right = x + width - margin;
                let now = crate::ai_panel::now_secs();
                let detail_mode = self.ai_panel.history_detail_id.is_some();
                let page_indices = if detail_mode {
                    Vec::new()
                } else {
                    self.ai_panel.history_page_indices()
                };
                let empty_hint_h = if !detail_mode && page_indices.is_empty() {
                    20.0
                } else {
                    0.0
                };
                let list_h = if detail_mode {
                    let msg_n = self
                        .ai_panel
                        .history_detail_conv
                        .as_ref()
                        .map(|c| c.messages.len().min(8))
                        .unwrap_or(1);
                    header_h + 18.0 + msg_n as f32 * 24.0 + 12.0
                } else {
                    header_h
                        + filter_h
                        + page_indices.len() as f32 * item_h
                        + empty_hint_h
                        + footer_h
                        + 8.0
                };
                // 面板背景与边框
                let hist_rect = D2D_RECT_F {
                    left: panel_left,
                    top: hist_y,
                    right: panel_right,
                    bottom: hist_y + list_h,
                };
                if let Ok(hb) = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &color_f(0.12, 0.12, 0.14, 1.0))
                {
                    target.FillRectangle(&hist_rect, &hb);
                }
                if let Ok(br) = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &color_f(0.28, 0.28, 0.32, 1.0))
                {
                    target.DrawRectangle(&hist_rect, &br, 1.0, None);
                }
                let mut iy = hist_y + 4.0;

                if detail_mode {
                    // —— 详情视图：返回 / 恢复按钮 + 元信息 + 消息预览 ——
                    let btn_h = 16.0f32;
                    // ‹ 返回
                    {
                        let bw = 46.0f32;
                        let bx = panel_left + 4.0;
                        if let Ok(b) = self
                            .render_ctx
                            .brush_cache
                            .get_brush(target, &color_f(0.20, 0.21, 0.24, 1.0))
                        {
                            target.FillRectangle(
                                &D2D_RECT_F {
                                    left: bx,
                                    top: iy,
                                    right: bx + bw,
                                    bottom: iy + btn_h,
                                },
                                &b,
                            );
                        }
                        let t: Vec<u16> = "‹ 返回".encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &t,
                            &small_format,
                            &D2D_RECT_F {
                                left: bx + 5.0,
                                top: iy + 1.0,
                                right: bx + bw,
                                bottom: iy + btn_h,
                            },
                            &white_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        self.ai_panel.history_detail_back_region = Some((bx, iy, bw, btn_h));
                    }
                    // 恢复此对话
                    {
                        let bw = 66.0f32;
                        let bx = panel_right - 4.0 - bw;
                        target.FillRectangle(
                            &D2D_RECT_F {
                                left: bx,
                                top: iy,
                                right: bx + bw,
                                bottom: iy + btn_h,
                            },
                            &accent_brush,
                        );
                        let t: Vec<u16> = "恢复此对话".encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &t,
                            &small_format,
                            &D2D_RECT_F {
                                left: bx + 6.0,
                                top: iy + 1.0,
                                right: bx + bw,
                                bottom: iy + btn_h,
                            },
                            &white_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        self.ai_panel.history_detail_restore_region = Some((bx, iy, bw, btn_h));
                    }
                    iy += header_h;
                    // 元信息行：标题 (N 条) · 模式 · 相对时间
                    let meta_line = self
                        .ai_panel
                        .history_detail_id
                        .as_ref()
                        .and_then(|id| self.ai_panel.history.iter().find(|m| &m.id == id))
                        .map(|m| {
                            let mode = if m.mode.is_empty() {
                                "-"
                            } else {
                                m.mode.as_str()
                            };
                            format!(
                                "{}  ({} 条)  {}  {}",
                                m.title,
                                m.message_count,
                                mode,
                                crate::ai_panel::relative_time(m.updated_at, now)
                            )
                        })
                        .unwrap_or_default();
                    let ml: Vec<u16> = meta_line.encode_utf16().chain(Some(0)).collect();
                    target.DrawText(
                        &ml,
                        &small_format,
                        &D2D_RECT_F {
                            left: panel_left + 6.0,
                            top: iy,
                            right: panel_right - 6.0,
                            bottom: iy + 14.0,
                        },
                        &white_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    iy += 18.0;
                    // 消息预览（最多 8 条，单条截断）
                    if let Some(conv) = self.ai_panel.history_detail_conv.as_ref() {
                        for msg in conv.messages.iter().take(8) {
                            let role = match msg.role {
                                crate::ai_panel::AiRole::User => "我",
                                crate::ai_panel::AiRole::Assistant => "AI",
                                crate::ai_panel::AiRole::System => "系统",
                            };
                            let content: String = msg.content.trim().chars().take(40).collect();
                            let line: Vec<u16> = format!("{}: {}", role, content)
                                .encode_utf16()
                                .chain(Some(0))
                                .collect();
                            target.DrawText(
                                &line,
                                &small_format,
                                &D2D_RECT_F {
                                    left: panel_left + 6.0,
                                    top: iy + 4.0,
                                    right: panel_right - 6.0,
                                    bottom: iy + 22.0,
                                },
                                &dim_brush,
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );
                            iy += 24.0;
                        }
                    } else {
                        let t: Vec<u16> = "（无法加载会话内容）"
                            .encode_utf16()
                            .chain(Some(0))
                            .collect();
                        target.DrawText(
                            &t,
                            &small_format,
                            &D2D_RECT_F {
                                left: panel_left + 6.0,
                                top: iy + 4.0,
                                right: panel_right - 6.0,
                                bottom: iy + 22.0,
                            },
                            &dim_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                } else {
                    // —— 列表视图 ——
                    // 头部：「仅当前工作区」开关 + 「清空」按钮
                    {
                        let tgl_text: Vec<u16> = if self.ai_panel.history_workspace_only {
                            "[√] 仅当前工作区"
                        } else {
                            "[  ] 仅当前工作区"
                        }
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                        target.DrawText(
                            &tgl_text,
                            &small_format,
                            &D2D_RECT_F {
                                left: panel_left + 6.0,
                                top: iy + 2.0,
                                right: panel_right - 50.0,
                                bottom: iy + header_h,
                            },
                            &dim_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        self.ai_panel.history_ws_toggle_region = Some((
                            panel_left + 2.0,
                            iy,
                            panel_right - panel_left - 50.0,
                            header_h,
                        ));
                        // 清空按钮
                        let cw = 40.0f32;
                        let cx = panel_right - 4.0 - cw;
                        if let Ok(b) = self
                            .render_ctx
                            .brush_cache
                            .get_brush(target, &color_f(0.45, 0.16, 0.16, 1.0))
                        {
                            target.FillRectangle(
                                &D2D_RECT_F {
                                    left: cx,
                                    top: iy + 1.0,
                                    right: cx + cw,
                                    bottom: iy + header_h - 2.0,
                                },
                                &b,
                            );
                        }
                        let ct: Vec<u16> = "清空".encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &ct,
                            &small_format,
                            &D2D_RECT_F {
                                left: cx,
                                top: iy + 3.0,
                                right: cx + cw,
                                bottom: iy + header_h - 2.0,
                            },
                            &white_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        self.ai_panel.history_clear_all_region =
                            Some((cx, iy + 1.0, cw, header_h - 3.0));
                        iy += header_h;
                    }
                    // 筛选行：时间筛选 + 类型筛选
                    {
                        let btn_h = 17.0f32;
                        let mut fx = panel_left + 4.0;
                        for (fi, f) in crate::ai_panel::HistoryTimeFilter::ALL.iter().enumerate() {
                            let bw = 34.0f32;
                            let active = self.ai_panel.history_time_filter == *f;
                            let bg = if active {
                                color_f(0.0, 0.47, 0.83, 1.0)
                            } else {
                                color_f(0.20, 0.21, 0.24, 1.0)
                            };
                            if let Ok(b) = self.render_ctx.brush_cache.get_brush(target, &bg) {
                                target.FillRectangle(
                                    &D2D_RECT_F {
                                        left: fx,
                                        top: iy,
                                        right: fx + bw,
                                        bottom: iy + btn_h,
                                    },
                                    &b,
                                );
                            }
                            let t: Vec<u16> = f.label().encode_utf16().chain(Some(0)).collect();
                            target.DrawText(
                                &t,
                                &small_format,
                                &D2D_RECT_F {
                                    left: fx,
                                    top: iy + 2.0,
                                    right: fx + bw,
                                    bottom: iy + btn_h,
                                },
                                &white_brush,
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );
                            self.ai_panel
                                .history_time_filter_regions
                                .push((fi, fx, iy, bw, btn_h));
                            fx += bw + 3.0;
                        }
                        fx += 5.0;
                        for (fi, tf) in crate::ai_panel::HISTORY_TYPE_FILTERS.iter().enumerate() {
                            let label = tf.unwrap_or("全部");
                            let bw = 38.0f32;
                            let active = self.ai_panel.history_type_filter.as_deref() == *tf;
                            let bg = if active {
                                color_f(0.0, 0.47, 0.83, 1.0)
                            } else {
                                color_f(0.20, 0.21, 0.24, 1.0)
                            };
                            if let Ok(b) = self.render_ctx.brush_cache.get_brush(target, &bg) {
                                target.FillRectangle(
                                    &D2D_RECT_F {
                                        left: fx,
                                        top: iy,
                                        right: fx + bw,
                                        bottom: iy + btn_h,
                                    },
                                    &b,
                                );
                            }
                            let t: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                            target.DrawText(
                                &t,
                                &small_format,
                                &D2D_RECT_F {
                                    left: fx,
                                    top: iy + 2.0,
                                    right: fx + bw,
                                    bottom: iy + btn_h,
                                },
                                &white_brush,
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );
                            self.ai_panel
                                .history_type_filter_regions
                                .push((fi, fx, iy, bw, btn_h));
                            fx += bw + 3.0;
                        }
                        iy += filter_h;
                    }
                    // 当前页条目
                    for hi in page_indices.iter().copied() {
                        let hmeta = self.ai_panel.history[hi].clone();
                        let del_w = 26.0f32;
                        let item_rect = D2D_RECT_F {
                            left: panel_left + 2.0,
                            top: iy,
                            right: panel_right - 2.0,
                            bottom: iy + item_h - 2.0,
                        };
                        // 悬停高亮
                        if self.ai_panel.hover_tab == Some(hi) {
                            if let Ok(hl) = self
                                .render_ctx
                                .brush_cache
                                .get_brush(target, &color_f(0.18, 0.20, 0.26, 1.0))
                            {
                                target.FillRectangle(&item_rect, &hl);
                            }
                        }
                        // 标题：直接摘要用户问题（行业标准：不显示消息数）
                        let title_text: Vec<u16> =
                            hmeta.title.encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &title_text,
                            &small_format,
                            &D2D_RECT_F {
                                left: item_rect.left + 6.0,
                                top: iy + 2.0,
                                right: item_rect.right - del_w - 8.0,
                                bottom: iy + 15.0,
                            },
                            &white_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        // 第二行：相对时间 + 模式（灰色小字）
                        let mode = if hmeta.mode.is_empty() {
                            "-".to_string()
                        } else {
                            hmeta.mode.clone()
                        };
                        let sub_text: Vec<u16> = format!(
                            "{}  ·  {}",
                            crate::ai_panel::relative_time(hmeta.updated_at, now),
                            mode
                        )
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                        target.DrawText(
                            &sub_text,
                            &small_format,
                            &D2D_RECT_F {
                                left: item_rect.left + 6.0,
                                top: iy + 15.0,
                                right: item_rect.right - del_w - 8.0,
                                bottom: iy + 28.0,
                            },
                            &dim_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        // 删除按钮
                        let dx = item_rect.right - 4.0 - del_w;
                        if let Ok(b) = self
                            .render_ctx
                            .brush_cache
                            .get_brush(target, &color_f(0.45, 0.16, 0.16, 1.0))
                        {
                            target.FillRectangle(
                                &D2D_RECT_F {
                                    left: dx,
                                    top: iy + 5.0,
                                    right: dx + del_w,
                                    bottom: iy + item_h - 7.0,
                                },
                                &b,
                            );
                        }
                        let dt: Vec<u16> = "删".encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &dt,
                            &small_format,
                            &D2D_RECT_F {
                                left: dx,
                                top: iy + 7.0,
                                right: dx + del_w,
                                bottom: iy + item_h - 7.0,
                            },
                            &white_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        self.ai_panel.history_delete_regions.push((
                            hi,
                            dx,
                            iy + 5.0,
                            del_w,
                            item_h - 12.0,
                        ));
                        self.ai_panel.history_item_regions.push((
                            hi,
                            item_rect.left,
                            iy,
                            item_rect.right - item_rect.left - del_w - 6.0,
                            item_h - 2.0,
                        ));
                        iy += item_h;
                    }
                    if page_indices.is_empty() {
                        let t: Vec<u16> = "暂无符合条件的历史记录"
                            .encode_utf16()
                            .chain(Some(0))
                            .collect();
                        target.DrawText(
                            &t,
                            &small_format,
                            &D2D_RECT_F {
                                left: panel_left + 6.0,
                                top: iy + 3.0,
                                right: panel_right - 6.0,
                                bottom: iy + 18.0,
                            },
                            &dim_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        iy += empty_hint_h;
                    }
                    // 页脚：分页
                    {
                        let pc = self.ai_panel.history_page_count().max(1);
                        let page = self.ai_panel.history_page + 1;
                        let pw = 52.0f32;
                        let ph = 17.0f32;
                        // ‹ 上一页
                        let prev_enabled = self.ai_panel.history_page > 0;
                        let px = panel_left + 4.0;
                        let prev_bg = if prev_enabled {
                            color_f(0.20, 0.21, 0.24, 1.0)
                        } else {
                            color_f(0.14, 0.14, 0.16, 1.0)
                        };
                        if let Ok(b) = self.render_ctx.brush_cache.get_brush(target, &prev_bg) {
                            target.FillRectangle(
                                &D2D_RECT_F {
                                    left: px,
                                    top: iy,
                                    right: px + pw,
                                    bottom: iy + ph,
                                },
                                &b,
                            );
                        }
                        let t: Vec<u16> = "‹ 上一页".encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &t,
                            &small_format,
                            &D2D_RECT_F {
                                left: px + 6.0,
                                top: iy + 2.0,
                                right: px + pw,
                                bottom: iy + ph,
                            },
                            if prev_enabled {
                                &white_brush
                            } else {
                                &dim_brush
                            },
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        if prev_enabled {
                            self.ai_panel.history_page_prev_region = Some((px, iy, pw, ph));
                        }
                        // 页码
                        let pi: Vec<u16> = format!("{}/{}", page, pc)
                            .encode_utf16()
                            .chain(Some(0))
                            .collect();
                        target.DrawText(
                            &pi,
                            &small_format,
                            &D2D_RECT_F {
                                left: px + pw + 6.0,
                                top: iy + 2.0,
                                right: panel_right - pw - 10.0,
                                bottom: iy + ph,
                            },
                            &dim_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        // 下一页 ›
                        let next_enabled = page < pc;
                        let nx = panel_right - 4.0 - pw;
                        let next_bg = if next_enabled {
                            color_f(0.20, 0.21, 0.24, 1.0)
                        } else {
                            color_f(0.14, 0.14, 0.16, 1.0)
                        };
                        if let Ok(b) = self.render_ctx.brush_cache.get_brush(target, &next_bg) {
                            target.FillRectangle(
                                &D2D_RECT_F {
                                    left: nx,
                                    top: iy,
                                    right: nx + pw,
                                    bottom: iy + ph,
                                },
                                &b,
                            );
                        }
                        let t: Vec<u16> = "下一页 ›".encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &t,
                            &small_format,
                            &D2D_RECT_F {
                                left: nx + 6.0,
                                top: iy + 2.0,
                                right: nx + pw,
                                bottom: iy + ph,
                            },
                            if next_enabled {
                                &white_brush
                            } else {
                                &dim_brush
                            },
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        if next_enabled {
                            self.ai_panel.history_page_next_region = Some((nx, iy, pw, ph));
                        }
                    }
                }
                cy += list_h + 4.0;
            }

            // ===== Playbook 策略库管理面板 =====
            if self.ai_panel.playbook_open {
                let pb_y = cy;
                let item_h = 30.0f32;
                let header_h = 24.0f32;
                let max_items = 8usize;
                let n = self.ai_panel.playbook_items.len().min(max_items);
                let list_h = header_h + n as f32 * item_h + 8.0;
                // 面板背景与边框
                let pb_bg = color_f(0.12, 0.12, 0.14, 1.0);
                let panel_rect = D2D_RECT_F {
                    left: x + margin,
                    top: pb_y,
                    right: x + width - margin,
                    bottom: pb_y + list_h,
                };
                if let Ok(b) = self.render_ctx.brush_cache.get_brush(target, &pb_bg) {
                    target.FillRectangle(&panel_rect, &b);
                }
                let pb_border = color_f(0.28, 0.28, 0.32, 1.0);
                if let Ok(b) = self.render_ctx.brush_cache.get_brush(target, &pb_border) {
                    target.DrawRectangle(&panel_rect, &b, 1.0, None);
                }
                // 标题行
                let header: Vec<u16> =
                    format!("已沉淀策略（共 {} 条）", self.ai_panel.playbook_items.len())
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                target.DrawText(
                    &header,
                    &small_format,
                    &D2D_RECT_F {
                        left: x + margin + 6.0,
                        top: pb_y + 5.0,
                        right: x + width - margin - 6.0,
                        bottom: pb_y + header_h,
                    },
                    &white_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                // 条目列表
                let mut iy = pb_y + header_h;
                for (bi, bullet) in self.ai_panel.playbook_items.iter().enumerate().take(n) {
                    let del_w = 30.0f32;
                    let line: Vec<u16> = format!(
                        "[{}] {}  (+{}/-{})",
                        bullet.section, bullet.content, bullet.helpful_count, bullet.harmful_count
                    )
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                    target.DrawText(
                        &line,
                        &small_format,
                        &D2D_RECT_F {
                            left: x + margin + 6.0,
                            top: iy + 3.0,
                            right: x + width - margin - del_w - 10.0,
                            bottom: iy + item_h - 3.0,
                        },
                        &dim_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    // 删除按钮（点击后需二次确认）
                    let del_x = x + width - margin - del_w - 4.0;
                    let del_rect = D2D_RECT_F {
                        left: del_x,
                        top: iy + 3.0,
                        right: del_x + del_w,
                        bottom: iy + item_h - 5.0,
                    };
                    let del_bg = color_f(0.45, 0.16, 0.16, 1.0);
                    if let Ok(b) = self.render_ctx.brush_cache.get_brush(target, &del_bg) {
                        target.FillRectangle(&del_rect, &b);
                    }
                    let del_text: Vec<u16> = "删".encode_utf16().chain(Some(0)).collect();
                    target.DrawText(
                        &del_text,
                        &small_format,
                        &D2D_RECT_F {
                            left: del_x,
                            top: iy + 4.0,
                            right: del_x + del_w,
                            bottom: iy + item_h - 4.0,
                        },
                        &white_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    self.ai_panel.playbook_delete_regions.push((
                        bi,
                        del_x,
                        iy + 3.0,
                        del_w,
                        item_h - 8.0,
                    ));
                    iy += item_h;
                }
                if self.ai_panel.playbook_items.is_empty() {
                    let empty: Vec<u16> = "暂无沉淀策略，对话归档后会自动提炼"
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                    target.DrawText(
                        &empty,
                        &small_format,
                        &D2D_RECT_F {
                            left: x + margin + 6.0,
                            top: iy + 3.0,
                            right: x + width - margin - 6.0,
                            bottom: iy + item_h,
                        },
                        &dim_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
                cy += list_h + 4.0;
            }

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
            // 输入框区域高度 80 + 底部间距 8，确保内容不被输入框遮挡
            let input_area_h = 80.0f32;
            let chat_top = cy;
            let chat_bottom = y + height - input_area_h - 8.0;
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
            let mut reasoning_regions_local: Vec<(usize, f32, f32, f32, f32)> = Vec::new();

            // 设置消息区域裁剪，防止滚动内容覆盖到上方标签栏和下方输入框
            let chat_clip_rect = D2D_RECT_F {
                left: x,
                top: chat_top,
                right: x + width,
                bottom: chat_bottom,
            };
            target.PushAxisAlignedClip(&chat_clip_rect, D2D1_ANTIALIAS_MODE_ALIASED);

            for (msg_index, msg) in self.ai_panel.messages.iter().enumerate() {
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

                // 思考过程（DeepSeek 深度思考 reasoning_content）：独立分类、可折叠展示。
                // 与"回答"、"操作卡片"分开，视觉上弱化（紫灰、缩进、左强调条）。
                if !is_user {
                    if let Some(reasoning) = msg.reasoning.as_ref().filter(|r| !r.trim().is_empty())
                    {
                        let collapsed = msg.reasoning_collapsed;
                        let hdr_h = 20.0f32;
                        if msg_y + hdr_h >= chat_top && msg_y <= chat_bottom {
                            let arrow = if collapsed { "▶" } else { "▼" };
                            let hdr_text = format!("{}  思考过程（深度思考）", arrow);
                            let hw: Vec<u16> = hdr_text.encode_utf16().chain(Some(0)).collect();
                            if let Ok(hb) = self
                                .render_ctx
                                .brush_cache
                                .get_brush(target, &color_f(0.62, 0.55, 0.85, 1.0))
                            {
                                target.DrawText(
                                    &hw,
                                    &small_format,
                                    &D2D_RECT_F {
                                        left: content_left + 4.0,
                                        top: msg_y,
                                        right: content_right,
                                        bottom: msg_y + hdr_h,
                                    },
                                    &hb,
                                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                                    DWRITE_MEASURING_MODE_NATURAL,
                                );
                            }
                        }
                        reasoning_regions_local.push((
                            msg_index,
                            content_left,
                            msg_y,
                            content_right - content_left,
                            hdr_h,
                        ));
                        msg_y += hdr_h;
                        if !collapsed {
                            let inner_w =
                                (content_right - content_left - seg_pad * 2.0 - 10.0).max(20.0);
                            let r_wide: Vec<u16> = reasoning.encode_utf16().collect();
                            if let Ok(layout) =
                                dwrite.CreateTextLayout(&r_wide, &small_format, inner_w, 100000.0)
                            {
                                let mut m = windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_METRICS::default();
                                let text_h = if layout.GetMetrics(&mut m).is_ok() {
                                    m.height.max(12.0)
                                } else {
                                    12.0
                                };
                                let box_h = text_h + seg_pad * 2.0;
                                if msg_y + box_h >= chat_top && msg_y <= chat_bottom {
                                    if let Ok(bg) = self
                                        .render_ctx
                                        .brush_cache
                                        .get_brush(target, &color_f(0.14, 0.13, 0.18, 1.0))
                                    {
                                        target.FillRectangle(
                                            &D2D_RECT_F {
                                                left: content_left + 6.0,
                                                top: msg_y,
                                                right: content_right,
                                                bottom: msg_y + box_h,
                                            },
                                            &bg,
                                        );
                                    }
                                    if let Ok(ab) = self
                                        .render_ctx
                                        .brush_cache
                                        .get_brush(target, &color_f(0.55, 0.48, 0.80, 1.0))
                                    {
                                        target.FillRectangle(
                                            &D2D_RECT_F {
                                                left: content_left + 6.0,
                                                top: msg_y,
                                                right: content_left + 9.0,
                                                bottom: msg_y + box_h,
                                            },
                                            &ab,
                                        );
                                    }
                                    if let Ok(fg) = self
                                        .render_ctx
                                        .brush_cache
                                        .get_brush(target, &color_f(0.66, 0.68, 0.74, 1.0))
                                    {
                                        let origin = windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                                            x: content_left + 14.0,
                                            y: msg_y + seg_pad,
                                        };
                                        target.DrawTextLayout(
                                            origin,
                                            &layout,
                                            &fg,
                                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                                        );
                                    }
                                }
                                msg_y += box_h + seg_gap;
                            }
                        }
                    }
                }

                // 将消息拆为渲染项：文本/代码段 + AI 文件/命令操作卡片。
                // 助手消息里的 <<<<<<< FILE/RUN >>>>>>> 标记转为清晰的操作卡片，隐藏原始标记；
                // 用户消息无标记，整体作为一段文本。
                let display_blocks = if is_user {
                    vec![crate::ai_agent::AgentDisplayBlock::Text(
                        msg.content.clone(),
                    )]
                } else {
                    crate::ai_agent::parse_display_blocks(&msg.content)
                };
                let mut render_items: Vec<AiRenderItem> = Vec::new();
                for block in &display_blocks {
                    match block {
                        crate::ai_agent::AgentDisplayBlock::Text(t) => {
                            // 按 ``` 代码围栏拆分为普通段 / 代码段
                            let mut in_code = false;
                            let mut buf: Vec<&str> = Vec::new();
                            for line in t.lines() {
                                if line.trim_start().starts_with("```") {
                                    if !buf.is_empty() {
                                        render_items.push(AiRenderItem::Seg {
                                            is_code: in_code,
                                            text: buf.join("\n"),
                                        });
                                        buf.clear();
                                    }
                                    in_code = !in_code;
                                    continue;
                                }
                                buf.push(line);
                            }
                            if !buf.is_empty() {
                                render_items.push(AiRenderItem::Seg {
                                    is_code: in_code,
                                    text: buf.join("\n"),
                                });
                            }
                        }
                        crate::ai_agent::AgentDisplayBlock::File { kind, path } => {
                            render_items.push(AiRenderItem::File {
                                kind: kind.clone(),
                                path: path.clone(),
                            });
                        }
                        crate::ai_agent::AgentDisplayBlock::Run { cmd } => {
                            render_items.push(AiRenderItem::Run { cmd: cmd.clone() });
                        }
                    }
                }
                if render_items.is_empty() {
                    render_items.push(AiRenderItem::Seg {
                        is_code: false,
                        text: String::new(),
                    });
                }

                for item in &render_items {
                    // AI 文件/命令操作 → 渲染为清晰的操作卡片；其余按文本/代码段渲染
                    let (is_code, seg_text) = match item {
                        AiRenderItem::Seg { is_code, text } => (is_code, text),
                        _ => {
                            let card_h = 30.0f32;
                            if msg_y + card_h >= chat_top && msg_y <= chat_bottom {
                                let (glyph, label, detail, op_color) = agent_op_display(item);
                                if let Ok(cb) = self
                                    .render_ctx
                                    .brush_cache
                                    .get_brush(target, &color_f(0.16, 0.17, 0.20, 1.0))
                                {
                                    target.FillRectangle(
                                        &D2D_RECT_F {
                                            left: content_left,
                                            top: msg_y,
                                            right: content_right,
                                            bottom: msg_y + card_h,
                                        },
                                        &cb,
                                    );
                                }
                                if let Ok(ab) =
                                    self.render_ctx.brush_cache.get_brush(target, &op_color)
                                {
                                    target.FillRectangle(
                                        &D2D_RECT_F {
                                            left: content_left,
                                            top: msg_y,
                                            right: content_left + 3.0,
                                            bottom: msg_y + card_h,
                                        },
                                        &ab,
                                    );
                                    let gw: Vec<u16> =
                                        glyph.encode_utf16().chain(Some(0)).collect();
                                    target.DrawText(
                                        &gw,
                                        &small_format,
                                        &D2D_RECT_F {
                                            left: content_left + 10.0,
                                            top: msg_y,
                                            right: content_left + 30.0,
                                            bottom: msg_y + card_h,
                                        },
                                        &ab,
                                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                                        DWRITE_MEASURING_MODE_NATURAL,
                                    );
                                    let lw: Vec<u16> =
                                        label.encode_utf16().chain(Some(0)).collect();
                                    target.DrawText(
                                        &lw,
                                        &small_format,
                                        &D2D_RECT_F {
                                            left: content_left + 30.0,
                                            top: msg_y,
                                            right: content_left + 96.0,
                                            bottom: msg_y + card_h,
                                        },
                                        &ab,
                                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                                        DWRITE_MEASURING_MODE_NATURAL,
                                    );
                                }
                                if let Ok(db) = self
                                    .render_ctx
                                    .brush_cache
                                    .get_brush(target, &color_f(0.78, 0.80, 0.84, 1.0))
                                {
                                    let dw: Vec<u16> =
                                        detail.encode_utf16().chain(Some(0)).collect();
                                    target.DrawText(
                                        &dw,
                                        &small_format,
                                        &D2D_RECT_F {
                                            left: content_left + 100.0,
                                            top: msg_y,
                                            right: content_right - 8.0,
                                            bottom: msg_y + card_h,
                                        },
                                        &db,
                                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                                        DWRITE_MEASURING_MODE_NATURAL,
                                    );
                                }
                            }
                            msg_y += card_h + seg_gap;
                            continue;
                        }
                    };
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
            // 提交本帧收集的思考块折叠命中区（循环内借用了 messages，无法直接写回，故循环后赋值）
            self.ai_panel.reasoning_toggle_regions = reasoning_regions_local;

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

            // 弹出消息区域裁剪
            target.PopAxisAlignedClip();

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

            // ===== 变更列表 + Diff 预览已移除（Edit 模式删除，Agent 生成完成直接落盘） =====

            // ===== 输入框区域（新设计：参考图样式） =====
            let input_area_h = 80.0f32; // 输入区域总高度（与 chat_bottom 计算保持一致）
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
                        let item_wide: Vec<u16> = item_str.encode_utf16().chain(Some(0)).collect();
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

/// AI 面板消息渲染项：文本/代码段，或 AI 文件/命令操作卡片。
enum AiRenderItem {
    Seg {
        is_code: bool,
        text: String,
    },
    File {
        kind: crate::ai_agent::FileOpKind,
        path: String,
    },
    Run {
        cmd: String,
    },
}

/// 返回操作卡片的展示要素：(图标, 类型标签, 详情文本, 主题色)。
fn agent_op_display(
    item: &AiRenderItem,
) -> (
    &'static str,
    &'static str,
    String,
    windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
) {
    match item {
        AiRenderItem::File { kind, path } => {
            let (glyph, label, color) = match kind {
                crate::ai_agent::FileOpKind::Create => {
                    ("●", "新建文件", color_f(0.40, 0.80, 0.52, 1.0))
                }
                crate::ai_agent::FileOpKind::Modify => {
                    ("●", "修改文件", color_f(0.40, 0.70, 1.0, 1.0))
                }
                crate::ai_agent::FileOpKind::Delete => {
                    ("●", "删除文件", color_f(0.92, 0.52, 0.52, 1.0))
                }
            };
            (glyph, label, path.clone(), color)
        }
        AiRenderItem::Run { cmd } => ("▶", "运行命令", cmd.clone(), color_f(0.70, 0.62, 1.0, 1.0)),
        AiRenderItem::Seg { .. } => ("", "", String::new(), color_f(0.5, 0.5, 0.5, 1.0)),
    }
}
