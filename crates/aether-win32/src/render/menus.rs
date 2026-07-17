use super::*;

impl EditorState {
    /// 渲染资源管理器空白区域上下文菜单。
    ///
    /// 复用 user_menu 的视觉风格（背景、阴影、边框、hover 高亮），
    /// 但无用户名头部，菜单从顶部 padding 开始直接排列菜单项。
    pub(super) fn render_explorer_context_menu(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
    ) {
        use crate::context_menu::ExplorerContextMenu;

        unsafe {
            let menu_width = self.explorer_context_menu.menu_width();
            let menu_height = self.explorer_context_menu.menu_height();
            let menu_x = self.explorer_context_menu.origin_x;
            let menu_y = self.explorer_context_menu.origin_y;

            // 背景
            let bg_color = if self.theme.glass_enabled {
                self.theme.submenu_bg
            } else {
                color_f(0.18, 0.18, 0.18, 1.0)
            };
            let bg_brush = match self.render_ctx.brush_cache.get_brush(target, &bg_color) {
                Ok(b) => b,
                Err(_) => return,
            };
            let menu_rect = D2D_RECT_F {
                left: menu_x,
                top: menu_y,
                right: menu_x + menu_width,
                bottom: menu_y + menu_height,
            };

            // 阴影（右侧 + 底部，与 user_menu 一致）
            let shadow_color = color_f(0.0, 0.0, 0.0, 0.35);
            if let Ok(shadow_brush) = self.render_ctx.brush_cache.get_brush(target, &shadow_color) {
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
            }

            target.FillRectangle(&menu_rect, &bg_brush);

            // 边框
            let border_color = color_f(0.3, 0.3, 0.3, 1.0);
            if let Ok(border_brush) = self.render_ctx.brush_cache.get_brush(target, &border_color) {
                target.DrawRectangle(&menu_rect, &border_brush, 1.0, None);
            }

            // 保存菜单区域供 hit_test 使用
            self.explorer_context_menu.menu_rect = Some(crate::layout::Region::new(
                menu_x,
                menu_y,
                menu_width,
                menu_height,
            ));

            let text_color = color_f(0.85, 0.85, 0.85, 1.0);
            let text_brush = match self.render_ctx.brush_cache.get_brush(target, &text_color) {
                Ok(b) => b,
                Err(_) => return,
            };
            let hover_bg = color_f(0.0, 0.47, 0.83, 1.0);
            let hover_brush = match self.render_ctx.brush_cache.get_brush(target, &hover_bg) {
                Ok(b) => b,
                Err(_) => return,
            };
            let sep_color = color_f(0.3, 0.3, 0.3, 1.0);
            let sep_brush = match self.render_ctx.brush_cache.get_brush(target, &sep_color) {
                Ok(b) => b,
                Err(_) => return,
            };

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

            // 从顶部 padding 开始绘制菜单项
            let mut current_y = menu_y + ExplorerContextMenu::TOP_PADDING;
            for (i, item) in self.explorer_context_menu.items.iter().enumerate() {
                if item.is_separator() {
                    let sep_rect = D2D_RECT_F {
                        left: menu_x + 8.0,
                        top: current_y + 4.0,
                        right: menu_x + menu_width - 8.0,
                        bottom: current_y + 5.0,
                    };
                    target.FillRectangle(&sep_rect, &sep_brush);
                    current_y += ExplorerContextMenu::SEPARATOR_HEIGHT;
                } else {
                    let is_hover = self.explorer_context_menu.hover_index == Some(i);
                    if is_hover {
                        let item_rect = D2D_RECT_F {
                            left: menu_x + 4.0,
                            top: current_y,
                            right: menu_x + menu_width - 4.0,
                            bottom: current_y + ExplorerContextMenu::ITEM_HEIGHT,
                        };
                        target.FillRectangle(&item_rect, &hover_brush);
                    }

                    let label_wide: Vec<u16> = item.label().encode_utf16().chain(Some(0)).collect();
                    let label_rect = D2D_RECT_F {
                        left: menu_x + 16.0,
                        top: current_y,
                        right: menu_x + menu_width - 16.0,
                        bottom: current_y + ExplorerContextMenu::ITEM_HEIGHT,
                    };
                    target.DrawText(
                        &label_wide,
                        &text_format,
                        &label_rect,
                        &text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    current_y += ExplorerContextMenu::ITEM_HEIGHT;
                }
            }
        }
    }

    /// 标签右键上下文菜单渲染。
    ///
    /// - 背景：圆角半透明矩形 RGBA(40,44,52,240)
    /// - 边框：1px RGBA(80,80,80,255)
    /// - 普通项：文本 RGBA(220,220,220,255)
    /// - hover 项：背景 RGBA(80,120,200,200)，文本 RGBA(255,255,255,255)
    /// - disabled 项：文本 RGBA(120,120,120,255)
    /// - 分隔符：1px 水平线 RGBA(80,80,80,200)
    pub(super) fn render_tab_context_menu(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
    ) {
        unsafe {
            let menu_width = self.tab_context_menu.width;
            let menu_height = self.tab_context_menu.menu_height();
            let menu_x = self.tab_context_menu.x;
            let menu_y = self.tab_context_menu.y;

            // 背景：圆角半透明矩形
            let bg_color = color_f(40.0 / 255.0, 44.0 / 255.0, 52.0 / 255.0, 240.0 / 255.0);
            let bg_brush = match self.render_ctx.brush_cache.get_brush(target, &bg_color) {
                Ok(b) => b,
                Err(_) => return,
            };
            let menu_rect = D2D_RECT_F {
                left: menu_x,
                top: menu_y,
                right: menu_x + menu_width,
                bottom: menu_y + menu_height,
            };
            let rounded_rect = windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                rect: menu_rect,
                radiusX: 4.0,
                radiusY: 4.0,
            };
            target.FillRoundedRectangle(&rounded_rect, &bg_brush);

            // 边框：1px 细线
            let border_color = color_f(80.0 / 255.0, 80.0 / 255.0, 80.0 / 255.0, 1.0);
            if let Ok(border_brush) = self.render_ctx.brush_cache.get_brush(target, &border_color) {
                target.DrawRoundedRectangle(&rounded_rect, &border_brush, 1.0, None);
            }

            // 阴影（右侧 + 底部，与其他菜单一致）
            let shadow_color = color_f(0.0, 0.0, 0.0, 0.35);
            if let Ok(shadow_brush) = self.render_ctx.brush_cache.get_brush(target, &shadow_color) {
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
            }

            // 文本画刷
            let normal_text_color = color_f(220.0 / 255.0, 220.0 / 255.0, 220.0 / 255.0, 1.0);
            let normal_text_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &normal_text_color)
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let hover_text_color = color_f(1.0, 1.0, 1.0, 1.0);
            let hover_text_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_text_color)
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let disabled_text_color = color_f(120.0 / 255.0, 120.0 / 255.0, 120.0 / 255.0, 1.0);
            let disabled_text_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &disabled_text_color)
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let hover_bg_color = color_f(80.0 / 255.0, 120.0 / 255.0, 200.0 / 255.0, 200.0 / 255.0);
            let hover_bg_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_bg_color)
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let sep_color = color_f(80.0 / 255.0, 80.0 / 255.0, 80.0 / 255.0, 200.0 / 255.0);
            let sep_brush = match self.render_ctx.brush_cache.get_brush(target, &sep_color) {
                Ok(b) => b,
                Err(_) => return,
            };

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

            // 从顶部 padding 开始绘制菜单项
            let mut current_y = menu_y + self.tab_context_menu.top_padding;
            for (i, item) in self.tab_context_menu.items.iter().enumerate() {
                if item.is_separator() {
                    // 分隔符：1px 水平线
                    let sep_rect = D2D_RECT_F {
                        left: menu_x + 8.0,
                        top: current_y + (self.tab_context_menu.separator_height - 1.0) / 2.0,
                        right: menu_x + menu_width - 8.0,
                        bottom: current_y
                            + (self.tab_context_menu.separator_height - 1.0) / 2.0
                            + 1.0,
                    };
                    target.FillRectangle(&sep_rect, &sep_brush);
                    current_y += self.tab_context_menu.separator_height;
                } else {
                    let is_hover = self.tab_context_menu.hover_index == Some(i);
                    if is_hover {
                        // hover 项背景（圆角）
                        let item_rect = D2D_RECT_F {
                            left: menu_x + 3.0,
                            top: current_y,
                            right: menu_x + menu_width - 3.0,
                            bottom: current_y + self.tab_context_menu.item_height,
                        };
                        let item_rounded = windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                            rect: item_rect,
                            radiusX: 3.0,
                            radiusY: 3.0,
                        };
                        target.FillRoundedRectangle(&item_rounded, &hover_bg_brush);
                    }

                    // 文本
                    let label_wide: Vec<u16> = item.label.encode_utf16().chain(Some(0)).collect();
                    let label_rect = D2D_RECT_F {
                        left: menu_x + 12.0,
                        top: current_y,
                        right: menu_x + menu_width - 12.0,
                        bottom: current_y + self.tab_context_menu.item_height,
                    };
                    let text_brush = if !item.enabled {
                        &disabled_text_brush
                    } else if is_hover {
                        &hover_text_brush
                    } else {
                        &normal_text_brush
                    };
                    target.DrawText(
                        &label_wide,
                        &text_format,
                        &label_rect,
                        text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    current_y += self.tab_context_menu.item_height;
                }
            }
        }
    }

    /// 渲染活动栏右键上下文菜单。
    ///
    /// 视觉风格与 `render_tab_context_menu` 一致：
    /// - 背景：圆角半透明矩形 RGBA(40,44,52,240)
    /// - 边框：1px RGBA(80,80,80,255)
    /// - hover 项：背景 RGBA(80,120,200,200)
    /// - disabled 项：文本灰化
    /// - 分隔符：1px 水平线
    /// - checked 项：左侧绘制 ✓ 勾选标记
    pub(super) fn render_activity_bar_context_menu(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
    ) {
        unsafe {
            let menu_width = self.activity_bar_context_menu.width;
            let menu_height = self.activity_bar_context_menu.menu_height();
            let menu_x = self.activity_bar_context_menu.x;
            let menu_y = self.activity_bar_context_menu.y;

            // 背景：圆角半透明矩形
            let bg_color = color_f(40.0 / 255.0, 44.0 / 255.0, 52.0 / 255.0, 240.0 / 255.0);
            let bg_brush = match self.render_ctx.brush_cache.get_brush(target, &bg_color) {
                Ok(b) => b,
                Err(_) => return,
            };
            let menu_rect = D2D_RECT_F {
                left: menu_x,
                top: menu_y,
                right: menu_x + menu_width,
                bottom: menu_y + menu_height,
            };
            let rounded_rect = windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                rect: menu_rect,
                radiusX: 4.0,
                radiusY: 4.0,
            };
            target.FillRoundedRectangle(&rounded_rect, &bg_brush);

            // 边框：1px 细线
            let border_color = color_f(80.0 / 255.0, 80.0 / 255.0, 80.0 / 255.0, 1.0);
            if let Ok(border_brush) = self.render_ctx.brush_cache.get_brush(target, &border_color) {
                target.DrawRoundedRectangle(&rounded_rect, &border_brush, 1.0, None);
            }

            // 阴影（右侧 + 底部，与其他菜单一致）
            let shadow_color = color_f(0.0, 0.0, 0.0, 0.35);
            if let Ok(shadow_brush) = self.render_ctx.brush_cache.get_brush(target, &shadow_color) {
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
            }

            // 文本画刷
            let normal_text_color = color_f(220.0 / 255.0, 220.0 / 255.0, 220.0 / 255.0, 1.0);
            let normal_text_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &normal_text_color)
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let hover_text_color = color_f(1.0, 1.0, 1.0, 1.0);
            let hover_text_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_text_color)
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let disabled_text_color = color_f(120.0 / 255.0, 120.0 / 255.0, 120.0 / 255.0, 1.0);
            let disabled_text_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &disabled_text_color)
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let hover_bg_color = color_f(80.0 / 255.0, 120.0 / 255.0, 200.0 / 255.0, 200.0 / 255.0);
            let hover_bg_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_bg_color)
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let sep_color = color_f(80.0 / 255.0, 80.0 / 255.0, 80.0 / 255.0, 200.0 / 255.0);
            let sep_brush = match self.render_ctx.brush_cache.get_brush(target, &sep_color) {
                Ok(b) => b,
                Err(_) => return,
            };
            // 勾选标记画刷（使用 hover 文本色）
            let check_color = color_f(180.0 / 255.0, 220.0 / 255.0, 1.0, 1.0);
            let check_brush = match self.render_ctx.brush_cache.get_brush(target, &check_color) {
                Ok(b) => b,
                Err(_) => return,
            };

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

            // 从顶部 padding 开始绘制菜单项
            let mut current_y = menu_y + self.activity_bar_context_menu.top_padding;
            for (i, item) in self.activity_bar_context_menu.items.iter().enumerate() {
                if item.is_separator() {
                    // 分隔符：1px 水平线
                    let sep_rect = D2D_RECT_F {
                        left: menu_x + 8.0,
                        top: current_y
                            + (self.activity_bar_context_menu.separator_height - 1.0) / 2.0,
                        right: menu_x + menu_width - 8.0,
                        bottom: current_y
                            + (self.activity_bar_context_menu.separator_height - 1.0) / 2.0
                            + 1.0,
                    };
                    target.FillRectangle(&sep_rect, &sep_brush);
                    current_y += self.activity_bar_context_menu.separator_height;
                } else {
                    let is_hover = self.activity_bar_context_menu.hover_index == Some(i);
                    if is_hover {
                        // hover 项背景（圆角）
                        let item_rect = D2D_RECT_F {
                            left: menu_x + 3.0,
                            top: current_y,
                            right: menu_x + menu_width - 3.0,
                            bottom: current_y + self.activity_bar_context_menu.item_height,
                        };
                        let item_rounded = windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                            rect: item_rect,
                            radiusX: 3.0,
                            radiusY: 3.0,
                        };
                        target.FillRoundedRectangle(&item_rounded, &hover_bg_brush);
                    }

                    // 勾选标记：checked 项在左侧绘制 ✓
                    if item.checked {
                        let cx = menu_x + 12.0;
                        let cy = current_y;
                        // ✓ 由两段线段构成：下笔 → 底部 → 右上
                        let p0 = D2D_POINT_2F {
                            x: cx,
                            y: cy + 15.0,
                        };
                        let p1 = D2D_POINT_2F {
                            x: cx + 4.0,
                            y: cy + 19.0,
                        };
                        let p2 = D2D_POINT_2F {
                            x: cx + 10.0,
                            y: cy + 11.0,
                        };
                        target.DrawLine(p0, p1, &check_brush, 1.5, None);
                        target.DrawLine(p1, p2, &check_brush, 1.5, None);
                    }

                    // 文本（统一缩进 32px，为勾选标记预留空间）
                    let label_wide: Vec<u16> = item.label.encode_utf16().chain(Some(0)).collect();
                    let label_rect = D2D_RECT_F {
                        left: menu_x + 32.0,
                        top: current_y,
                        right: menu_x + menu_width - 12.0,
                        bottom: current_y + self.activity_bar_context_menu.item_height,
                    };
                    let text_brush = if !item.enabled {
                        &disabled_text_brush
                    } else if is_hover {
                        &hover_text_brush
                    } else {
                        &normal_text_brush
                    };
                    target.DrawText(
                        &label_wide,
                        &text_format,
                        &label_rect,
                        text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    current_y += self.activity_bar_context_menu.item_height;
                }
            }
        }
    }

    /// REQ-P3-02: 测量子菜单宽度（逻辑像素）
    ///
    /// 遍历菜单项的 label 与 shortcut，使用 DirectWrite 精确测量文本宽度，
    /// 取最大行宽加上内边距作为子菜单宽度。返回值供 hit_test 与 render 复用。
    pub(super) fn measure_submenu_width(
        &mut self,
        menu_item: &crate::menu_bar::MenuBarItem,
    ) -> f32 {
        const LABEL_FONT_SIZE: f32 = 13.0;
        const SHORTCUT_FONT_SIZE: f32 = 12.0;
        // 内边距：左 12 + 右 12 + label/shortcut 间距 24
        const PADDING: f32 = 48.0;
        const MIN_MENU_WIDTH: f32 = 160.0;
        const FALLBACK_WIDTH: f32 = 220.0;

        let normal_weight = DWRITE_FONT_WEIGHT_NORMAL.0 as u32;
        let mut max_content_width: f32 = 0.0;

        for item in &menu_item.items {
            if item.label == "-" {
                continue;
            }
            let label_w = self
                .render_ctx
                .text_format_cache
                .measure_text_width(&item.label, LABEL_FONT_SIZE, normal_weight)
                .unwrap_or(0.0);
            let shortcut_w = item
                .shortcut
                .as_ref()
                .and_then(|s| {
                    self.render_ctx.text_format_cache.measure_text_width(
                        s,
                        SHORTCUT_FONT_SIZE,
                        normal_weight,
                    )
                })
                .unwrap_or(0.0);
            let row_w = label_w + 24.0 + shortcut_w;
            if row_w > max_content_width {
                max_content_width = row_w;
            }
        }

        // 若测量失败（所有项均无内容），回退到默认宽度
        if max_content_width <= 0.0 {
            return FALLBACK_WIDTH;
        }
        (max_content_width + PADDING).max(MIN_MENU_WIDTH)
    }

    pub(super) fn render_submenu(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        menu_item: &crate::menu_bar::MenuBarItem,
    ) {
        unsafe {
            // 子菜单需要保证可读性，背景强制不透明，避免后面文件树/编辑器内容干扰
            let bg_color = if self.theme.glass_enabled {
                let mut c = self.theme.submenu_bg;
                c.a = 1.0;
                c
            } else {
                color_f(0.18, 0.18, 0.18, 1.0)
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
            let disabled_color = color_f(0.5, 0.5, 0.5, 1.0);
            let disabled_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &disabled_color)
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

            let text_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let shortcut_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();

            // REQ-P3-02: 子菜单宽度由调用方测量后写入 menu_item.submenu_width
            // 此处直接读取，避免在渲染函数内重复测量造成借用冲突
            let menu_width = if menu_item.submenu_width > 0.0 {
                menu_item.submenu_width
            } else {
                220.0
            };

            let mut total_height = 8.0;
            for item in &menu_item.items {
                total_height += if item.label == "-" { 8.0 } else { 26.0 };
            }
            total_height += 8.0;

            let bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + menu_width,
                bottom: y + total_height,
            };
            target.FillRectangle(&bg_rect, &bg_brush);

            // 玻璃模式下添加边框和阴影
            if self.theme.glass_enabled {
                let border_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &self.theme.panel_border)
                    .unwrap();
                let top_border = D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + menu_width,
                    bottom: y + 1.0,
                };
                target.FillRectangle(&top_border, &border_brush);
                let bottom_border = D2D_RECT_F {
                    left: x,
                    top: y + total_height - 1.0,
                    right: x + menu_width,
                    bottom: y + total_height,
                };
                target.FillRectangle(&bottom_border, &border_brush);
                let _ = glass::draw_panel_shadow(
                    target,
                    &mut self.render_ctx.brush_cache,
                    &bg_rect,
                    &self.theme.shadow,
                    4.0,
                );
            }

            let mut item_y = y + 8.0;
            for item in &menu_item.items {
                if item.label == "-" {
                    let sep_rect = D2D_RECT_F {
                        left: x + 10.0,
                        top: item_y + 3.0,
                        right: x + menu_width - 10.0,
                        bottom: item_y + 5.0,
                    };
                    target.FillRectangle(&sep_rect, &sep_brush);
                    item_y += 8.0;
                } else {
                    let brush = if item.enabled {
                        &text_brush
                    } else {
                        &disabled_brush
                    };
                    let wide: Vec<u16> = item.label.encode_utf16().chain(Some(0)).collect();
                    let text_rect = D2D_RECT_F {
                        left: x + 12.0,
                        top: item_y,
                        right: x + menu_width - 12.0,
                        bottom: item_y + 26.0,
                    };
                    target.DrawText(
                        &wide,
                        &text_format,
                        &text_rect,
                        brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    if let Some(shortcut) = &item.shortcut {
                        let sc_wide: Vec<u16> = shortcut.encode_utf16().chain(Some(0)).collect();
                        let sc_rect = D2D_RECT_F {
                            left: x + menu_width - 100.0,
                            top: item_y,
                            right: x + menu_width - 12.0,
                            bottom: item_y + 26.0,
                        };
                        target.DrawText(
                            &sc_wide,
                            &shortcut_format,
                            &sc_rect,
                            brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }

                    item_y += 26.0;
                }
            }
        }
    }

    /// 渲染命令面板
    pub(super) fn render_command_palette(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
    ) {
        unsafe {
            let input_height = 40.0;
            let item_height = 36.0;
            let visible_count = self.command_palette.visible_count();
            let total_height = input_height + (visible_count as f32 * item_height) + 16.0;

            let bg_color = if self.theme.glass_enabled {
                self.theme.command_palette_bg
            } else {
                color_f(0.18, 0.18, 0.18, 1.0)
            };
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bg_color)
                .unwrap();
            let border_color = color_f(0.0, 0.47, 0.83, 1.0);
            let border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &border_color)
                .unwrap();
            let input_bg_color = if self.theme.glass_enabled {
                color_f(0.12, 0.12, 0.12, 0.85)
            } else {
                color_f(0.12, 0.12, 0.12, 1.0)
            };
            let input_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &input_bg_color)
                .unwrap();
            let text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.text_default)
                .unwrap();
            let selected_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &border_color)
                .unwrap();
            let desc_color = color_f(0.6, 0.6, 0.6, 1.0);
            let desc_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &desc_color)
                .unwrap();
            let shortcut_color = color_f(0.5, 0.5, 0.5, 1.0);
            let shortcut_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &shortcut_color)
                .unwrap();

            let bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + total_height,
            };
            target.FillRectangle(&bg_rect, &bg_brush);

            // 玻璃模式下添加边框和阴影
            if self.theme.glass_enabled {
                let panel_border = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &self.theme.panel_border)
                    .unwrap();
                let top_border = D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + width,
                    bottom: y + 1.0,
                };
                target.FillRectangle(&top_border, &panel_border);
                let bottom_border = D2D_RECT_F {
                    left: x,
                    top: y + total_height - 1.0,
                    right: x + width,
                    bottom: y + total_height,
                };
                target.FillRectangle(&bottom_border, &panel_border);
                let _ = glass::draw_panel_shadow(
                    target,
                    &mut self.render_ctx.brush_cache,
                    &bg_rect,
                    &self.theme.shadow,
                    6.0,
                );
            }

            let border_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + 2.0,
            };
            target.FillRectangle(&border_rect, &border_brush);

            let input_rect = D2D_RECT_F {
                left: x + 8.0,
                top: y + 8.0,
                right: x + width - 8.0,
                bottom: y + input_height - 4.0,
            };
            target.FillRectangle(&input_rect, &input_bg_brush);

            let input_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    14.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let query = self.command_palette.query.clone();
            let query_wide: Vec<u16> = query.encode_utf16().chain(Some(0)).collect();
            let query_rect = D2D_RECT_F {
                left: x + 16.0,
                top: y + 10.0,
                right: x + width - 16.0,
                bottom: y + input_height - 6.0,
            };
            target.DrawText(
                &query_wide,
                &input_format,
                &query_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            let item_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let desc_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    11.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let shortcut_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    11.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();

            // 确保矢量图标几何已创建
            self.icons.ensure_created_from_target(target);

            for i in 0..visible_count {
                let item_y = y + input_height + 8.0 + (i as f32 * item_height);
                let is_selected = i == self.command_palette.selected_index;

                if is_selected {
                    let sel_rect = D2D_RECT_F {
                        left: x + 4.0,
                        top: item_y,
                        right: x + width - 4.0,
                        bottom: item_y + item_height,
                    };
                    target.FillRectangle(&sel_rect, &selected_brush);
                }

                if let Some(item) = self.command_palette.get_item(i) {
                    // 前置矢量图标
                    let mut text_left = x + 16.0;
                    if let Some(icon_kind) = item.icon {
                        let icon_size = 18.0f32;
                        let icon_y = item_y + (item_height - icon_size) / 2.0;
                        self.icons.draw(
                            target,
                            icon_kind,
                            x + 16.0,
                            icon_y,
                            icon_size,
                            icon_size,
                            &text_brush,
                        );
                        text_left = x + 16.0 + icon_size + 8.0;
                    }

                    let label_wide: Vec<u16> = item.label.encode_utf16().chain(Some(0)).collect();
                    let label_rect = D2D_RECT_F {
                        left: text_left,
                        top: item_y + 4.0,
                        right: x + width - 100.0,
                        bottom: item_y + 22.0,
                    };
                    target.DrawText(
                        &label_wide,
                        &item_format,
                        &label_rect,
                        &text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    if let Some(desc) = &item.description {
                        let desc_wide: Vec<u16> = desc.encode_utf16().chain(Some(0)).collect();
                        let desc_rect = D2D_RECT_F {
                            left: text_left,
                            top: item_y + 20.0,
                            right: x + width - 100.0,
                            bottom: item_y + 34.0,
                        };
                        target.DrawText(
                            &desc_wide,
                            &desc_format,
                            &desc_rect,
                            &desc_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }

                    if let Some(shortcut) = &item.shortcut {
                        let sc_wide: Vec<u16> = shortcut.encode_utf16().chain(Some(0)).collect();
                        let sc_rect = D2D_RECT_F {
                            left: x + width - 90.0,
                            top: item_y + 10.0,
                            right: x + width - 16.0,
                            bottom: item_y + 26.0,
                        };
                        target.DrawText(
                            &sc_wide,
                            &shortcut_format,
                            &sc_rect,
                            &shortcut_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                }
            }
        }
    }
}
