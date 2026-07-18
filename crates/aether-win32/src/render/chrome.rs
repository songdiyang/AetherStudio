use super::*;

impl EditorState {
    pub(super) fn render_statusbar(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        region: &Region,
    ) {
        let x = region.x;
        let y = region.y;
        let width = region.width;
        let height = region.height;

        unsafe {
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.statusbar_bg)
                .unwrap();
            let text_color = color_f(1.0, 1.0, 1.0, 1.0);
            let text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &text_color)
                .unwrap();
            let sep_color = if self.theme.glass_enabled {
                self.theme.panel_border
            } else {
                color_f(0.3, 0.3, 0.3, 1.0)
            };
            let sep_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sep_color)
                .unwrap();

            let bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&bg_rect, &bg_brush);

            // Glass 模式下添加顶部柔和边框和阴影
            if self.theme.glass_enabled {
                let top_border = D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + width,
                    bottom: y + 1.0,
                };
                target.FillRectangle(&top_border, &sep_brush);
                let _ = glass::draw_panel_shadow(
                    target,
                    &mut self.render_ctx.brush_cache,
                    &bg_rect,
                    &self.theme.shadow,
                    3.0,
                );
            }

            // 更新状态栏数据
            let mut status = self.status_bar.clone();
            // P2-1: 状态栏列号显示视觉列（字符数）而非字节偏移
            let visual_col = self
                .content
                .buffer
                .get_line(self.content.cursor_line)
                .map(|line| {
                    // 把字节偏移转换为字符索引（对齐到不超出的最大字符边界）
                    let byte_pos = self.content.cursor_col.min(line.len());
                    let mut count = 0usize;
                    for (i, _) in line.char_indices() {
                        if i >= byte_pos {
                            break;
                        }
                        count += 1;
                    }
                    count
                })
                .unwrap_or(self.content.cursor_col);
            status.update_cursor_position(self.content.cursor_line, visual_col);
            status.update_status(&self.status_message);
            let lang_name = match self.content.language {
                Language::PlainText => "Plain Text",
                Language::C => "C",
                Language::Rust => "Rust",
                Language::Python => "Python",
                Language::JavaScript => "JavaScript",
                Language::TypeScript => "TypeScript",
                Language::Json => "JSON",
                Language::Markdown => "Markdown",
                Language::Toml => "TOML",
                Language::Html => "HTML",
                Language::Css => "CSS",
                Language::Go => "Go",
                Language::Java => "Java",
                Language::Image => "Image",
            };
            status.update_language(lang_name);
            let branch = if self.git.is_repo() {
                self.git.current_branch_name()
            } else {
                None
            };
            status.update_git_branch(branch.as_deref());

            let text_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();

            // SubTask 10.3: 根据文本测量自适应更新各分区宽度
            // 需要在获取 text_format 之后调用（共用同一 font_size/weight）
            {
                let cache_ref: &aether_render::d2d::brush_cache::TextFormatCache =
                    &self.render_ctx.text_format_cache;
                status.update_widths(cache_ref, 12.0, DWRITE_FONT_WEIGHT_NORMAL.0 as u32);
            }

            // SubTask 10.1: hover 背景画刷（RGBA(255,255,255,30) 半透明白色）
            let hover_color = color_f(1.0, 1.0, 1.0, 30.0 / 255.0);
            let hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_color)
                .unwrap();

            // 确保矢量图标几何已创建（状态栏 Git 分支等需要）
            self.icons.ensure_created_from_target(target);

            // 绘制各区域
            let regions = status.section_regions(width);
            for (orig_idx, rx, _ry, rw, _rh) in regions.iter() {
                if *orig_idx < status.sections.len() {
                    let section = &status.sections[*orig_idx];

                    // SubTask 10.1: hover 背景在文本之前绘制
                    // 仅对 clickable 分区且当前 hover_index 命中时绘制
                    if section.clickable && status.hover_index == Some(*orig_idx) {
                        let hover_rect = D2D_RECT_F {
                            left: x + rx,
                            top: y,
                            right: x + rx + rw,
                            bottom: y + height,
                        };
                        target.FillRectangle(&hover_rect, &hover_brush);
                    }

                    // 若有前置矢量图标，先绘制并给文本留出空间
                    let mut text_left = x + rx;
                    if let Some(icon_kind) = section.icon {
                        let icon_size = 14.0f32;
                        let icon_y = y + (height - icon_size) / 2.0;
                        self.icons.draw(
                            target,
                            icon_kind,
                            x + rx,
                            icon_y,
                            icon_size,
                            icon_size,
                            &text_brush,
                        );
                        text_left += icon_size + 4.0;
                    }

                    let wide: Vec<u16> = section.label.encode_utf16().chain(Some(0)).collect();
                    let text_rect = D2D_RECT_F {
                        left: text_left,
                        top: y + 3.0,
                        right: x + rx + rw,
                        bottom: y + height,
                    };
                    target.DrawText(
                        &wide,
                        &text_format,
                        &text_rect,
                        &text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    // TEST: 注册状态栏区域命中区域
                    crate::hit_test::register_hit_region(
                        format!("status:{}", section.label),
                        x + rx,
                        y,
                        *rw,
                        height,
                    );

                    // 分隔线
                    if *orig_idx > 0 && *orig_idx < 3 {
                        let sep_rect = D2D_RECT_F {
                            left: x + rx - 5.0,
                            top: y + 4.0,
                            right: x + rx - 4.0,
                            bottom: y + height - 4.0,
                        };
                        target.FillRectangle(&sep_rect, &sep_brush);
                    }
                }
            }
        }
    }

    pub(super) fn render_menu_bar(
        &mut self,
        item_x_positions: &[f32],
        item_widths: &[f32],
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        region: &Region,
    ) {
        let x = region.x;
        let y = region.y;
        let width = region.width;
        let height = region.height;

        // 如果菜单栏高度为0，说明已合并到标题栏，不绘制独立背景
        if height <= 0.0 {
            return;
        }

        unsafe {
            let bg_color = if self.theme.glass_enabled {
                self.theme.titlebar_bg
            } else {
                color_f(0.137, 0.137, 0.137, 1.0)
            };
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bg_color)
                .unwrap();
            let text_color = color_f(0.85, 0.85, 0.85, 1.0);
            let text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &text_color)
                .unwrap();
            let hover_color = if self.theme.glass_enabled {
                color_f(0.25, 0.25, 0.25, 0.80)
            } else {
                color_f(0.25, 0.25, 0.25, 1.0)
            };
            let hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_color)
                .unwrap();
            let active_color = color_f(0.0, 0.47, 0.83, 1.0);
            let active_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &active_color)
                .unwrap();

            let bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&bg_rect, &bg_brush);

            let text_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();

            for (i, item) in self.menu_bar.items.iter().enumerate() {
                let item_x_pos = item_x_positions[i];
                let item_width = item_widths[i];
                let is_hover = self.menu_bar.hover_index == Some(i);
                let is_active = self.menu_bar.active_index == Some(i);

                if is_active || is_hover {
                    let hover_rect = D2D_RECT_F {
                        left: item_x_pos,
                        top: y + 2.0,
                        right: item_x_pos + item_width,
                        bottom: y + height - 2.0,
                    };
                    let brush = if is_active {
                        &active_brush
                    } else {
                        &hover_brush
                    };
                    target.FillRectangle(&hover_rect, brush);
                }

                let wide: Vec<u16> = item.label.encode_utf16().chain(Some(0)).collect();
                let text_rect = D2D_RECT_F {
                    left: item_x_pos,
                    top: y,
                    right: item_x_pos + item_width,
                    bottom: y + height,
                };
                target.DrawText(
                    &wide,
                    &text_format,
                    &text_rect,
                    &text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    pub(super) fn render_title_bar(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        region: &Region,
    ) {
        let x = region.x;
        let y = region.y;
        let width = region.width;
        let height = region.height;

        unsafe {
            // 标题栏背景 — 玻璃模式下使用半透明暗色
            let bg_color = if self.theme.glass_enabled {
                self.theme.titlebar_bg
            } else {
                color_f(0.137, 0.137, 0.137, 1.0)
            };
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bg_color)
                .unwrap();
            let bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&bg_rect, &bg_brush);

            // 玻璃模式下添加底部柔和边框和阴影
            if self.theme.glass_enabled {
                let border_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &self.theme.panel_border)
                    .unwrap();
                let bottom_border = D2D_RECT_F {
                    left: x,
                    top: y + height - 1.0,
                    right: x + width,
                    bottom: y + height,
                };
                target.FillRectangle(&bottom_border, &border_brush);
                let _ = glass::draw_panel_shadow(
                    target,
                    &mut self.render_ctx.brush_cache,
                    &bg_rect,
                    &self.theme.shadow,
                    2.0,
                );
            }

            // 按钮宽度
            let btn_width = 40.0;
            let btn_height = height;
            let close_x = x + width - btn_width;
            let maximize_x = close_x - btn_width;
            let minimize_x = maximize_x - btn_width;

            // 右侧自定义工具栏按钮（在窗口控制按钮左侧）
            let tool_btn_size = 28.0f32;
            let tool_btn_gap = 2.0f32;

            // 从右往左计算位置：关闭/最大化/最小化 → 用户 → 设置 → 面板按钮 → 分隔线 → 前进/返回
            let user_btn_size = 24.0f32;
            let user_btn_x = minimize_x - tool_btn_gap - user_btn_size;
            let user_btn_y = y + (height - user_btn_size) / 2.0;

            let settings_btn_x = user_btn_x - tool_btn_gap - tool_btn_size;

            let right_panel_btn_x = settings_btn_x - tool_btn_gap - tool_btn_size;
            let bottom_panel_btn_x = right_panel_btn_x - tool_btn_gap - tool_btn_size;
            let left_sidebar_btn_x = bottom_panel_btn_x - tool_btn_gap - tool_btn_size;

            let divider_x = left_sidebar_btn_x - tool_btn_gap - 4.0;

            let forward_btn_x = divider_x - tool_btn_gap - tool_btn_size;
            let back_btn_x = forward_btn_x - tool_btn_gap - tool_btn_size;

            // 在标题栏中间显示当前工作区（打开的文件夹）或应用名
            // UI-T01: 不要显示“未命名”，优先显示打开的文件夹名
            let title_text = if let Some(folder) = &self.current_folder {
                let folder_name = folder
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| folder.to_string_lossy().to_string());
                if self.content.is_dirty {
                    format!("{} ● - Aether", folder_name)
                } else {
                    format!("{} - Aether", folder_name)
                }
            } else {
                "Aether".to_string()
            };
            let title_wide: Vec<u16> = title_text.encode_utf16().chain(Some(0)).collect();
            let title_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();
            let title_text_color = color_f(0.85, 0.85, 0.85, 1.0);
            let title_text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &title_text_color)
                .unwrap();
            // 计算标题区域：在菜单项右侧、按钮左侧
            let menu_end_x = if !self.menu_bar.item_x_positions.is_empty() {
                self.menu_bar
                    .item_x_positions
                    .last()
                    .copied()
                    .unwrap_or(0.0)
                    + self.menu_bar.item_widths.last().copied().unwrap_or(0.0)
            } else {
                0.0
            };
            let title_rect = D2D_RECT_F {
                left: menu_end_x + 10.0,
                top: y,
                right: back_btn_x - 10.0,
                bottom: y + height,
            };
            target.DrawText(
                &title_wide,
                &title_format,
                &title_rect,
                &title_text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 按钮颜色
            let default_bg = if self.theme.glass_enabled {
                self.theme.titlebar_bg
            } else {
                color_f(0.137, 0.137, 0.137, 1.0)
            };
            let hover_min_bg = if self.theme.glass_enabled {
                color_f(0.25, 0.25, 0.25, 0.80)
            } else {
                color_f(0.25, 0.25, 0.25, 1.0)
            };
            let hover_max_bg = if self.theme.glass_enabled {
                color_f(0.25, 0.25, 0.25, 0.80)
            } else {
                color_f(0.25, 0.25, 0.25, 1.0)
            };
            let hover_close_bg = color_f(0.85, 0.15, 0.15, 1.0);
            let icon_color = color_f(0.85, 0.85, 0.85, 1.0);
            let icon_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &icon_color)
                .unwrap();
            let active_icon_color = color_f(0.0, 0.47, 0.83, 1.0);
            let active_icon_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &active_icon_color)
                .unwrap();

            // 在标题栏左侧绘制菜单项
            let text_color = color_f(0.85, 0.85, 0.85, 1.0);
            let text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &text_color)
                .unwrap();
            let hover_color = if self.theme.glass_enabled {
                color_f(0.25, 0.25, 0.25, 0.80)
            } else {
                color_f(0.25, 0.25, 0.25, 1.0)
            };
            let hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_color)
                .unwrap();
            let active_color = color_f(0.0, 0.47, 0.83, 1.0);
            let active_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &active_color)
                .unwrap();

            let text_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();

            for (i, item) in self.menu_bar.items.iter().enumerate() {
                let item_x_pos = self.menu_bar.item_x_positions[i];
                let item_width = self.menu_bar.item_widths[i];
                let is_hover = self.menu_bar.hover_index == Some(i);
                let is_active = self.menu_bar.active_index == Some(i);

                if is_active || is_hover {
                    let hover_rect = D2D_RECT_F {
                        left: item_x_pos,
                        top: y + 2.0,
                        right: item_x_pos + item_width,
                        bottom: y + height - 2.0,
                    };
                    let brush = if is_active {
                        &active_brush
                    } else {
                        &hover_brush
                    };
                    target.FillRectangle(&hover_rect, brush);
                }

                // 自定义模式：拖拽中项的半透明高亮覆盖
                if self.menu_bar.customize_mode && self.menu_bar.drag_index == Some(i) {
                    let drag_color = color_f(0.4, 0.6, 1.0, 0.45);
                    let drag_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &drag_color)
                        .unwrap();
                    let drag_rect = D2D_RECT_F {
                        left: item_x_pos,
                        top: y + 2.0,
                        right: item_x_pos + item_width,
                        bottom: y + height - 2.0,
                    };
                    target.FillRectangle(&drag_rect, &drag_brush);
                }

                let wide: Vec<u16> = item.label.encode_utf16().chain(Some(0)).collect();
                let text_rect = D2D_RECT_F {
                    left: item_x_pos,
                    top: y,
                    right: item_x_pos + item_width,
                    bottom: y + height,
                };
                target.DrawText(
                    &wide,
                    &text_format,
                    &text_rect,
                    &text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // TEST: 注册菜单项命中区域
                crate::hit_test::register_hit_region(
                    format!("menu:{}", item.label),
                    item_x_pos,
                    y,
                    item_width,
                    height,
                );
            }

            // 自定义模式：菜单栏拖拽放置指示线（垂直）
            if self.menu_bar.customize_mode {
                if let Some(drop_idx) = self.menu_bar.drop_index {
                    let indicator_x = if drop_idx >= self.menu_bar.items.len() {
                        // 放在最后一项的右边缘
                        let last = self.menu_bar.items.len().saturating_sub(1);
                        self.menu_bar
                            .item_x_positions
                            .get(last)
                            .copied()
                            .unwrap_or(0.0)
                            + self.menu_bar.item_widths.get(last).copied().unwrap_or(0.0)
                    } else {
                        self.menu_bar
                            .item_x_positions
                            .get(drop_idx)
                            .copied()
                            .unwrap_or(0.0)
                    };
                    let line_color = color_f(1.0, 0.85, 0.2, 0.95);
                    let line_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &line_color)
                        .unwrap();
                    let line_rect = D2D_RECT_F {
                        left: indicator_x - 1.5,
                        top: y + 2.0,
                        right: indicator_x + 1.5,
                        bottom: y + height - 2.0,
                    };
                    target.FillRectangle(&line_rect, &line_brush);
                }
            }

            // 最小化按钮
            let min_bg = if self.titlebar_hover_button == Some(0) {
                &hover_min_bg
            } else {
                &default_bg
            };
            let min_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, min_bg)
                .unwrap();
            let min_rect = D2D_RECT_F {
                left: minimize_x,
                top: y,
                right: minimize_x + btn_width,
                bottom: y + btn_height,
            };
            target.FillRectangle(&min_rect, &min_bg_brush);
            // 最小化图标（横线）
            let line_y = y + height / 2.0 + 4.0;
            let line_rect = D2D_RECT_F {
                left: minimize_x + 18.0,
                top: line_y,
                right: minimize_x + btn_width - 18.0,
                bottom: line_y + 1.0,
            };
            target.FillRectangle(&line_rect, &icon_brush);

            // 最大化/还原按钮
            let max_bg = if self.titlebar_hover_button == Some(1) {
                &hover_max_bg
            } else {
                &default_bg
            };
            let max_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, max_bg)
                .unwrap();
            let max_rect = D2D_RECT_F {
                left: maximize_x,
                top: y,
                right: maximize_x + btn_width,
                bottom: y + btn_height,
            };
            target.FillRectangle(&max_rect, &max_bg_brush);
            // 最大化/还原图标
            if self.is_maximized {
                // 还原图标（两个重叠矩形）
                let outer_rect = D2D_RECT_F {
                    left: maximize_x + 16.0,
                    top: y + 10.0,
                    right: maximize_x + 30.0,
                    bottom: y + 20.0,
                };
                target.DrawRectangle(&outer_rect, &icon_brush, 1.0, None);
                let inner_rect = D2D_RECT_F {
                    left: maximize_x + 18.0,
                    top: y + 12.0,
                    right: maximize_x + 28.0,
                    bottom: y + 18.0,
                };
                target.FillRectangle(&inner_rect, &icon_brush);
            } else {
                // 最大化图标（空心矩形）
                let outer_rect = D2D_RECT_F {
                    left: maximize_x + 16.0,
                    top: y + 10.0,
                    right: maximize_x + 30.0,
                    bottom: y + 22.0,
                };
                target.DrawRectangle(&outer_rect, &icon_brush, 1.0, None);
            }

            // 关闭按钮
            let close_bg = if self.titlebar_hover_button == Some(2) {
                &hover_close_bg
            } else {
                &default_bg
            };
            let close_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, close_bg)
                .unwrap();
            let close_rect = D2D_RECT_F {
                left: close_x,
                top: y,
                right: close_x + btn_width,
                bottom: y + btn_height,
            };
            target.FillRectangle(&close_rect, &close_bg_brush);
            // 关闭图标（X）— UI-UX: 使用矢量 Close 图标替代像素点阵
            let close_icon_size = 16.0f32;
            let close_icon_x = close_x + (btn_width - close_icon_size) / 2.0;
            let close_icon_y = y + (btn_height - close_icon_size) / 2.0;
            let close_brush = if self.titlebar_hover_button == Some(2) {
                self.render_ctx
                    .brush_cache
                    .get_brush(target, &color_f(1.0, 1.0, 1.0, 1.0))
                    .unwrap()
            } else {
                icon_brush.clone()
            };
            self.icons.draw(
                target,
                crate::icons::IconKind::Close,
                close_icon_x,
                close_icon_y,
                close_icon_size,
                close_icon_size,
                &close_brush,
            );

            // 工具栏按钮背景画刷
            let default_tool_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &default_bg)
                .unwrap();
            let hover_tool_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_min_bg)
                .unwrap();
            let tool_btn_rect = |btn_x: f32| {
                let top = y + (btn_height - tool_btn_size) / 2.0;
                D2D_RECT_F {
                    left: btn_x,
                    top,
                    right: btn_x + tool_btn_size,
                    bottom: top + tool_btn_size,
                }
            };
            let tool_top = y + (btn_height - tool_btn_size) / 2.0;

            // 返回按钮 ←
            target.FillRectangle(
                &tool_btn_rect(back_btn_x),
                if self.titlebar_hover_button == Some(9) {
                    &hover_tool_bg_brush
                } else {
                    &default_tool_bg_brush
                },
            );
            let arrow_brush = if self.titlebar_hover_button == Some(9) {
                &active_icon_brush
            } else {
                &icon_brush
            };
            // UI-UX: 使用矢量 Back 图标替代像素点阵
            self.icons.draw(
                target,
                crate::icons::IconKind::Back,
                back_btn_x,
                tool_top,
                tool_btn_size,
                tool_btn_size,
                arrow_brush,
            );

            // 前进按钮 →
            target.FillRectangle(
                &tool_btn_rect(forward_btn_x),
                if self.titlebar_hover_button == Some(8) {
                    &hover_tool_bg_brush
                } else {
                    &default_tool_bg_brush
                },
            );
            let arrow_brush = if self.titlebar_hover_button == Some(8) {
                &active_icon_brush
            } else {
                &icon_brush
            };
            // UI-UX: 使用矢量 Forward 图标替代像素点阵
            self.icons.draw(
                target,
                crate::icons::IconKind::Forward,
                forward_btn_x,
                tool_top,
                tool_btn_size,
                tool_btn_size,
                arrow_brush,
            );

            // 分隔线
            let divider_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &color_f(0.3, 0.3, 0.3, 1.0))
                .unwrap();
            let divider_rect = D2D_RECT_F {
                left: divider_x,
                top: y + 8.0,
                right: divider_x + 1.0,
                bottom: y + height - 8.0,
            };
            target.FillRectangle(&divider_rect, &divider_brush);

            // 左侧边栏按钮：小矩形在左侧
            target.FillRectangle(
                &tool_btn_rect(left_sidebar_btn_x),
                if self.titlebar_hover_button == Some(7) {
                    &hover_tool_bg_brush
                } else {
                    &default_tool_bg_brush
                },
            );
            let ls_brush = if self.layout.sidebar_visible {
                &active_icon_brush
            } else {
                &icon_brush
            };
            let ls_outer = D2D_RECT_F {
                left: left_sidebar_btn_x + 7.0,
                top: tool_top + 5.0,
                right: left_sidebar_btn_x + tool_btn_size - 7.0,
                bottom: tool_top + tool_btn_size - 5.0,
            };
            target.DrawRectangle(&ls_outer, ls_brush, 1.0, None);
            let ls_inner = D2D_RECT_F {
                left: left_sidebar_btn_x + 9.0,
                top: tool_top + 5.0,
                right: left_sidebar_btn_x + 13.0,
                bottom: tool_top + tool_btn_size - 5.0,
            };
            target.FillRectangle(&ls_inner, ls_brush);

            // 底部面板按钮：小矩形在底部
            target.FillRectangle(
                &tool_btn_rect(bottom_panel_btn_x),
                if self.titlebar_hover_button == Some(6) {
                    &hover_tool_bg_brush
                } else {
                    &default_tool_bg_brush
                },
            );
            let bp_brush = if self.layout.bottom_panel_visible {
                &active_icon_brush
            } else {
                &icon_brush
            };
            let bp_outer = D2D_RECT_F {
                left: bottom_panel_btn_x + 7.0,
                top: tool_top + 5.0,
                right: bottom_panel_btn_x + tool_btn_size - 7.0,
                bottom: tool_top + tool_btn_size - 5.0,
            };
            target.DrawRectangle(&bp_outer, bp_brush, 1.0, None);
            let bp_inner = D2D_RECT_F {
                left: bottom_panel_btn_x + 7.0,
                top: tool_top + tool_btn_size - 11.0,
                right: bottom_panel_btn_x + tool_btn_size - 7.0,
                bottom: tool_top + tool_btn_size - 5.0,
            };
            target.FillRectangle(&bp_inner, bp_brush);

            // 右侧面板按钮：小矩形在右侧
            target.FillRectangle(
                &tool_btn_rect(right_panel_btn_x),
                if self.titlebar_hover_button == Some(5) {
                    &hover_tool_bg_brush
                } else {
                    &default_tool_bg_brush
                },
            );
            let rp_brush = if self.layout.right_panel_visible {
                &active_icon_brush
            } else {
                &icon_brush
            };
            let rp_outer = D2D_RECT_F {
                left: right_panel_btn_x + 7.0,
                top: tool_top + 5.0,
                right: right_panel_btn_x + tool_btn_size - 7.0,
                bottom: tool_top + tool_btn_size - 5.0,
            };
            target.DrawRectangle(&rp_outer, rp_brush, 1.0, None);
            let rp_inner = D2D_RECT_F {
                left: right_panel_btn_x + tool_btn_size - 13.0,
                top: tool_top + 5.0,
                right: right_panel_btn_x + tool_btn_size - 9.0,
                bottom: tool_top + tool_btn_size - 5.0,
            };
            target.FillRectangle(&rp_inner, rp_brush);

            // 设置按钮：齿轮图标
            target.FillRectangle(
                &tool_btn_rect(settings_btn_x),
                if self.titlebar_hover_button == Some(4) {
                    &hover_tool_bg_brush
                } else {
                    &default_tool_bg_brush
                },
            );
            let settings_brush = if self.titlebar_hover_button == Some(4) {
                &active_icon_brush
            } else {
                &icon_brush
            };
            // UI-UX: 使用矢量 Settings 图标替代手绘齿轮
            self.icons.draw(
                target,
                crate::icons::IconKind::Settings,
                settings_btn_x,
                tool_top,
                tool_btn_size,
                tool_btn_size,
                settings_brush,
            );

            // 用户头像按钮：人形轮廓
            let user_btn_hover = self.user_menu.is_open || self.titlebar_hover_button == Some(3);
            let user_bg = if user_btn_hover {
                &hover_min_bg
            } else {
                &default_bg
            };
            let user_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, user_bg)
                .unwrap();
            let user_btn_top = y + (btn_height - user_btn_size) / 2.0;
            let user_rect = D2D_RECT_F {
                left: user_btn_x,
                top: user_btn_top,
                right: user_btn_x + user_btn_size,
                bottom: user_btn_top + user_btn_size,
            };
            target.FillRectangle(&user_rect, &user_bg_brush);
            let user_brush = if self.titlebar_hover_button == Some(3) {
                &active_icon_brush
            } else {
                &icon_brush
            };
            // UI-UX: 使用矢量 User 图标替代像素点阵
            self.icons.draw(
                target,
                crate::icons::IconKind::User,
                user_btn_x,
                user_btn_top,
                user_btn_size,
                user_btn_size,
                user_brush,
            );

            // TEST: 注册标题栏控制按钮命中区域
            crate::hit_test::register_hit_region(
                "titlebar:minimize",
                minimize_x,
                y,
                btn_width,
                btn_height,
            );
            crate::hit_test::register_hit_region(
                "titlebar:maximize",
                maximize_x,
                y,
                btn_width,
                btn_height,
            );
            crate::hit_test::register_hit_region(
                "titlebar:close",
                close_x,
                y,
                btn_width,
                btn_height,
            );
            crate::hit_test::register_hit_region(
                "titlebar:user",
                user_btn_x,
                user_btn_y,
                user_btn_size,
                user_btn_size,
            );
            crate::hit_test::register_hit_region(
                "titlebar:settings",
                settings_btn_x,
                y,
                tool_btn_size,
                btn_height,
            );
            crate::hit_test::register_hit_region(
                "titlebar:right_panel",
                right_panel_btn_x,
                y,
                tool_btn_size,
                btn_height,
            );
            crate::hit_test::register_hit_region(
                "titlebar:bottom_panel",
                bottom_panel_btn_x,
                y,
                tool_btn_size,
                btn_height,
            );
            crate::hit_test::register_hit_region(
                "titlebar:left_sidebar",
                left_sidebar_btn_x,
                y,
                tool_btn_size,
                btn_height,
            );
            crate::hit_test::register_hit_region(
                "titlebar:forward",
                forward_btn_x,
                y,
                tool_btn_size,
                btn_height,
            );
            crate::hit_test::register_hit_region(
                "titlebar:back",
                back_btn_x,
                y,
                tool_btn_size,
                btn_height,
            );
        }
    }

    pub(super) fn render_activity_bar(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        region: &Region,
    ) {
        let x = region.x;
        let y = region.y;
        let width = region.width;
        let height = region.height;

        unsafe {
            let bg_color = if self.theme.glass_enabled {
                self.theme.activity_bar_bg
            } else {
                color_f(0.137, 0.137, 0.137, 1.0)
            };
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bg_color)
                .unwrap();
            let active_color = color_f(1.0, 1.0, 1.0, 1.0);
            let active_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &active_color)
                .unwrap();
            let inactive_color = color_f(0.55, 0.55, 0.55, 1.0);
            let inactive_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &inactive_color)
                .unwrap();
            let hover_color = if self.theme.glass_enabled {
                color_f(0.25, 0.25, 0.27, 0.80)
            } else {
                color_f(0.25, 0.25, 0.25, 1.0)
            };
            let hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_color)
                .unwrap();
            // 选中态背景：比 hover 更亮，与活动栏背景形成明显对比
            let selected_color = if self.theme.glass_enabled {
                color_f(0.35, 0.35, 0.37, 0.90)
            } else {
                color_f(0.33, 0.33, 0.33, 1.0)
            };
            let selected_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &selected_color)
                .unwrap();
            let active_indicator_color = color_f(1.0, 1.0, 1.0, 1.0);
            let active_indicator_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &active_indicator_color)
                .unwrap();

            let bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&bg_rect, &bg_brush);

            // 玻璃模式下右侧柔和边框
            if self.theme.glass_enabled {
                let border_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &self.theme.panel_border)
                    .unwrap();
                let right_border = D2D_RECT_F {
                    left: x + width - 1.0,
                    top: y,
                    right: x + width,
                    bottom: y + height,
                };
                target.FillRectangle(&right_border, &border_brush);
            }

            let icon_size = ACTIVITY_BAR_BUTTON_SIZE;
            for (i, item) in self.activity_bar.items.iter().enumerate() {
                let icon_y = y + i as f32 * icon_size;
                let is_active = i == self.activity_bar.active_index;
                let is_hover = self.activity_bar.hover_index == Some(i);

                if is_active {
                    // 选中态背景：与 hover 区分，更明显
                    let active_rect = D2D_RECT_F {
                        left: x,
                        top: icon_y,
                        right: x + width,
                        bottom: icon_y + icon_size,
                    };
                    target.FillRectangle(&active_rect, &selected_brush);

                    // 左侧高亮条（加宽至 3px，上下留边距更醒目）
                    let indicator_rect = D2D_RECT_F {
                        left: x,
                        top: icon_y + 6.0,
                        right: x + 2.0,
                        bottom: icon_y + icon_size - 6.0,
                    };
                    target.FillRectangle(&indicator_rect, &active_indicator_brush);
                } else if is_hover {
                    let hover_rect = D2D_RECT_F {
                        left: x,
                        top: icon_y,
                        right: x + width,
                        bottom: icon_y + icon_size,
                    };
                    target.FillRectangle(&hover_rect, &hover_brush);
                }

                // 自定义模式：拖拽中项的半透明高亮覆盖
                if self.activity_bar.customize_mode && self.activity_bar.drag_index == Some(i) {
                    let drag_color = color_f(0.4, 0.6, 1.0, 0.45);
                    let drag_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &drag_color)
                        .unwrap();
                    let drag_rect = D2D_RECT_F {
                        left: x,
                        top: icon_y,
                        right: x + width,
                        bottom: icon_y + icon_size,
                    };
                    target.FillRectangle(&drag_rect, &drag_brush);
                }

                // UI-UX: 使用矢量图标替代 emoji，保持视觉一致性
                let icon_kind = item.view.icon();
                // 矢量图标在 20x20 区域内绘制（活动栏变细到 32px 后与按钮保持合适留白）
                let icon_draw_size = 20.0f32;
                let icon_draw_x = x + (width - icon_draw_size) / 2.0;
                let icon_draw_y = icon_y + (icon_size - icon_draw_size) / 2.0;
                let brush = if is_active {
                    &active_brush
                } else {
                    &inactive_brush
                };
                self.icons.draw(
                    target,
                    icon_kind,
                    icon_draw_x,
                    icon_draw_y,
                    icon_draw_size,
                    icon_draw_size,
                    brush,
                );

                // TEST: 注册活动栏按钮命中区域
                crate::hit_test::register_hit_region(
                    format!("activity:{:?}", item.view),
                    x,
                    icon_y,
                    width,
                    icon_size,
                );
            }

            // 自定义模式：拖拽放置指示线
            if self.activity_bar.customize_mode {
                if let Some(drop_idx) = self.activity_bar.drop_index {
                    let indicator_y = y + drop_idx as f32 * icon_size;
                    let line_color = color_f(1.0, 0.85, 0.2, 0.95);
                    let line_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &line_color)
                        .unwrap();
                    let line_rect = D2D_RECT_F {
                        left: x,
                        top: indicator_y - 1.5,
                        right: x + width,
                        bottom: indicator_y + 1.5,
                    };
                    target.FillRectangle(&line_rect, &line_brush);
                }
            }
        }
    }
}
