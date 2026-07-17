use super::*;

impl EditorState {
    /// 在 render 之前更新标签栏布局缓存
    pub(super) fn update_tab_layouts(&mut self, x: f32, width: f32, _height: f32) {
        let close_btn_width = 20.0;
        let min_tab_width = 80.0;
        let max_tab_width = 200.0;
        let gap = 2.0;

        let tab_count = self.tabs.len();
        let available_width = width - 8.0;
        let tab_width = (available_width / tab_count as f32 - gap)
            .max(min_tab_width)
            .min(max_tab_width);

        let mut tab_x = x + 4.0 - self.tab_scroll_x;
        self.tab_layouts.clear();

        for i in 0..self.tabs.len() {
            let tw = tab_width;
            self.tab_layouts.push(crate::tabs::TabLayout {
                index: i,
                x: tab_x - x - 4.0 + self.tab_scroll_x,
                width: tw,
                close_x: tab_x - x - 4.0 + self.tab_scroll_x + tw - close_btn_width + 4.0,
                close_width: 16.0,
            });
            tab_x += tw + gap;
        }
    }

    pub(super) fn render_tab_bar(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        unsafe {
            let bg_color = if self.theme.glass_enabled {
                self.theme.tab_inactive_bg
            } else {
                color_f(0.145, 0.145, 0.149, 1.0)
            };
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bg_color)
                .unwrap();
            let _active_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.tab_active_bg)
                .unwrap();
            let inactive_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.tab_inactive_bg)
                .unwrap();
            let hover_color = if self.theme.glass_enabled {
                color_f(0.25, 0.25, 0.27, 0.85)
            } else {
                color_f(0.22, 0.22, 0.24, 1.0)
            };
            let hover_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_color)
                .unwrap();
            let text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.text_default)
                .unwrap();
            let active_text_color = color_f(1.0, 1.0, 1.0, 1.0);
            let active_text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &active_text_color)
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
            // 活动标签发光颜色（玻璃模式下 brighter glow）
            let glow_color = if self.theme.glass_enabled {
                color_f(0.35, 0.35, 0.38, 0.90)
            } else {
                color_f(0.22, 0.22, 0.24, 1.0)
            };
            let glow_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &glow_color)
                .unwrap();

            // SubTask 7.3: 关闭按钮矢量图标颜色 — 默认灰，hover 白
            let close_default_color = color_f(180.0 / 255.0, 180.0 / 255.0, 180.0 / 255.0, 1.0);
            let close_default_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &close_default_color)
                .unwrap();
            let close_hover_icon_color = color_f(1.0, 1.0, 1.0, 1.0);
            let close_hover_icon_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &close_hover_icon_color)
                .unwrap();
            // 关闭按钮 hover 时的圆角矩形背景
            let close_hover_bg_color = color_f(0.4, 0.4, 0.4, 1.0);
            let close_hover_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &close_hover_bg_color)
                .unwrap();

            // SubTask 7.4: dirty 圆点画刷（金黄色 RGBA(255,200,0,255)）
            let dirty_color = color_f(1.0, 200.0 / 255.0, 0.0, 1.0);
            let dirty_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &dirty_color)
                .unwrap();

            // SubTask 7.2/7.3: 确保矢量图标几何已创建（Plus / Close）
            self.icons.ensure_created_from_target(target);

            // 背景
            let bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&bg_rect, &bg_brush);

            let tab_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();

            let mut tab_x = x + 4.0 - self.tab_scroll_x;
            let close_btn_width = 20.0;
            let gap = 2.0;
            // SubTask 7.2: 记录最后一个标签右侧位置，用于定位 "+" 按钮
            let mut last_tab_right = tab_x;

            for (i, tab) in self.tabs.iter().enumerate() {
                let is_active = i == self.active_tab;
                let is_hover = self.hover_tab == Some(i);
                let tw = if i < self.tab_layouts.len() {
                    self.tab_layouts[i].width
                } else {
                    100.0
                };
                // 活动标签延伸到标签栏底部，与编辑器背景无缝衔接
                let tab_rect = D2D_RECT_F {
                    left: tab_x,
                    top: y + 2.0,
                    right: tab_x + tw,
                    bottom: if is_active {
                        y + height
                    } else {
                        y + height - 2.0
                    },
                };

                // 标签背景 — 玻璃模式下活动标签使用更亮的 elevated surface
                let bg = if is_active {
                    &glow_brush
                } else if is_hover {
                    &hover_bg_brush
                } else {
                    &inactive_bg_brush
                };
                target.FillRectangle(&tab_rect, bg);

                // 活动标签顶部高亮线
                if is_active {
                    let top_line = D2D_RECT_F {
                        left: tab_x,
                        top: y + 2.0,
                        right: tab_x + tw,
                        bottom: y + 4.0,
                    };
                    target.FillRectangle(&top_line, &active_text_brush);
                }

                // 文件名
                // REQ-P1-09: 活动文件标签页的状态在 self.content 中，需从中读取。
                // 但设置/欢迎等非文件标签没有独立 content，self.content 可能残留上一个
                // 文件的内容，若直接用 self.content.file_name() 会导致活动的“设置”标签
                // 错误显示成某个文件名。故非文件标签一律用标签自身标题。
                // SubTask 7.4: 不再在文件名中拼接 "●"，改为独立小圆点
                let (name, is_dirty) = if is_active {
                    if tab.is_file() {
                        (self.content.file_name(), self.content.is_dirty)
                    } else {
                        (tab.title(), false)
                    }
                } else {
                    (tab.file_name(), tab.is_dirty())
                };
                let name_wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
                let text_rect = D2D_RECT_F {
                    left: tab_x + 10.0,
                    top: y + 2.0,
                    right: tab_x + tw - close_btn_width - 4.0,
                    bottom: if is_active {
                        y + height
                    } else {
                        y + height - 2.0
                    },
                };
                target.DrawText(
                    &name_wide,
                    &tab_format,
                    &text_rect,
                    if is_active {
                        &active_text_brush
                    } else {
                        &text_brush
                    },
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // SubTask 7.4: dirty 状态独立小圆点（6x6 填充椭圆，金黄色）
                // 位置：文件名右侧、关闭按钮左侧
                if is_dirty {
                    let dot_cx = tab_x + tw - close_btn_width - 4.0 - 3.0;
                    let dot_cy = y + height / 2.0;
                    let dot_ellipse = windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                            x: dot_cx,
                            y: dot_cy,
                        },
                        radiusX: 3.0,
                        radiusY: 3.0,
                    };
                    target.FillEllipse(&dot_ellipse, &dirty_brush);
                }

                // SubTask 7.3: 关闭按钮 — 矢量图标 IconKind::Close（12x12，居中于 20x20 点击区域）
                let close_click_size = 20.0f32;
                let close_icon_size = 12.0f32;
                let close_x = tab_x + tw - close_btn_width + 4.0;
                // 20x20 点击区域：以 close_x 为左边界
                let close_click_left = close_x - 4.0;
                let close_click_top = y + (height - close_click_size) / 2.0;
                // hover 时背景圆角矩形高亮
                if is_hover {
                    let close_bg_rect = D2D_RECT_F {
                        left: close_click_left,
                        top: close_click_top,
                        right: close_click_left + close_click_size,
                        bottom: close_click_top + close_click_size,
                    };
                    let rounded_rect = windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                        rect: close_bg_rect,
                        radiusX: 3.0,
                        radiusY: 3.0,
                    };
                    target.FillRoundedRectangle(&rounded_rect, &close_hover_bg_brush);
                }
                // 矢量图标：默认 RGBA(180,180,180,255)，hover 时 RGBA(255,255,255,255)
                let close_icon_brush = if is_hover {
                    &close_hover_icon_brush
                } else {
                    &close_default_brush
                };
                let close_icon_x = close_click_left + (close_click_size - close_icon_size) / 2.0;
                let close_icon_y = close_click_top + (close_click_size - close_icon_size) / 2.0;
                self.icons.draw(
                    target,
                    crate::icons::IconKind::Close,
                    close_icon_x,
                    close_icon_y,
                    close_icon_size,
                    close_icon_size,
                    close_icon_brush,
                );

                tab_x += tw + gap;
                last_tab_right = tab_x;
            }

            // Task 8.5: 拖拽插入指示线（蓝色 2px 垂直线）
            if let (Some(drag_idx), Some(drop_idx)) = (self.dragging_tab, self.tab_drop_index) {
                if drag_idx < self.tabs.len() && drop_idx <= self.tabs.len() {
                    let drop_line_color = color_f(100.0 / 255.0, 150.0 / 255.0, 1.0, 1.0);
                    let drop_line_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &drop_line_color)
                        .unwrap();
                    let mut line_x = x + 4.0 - self.tab_scroll_x;
                    for i in 0..drop_idx.min(self.tab_layouts.len()) {
                        line_x += self.tab_layouts[i].width + gap;
                    }
                    let line_rect = D2D_RECT_F {
                        left: line_x - 1.0,
                        top: y + 2.0,
                        right: line_x + 1.0,
                        bottom: y + height,
                    };
                    target.FillRectangle(&line_rect, &drop_line_brush);
                }
            }

            // SubTask 7.2: 标签栏右侧 "+" 新建标签按钮（28x28）
            let plus_btn_size = 28.0f32;
            let plus_gap = 8.0;
            let plus_x = last_tab_right + plus_gap;
            let plus_y = y + (height - plus_btn_size) / 2.0;
            let plus_right = plus_x + plus_btn_size;
            let plus_bottom = plus_y + plus_btn_size;
            // 仅在有足够空间时渲染并更新命中区域
            if plus_right <= x + width {
                if self.plus_button_hover {
                    let plus_bg_rect = D2D_RECT_F {
                        left: plus_x,
                        top: plus_y,
                        right: plus_right,
                        bottom: plus_bottom,
                    };
                    let rounded_rect = windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                        rect: plus_bg_rect,
                        radiusX: 4.0,
                        radiusY: 4.0,
                    };
                    target.FillRoundedRectangle(&rounded_rect, &hover_bg_brush);
                }
                let plus_icon_color = if self.plus_button_hover {
                    color_f(1.0, 1.0, 1.0, 1.0)
                } else {
                    color_f(0.7, 0.7, 0.7, 1.0)
                };
                let plus_icon_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &plus_icon_color)
                    .unwrap();
                let plus_icon_size = 16.0f32;
                self.icons.draw(
                    target,
                    crate::icons::IconKind::Plus,
                    plus_x + (plus_btn_size - plus_icon_size) / 2.0,
                    plus_y + (plus_btn_size - plus_icon_size) / 2.0,
                    plus_icon_size,
                    plus_icon_size,
                    &plus_icon_brush,
                );
                self.plus_button_rect = Some((plus_x, plus_y, plus_right, plus_bottom));
            } else {
                self.plus_button_rect = None;
            }

            // 底部边框线
            let bottom_line = D2D_RECT_F {
                left: x,
                top: y + height - 1.0,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&bottom_line, &border_brush);
        }
    }
}
