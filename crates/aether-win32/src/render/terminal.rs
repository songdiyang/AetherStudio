use super::*;

impl EditorState {
    #[allow(dead_code)]
    pub(super) fn render_terminal_sidebar(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
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

            // 标题
            let title: Vec<u16> = "终端".encode_utf16().chain(Some(0)).collect();
            let title_rect = D2D_RECT_F {
                left: x + 10.0,
                top: y + 8.0,
                right: x + width - 10.0,
                bottom: y + 28.0,
            };
            target.DrawText(
                &title,
                &ui_format,
                &title_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 分隔线
            let sep_color = if self.theme.glass_enabled {
                self.theme.panel_border
            } else {
                color_f(0.2, 0.2, 0.2, 1.0)
            };
            let sep_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sep_color)
                .unwrap();
            let sep_rect = D2D_RECT_F {
                left: x,
                top: y + 30.0,
                right: x + width,
                bottom: y + 31.0,
            };
            target.FillRectangle(&sep_rect, &sep_brush);

            // 终端输出内容
            let output_color = color_f(0.8, 0.8, 0.8, 1.0);
            let output_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &output_color)
                .unwrap();
            let mut line_y = y + 40.0;
            for line in self.terminal_panel.visible_output() {
                let text: Vec<u16> = line.encode_utf16().chain(Some(0)).collect();
                let text_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: line_y,
                    right: x + width - 10.0,
                    bottom: line_y + 18.0,
                };
                target.DrawText(
                    &text,
                    &mono_format,
                    &text_rect,
                    &output_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                line_y += 16.0;
                if line_y > y + height - 30.0 {
                    break;
                }
            }

            // ConPTY 模式：输入回显由 shell 处理，无需本地渲染输入行
        }
    }

    // 保留 render_central_terminal 方法定义，但不再被调用
    // 终端已迁移到底部面板 (render_bottom_panel)
    #[allow(dead_code)]
    pub(super) fn render_central_terminal(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        if width < 2.0 || height < 2.0 {
            return;
        }
        unsafe {
            // 背景画笔
            let bg_color = if self.theme.glass_enabled {
                color_f(0.12, 0.12, 0.13, 0.98)
            } else {
                color_f(0.12, 0.12, 0.13, 1.0)
            };
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bg_color)
                .unwrap();
            let border_color = if self.theme.glass_enabled {
                self.theme.panel_border
            } else {
                color_f(0.2, 0.2, 0.2, 1.0)
            };
            let border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &border_color)
                .unwrap();
            let title_color = color_f(0.95, 0.95, 0.95, 1.0);
            let title_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &title_color)
                .unwrap();
            let dim_color = color_f(0.55, 0.55, 0.55, 1.0);
            let dim_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &dim_color)
                .unwrap();
            let output_color = color_f(0.85, 0.85, 0.85, 1.0);
            let output_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &output_color)
                .unwrap();
            let _prompt_color = color_f(0.0, 0.8, 0.4, 1.0);
            let accent_color = color_f(0.25, 0.65, 0.95, 1.0);
            let accent_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &accent_color)
                .unwrap();
            let _cursor_color = color_f(0.9, 0.9, 0.9, 1.0);

            let ui_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();
            let mono_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();

            // 1. 面板背景
            let bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&bg_rect, &bg_brush);

            // 2. 边框（界定中央区域，与侧边栏视觉分离）
            let border_top = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + 1.0,
            };
            target.FillRectangle(&border_top, &border_brush);

            // 3. 标题栏
            let title_bar_h = 30.0;
            let title_bar_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + title_bar_h,
            };
            let title_bg_color = color_f(0.16, 0.16, 0.18, 1.0);
            let title_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &title_bg_color)
                .unwrap();
            target.FillRectangle(&title_bar_rect, &title_bg_brush);

            // 标题文字 + 运行状态指示
            let title_str = if self.terminal_panel.running {
                "⌨ 终端  ● 运行中"
            } else {
                "⌨ 终端  ○ 未启动"
            };
            let title_wide: Vec<u16> = title_str.encode_utf16().chain(Some(0)).collect();
            let title_text_rect = D2D_RECT_F {
                left: x + 12.0,
                top: y,
                right: x + width - 100.0,
                bottom: y + title_bar_h,
            };
            target.DrawText(
                &title_wide,
                &ui_format,
                &title_text_rect,
                &title_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // cwd 显示（右侧）
            let cwd_display: String = self
                .terminal_panel
                .cwd
                .chars()
                .rev()
                .take(40)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            let cwd_wide: Vec<u16> = cwd_display.encode_utf16().chain(Some(0)).collect();
            let cwd_rect = D2D_RECT_F {
                left: x + 180.0,
                top: y,
                right: x + width - 40.0,
                bottom: y + title_bar_h,
            };
            target.DrawText(
                &cwd_wide,
                &ui_format,
                &cwd_rect,
                &dim_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 关闭按钮 (×) —— 位于标题栏右侧，点击关闭中央终端
            let close_btn_size = 28.0;
            let close_btn_x = x + width - close_btn_size;
            let close_wide: Vec<u16> = "×".encode_utf16().chain(Some(0)).collect();
            let close_text_rect = D2D_RECT_F {
                left: close_btn_x,
                top: y,
                right: close_btn_x + close_btn_size,
                bottom: y + title_bar_h,
            };
            target.DrawText(
                &close_wide,
                &ui_format,
                &close_text_rect,
                &dim_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 4. 输出区域（ConPTY 模式下输出已包含提示符和输入回显，无需单独渲染输入行）
            let line_h = 18.0;
            let content_y = y + title_bar_h + 6.0;
            let content_bottom = y + height - 6.0; // 不再预留输入行空间
            let visible_lines = ((content_bottom - content_y) / line_h).floor() as usize;
            // 同步 ConPTY 尺寸到面板实际可用区域
            // 使用 DirectWrite 实测 11pt 等宽字符宽度
            let cell_w = self
                .render_ctx
                .text_format_cache
                .measure_text_width("M", 11.0, DWRITE_FONT_WEIGHT_NORMAL.0 as u32)
                .unwrap_or(7.0);
            let term_cols = ((width - 20.0) / cell_w).max(20.0) as i16;
            let term_rows = visible_lines.max(5) as i16;
            self.terminal_panel.set_size(term_cols, term_rows);
            let lines = self.terminal_panel.visible_window(visible_lines);

            // 滚动提示：当用户向上滚动浏览历史时显示提示
            if self.terminal_panel.scroll_offset > 0 {
                let hint_wide: Vec<u16> = "↑ 历史输出（回车回到最新）"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let hint_rect = D2D_RECT_F {
                    left: x + 12.0,
                    top: content_y - 2.0,
                    right: x + width - 12.0,
                    bottom: content_y + 16.0,
                };
                target.DrawText(
                    &hint_wide,
                    &ui_format,
                    &hint_rect,
                    &accent_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            let mut line_y = content_y;
            for line in &lines {
                if line_y + line_h > content_bottom {
                    break;
                }
                let text: Vec<u16> = line.encode_utf16().chain(Some(0)).collect();
                let text_rect = D2D_RECT_F {
                    left: x + 12.0,
                    top: line_y,
                    right: x + width - 12.0,
                    bottom: line_y + line_h,
                };
                target.DrawText(
                    &text,
                    &mono_format,
                    &text_rect,
                    &output_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                line_y += line_h;
            }

            // 5. 底部分隔线
            let bottom_sep = D2D_RECT_F {
                left: x,
                top: y + height - 1.0,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&bottom_sep, &border_brush);
        }
    }

    pub(super) fn render_bottom_panel(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        unsafe {
            let bg_color = if self.theme.glass_enabled {
                color_f(0.13, 0.13, 0.14, 0.95)
            } else {
                color_f(0.13, 0.13, 0.14, 1.0)
            };
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bg_color)
                .unwrap();
            let border_color = if self.theme.glass_enabled {
                self.theme.panel_border
            } else {
                color_f(0.2, 0.2, 0.2, 1.0)
            };
            let _border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &border_color)
                .unwrap();
            let text_color = color_f(0.8, 0.8, 0.8, 1.0);
            let _text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &text_color)
                .unwrap();
            let active_color = color_f(1.0, 1.0, 1.0, 1.0);
            let active_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &active_color)
                .unwrap();
            let dim_color = color_f(0.5, 0.5, 0.5, 1.0);
            let dim_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &dim_color)
                .unwrap();
            let output_color = color_f(0.8, 0.8, 0.8, 1.0);
            let output_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &output_color)
                .unwrap();
            let _prompt_color = color_f(0.0, 0.8, 0.0, 1.0);

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

            // 背景
            let bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&bg_rect, &bg_brush);

            // 顶部边框（聚焦时高亮为强调色，提供视觉反馈）
            let top_border_color = if self.terminal_panel.focused {
                color_f(0.3, 0.55, 0.85, 1.0)
            } else {
                border_color
            };
            let top_border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &top_border_color)
                .unwrap();
            let top_border = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + 2.0,
            };
            target.FillRectangle(&top_border, &top_border_brush);

            // ===== 全局搜索面板（覆盖默认终端内容） =====
            if self.search_panel.visible {
                // 搜索输入框
                let input_height = 24.0;
                let input_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: y + 6.0,
                    right: x + width - 10.0,
                    bottom: y + 6.0 + input_height,
                };
                let input_bg = color_f(0.18, 0.18, 0.2, 1.0);
                let input_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &input_bg)
                    .unwrap();
                target.FillRectangle(&input_rect, &input_bg_brush);

                // 输入框边框（聚焦时高亮）
                let border_focused = color_f(0.3, 0.55, 0.85, 1.0);
                let border_dim = color_f(0.3, 0.3, 0.3, 1.0);
                let input_border_color = if self.search_panel.visible {
                    border_focused
                } else {
                    border_dim
                };
                let input_border_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &input_border_color)
                    .unwrap();
                // 1px 边框
                let b = 1.0;
                let border_rects = [
                    D2D_RECT_F {
                        left: input_rect.left,
                        top: input_rect.top,
                        right: input_rect.right,
                        bottom: input_rect.top + b,
                    },
                    D2D_RECT_F {
                        left: input_rect.left,
                        top: input_rect.bottom - b,
                        right: input_rect.right,
                        bottom: input_rect.bottom,
                    },
                    D2D_RECT_F {
                        left: input_rect.left,
                        top: input_rect.top,
                        right: input_rect.left + b,
                        bottom: input_rect.bottom,
                    },
                    D2D_RECT_F {
                        left: input_rect.right - b,
                        top: input_rect.top,
                        right: input_rect.right,
                        bottom: input_rect.bottom,
                    },
                ];
                for r in &border_rects {
                    target.FillRectangle(r, &input_border_brush);
                }

                // 搜索图标 + 输入文本
                let prefix = "🔍 ";
                let prefix_wide: Vec<u16> = prefix.encode_utf16().chain(Some(0)).collect();
                let prefix_rect = D2D_RECT_F {
                    left: input_rect.left + 6.0,
                    top: input_rect.top + 4.0,
                    right: input_rect.left + 30.0,
                    bottom: input_rect.bottom - 2.0,
                };
                target.DrawText(
                    &prefix_wide,
                    &ui_format,
                    &prefix_rect,
                    &dim_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                let query_text = if self.search_panel.query.is_empty() {
                    "输入搜索内容...".to_string()
                } else {
                    self.search_panel.query.clone()
                };
                let query_wide: Vec<u16> = query_text.encode_utf16().chain(Some(0)).collect();
                let query_rect = D2D_RECT_F {
                    left: input_rect.left + 30.0,
                    top: input_rect.top + 4.0,
                    right: input_rect.right - 100.0,
                    bottom: input_rect.bottom - 2.0,
                };
                let query_brush = if self.search_panel.query.is_empty() {
                    &dim_brush
                } else {
                    &active_brush
                };
                target.DrawText(
                    &query_wide,
                    &ui_format,
                    &query_rect,
                    query_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // 选项标签：Aa（大小写）、.*（正则）
                let case_label = if self.search_panel.case_sensitive {
                    "Aa✓"
                } else {
                    "Aa"
                };
                let regex_label = if self.search_panel.regex {
                    ".*✓"
                } else {
                    ".*"
                };
                let opts_x = input_rect.right - 90.0;
                let case_wide: Vec<u16> = case_label.encode_utf16().chain(Some(0)).collect();
                let case_rect = D2D_RECT_F {
                    left: opts_x,
                    top: input_rect.top + 4.0,
                    right: opts_x + 40.0,
                    bottom: input_rect.bottom - 2.0,
                };
                target.DrawText(
                    &case_wide,
                    &ui_format,
                    &case_rect,
                    if self.search_panel.case_sensitive {
                        &active_brush
                    } else {
                        &dim_brush
                    },
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                let regex_wide: Vec<u16> = regex_label.encode_utf16().chain(Some(0)).collect();
                let regex_rect = D2D_RECT_F {
                    left: opts_x + 45.0,
                    top: input_rect.top + 4.0,
                    right: opts_x + 85.0,
                    bottom: input_rect.bottom - 2.0,
                };
                target.DrawText(
                    &regex_wide,
                    &ui_format,
                    &regex_rect,
                    if self.search_panel.regex {
                        &active_brush
                    } else {
                        &dim_brush
                    },
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // 状态行
                let status_y = input_rect.bottom + 4.0;
                let status_text = if self.search_panel.is_searching {
                    "搜索中...".to_string()
                } else if self.search_panel.status.is_empty() {
                    "按 Enter 搜索 · Esc 关闭".to_string()
                } else {
                    self.search_panel.status.clone()
                };
                let status_wide: Vec<u16> = status_text.encode_utf16().chain(Some(0)).collect();
                let status_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: status_y,
                    right: x + width - 10.0,
                    bottom: status_y + 16.0,
                };
                target.DrawText(
                    &status_wide,
                    &ui_format,
                    &status_rect,
                    &dim_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // 结果列表
                let results_y = status_y + 18.0;
                let mut line_y = results_y;
                let max_y = y + height - 6.0;
                let line_h = 16.0;
                let results = self.search_panel.results.clone();
                let selected = self.search_panel.selected_index;
                for (i, r) in results.iter().enumerate() {
                    if line_y + line_h > max_y {
                        break;
                    }
                    // 选中行高亮
                    if i == selected {
                        let sel_rect = D2D_RECT_F {
                            left: x + 4.0,
                            top: line_y - 1.0,
                            right: x + width - 4.0,
                            bottom: line_y + line_h - 1.0,
                        };
                        let sel_bg = color_f(0.2, 0.3, 0.5, 1.0);
                        let sel_bg_brush = self
                            .render_ctx
                            .brush_cache
                            .get_brush(target, &sel_bg)
                            .unwrap();
                        target.FillRectangle(&sel_rect, &sel_bg_brush);
                    }

                    // 文件路径（相对路径）+ 行号
                    let rel_path = self
                        .current_folder
                        .as_ref()
                        .and_then(|root| r.path.strip_prefix(root).ok())
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| r.path.to_string_lossy().to_string());
                    let header = format!("{}:{}:{}", rel_path, r.line, r.col);
                    let header_wide: Vec<u16> = header.encode_utf16().chain(Some(0)).collect();
                    let header_rect = D2D_RECT_F {
                        left: x + 12.0,
                        top: line_y,
                        right: x + width - 12.0,
                        bottom: line_y + line_h,
                    };
                    let header_brush = if i == selected {
                        &active_brush
                    } else {
                        &output_brush
                    };
                    target.DrawText(
                        &header_wide,
                        &mono_format,
                        &header_rect,
                        header_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    line_y += line_h;

                    // 匹配行内容（截断显示）
                    if line_y + line_h > max_y {
                        break;
                    }
                    let content = r.text.trim_end();
                    let content_display = if content.chars().count() > 200 {
                        format!("{}...", content.chars().take(200).collect::<String>())
                    } else {
                        content.to_string()
                    };
                    let content_wide: Vec<u16> =
                        content_display.encode_utf16().chain(Some(0)).collect();
                    let content_rect = D2D_RECT_F {
                        left: x + 24.0,
                        top: line_y,
                        right: x + width - 12.0,
                        bottom: line_y + line_h,
                    };
                    target.DrawText(
                        &content_wide,
                        &mono_format,
                        &content_rect,
                        &dim_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    line_y += line_h + 2.0;
                }
                // 搜索面板模式下结束渲染（不显示终端内容）
                return;
            }

            // 底部面板标签栏（类似 VS Code 底部面板标签）
            // 注意：标签顺序必须与 BottomPanelTab 枚举的 discriminant 一致。
            // 当前只保留"终端"和"问题"两个标签；"输出"标签已移除，
            // 问题面板的引擎/数据采集待后续设计。
            let tab_height = 28.0;
            let tabs: [BottomPanelTab; 2] = [BottomPanelTab::Terminal, BottomPanelTab::Problems];
            let mut tab_x = x + 10.0;
            let tab_w = 60.0;
            for tab_kind in tabs.iter() {
                let is_active = *tab_kind == self.bottom_panel_tab;
                let tab_rect = D2D_RECT_F {
                    left: tab_x,
                    top: y + 2.0,
                    right: tab_x + tab_w,
                    bottom: y + tab_height - 2.0,
                };
                if is_active {
                    let active_bg = color_f(0.18, 0.18, 0.2, 1.0);
                    let active_bg_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &active_bg)
                        .unwrap();
                    target.FillRectangle(&tab_rect, &active_bg_brush);
                    let top_line = D2D_RECT_F {
                        left: tab_x,
                        top: y + 2.0,
                        right: tab_x + tab_w,
                        bottom: y + 4.0,
                    };
                    target.FillRectangle(&top_line, &active_brush);
                }
                let tab_wide: Vec<u16> = tab_kind.label().encode_utf16().chain(Some(0)).collect();
                let tab_text_rect = D2D_RECT_F {
                    left: tab_x + 8.0,
                    top: y + 4.0,
                    right: tab_x + tab_w - 4.0,
                    bottom: y + tab_height - 4.0,
                };
                target.DrawText(
                    &tab_wide,
                    &ui_format,
                    &tab_text_rect,
                    if is_active { &active_brush } else { &dim_brush },
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                tab_x += tab_w + 4.0;
            }

            // 标签下方的内容：根据当前 tab 分支渲染
            // 0 = 终端（已有逻辑）；1 = 问题面板（暂未实现）
            let content_y = y + tab_height + 4.0;
            let content_h = height - tab_height - 8.0;

            // P-问题: 问题面板占位。问题数据/采集引擎后续从 diagnostics 字段设计。
            // 当前仅渲染居中提示，让用户能验证"终端/问题"切换能力已生效。
            if self.bottom_panel_tab == BottomPanelTab::Problems {
                let hint_color = color_f(150.0 / 255.0, 150.0 / 255.0, 150.0 / 255.0, 1.0);
                let hint_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &hint_color)
                    .unwrap();
                let hint_format = self
                    .render_ctx
                    .text_format_cache
                    .get_format(
                        14.0,
                        DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                        DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                        DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                    )
                    .unwrap();
                let hint_text: Vec<u16> =
                    "问题面板（待实现）".encode_utf16().chain(Some(0)).collect();
                let hint_rect = D2D_RECT_F {
                    left: x,
                    top: content_y,
                    right: x + width,
                    bottom: content_y + content_h,
                };
                target.DrawText(
                    &hint_text,
                    &hint_format,
                    &hint_rect,
                    &hint_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                return;
            }

            // 终端未启动时：若有历史输出（进程已退出）则显示输出+重启提示；
            // 否则显示居中引导文案
            if !self.terminal_panel.running {
                if self.terminal_panel.output_lines.is_empty() {
                    // 从未启动：居中提示
                    let hint_color = color_f(150.0 / 255.0, 150.0 / 255.0, 150.0 / 255.0, 1.0);
                    let hint_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &hint_color)
                        .unwrap();
                    let hint_format = self
                        .render_ctx
                        .text_format_cache
                        .get_format(
                            14.0,
                            DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                            DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                            DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                        )
                        .unwrap();
                    let hint_text: Vec<u16> =
                        "按 Ctrl+` 启动终端".encode_utf16().chain(Some(0)).collect();
                    let hint_rect = D2D_RECT_F {
                        left: x,
                        top: content_y,
                        right: x + width,
                        bottom: content_y + content_h,
                    };
                    target.DrawText(
                        &hint_text,
                        &hint_format,
                        &hint_rect,
                        &hint_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                } else {
                    // 进程已退出：显示历史输出 + 底部重启提示
                    let line_h = 14.0;
                    let content_bottom = y + height - 24.0; // 底部留空给重启提示
                    let visible_lines =
                        ((content_bottom - content_y) / line_h).floor().max(1.0) as usize;
                    let lines = self.terminal_panel.visible_window(visible_lines);
                    let mut line_y = content_y;
                    for line in &lines {
                        if line_y + line_h > content_bottom {
                            break;
                        }
                        let text: Vec<u16> = line.encode_utf16().chain(Some(0)).collect();
                        let text_rect = D2D_RECT_F {
                            left: x + 10.0,
                            top: line_y,
                            right: x + width - 10.0,
                            bottom: line_y + line_h,
                        };
                        target.DrawText(
                            &text,
                            &mono_format,
                            &text_rect,
                            &output_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        line_y += line_h;
                    }
                    // 底部重启提示
                    let restart_color = color_f(0.3, 0.55, 0.85, 1.0);
                    let restart_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &restart_color)
                        .unwrap();
                    let restart_text: Vec<u16> = "点击此处重新启动终端"
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                    let restart_rect = D2D_RECT_F {
                        left: x + 10.0,
                        top: y + height - 22.0,
                        right: x + width - 10.0,
                        bottom: y + height - 6.0,
                    };
                    target.DrawText(
                        &restart_text,
                        &ui_format,
                        &restart_rect,
                        &restart_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            } else {
                // 计算可见行数并同步 ConPTY 尺寸
                // 使用 DirectWrite 实测 11pt Consolas 等宽字符宽度，避免硬编码 7px 与渲染偏差
                let cell_w = self
                    .render_ctx
                    .text_format_cache
                    .measure_text_width("M", 11.0, DWRITE_FONT_WEIGHT_NORMAL.0 as u32)
                    .unwrap_or(7.0);
                let line_h = 14.0;
                let content_bottom = y + height - 6.0;
                let visible_lines =
                    ((content_bottom - content_y) / line_h).floor().max(1.0) as usize;
                let term_cols = ((width - 20.0) / cell_w).max(20.0) as i16;
                let term_rows = visible_lines.max(5) as i16;
                self.terminal_panel.set_size(term_cols, term_rows);
                let lines = self.terminal_panel.visible_window(visible_lines);

                // 滚动提示：用户向上浏览历史时显示提示
                if self.terminal_panel.scroll_offset > 0 {
                    let hint_wide: Vec<u16> = "↑ 历史输出（回车回到最新）"
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                    let hint_rect = D2D_RECT_F {
                        left: x + 12.0,
                        top: content_y - 2.0,
                        right: x + width - 12.0,
                        bottom: content_y + 16.0,
                    };
                    target.DrawText(
                        &hint_wide,
                        &ui_format,
                        &hint_rect,
                        &active_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }

                let mut line_y = content_y;
                for line in &lines {
                    if line_y + line_h > content_bottom {
                        break;
                    }
                    let text: Vec<u16> = line.encode_utf16().chain(Some(0)).collect();
                    let text_rect = D2D_RECT_F {
                        left: x + 10.0,
                        top: line_y,
                        right: x + width - 10.0,
                        bottom: line_y + line_h,
                    };
                    target.DrawText(
                        &text,
                        &mono_format,
                        &text_rect,
                        &output_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    line_y += line_h;
                }

                // 渲染光标：在光标位置绘制一个半透明方块
                // ConPTY 模式下光标位置由 ANSI 解析器跟踪
                let total_lines = self.terminal_panel.output_lines.len();
                let scroll_off = self.terminal_panel.scroll_offset;
                let end_line = total_lines.saturating_sub(scroll_off);
                let start_line = end_line.saturating_sub(visible_lines);
                let (cursor_row, cursor_col) = self.terminal_panel.cursor_position();
                if cursor_row >= start_line && cursor_row < end_line {
                    let display_row = cursor_row - start_line;
                    // 光标 x 使用 DirectWrite HitTestTextPosition 获取光标行前缀尾端的精确像素坐标
                    // cursor_col 是字符索引（非显示列宽），因此按字符个数取前缀
                    let cursor_x =
                        if let Some(line) = self.terminal_panel.output_lines.get(cursor_row) {
                            let char_count = line.chars().count();
                            let take = cursor_col.min(char_count);
                            let mut prefix_len = 0usize;
                            let mut prefix_utf16_len = 0usize;
                            for (idx, ch) in line.char_indices().take(take) {
                                prefix_len = idx + ch.len_utf8();
                                prefix_utf16_len += ch.encode_utf16(&mut [0; 2]).len();
                            }
                            let prefix = &line[..prefix_len];
                            let prefix_x = self
                                .render_ctx
                                .text_format_cache
                                .text_position_x(
                                    prefix,
                                    prefix_utf16_len,
                                    11.0,
                                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                                )
                                .unwrap_or(cursor_col as f32 * cell_w);
                            let extra = (cursor_col.saturating_sub(char_count)) as f32 * cell_w;
                            x + 10.0 + prefix_x + extra
                        } else {
                            x + 10.0 + cursor_col as f32 * cell_w
                        };
                    let cursor_y = content_y + display_row as f32 * line_h;
                    let cursor_w =
                        if let Some(line) = self.terminal_panel.output_lines.get(cursor_row) {
                            line.chars()
                                .nth(cursor_col)
                                .map(|ch| (unicode_char_width(ch) as f32).max(1.0) * cell_w)
                                .unwrap_or(cell_w)
                        } else {
                            cell_w
                        };
                    let cursor_h = line_h;
                    // 只在光标可见区域内绘制
                    if cursor_y + cursor_h <= content_bottom {
                        let cursor_color = color_f(0.8, 0.8, 0.8, 0.6);
                        let cursor_brush = self
                            .render_ctx
                            .brush_cache
                            .get_brush(target, &cursor_color)
                            .unwrap();
                        let cursor_rect = D2D_RECT_F {
                            left: cursor_x,
                            top: cursor_y,
                            right: cursor_x + cursor_w,
                            bottom: cursor_y + cursor_h,
                        };
                        target.FillRectangle(&cursor_rect, &cursor_brush);
                    }
                }
            }
        }
    }
}
