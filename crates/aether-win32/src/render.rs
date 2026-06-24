use aether_core::lexer::Language;
use aether_core::workspace::file_tree::{FileKind, FileTree};
use aether_render::d2d::factory::color_f;
use aether_render::d2d::glass;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush;
use windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE;
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
};

use crate::editor::EditorState;
use crate::layout::Region;

impl EditorState {
    pub fn render(&mut self) {
        // 避免0尺寸渲染
        if self.window_width == 0 || self.window_height == 0 {
            return;
        }

        tracing::debug!("=== NEW BINARY V3 MARKER ===");
        tracing::debug!(
            win_w = self.window_width,
            win_h = self.window_height,
            showing_welcome = self.show_welcome(),
            right_panel_visible = self.layout.right_panel_visible,
            "[v2] render() start"
        );

        // 确保渲染目标存在（设备丢失后重建）
        if self.render_ctx.target.is_none() {
            let _ = self.init_render_target();
            // 渲染目标就绪后预初始化常用画笔和文本格式
            if let Some(rt) = &self.render_ctx.target {
                let target = rt.target().clone();
                let common_colors = [
                    self.theme.editor_bg,
                    self.theme.line_number_bg,
                    self.theme.line_number_fg,
                    self.theme.line_highlight_bg,
                    self.theme.selection_bg,
                    self.theme.cursor_color,
                    self.theme.sidebar_bg,
                    self.theme.statusbar_bg,
                    self.theme.text_default,
                    self.theme.tab_active_bg,
                    self.theme.tab_inactive_bg,
                    self.theme.titlebar_bg,
                    self.theme.activity_bar_bg,
                    self.theme.panel_border,
                    self.theme.shadow,
                    self.theme.glow_selection,
                    self.theme.command_palette_bg,
                    self.theme.submenu_bg,
                ];
                self.render_ctx
                    .brush_cache
                    .init_common_brushes(&target, &common_colors);
                let font_size = self.text_renderer.font_size();
                self.render_ctx
                    .text_format_cache
                    .init_common_formats(font_size);
            }
        }

        // 计算编辑器可见行范围，用于增量缓存重建
        let has_multiple_tabs = self.tabs.len() > 1;
        let editor_content_region = self.layout.editor_content_region(has_multiple_tabs);
        let line_height = self.text_renderer.line_height();
        let total_lines = self.buffer.len_lines().max(1);
        let visible_start = (self.scroll_y / line_height) as usize;
        let visible_lines = (editor_content_region.height / line_height) as usize + 2;
        let visible_end = (visible_start + visible_lines).min(total_lines);

        self.rebuild_cache(visible_start, visible_end);

        // 使用布局管理器计算各区域
        let titlebar_region = self.layout.title_bar_region();
        let menu_region = self.layout.menu_bar_region();
        let activity_region = self.layout.activity_bar_region();
        let sidebar_region = self.layout.sidebar_region();
        let editor_region = self.layout.editor_region();
        let tab_region = self.layout.tab_bar_region(has_multiple_tabs);
        let status_region = self.layout.status_bar_region();
        let right_panel_region = self.layout.right_panel_region();

        // 预计算标签栏布局
        if has_multiple_tabs {
            self.update_tab_layouts(editor_region.x, editor_region.width, tab_region.height);
        }

        // 预计算菜单栏 item 位置（用于子菜单定位和 hover 检测）
        // 菜单项现在绘制在标题栏内，从左侧开始，避开窗口控制按钮区域
        // 优化：只在 layout_dirty 时重建，避免每帧分配
        if self.menu_bar.layout_dirty {
            self.menu_bar.item_widths.clear();
            self.menu_bar.item_widths.reserve(self.menu_bar.items.len());
            for item in &self.menu_bar.items {
                let text_width: f32 = item
                    .label
                    .chars()
                    .map(|ch| if ch.is_ascii() { 8.0 } else { 13.0 })
                    .sum();
                let item_width = text_width + 24.0; // 左右各 12px padding
                self.menu_bar.item_widths.push(item_width);
            }
            self.menu_bar.layout_dirty = false;
        }
        // 每帧只需重新计算 x 位置（因为起始 x 可能随标题栏变化）
        {
            let mut item_x = titlebar_region.x + 8.0;
            self.menu_bar.item_x_positions.clear();
            self.menu_bar
                .item_x_positions
                .reserve(self.menu_bar.items.len());
            for (i, _item) in self.menu_bar.items.iter().enumerate() {
                let item_width = self.menu_bar.item_widths.get(i).copied().unwrap_or(60.0);
                self.menu_bar.item_x_positions.push(item_x);
                item_x += item_width;
            }
        }

        // 脏矩形检测：对比上一帧状态，标记变化区域
        let cursor_moved =
            self.cursor_line != self.last_cursor_line || self.cursor_col != self.last_cursor_col;
        let scroll_changed = (self.scroll_y - self.last_scroll_y).abs() > 0.01;
        let selection_changed = self.selection_start != self.last_selection_start
            || self.selection_end != self.last_selection_end;
        let sidebar_changed = self.sidebar_content != self.last_sidebar_content;
        let right_panel_changed = self.layout.right_panel_visible != self.last_right_panel_visible;
        let bottom_panel_changed =
            self.layout.bottom_panel_visible != self.last_bottom_panel_visible;
        let status_changed = self.status_message != self.last_status_message;
        let dialog_visible =
            self.ssh_dialog.visible || self.clone_dialog.visible || self.command_palette.visible;

        // 根据状态变化推断最优渲染命令
        let render_cmd = crate::dirty_rect::RenderCommand::infer_from_state(
            cursor_moved,
            selection_changed,
            false,
            scroll_changed,
            sidebar_changed,
            right_panel_changed,
            bottom_panel_changed,
            status_changed,
            dialog_visible,
        );

        // 标记脏区域：根据状态变化自动推断，同时保留外部显式标记
        // 如果已经有全窗口标记，不需要再推断
        if !self.dirty_tracker.is_full_window() {
            match render_cmd {
                crate::dirty_rect::RenderCommand::EditorOnly => {
                    let line_height = self.text_renderer.line_height();
                    let editor_content_region =
                        self.layout.editor_content_region(has_multiple_tabs);
                    let cursor_y = editor_content_region.y + self.cursor_line as f32 * line_height
                        - self.scroll_y;
                    self.dirty_tracker.mark_cursor(
                        editor_content_region.x,
                        cursor_y,
                        2.0,
                        line_height,
                    );
                }
                crate::dirty_rect::RenderCommand::EditorAndStatus => {
                    self.dirty_tracker.mark_region(
                        editor_region.x,
                        editor_region.y,
                        editor_region.width,
                        editor_region.height,
                        crate::dirty_rect::DirtyRegionType::EditorContent,
                    );
                    self.dirty_tracker.mark_status_bar(
                        status_region.x,
                        status_region.y,
                        status_region.width,
                        status_region.height,
                    );
                }
                crate::dirty_rect::RenderCommand::SidebarOnly => {
                    self.dirty_tracker.mark_region(
                        sidebar_region.x,
                        sidebar_region.y,
                        sidebar_region.width,
                        sidebar_region.height,
                        crate::dirty_rect::DirtyRegionType::Sidebar,
                    );
                }
                crate::dirty_rect::RenderCommand::RightPanelOnly => {
                    self.dirty_tracker.mark_region(
                        right_panel_region.x,
                        right_panel_region.y,
                        right_panel_region.width,
                        right_panel_region.height,
                        crate::dirty_rect::DirtyRegionType::RightPanel,
                    );
                }
                crate::dirty_rect::RenderCommand::BottomPanelOnly => {
                    let bottom_region = self.layout.bottom_panel_region();
                    if bottom_region.height > 0.0 {
                        self.dirty_tracker.mark_region(
                            bottom_region.x,
                            bottom_region.y,
                            bottom_region.width,
                            bottom_region.height,
                            crate::dirty_rect::DirtyRegionType::BottomPanel,
                        );
                    }
                }
                crate::dirty_rect::RenderCommand::FullRedraw => {
                    self.dirty_tracker.mark_full_window();
                }
            }
        }

        // 获取渲染目标，开始绘制
        let target = {
            let Some(rt) = &self.render_ctx.target else {
                return;
            };
            rt.target().clone()
        };
        self.render_ctx.begin_draw();

        // 设置裁剪区域（脏矩形优化）
        let use_clip = !self.dirty_tracker.is_full_window() && self.dirty_tracker.has_dirty();
        if use_clip {
            if let Some(first_rect) = self.dirty_tracker.rects().first() {
                self.render_ctx.push_clip(
                    first_rect.x,
                    first_rect.y,
                    first_rect.width,
                    first_rect.height,
                );
            }
        }

        // 全窗口清除只在全窗口重绘时执行
        if self.dirty_tracker.is_full_window() || !use_clip {
            self.render_ctx.clear(&self.theme.editor_bg);
        }

        // 预提取菜单栏数据，避免借用冲突
        let item_x_positions = self.menu_bar.item_x_positions.clone();
        let item_widths = self.menu_bar.item_widths.clone();

        // 0. 标题栏（最先渲染，作为背景）
        if self.layout.title_bar_visible {
            self.render_title_bar(&target, &titlebar_region);
        }

        // 1. 菜单栏
        if self.layout.menu_bar_visible {
            self.render_menu_bar(&item_x_positions, &item_widths, &target, &menu_region);
        }

        // 2. 活动栏
        if self.layout.activity_bar_visible {
            self.render_activity_bar(&target, &activity_region);
        }

        // 3. 侧边栏（欢迎页显示时跳过，因欢迎页全屏覆盖）
        let showing_welcome = self.show_welcome();
        if self.layout.sidebar_visible && !showing_welcome {
            self.render_sidebar(&target, &sidebar_region);
        }

        // 4. 标签栏
        if has_multiple_tabs {
            self.render_tab_bar(
                &target,
                tab_region.x,
                tab_region.y,
                tab_region.width,
                tab_region.height,
            );
        }

        // 5. 编辑器内容/欢迎页/图片预览
        if showing_welcome {
            tracing::trace!("render: before welcome_page");
            // 欢迎页覆盖整个窗口内容区域（忽略侧边栏，类似 VS Code）
            let welcome_x = if self.layout.activity_bar_visible {
                self.layout.activity_bar_width
            } else {
                0.0
            };
            let welcome_width = (self.window_width as f32
                - welcome_x
                - if self.layout.right_panel_visible {
                    self.layout.right_panel_width
                } else {
                    0.0
                })
            .max(200.0);
            let welcome_y = self.layout.top_offset();
            let welcome_height = (self.window_height as f32
                - welcome_y
                - if self.layout.status_bar_visible {
                    self.layout.status_bar_height
                } else {
                    0.0
                })
            .max(200.0);
            self.render_welcome_page(&target, welcome_x, welcome_y, welcome_width, welcome_height);
            tracing::trace!("render: after welcome_page");
        } else if self.language == Language::Image {
            self.render_image_preview(
                &target,
                editor_content_region.x,
                editor_content_region.y,
                editor_content_region.width,
                editor_content_region.height,
            );
        } else {
            self.render_editor(
                &target,
                editor_content_region.x,
                editor_content_region.y,
                editor_content_region.width,
                editor_content_region.height,
            );
        }

        // 5.5 查找替换框
        if self.find_visible {
            self.render_find_replace(
                &target,
                editor_content_region.x,
                editor_content_region.y,
                editor_content_region.width,
            );
        }

        // 6. 右侧面板（AI面板等）
        if self.layout.right_panel_visible
            && right_panel_region.width > 1.0
            && right_panel_region.height > 1.0
        {
            tracing::trace!(region = ?right_panel_region, "render: before right_panel");
            self.render_right_panel(&target, &right_panel_region);
            tracing::trace!("render: after right_panel");
        }

        // 7. 底部面板（终端、输出等）
        if self.layout.bottom_panel_visible && !showing_welcome {
            let bottom_region = self.layout.bottom_panel_region();
            self.render_bottom_panel(
                &target,
                bottom_region.x,
                bottom_region.y,
                bottom_region.width,
                bottom_region.height,
            );
        }

        // 8. 状态栏
        if self.layout.status_bar_visible {
            self.render_statusbar(&target, &status_region);
        }

        // 8. 子菜单（最后渲染，避免被欢迎页/编辑器遮盖）
        // 预提取子菜单数据，避免借用冲突
        let submenu_data = self.menu_bar.active_index.and_then(|active_idx| {
            self.menu_bar
                .items
                .get(active_idx)
                .filter(|item| item.expanded)
                .map(|item| {
                    let submenu_x = self.menu_bar.item_x_positions.get(active_idx).copied();
                    (submenu_x, item.clone())
                })
        });
        if let Some((Some(submenu_x), item)) = submenu_data {
            // 子菜单从标题栏下方弹出
            let submenu_y = titlebar_region.y + titlebar_region.height;
            self.render_submenu(&target, submenu_x, submenu_y, &item);
        }

        // 8. 命令面板（最上层渲染）
        if self.command_palette.visible {
            let palette_width = 600.0;
            let palette_x = (self.window_width as f32 - palette_width) / 2.0;
            let palette_y = titlebar_region.y + titlebar_region.height + 20.0;
            self.render_command_palette(&target, palette_x, palette_y, palette_width);
        }

        // 9. SSH 连接对话框
        if self.ssh_dialog.visible {
            self.render_ssh_dialog(&target);
        }

        // 10. 克隆仓库对话框
        if self.clone_dialog.visible {
            self.render_clone_dialog(&target);
        }

        // 弹出裁剪区域（如果设置了）——必须在 end_draw 之前
        if use_clip {
            self.render_ctx.pop_clip();
        }

        match self.render_ctx.end_draw() {
            Ok(()) => {}
            Err(e) => {
                // 设备丢失（D2DERR_RECREATE_TARGET = 0x8899000C），需要重建渲染目标
                if e.code().0 as u32 == 0x8899000C {
                    self.render_ctx.handle_device_lost();
                    // 重建渲染目标并重新预初始化
                    let _ = self.init_render_target();
                    if let Some(rt) = self.render_ctx.target_ref() {
                        let target = rt.target().clone();
                        let common_colors = [
                            self.theme.editor_bg,
                            self.theme.line_number_bg,
                            self.theme.line_number_fg,
                            self.theme.line_highlight_bg,
                            self.theme.selection_bg,
                            self.theme.cursor_color,
                            self.theme.sidebar_bg,
                            self.theme.statusbar_bg,
                            self.theme.text_default,
                            self.theme.tab_active_bg,
                            self.theme.tab_inactive_bg,
                            self.theme.titlebar_bg,
                            self.theme.activity_bar_bg,
                            self.theme.panel_border,
                            self.theme.shadow,
                            self.theme.glow_selection,
                            self.theme.command_palette_bg,
                            self.theme.submenu_bg,
                        ];
                        self.render_ctx
                            .brush_cache
                            .init_common_brushes(&target, &common_colors);
                        let font_size = self.text_renderer.font_size();
                        self.render_ctx
                            .text_format_cache
                            .init_common_formats(font_size);
                    }
                }
            }
        }

        // 更新上一帧状态追踪
        self.last_cursor_line = self.cursor_line;
        self.last_cursor_col = self.cursor_col;
        self.last_scroll_y = self.scroll_y;
        self.last_selection_start = self.selection_start;
        self.last_selection_end = self.selection_end;
        self.last_sidebar_content = self.sidebar_content.clone();
        self.last_right_panel_visible = self.layout.right_panel_visible;
        self.last_bottom_panel_visible = self.layout.bottom_panel_visible;
        self.last_status_message.clone_from(&self.status_message);

        // 清除脏矩形标记（渲染完成）
        self.dirty_tracker.clear();
    }

    fn render_right_panel(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        region: &Region,
    ) {
        let x = region.x;
        let y = region.y;
        let width = region.width;
        let height = region.height;

        // 防护：尺寸无效时跳过渲染
        if width < 1.0 || height < 1.0 {
            return;
        }

        tracing::trace!(
            x = x,
            y = y,
            w = width,
            h = height,
            "render_right_panel enter"
        );

        unsafe {
            // 安全获取画刷，失败时跳过渲染（避免设备丢失时 panic）
            let bg_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.sidebar_bg)
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let border_color = if self.theme.glass_enabled {
                self.theme.panel_border
            } else {
                color_f(0.2, 0.2, 0.2, 1.0)
            };
            let border_brush = match self.render_ctx.brush_cache.get_brush(target, &border_color) {
                Ok(b) => b,
                Err(_) => return,
            };
            let text_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.text_default)
            {
                Ok(b) => b,
                Err(_) => return,
            };

            let bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&bg_rect, &bg_brush);

            // 右侧面板左边缘柔和边框
            let border_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + 1.0,
                bottom: y + height,
            };
            target.FillRectangle(&border_rect, &border_brush);

            // Glass 模式下添加微妙阴影
            if self.theme.glass_enabled {
                let _ = glass::draw_panel_shadow(
                    target,
                    &mut self.render_ctx.brush_cache,
                    &bg_rect,
                    &self.theme.shadow,
                    2.0,
                );
            }

            // 根据当前活动视图渲染右侧面板内容
            match &self.sidebar_content {
                crate::layout::SidebarContent::AiAssistantPanel => {
                    self.render_ai_assistant_sidebar(target, x, y, width, height, &text_brush);
                }
                _ => {
                    // 默认显示 AI 面板
                    self.render_ai_assistant_sidebar(target, x, y, width, height, &text_brush);
                }
            }
        }

        tracing::trace!("render_right_panel exit OK");
    }

    fn render_sidebar(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        region: &Region,
    ) {
        let x = region.x;
        let y = region.y;
        let width = region.width;
        let height = region.height;

        unsafe {
            // 安全获取画刷，失败时跳过渲染（避免设备丢失时 panic）
            let bg_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.sidebar_bg)
            {
                Ok(b) => b,
                Err(_) => return,
            };
            let border_color = if self.theme.glass_enabled {
                self.theme.panel_border
            } else {
                color_f(0.2, 0.2, 0.2, 1.0)
            };
            let border_brush = match self.render_ctx.brush_cache.get_brush(target, &border_color) {
                Ok(b) => b,
                Err(_) => return,
            };
            let text_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.text_default)
            {
                Ok(b) => b,
                Err(_) => return,
            };

            let bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&bg_rect, &bg_brush);

            // 侧边栏右边缘柔和边框
            let border_rect = D2D_RECT_F {
                left: x + width - 1.0,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&border_rect, &border_brush);

            // Glass 模式下添加微妙阴影，增加层次感
            if self.theme.glass_enabled {
                let _ = glass::draw_panel_shadow(
                    target,
                    &mut self.render_ctx.brush_cache,
                    &bg_rect,
                    &self.theme.shadow,
                    2.0,
                );
            }

            match &self.sidebar_content {
                crate::layout::SidebarContent::FileTree => {
                    self.render_file_tree_sidebar(target, x, y, width, height, &text_brush);
                }
                crate::layout::SidebarContent::SourceControlPanel => {
                    self.render_source_control_sidebar(target, x, y, width, height, &text_brush);
                }
                crate::layout::SidebarContent::AiAssistantPanel => {
                    self.render_ai_assistant_sidebar(target, x, y, width, height, &text_brush);
                }
                crate::layout::SidebarContent::SettingsPanel => {
                    self.render_settings_sidebar(target, x, y, width, height, &text_brush);
                }
                crate::layout::SidebarContent::RemoteFileTree => {
                    self.render_remote_file_tree_sidebar(target, x, y, width, height, &text_brush);
                }
                crate::layout::SidebarContent::TerminalPanel => {
                    // 终端面板在底部显示，侧边栏不渲染
                }
            }
        }
    }

    fn render_file_tree_sidebar(
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
            let tree_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let dir_color = color_f(0.9, 0.9, 0.9, 1.0);
            let dir_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &dir_color)
                .unwrap();

            if let Some(tree) = &self.file_tree {
                let mut current_y = y + 10.0 - self.sidebar_scroll_y;
                let sel_color = if self.theme.glass_enabled {
                    self.theme.glow_selection
                } else {
                    color_f(0.0, 0.47, 0.83, 1.0)
                };
                let sel_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &sel_color)
                    .unwrap();
                let hover_color = if self.theme.glass_enabled {
                    color_f(0.25, 0.25, 0.27, 0.70)
                } else {
                    color_f(0.2, 0.2, 0.2, 1.0)
                };
                let hover_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &hover_color)
                    .unwrap();
                self.render_tree_nodes(
                    target,
                    tree,
                    u32::MAX,
                    x + 10.0,
                    &mut current_y,
                    y,
                    height,
                    width,
                    &tree_format,
                    &text_brush,
                    &dir_brush,
                    &sel_brush,
                    &hover_brush,
                );
            } else {
                let text: Vec<u16> = "按 Ctrl+K 打开文件夹"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let text_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: y + 10.0,
                    right: x + width - 10.0,
                    bottom: y + 30.0,
                };
                target.DrawText(
                    &text,
                    &ui_format,
                    &text_rect,
                    text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    fn render_source_control_sidebar(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        _text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
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
            let bold_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
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

            let text_color = color_f(0.9, 0.9, 0.9, 1.0);
            let text_br2 = self
                .render_ctx
                .brush_cache
                .get_brush(target, &text_color)
                .unwrap();
            let dim_color = color_f(0.5, 0.5, 0.5, 1.0);
            let dim_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &dim_color)
                .unwrap();
            let sel_color = color_f(0.0, 0.47, 0.83, 1.0);
            let sel_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sel_color)
                .unwrap();
            let hover_color = color_f(0.2, 0.2, 0.2, 1.0);
            let hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_color)
                .unwrap();
            let sep_color = color_f(0.2, 0.2, 0.2, 1.0);
            let sep_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sep_color)
                .unwrap();
            let green_color = color_f(0.2, 0.8, 0.3, 1.0);
            let green_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &green_color)
                .unwrap();
            let yellow_color = color_f(0.9, 0.7, 0.2, 1.0);
            let _yellow_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &yellow_color)
                .unwrap();
            let red_color = color_f(0.9, 0.2, 0.2, 1.0);
            let _red_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &red_color)
                .unwrap();
            let btn_bg_color = color_f(0.2, 0.2, 0.2, 1.0);
            let btn_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_bg_color)
                .unwrap();
            let btn_hover_color = color_f(0.3, 0.3, 0.3, 1.0);
            let btn_hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_hover_color)
                .unwrap();

            let mut current_y = y + 10.0 - self.git.scroll_y;

            // 标题
            let title: Vec<u16> = "源代码管理".encode_utf16().chain(Some(0)).collect();
            let title_rect = D2D_RECT_F {
                left: x + 10.0,
                top: current_y,
                right: x + width - 10.0,
                bottom: current_y + 20.0,
            };
            target.DrawText(
                &title,
                &bold_format,
                &title_rect,
                &text_br2,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            current_y += 24.0;

            if !self.git.is_repo() {
                let msg: Vec<u16> = "当前文件夹不是 Git 仓库"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let msg_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: current_y,
                    right: x + width - 10.0,
                    bottom: current_y + 20.0,
                };
                target.DrawText(
                    &msg,
                    &ui_format,
                    &msg_rect,
                    &dim_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                return;
            }

            // 分支名称
            if let Some(branch) = self.git.current_branch_name() {
                let branch_text: Vec<u16> = format!("{} {}", "🌿", branch)
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let branch_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: current_y,
                    right: x + width - 10.0,
                    bottom: current_y + 20.0,
                };
                target.DrawText(
                    &branch_text,
                    &ui_format,
                    &branch_rect,
                    &green_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
            current_y += 22.0;

            // 分隔线
            let sep_rect = D2D_RECT_F {
                left: x + 10.0,
                top: current_y,
                right: x + width - 10.0,
                bottom: current_y + 1.0,
            };
            target.FillRectangle(&sep_rect, &sep_brush);
            current_y += 6.0;

            // Commit 消息输入框
            let input_bg = D2D_RECT_F {
                left: x + 10.0,
                top: current_y,
                right: x + width - 10.0,
                bottom: current_y + 24.0,
            };
            let input_bg_color = color_f(0.18, 0.18, 0.18, 1.0);
            let input_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &input_bg_color)
                .unwrap();
            target.FillRectangle(&input_bg, &input_bg_brush);
            let msg_label = if self.git.commit_message.is_empty() {
                "输入提交消息..."
            } else {
                &self.git.commit_message
            };
            let msg_color = if self.git.commit_message.is_empty() {
                dim_brush.clone()
            } else {
                text_br2.clone()
            };
            let msg_text: Vec<u16> = msg_label.encode_utf16().chain(Some(0)).collect();
            let msg_rect = D2D_RECT_F {
                left: x + 14.0,
                top: current_y + 3.0,
                right: x + width - 14.0,
                bottom: current_y + 21.0,
            };
            target.DrawText(
                &msg_text,
                &mono_format,
                &msg_rect,
                &msg_color,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            current_y += 30.0;

            // 按钮：Commit 和 Refresh
            let btn_y = current_y;
            let btn_h = 24.0;
            let btn_w = 60.0;

            // Commit 按钮
            let commit_btn_rect = D2D_RECT_F {
                left: x + 10.0,
                top: btn_y,
                right: x + 10.0 + btn_w,
                bottom: btn_y + btn_h,
            };
            let is_commit_hover = self
                .git
                .hover_button
                .as_ref()
                .map(|s| s == "commit")
                .unwrap_or(false);
            target.FillRectangle(
                &commit_btn_rect,
                if is_commit_hover {
                    &btn_hover_brush
                } else {
                    &btn_bg_brush
                },
            );
            let commit_text: Vec<u16> = "提交".encode_utf16().chain(Some(0)).collect();
            let commit_text_rect = D2D_RECT_F {
                left: x + 10.0,
                top: btn_y + 3.0,
                right: x + 10.0 + btn_w,
                bottom: btn_y + btn_h - 2.0,
            };
            target.DrawText(
                &commit_text,
                &ui_format,
                &commit_text_rect,
                &text_br2,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // Refresh 按钮
            let refresh_btn_rect = D2D_RECT_F {
                left: x + 80.0,
                top: btn_y,
                right: x + 80.0 + btn_w,
                bottom: btn_y + btn_h,
            };
            let is_refresh_hover = self
                .git
                .hover_button
                .as_ref()
                .map(|s| s == "refresh")
                .unwrap_or(false);
            target.FillRectangle(
                &refresh_btn_rect,
                if is_refresh_hover {
                    &btn_hover_brush
                } else {
                    &btn_bg_brush
                },
            );
            let refresh_text: Vec<u16> = "刷新".encode_utf16().chain(Some(0)).collect();
            let refresh_text_rect = D2D_RECT_F {
                left: x + 80.0,
                top: btn_y + 3.0,
                right: x + 80.0 + btn_w,
                bottom: btn_y + btn_h - 2.0,
            };
            target.DrawText(
                &refresh_text,
                &ui_format,
                &refresh_text_rect,
                &text_br2,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            current_y += 36.0;

            // 分隔线
            let sep2_rect = D2D_RECT_F {
                left: x + 10.0,
                top: current_y,
                right: x + width - 10.0,
                bottom: current_y + 1.0,
            };
            target.FillRectangle(&sep2_rect, &sep_brush);
            current_y += 6.0;

            let item_h = 22.0;
            let section_header_h = 20.0;

            // Staged Changes
            let staged = self.git.staged_files();
            if !staged.is_empty() {
                let header_text: Vec<u16> = format!("已暂存的更改 ({})", staged.len())
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let header_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: current_y,
                    right: x + width - 10.0,
                    bottom: current_y + section_header_h,
                };
                target.DrawText(
                    &header_text,
                    &bold_format,
                    &header_rect,
                    &text_br2,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                current_y += section_header_h;

                for (file, status) in &staged {
                    if current_y + item_h > y + height {
                        break;
                    }
                    if current_y + item_h >= y {
                        let is_selected = self.git.selected_file.as_ref() == Some(file);
                        let is_hover = self.git.hover_file.as_ref() == Some(file);
                        let file_rect = D2D_RECT_F {
                            left: x + 10.0,
                            top: current_y,
                            right: x + width - 30.0,
                            bottom: current_y + item_h,
                        };
                        if is_selected {
                            target.FillRectangle(&file_rect, &sel_brush);
                        } else if is_hover {
                            target.FillRectangle(&file_rect, &hover_brush);
                        }

                        let icon = crate::git::GitRepository::status_icon(*status);
                        let icon_color = crate::git::GitRepository::status_color(*status);
                        let icon_brush = self
                            .render_ctx
                            .brush_cache
                            .get_brush(
                                target,
                                &color_f(icon_color.0, icon_color.1, icon_color.2, 1.0),
                            )
                            .unwrap();
                        let icon_text: Vec<u16> = icon.encode_utf16().chain(Some(0)).collect();
                        let icon_rect = D2D_RECT_F {
                            left: x + 14.0,
                            top: current_y + 2.0,
                            right: x + 30.0,
                            bottom: current_y + item_h - 2.0,
                        };
                        target.DrawText(
                            &icon_text,
                            &mono_format,
                            &icon_rect,
                            &icon_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        let file_name: Vec<u16> = file.encode_utf16().chain(Some(0)).collect();
                        let file_name_rect = D2D_RECT_F {
                            left: x + 32.0,
                            top: current_y + 2.0,
                            right: x + width - 40.0,
                            bottom: current_y + item_h - 2.0,
                        };
                        target.DrawText(
                            &file_name,
                            &mono_format,
                            &file_name_rect,
                            &text_br2,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        // 取消暂存按钮 (-)
                        let minus_rect = D2D_RECT_F {
                            left: x + width - 28.0,
                            top: current_y + 4.0,
                            right: x + width - 10.0,
                            bottom: current_y + item_h - 4.0,
                        };
                        let minus_text: Vec<u16> = "−".encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &minus_text,
                            &ui_format,
                            &minus_rect,
                            &dim_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                    current_y += item_h;
                }
                current_y += 6.0;
            }

            // Changes (unstaged)
            let unstaged = self.git.unstaged_files();
            if !unstaged.is_empty() {
                let header_text: Vec<u16> = format!("更改 ({})", unstaged.len())
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let header_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: current_y,
                    right: x + width - 10.0,
                    bottom: current_y + section_header_h,
                };
                target.DrawText(
                    &header_text,
                    &bold_format,
                    &header_rect,
                    &text_br2,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                current_y += section_header_h;

                for (file, status) in &unstaged {
                    if current_y + item_h > y + height {
                        break;
                    }
                    if current_y + item_h >= y {
                        let is_selected = self.git.selected_file.as_ref() == Some(file);
                        let is_hover = self.git.hover_file.as_ref() == Some(file);
                        let file_rect = D2D_RECT_F {
                            left: x + 10.0,
                            top: current_y,
                            right: x + width - 30.0,
                            bottom: current_y + item_h,
                        };
                        if is_selected {
                            target.FillRectangle(&file_rect, &sel_brush);
                        } else if is_hover {
                            target.FillRectangle(&file_rect, &hover_brush);
                        }

                        let icon = crate::git::GitRepository::status_icon(*status);
                        let icon_color = crate::git::GitRepository::status_color(*status);
                        let icon_brush = self
                            .render_ctx
                            .brush_cache
                            .get_brush(
                                target,
                                &color_f(icon_color.0, icon_color.1, icon_color.2, 1.0),
                            )
                            .unwrap();
                        let icon_text: Vec<u16> = icon.encode_utf16().chain(Some(0)).collect();
                        let icon_rect = D2D_RECT_F {
                            left: x + 14.0,
                            top: current_y + 2.0,
                            right: x + 30.0,
                            bottom: current_y + item_h - 2.0,
                        };
                        target.DrawText(
                            &icon_text,
                            &mono_format,
                            &icon_rect,
                            &icon_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        let file_name: Vec<u16> = file.encode_utf16().chain(Some(0)).collect();
                        let file_name_rect = D2D_RECT_F {
                            left: x + 32.0,
                            top: current_y + 2.0,
                            right: x + width - 40.0,
                            bottom: current_y + item_h - 2.0,
                        };
                        target.DrawText(
                            &file_name,
                            &mono_format,
                            &file_name_rect,
                            &text_br2,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        // 暂存按钮 (+)
                        let plus_rect = D2D_RECT_F {
                            left: x + width - 28.0,
                            top: current_y + 4.0,
                            right: x + width - 10.0,
                            bottom: current_y + item_h - 4.0,
                        };
                        let plus_text: Vec<u16> = "+".encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &plus_text,
                            &ui_format,
                            &plus_rect,
                            &green_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                    current_y += item_h;
                }
                current_y += 6.0;
            }

            // Untracked Files
            let untracked = self.git.untracked_files();
            if !untracked.is_empty() {
                let header_text: Vec<u16> = format!("未跟踪的文件 ({})", untracked.len())
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let header_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: current_y,
                    right: x + width - 10.0,
                    bottom: current_y + section_header_h,
                };
                target.DrawText(
                    &header_text,
                    &bold_format,
                    &header_rect,
                    &text_br2,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                current_y += section_header_h;

                for file in &untracked {
                    if current_y + item_h > y + height {
                        break;
                    }
                    if current_y + item_h >= y {
                        let is_selected = self.git.selected_file.as_ref() == Some(file);
                        let is_hover = self.git.hover_file.as_ref() == Some(file);
                        let file_rect = D2D_RECT_F {
                            left: x + 10.0,
                            top: current_y,
                            right: x + width - 30.0,
                            bottom: current_y + item_h,
                        };
                        if is_selected {
                            target.FillRectangle(&file_rect, &sel_brush);
                        } else if is_hover {
                            target.FillRectangle(&file_rect, &hover_brush);
                        }

                        let icon_text: Vec<u16> = "U".encode_utf16().chain(Some(0)).collect();
                        let icon_rect = D2D_RECT_F {
                            left: x + 14.0,
                            top: current_y + 2.0,
                            right: x + 30.0,
                            bottom: current_y + item_h - 2.0,
                        };
                        target.DrawText(
                            &icon_text,
                            &mono_format,
                            &icon_rect,
                            &dim_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        let file_name: Vec<u16> = file.encode_utf16().chain(Some(0)).collect();
                        let file_name_rect = D2D_RECT_F {
                            left: x + 32.0,
                            top: current_y + 2.0,
                            right: x + width - 40.0,
                            bottom: current_y + item_h - 2.0,
                        };
                        target.DrawText(
                            &file_name,
                            &mono_format,
                            &file_name_rect,
                            &text_br2,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        // 暂存按钮 (+)
                        let plus_rect = D2D_RECT_F {
                            left: x + width - 28.0,
                            top: current_y + 4.0,
                            right: x + width - 10.0,
                            bottom: current_y + item_h - 4.0,
                        };
                        let plus_text: Vec<u16> = "+".encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &plus_text,
                            &ui_format,
                            &plus_rect,
                            &green_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                    current_y += item_h;
                }
            }
        }
    }

    fn render_remote_file_tree_sidebar(
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
            let tree_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let dir_color = color_f(0.9, 0.9, 0.9, 1.0);
            let dir_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &dir_color)
                .unwrap();
            let sel_color = color_f(0.0, 0.47, 0.83, 1.0);
            let sel_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sel_color)
                .unwrap();
            let hover_color = color_f(0.2, 0.2, 0.2, 1.0);
            let hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_color)
                .unwrap();

            // 标题
            let title_text = if let Some(session) = &self.remote_session {
                format!(
                    "远程: {}@{}:{}",
                    session.config.username, session.config.host, session.config.port
                )
            } else {
                "远程文件".to_string()
            };
            let title: Vec<u16> = title_text.encode_utf16().chain(Some(0)).collect();
            let title_rect = D2D_RECT_F {
                left: x + 10.0,
                top: y + 10.0,
                right: x + width - 10.0,
                bottom: y + 30.0,
            };
            target.DrawText(
                &title,
                &ui_format,
                &title_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            if let Some(tree) = &self.remote_file_tree {
                let mut current_y = y + 40.0 - self.remote_scroll_y;
                for (i, node) in tree.nodes.iter().enumerate() {
                    if current_y > y + height {
                        break;
                    }
                    if current_y + 20.0 < y {
                        current_y += 20.0;
                        continue;
                    }

                    let indent = node.depth as f32 * 16.0;
                    let icon = if node.is_dir {
                        if node.is_expanded {
                            "📂"
                        } else {
                            "📁"
                        }
                    } else {
                        "📄"
                    };
                    let arrow = if node.is_dir {
                        if node.is_expanded {
                            "▼ "
                        } else {
                            "▶ "
                        }
                    } else {
                        ""
                    };
                    let display = format!("{}{} {}", arrow, icon, node.name);

                    let item_left = x + 10.0 + indent;
                    let item_right = x + width - 10.0;

                    let is_hover = self.hover_remote_node == Some(i);
                    if is_hover {
                        let hover_rect = D2D_RECT_F {
                            left: item_left - 4.0,
                            top: current_y,
                            right: item_right,
                            bottom: current_y + 20.0,
                        };
                        target.FillRectangle(&hover_rect, &hover_brush);
                    }

                    let is_selected = self.selected_remote_node == Some(i) && !node.is_dir;
                    if is_selected {
                        let sel_rect = D2D_RECT_F {
                            left: item_left - 4.0,
                            top: current_y,
                            right: item_right,
                            bottom: current_y + 20.0,
                        };
                        target.FillRectangle(&sel_rect, &sel_brush);
                    }

                    let brush = if node.is_dir { &dir_brush } else { text_brush };
                    let wide: Vec<u16> = display.encode_utf16().chain(Some(0)).collect();
                    let text_rect = D2D_RECT_F {
                        left: item_left,
                        top: current_y,
                        right: item_right,
                        bottom: current_y + 20.0,
                    };
                    target.DrawText(
                        &wide,
                        &tree_format,
                        &text_rect,
                        brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    current_y += 20.0;
                }
            } else {
                let msg: Vec<u16> = "未连接远程服务器".encode_utf16().chain(Some(0)).collect();
                let msg_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: y + 40.0,
                    right: x + width - 10.0,
                    bottom: y + 60.0,
                };
                target.DrawText(
                    &msg,
                    &ui_format,
                    &msg_rect,
                    text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    fn render_ssh_dialog(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
    ) {
        unsafe {
            let width = 400.0f32;
            let height = 420.0f32;
            let x = (self.window_width as f32 - width) / 2.0;
            let y = (self.window_height as f32 - height) / 2.0;

            let bg_color = color_f(0.18, 0.18, 0.18, 1.0);
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bg_color)
                .unwrap();
            let border_color = color_f(0.3, 0.3, 0.3, 1.0);
            let border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &border_color)
                .unwrap();
            let text_color = color_f(0.9, 0.9, 0.9, 1.0);
            let text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &text_color)
                .unwrap();
            let dim_color = color_f(0.5, 0.5, 0.5, 1.0);
            let dim_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &dim_color)
                .unwrap();
            let input_bg_color = color_f(0.12, 0.12, 0.12, 1.0);
            let input_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &input_bg_color)
                .unwrap();
            let btn_bg_color = color_f(0.0, 0.47, 0.83, 1.0);
            let btn_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_bg_color)
                .unwrap();
            let btn_hover_color = color_f(0.0, 0.55, 0.95, 1.0);
            let btn_hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_hover_color)
                .unwrap();
            let overlay_color = color_f(0.0, 0.0, 0.0, 0.5);
            let overlay_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &overlay_color)
                .unwrap();

            let format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let title_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    14.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();

            // 遮罩层
            let overlay_rect = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: self.window_width as f32,
                bottom: self.window_height as f32,
            };
            target.FillRectangle(&overlay_rect, &overlay_brush);

            // 对话框背景
            let dialog_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&dialog_rect, &bg_brush);
            let border_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.DrawRectangle(&border_rect, &border_brush, 1.0, None);

            let mut cy = y + 16.0;

            // 标题
            let title: Vec<u16> = "SSH 连接".encode_utf16().chain(Some(0)).collect();
            let title_rect = D2D_RECT_F {
                left: x + 16.0,
                top: cy,
                right: x + width - 16.0,
                bottom: cy + 22.0,
            };
            target.DrawText(
                &title,
                &title_format,
                &title_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 32.0;

            // 错误消息
            if let Some(err) = &self.ssh_dialog.error_message {
                let err_text: Vec<u16> = err.encode_utf16().chain(Some(0)).collect();
                let err_rect = D2D_RECT_F {
                    left: x + 16.0,
                    top: cy,
                    right: x + width - 16.0,
                    bottom: cy + 18.0,
                };
                let err_color = color_f(0.9, 0.2, 0.2, 1.0);
                let err_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &err_color)
                    .unwrap();
                target.DrawText(
                    &err_text,
                    &format,
                    &err_rect,
                    &err_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                cy += 22.0;
            }

            // 字段标签和输入框
            let fields = vec![
                ("主机:", &self.ssh_dialog.host, 0),
                ("端口:", &self.ssh_dialog.port, 1),
                ("用户名:", &self.ssh_dialog.username, 2),
            ];

            for (label, value, idx) in &fields {
                let label_text: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                let label_rect = D2D_RECT_F {
                    left: x + 16.0,
                    top: cy,
                    right: x + 80.0,
                    bottom: cy + 18.0,
                };
                target.DrawText(
                    &label_text,
                    &format,
                    &label_rect,
                    &dim_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                let input_rect = D2D_RECT_F {
                    left: x + 80.0,
                    top: cy - 2.0,
                    right: x + width - 16.0,
                    bottom: cy + 20.0,
                };
                target.FillRectangle(&input_rect, &input_bg_brush);
                let val_text: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
                let val_rect = D2D_RECT_F {
                    left: x + 84.0,
                    top: cy,
                    right: x + width - 20.0,
                    bottom: cy + 18.0,
                };
                target.DrawText(
                    &val_text,
                    &format,
                    &val_rect,
                    &text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // 焦点指示器
                if self.ssh_dialog.focus_field == *idx {
                    let focus_rect = D2D_RECT_F {
                        left: x + 80.0,
                        top: cy - 2.0,
                        right: x + width - 16.0,
                        bottom: cy + 20.0,
                    };
                    let focus_color = color_f(0.0, 0.47, 0.83, 1.0);
                    let focus_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &focus_color)
                        .unwrap();
                    target.DrawRectangle(&focus_rect, &focus_brush, 1.0, None);
                }

                cy += 28.0;
            }

            // 认证类型
            let auth_label: Vec<u16> = "认证:".encode_utf16().chain(Some(0)).collect();
            let auth_label_rect = D2D_RECT_F {
                left: x + 16.0,
                top: cy,
                right: x + 80.0,
                bottom: cy + 18.0,
            };
            target.DrawText(
                &auth_label,
                &format,
                &auth_label_rect,
                &dim_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            let auth_text = match self.ssh_dialog.auth_type {
                crate::ssh::SshAuthType::Password => "密码",
                crate::ssh::SshAuthType::Key => "私钥",
                crate::ssh::SshAuthType::Agent => "SSH Agent",
            };
            let auth_val: Vec<u16> = auth_text.encode_utf16().chain(Some(0)).collect();
            let auth_val_rect = D2D_RECT_F {
                left: x + 80.0,
                top: cy,
                right: x + width - 16.0,
                bottom: cy + 18.0,
            };
            target.DrawText(
                &auth_val,
                &format,
                &auth_val_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 28.0;

            // 根据认证类型显示不同字段
            match self.ssh_dialog.auth_type {
                crate::ssh::SshAuthType::Password => {
                    let label_text: Vec<u16> = "密码:".encode_utf16().chain(Some(0)).collect();
                    let label_rect = D2D_RECT_F {
                        left: x + 16.0,
                        top: cy,
                        right: x + 80.0,
                        bottom: cy + 18.0,
                    };
                    target.DrawText(
                        &label_text,
                        &format,
                        &label_rect,
                        &dim_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    let input_rect = D2D_RECT_F {
                        left: x + 80.0,
                        top: cy - 2.0,
                        right: x + width - 16.0,
                        bottom: cy + 20.0,
                    };
                    target.FillRectangle(&input_rect, &input_bg_brush);
                    let hidden = "*".repeat(self.ssh_dialog.password.len());
                    let val_text: Vec<u16> = hidden.encode_utf16().chain(Some(0)).collect();
                    let val_rect = D2D_RECT_F {
                        left: x + 84.0,
                        top: cy,
                        right: x + width - 20.0,
                        bottom: cy + 18.0,
                    };
                    target.DrawText(
                        &val_text,
                        &format,
                        &val_rect,
                        &text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    if self.ssh_dialog.focus_field == 3 {
                        let focus_rect = D2D_RECT_F {
                            left: x + 80.0,
                            top: cy - 2.0,
                            right: x + width - 16.0,
                            bottom: cy + 20.0,
                        };
                        let focus_color = color_f(0.0, 0.47, 0.83, 1.0);
                        let focus_brush = self
                            .render_ctx
                            .brush_cache
                            .get_brush(target, &focus_color)
                            .unwrap();
                        target.DrawRectangle(&focus_rect, &focus_brush, 1.0, None);
                    }
                    cy += 28.0;
                }
                crate::ssh::SshAuthType::Key => {
                    let label_text: Vec<u16> = "密钥路径:".encode_utf16().chain(Some(0)).collect();
                    let label_rect = D2D_RECT_F {
                        left: x + 16.0,
                        top: cy,
                        right: x + 80.0,
                        bottom: cy + 18.0,
                    };
                    target.DrawText(
                        &label_text,
                        &format,
                        &label_rect,
                        &dim_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    let input_rect = D2D_RECT_F {
                        left: x + 80.0,
                        top: cy - 2.0,
                        right: x + width - 16.0,
                        bottom: cy + 20.0,
                    };
                    target.FillRectangle(&input_rect, &input_bg_brush);
                    let val_text: Vec<u16> = self
                        .ssh_dialog
                        .key_path
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                    let val_rect = D2D_RECT_F {
                        left: x + 84.0,
                        top: cy,
                        right: x + width - 20.0,
                        bottom: cy + 18.0,
                    };
                    target.DrawText(
                        &val_text,
                        &format,
                        &val_rect,
                        &text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    if self.ssh_dialog.focus_field == 3 {
                        let focus_rect = D2D_RECT_F {
                            left: x + 80.0,
                            top: cy - 2.0,
                            right: x + width - 16.0,
                            bottom: cy + 20.0,
                        };
                        let focus_color = color_f(0.0, 0.47, 0.83, 1.0);
                        let focus_brush = self
                            .render_ctx
                            .brush_cache
                            .get_brush(target, &focus_color)
                            .unwrap();
                        target.DrawRectangle(&focus_rect, &focus_brush, 1.0, None);
                    }
                    cy += 28.0;

                    let label2_text: Vec<u16> = "密码短语:".encode_utf16().chain(Some(0)).collect();
                    let label2_rect = D2D_RECT_F {
                        left: x + 16.0,
                        top: cy,
                        right: x + 80.0,
                        bottom: cy + 18.0,
                    };
                    target.DrawText(
                        &label2_text,
                        &format,
                        &label2_rect,
                        &dim_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    let input2_rect = D2D_RECT_F {
                        left: x + 80.0,
                        top: cy - 2.0,
                        right: x + width - 16.0,
                        bottom: cy + 20.0,
                    };
                    target.FillRectangle(&input2_rect, &input_bg_brush);
                    let hidden2 = "*".repeat(self.ssh_dialog.key_passphrase.len());
                    let val2_text: Vec<u16> = hidden2.encode_utf16().chain(Some(0)).collect();
                    let val2_rect = D2D_RECT_F {
                        left: x + 84.0,
                        top: cy,
                        right: x + width - 20.0,
                        bottom: cy + 18.0,
                    };
                    target.DrawText(
                        &val2_text,
                        &format,
                        &val2_rect,
                        &text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    if self.ssh_dialog.focus_field == 4 {
                        let focus_rect = D2D_RECT_F {
                            left: x + 80.0,
                            top: cy - 2.0,
                            right: x + width - 16.0,
                            bottom: cy + 20.0,
                        };
                        let focus_color = color_f(0.0, 0.47, 0.83, 1.0);
                        let focus_brush = self
                            .render_ctx
                            .brush_cache
                            .get_brush(target, &focus_color)
                            .unwrap();
                        target.DrawRectangle(&focus_rect, &focus_brush, 1.0, None);
                    }
                    cy += 28.0;
                }
                crate::ssh::SshAuthType::Agent => {}
            }

            cy += 16.0;

            // 按钮：Connect 和 Cancel
            let btn_w = 80.0;
            let btn_h = 28.0;

            // Connect 按钮
            let connect_btn_rect = D2D_RECT_F {
                left: x + width - 16.0 - btn_w * 2.0 - 8.0,
                top: cy,
                right: x + width - 16.0 - btn_w - 8.0,
                bottom: cy + btn_h,
            };
            let is_connect_hover = self.ssh_dialog.hover_button == Some(0);
            target.FillRectangle(
                &connect_btn_rect,
                if is_connect_hover {
                    &btn_hover_brush
                } else {
                    &btn_bg_brush
                },
            );
            let connect_text: Vec<u16> = "连接".encode_utf16().chain(Some(0)).collect();
            let connect_text_rect = D2D_RECT_F {
                left: connect_btn_rect.left,
                top: cy + 4.0,
                right: connect_btn_rect.right,
                bottom: cy + btn_h - 2.0,
            };
            target.DrawText(
                &connect_text,
                &format,
                &connect_text_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // Cancel 按钮
            let cancel_btn_rect = D2D_RECT_F {
                left: x + width - 16.0 - btn_w,
                top: cy,
                right: x + width - 16.0,
                bottom: cy + btn_h,
            };
            let cancel_bg_color = color_f(0.25, 0.25, 0.25, 1.0);
            let cancel_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &cancel_bg_color)
                .unwrap();
            let cancel_hover_color = color_f(0.35, 0.35, 0.35, 1.0);
            let cancel_hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &cancel_hover_color)
                .unwrap();
            let is_cancel_hover = self.ssh_dialog.hover_button == Some(1);
            target.FillRectangle(
                &cancel_btn_rect,
                if is_cancel_hover {
                    &cancel_hover_brush
                } else {
                    &cancel_bg_brush
                },
            );
            let cancel_text: Vec<u16> = "取消".encode_utf16().chain(Some(0)).collect();
            let cancel_text_rect = D2D_RECT_F {
                left: cancel_btn_rect.left,
                top: cy + 4.0,
                right: cancel_btn_rect.right,
                bottom: cy + btn_h - 2.0,
            };
            target.DrawText(
                &cancel_text,
                &format,
                &cancel_text_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 存储按钮区域用于点击检测
            self.ssh_dialog.connect_btn_rect = Some(crate::layout::Region::new(
                connect_btn_rect.left,
                connect_btn_rect.top,
                connect_btn_rect.right - connect_btn_rect.left,
                connect_btn_rect.bottom - connect_btn_rect.top,
            ));
            self.ssh_dialog.cancel_btn_rect = Some(crate::layout::Region::new(
                cancel_btn_rect.left,
                cancel_btn_rect.top,
                cancel_btn_rect.right - cancel_btn_rect.left,
                cancel_btn_rect.bottom - cancel_btn_rect.top,
            ));
        }
    }

    fn render_clone_dialog(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
    ) {
        unsafe {
            let width = 400.0f32;
            let height = 200.0f32;
            let x = (self.window_width as f32 - width) / 2.0;
            let y = (self.window_height as f32 - height) / 2.0;

            let bg_color = color_f(0.18, 0.18, 0.18, 1.0);
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bg_color)
                .unwrap();
            let border_color = color_f(0.3, 0.3, 0.3, 1.0);
            let border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &border_color)
                .unwrap();
            let text_color = color_f(0.9, 0.9, 0.9, 1.0);
            let text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &text_color)
                .unwrap();
            let dim_color = color_f(0.5, 0.5, 0.5, 1.0);
            let dim_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &dim_color)
                .unwrap();
            let input_bg_color = color_f(0.12, 0.12, 0.12, 1.0);
            let input_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &input_bg_color)
                .unwrap();
            let btn_bg_color = color_f(0.0, 0.47, 0.83, 1.0);
            let btn_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_bg_color)
                .unwrap();
            let btn_hover_color = color_f(0.0, 0.55, 0.95, 1.0);
            let btn_hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_hover_color)
                .unwrap();
            let overlay_color = color_f(0.0, 0.0, 0.0, 0.5);
            let overlay_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &overlay_color)
                .unwrap();

            let format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let title_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    14.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();

            // 遮罩层
            let overlay_rect = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: self.window_width as f32,
                bottom: self.window_height as f32,
            };
            target.FillRectangle(&overlay_rect, &overlay_brush);

            // 对话框背景
            let dialog_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&dialog_rect, &bg_brush);
            target.DrawRectangle(&dialog_rect, &border_brush, 1.0, None);

            let mut cy = y + 16.0;

            // 标题
            let title: Vec<u16> = "克隆仓库".encode_utf16().chain(Some(0)).collect();
            let title_rect = D2D_RECT_F {
                left: x + 16.0,
                top: cy,
                right: x + width - 16.0,
                bottom: cy + 22.0,
            };
            target.DrawText(
                &title,
                &title_format,
                &title_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 32.0;

            // URL 输入
            let label_text: Vec<u16> = "仓库 URL:".encode_utf16().chain(Some(0)).collect();
            let label_rect = D2D_RECT_F {
                left: x + 16.0,
                top: cy,
                right: x + 90.0,
                bottom: cy + 18.0,
            };
            target.DrawText(
                &label_text,
                &format,
                &label_rect,
                &dim_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            let input_rect = D2D_RECT_F {
                left: x + 90.0,
                top: cy - 2.0,
                right: x + width - 16.0,
                bottom: cy + 20.0,
            };
            target.FillRectangle(&input_rect, &input_bg_brush);
            let val_text: Vec<u16> = self
                .clone_dialog
                .url
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let val_rect = D2D_RECT_F {
                left: x + 94.0,
                top: cy,
                right: x + width - 20.0,
                bottom: cy + 18.0,
            };
            target.DrawText(
                &val_text,
                &format,
                &val_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            if self.clone_dialog.focus_field == 0 {
                let focus_rect = D2D_RECT_F {
                    left: x + 90.0,
                    top: cy - 2.0,
                    right: x + width - 16.0,
                    bottom: cy + 20.0,
                };
                let focus_color = color_f(0.0, 0.47, 0.83, 1.0);
                let focus_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &focus_color)
                    .unwrap();
                target.DrawRectangle(&focus_rect, &focus_brush, 1.0, None);
            }
            cy += 36.0;

            // 错误消息
            if let Some(err) = &self.clone_dialog.error_message {
                let err_text: Vec<u16> = err.encode_utf16().chain(Some(0)).collect();
                let err_rect = D2D_RECT_F {
                    left: x + 16.0,
                    top: cy,
                    right: x + width - 16.0,
                    bottom: cy + 18.0,
                };
                let err_color = color_f(0.9, 0.2, 0.2, 1.0);
                let err_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &err_color)
                    .unwrap();
                target.DrawText(
                    &err_text,
                    &format,
                    &err_rect,
                    &err_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                cy += 22.0;
            }

            cy += 16.0;

            // 按钮：Clone 和 Cancel
            let btn_w = 80.0;
            let btn_h = 28.0;

            let clone_btn_rect = D2D_RECT_F {
                left: x + width - 16.0 - btn_w * 2.0 - 8.0,
                top: cy,
                right: x + width - 16.0 - btn_w - 8.0,
                bottom: cy + btn_h,
            };
            let is_clone_hover = self.clone_dialog.hover_button == Some(0);
            target.FillRectangle(
                &clone_btn_rect,
                if is_clone_hover {
                    &btn_hover_brush
                } else {
                    &btn_bg_brush
                },
            );
            let clone_text: Vec<u16> = "克隆".encode_utf16().chain(Some(0)).collect();
            let clone_text_rect = D2D_RECT_F {
                left: clone_btn_rect.left,
                top: cy + 4.0,
                right: clone_btn_rect.right,
                bottom: cy + btn_h - 2.0,
            };
            target.DrawText(
                &clone_text,
                &format,
                &clone_text_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            let cancel_btn_rect = D2D_RECT_F {
                left: x + width - 16.0 - btn_w,
                top: cy,
                right: x + width - 16.0,
                bottom: cy + btn_h,
            };
            let cancel_bg_color = color_f(0.25, 0.25, 0.25, 1.0);
            let cancel_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &cancel_bg_color)
                .unwrap();
            let cancel_hover_color = color_f(0.35, 0.35, 0.35, 1.0);
            let cancel_hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &cancel_hover_color)
                .unwrap();
            let is_cancel_hover = self.clone_dialog.hover_button == Some(1);
            target.FillRectangle(
                &cancel_btn_rect,
                if is_cancel_hover {
                    &cancel_hover_brush
                } else {
                    &cancel_bg_brush
                },
            );
            let cancel_text: Vec<u16> = "取消".encode_utf16().chain(Some(0)).collect();
            let cancel_text_rect = D2D_RECT_F {
                left: cancel_btn_rect.left,
                top: cy + 4.0,
                right: cancel_btn_rect.right,
                bottom: cy + btn_h - 2.0,
            };
            target.DrawText(
                &cancel_text,
                &format,
                &cancel_text_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 存储按钮区域用于点击检测
            self.clone_dialog.clone_btn_rect = Some(crate::layout::Region::new(
                clone_btn_rect.left,
                clone_btn_rect.top,
                clone_btn_rect.right - clone_btn_rect.left,
                clone_btn_rect.bottom - clone_btn_rect.top,
            ));
            self.clone_dialog.cancel_btn_rect = Some(crate::layout::Region::new(
                cancel_btn_rect.left,
                cancel_btn_rect.top,
                cancel_btn_rect.right - cancel_btn_rect.left,
                cancel_btn_rect.bottom - cancel_btn_rect.top,
            ));
        }
    }

    #[allow(dead_code)]
    fn render_terminal_sidebar(
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

            // 输入提示符
            let prompt_color = color_f(0.0, 0.8, 0.0, 1.0);
            let prompt_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &prompt_color)
                .unwrap();
            let prompt: Vec<u16> = "> ".encode_utf16().chain(Some(0)).collect();
            let prompt_rect = D2D_RECT_F {
                left: x + 10.0,
                top: line_y,
                right: x + 30.0,
                bottom: line_y + 18.0,
            };
            target.DrawText(
                &prompt,
                &mono_format,
                &prompt_rect,
                &prompt_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 输入行
            let input: Vec<u16> = self
                .terminal_panel
                .input_line
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let input_rect = D2D_RECT_F {
                left: x + 25.0,
                top: line_y,
                right: x + width - 10.0,
                bottom: line_y + 18.0,
            };
            target.DrawText(
                &input,
                &mono_format,
                &input_rect,
                &output_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    fn render_bottom_panel(
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
            let border_brush = self
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
            let prompt_color = color_f(0.0, 0.8, 0.0, 1.0);
            let prompt_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &prompt_color)
                .unwrap();

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

            // 顶部边框
            let top_border = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + 1.0,
            };
            target.FillRectangle(&top_border, &border_brush);

            // 底部面板标签栏（类似 VS Code 底部面板标签）
            let tab_height = 28.0;
            let tabs = vec!["终端", "输出", "问题"];
            let mut tab_x = x + 10.0;
            let tab_w = 60.0;
            for (i, tab) in tabs.iter().enumerate() {
                let is_active = i == 0; // 终端默认激活
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
                let tab_wide: Vec<u16> = tab.encode_utf16().chain(Some(0)).collect();
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

            // 终端输出内容
            let content_y = y + tab_height + 4.0;
            let _content_h = height - tab_height - 8.0;
            let mut line_y = content_y;
            for line in self.terminal_panel.visible_output() {
                if line_y > y + height - 30.0 {
                    break;
                }
                let text: Vec<u16> = line.encode_utf16().chain(Some(0)).collect();
                let text_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: line_y,
                    right: x + width - 10.0,
                    bottom: line_y + 16.0,
                };
                target.DrawText(
                    &text,
                    &mono_format,
                    &text_rect,
                    &output_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                line_y += 14.0;
            }

            // 输入提示符和输入行
            if line_y < y + height - 20.0 {
                let prompt: Vec<u16> = "> ".encode_utf16().chain(Some(0)).collect();
                let prompt_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: line_y,
                    right: x + 30.0,
                    bottom: line_y + 16.0,
                };
                target.DrawText(
                    &prompt,
                    &mono_format,
                    &prompt_rect,
                    &prompt_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                let input: Vec<u16> = self
                    .terminal_panel
                    .input_line
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let input_rect = D2D_RECT_F {
                    left: x + 25.0,
                    top: line_y,
                    right: x + width - 10.0,
                    bottom: line_y + 16.0,
                };
                target.DrawText(
                    &input,
                    &mono_format,
                    &input_rect,
                    &output_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    fn render_ai_assistant_sidebar(
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

            // ===== 欢迎页/空工作区提示 =====
            let has_workspace = self.current_folder.is_some() || self.file_path.is_some();
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

            // ===== 快捷操作按钮（2列网格，带圆角效果） =====
            let actions = crate::ai_panel::AiPanel::quick_actions();
            let btn_w = ((width - margin * 2.0 - 8.0) / 2.0).max(1.0);
            let btn_h = 30.0f32;
            let btn_gap = 8.0f32;
            let action_start_y = cy;

            for (i, action) in actions.iter().enumerate() {
                let col = i % 2;
                let row = i / 2;
                let bx = x + margin + col as f32 * (btn_w + btn_gap);
                let by = action_start_y + row as f32 * (btn_h + 6.0);
                let btn_rect = D2D_RECT_F {
                    left: bx,
                    top: by,
                    right: bx + btn_w,
                    bottom: by + btn_h,
                };

                let is_hover = self.ai_panel.hover_action == Some(*action);
                let btn_color = if is_hover {
                    color_f(0.28, 0.28, 0.30, 1.0)
                } else {
                    color_f(0.20, 0.20, 0.22, 1.0)
                };
                let btn_color_brush =
                    match self.render_ctx.brush_cache.get_brush(target, &btn_color) {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                target.FillRectangle(&btn_rect, &btn_color_brush);

                // 悬停时添加边框高亮
                if is_hover {
                    let hover_border = match self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &color_f(0.0, 0.47, 0.83, 0.6))
                    {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    let border_rect = D2D_RECT_F {
                        left: bx,
                        top: by,
                        right: bx + btn_w,
                        bottom: by + 1.0,
                    };
                    target.FillRectangle(&border_rect, &hover_border);
                }

                let label = format!("{} {}", action.icon(), action.label());
                let label_wide: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                let label_rect = D2D_RECT_F {
                    left: bx + 8.0,
                    top: by + 6.0,
                    right: bx + btn_w - 4.0,
                    bottom: by + btn_h - 4.0,
                };
                target.DrawText(
                    &label_wide,
                    &small_format,
                    &label_rect,
                    text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
            let action_rows = (actions.len() + 1) / 2;
            cy = action_start_y + action_rows as f32 * (btn_h + 6.0) + 10.0;
            self.ai_panel.action_rows = action_rows;

            // 分隔线
            let sep2_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 1.0,
            };
            target.FillRectangle(&sep2_rect, &sep_brush);
            cy += 10.0;

            // ===== 聊天消息区域 =====
            let chat_top = cy;
            let chat_bottom = y + height - 52.0;
            let chat_height = chat_bottom - chat_top;

            // 消息滚动区域
            let mut msg_y = chat_top - self.ai_panel.scroll_y;
            let line_h = 16.0f32;
            let max_lines_per_msg = ((chat_height - 16.0) / line_h).max(3.0) as usize;

            for msg in &self.ai_panel.messages {
                if msg_y > chat_bottom {
                    break;
                }
                if msg_y + line_h < chat_top {
                    msg_y += line_h;
                    continue;
                }

                let is_user = msg.role == crate::ai_panel::AiRole::User;
                let is_system = msg.role == crate::ai_panel::AiRole::System;

                // 跳过系统消息的渲染（只保留作为上下文）
                if is_system {
                    continue;
                }

                let label = if is_user { "你" } else { "AI" };
                let label_color: &ID2D1SolidColorBrush =
                    if is_user { &accent_brush } else { &green_brush };
                let label_wide: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                let label_rect = D2D_RECT_F {
                    left: x + margin + 4.0,
                    top: msg_y,
                    right: x + width - margin,
                    bottom: msg_y + 14.0,
                };
                target.DrawText(
                    &label_wide,
                    &small_format,
                    &label_rect,
                    label_color,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                msg_y += 14.0;

                // 消息内容
                let content_lines: Vec<&str> = msg.content.lines().collect();
                let visible_lines = content_lines.len().min(max_lines_per_msg);
                let msg_h = visible_lines as f32 * line_h + 10.0;

                if msg_y + msg_h > chat_top && msg_y < chat_bottom {
                    let bubble_bg: &ID2D1SolidColorBrush = if is_user {
                        &user_bg_brush
                    } else {
                        &assistant_bg_brush
                    };
                    let bubble_rect = D2D_RECT_F {
                        left: x + margin,
                        top: msg_y,
                        right: x + width - margin,
                        bottom: msg_y + msg_h,
                    };
                    target.FillRectangle(&bubble_rect, bubble_bg);

                    let mut in_code = false;
                    for (li, line) in content_lines.iter().take(visible_lines).enumerate() {
                        let trimmed = line.trim();
                        if trimmed.starts_with("```") {
                            in_code = !in_code;
                            continue;
                        }
                        let line_y = msg_y + 5.0 + li as f32 * line_h;
                        let line_rect = D2D_RECT_F {
                            left: x + margin + 8.0,
                            top: line_y,
                            right: x + width - margin - 8.0,
                            bottom: line_y + line_h,
                        };
                        if in_code {
                            let code_rect = D2D_RECT_F {
                                left: x + margin + 4.0,
                                top: line_y,
                                right: x + width - margin - 4.0,
                                bottom: line_y + line_h,
                            };
                            target.FillRectangle(&code_rect, &code_bg_brush);
                            let line_text: String = if line.chars().count() > 80 {
                                line.chars().take(80).collect()
                            } else {
                                line.to_string()
                            };
                            let line_wide: Vec<u16> =
                                line_text.encode_utf16().chain(Some(0)).collect();
                            target.DrawText(
                                &line_wide,
                                &msg_format,
                                &line_rect,
                                &code_text_brush,
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );
                        } else {
                            let line_text: String = if line.chars().count() > 80 {
                                line.chars().take(80).collect()
                            } else {
                                line.to_string()
                            };
                            let line_wide: Vec<u16> =
                                line_text.encode_utf16().chain(Some(0)).collect();
                            target.DrawText(
                                &line_wide,
                                &msg_format,
                                &line_rect,
                                text_brush,
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );
                        }
                    }

                    if content_lines.len() > max_lines_per_msg {
                        let more_wide: Vec<u16> = "...".encode_utf16().chain(Some(0)).collect();
                        let more_rect = D2D_RECT_F {
                            left: x + margin + 8.0,
                            top: msg_y + msg_h - 16.0,
                            right: x + width - margin - 8.0,
                            bottom: msg_y + msg_h,
                        };
                        target.DrawText(
                            &more_wide,
                            &msg_format,
                            &more_rect,
                            &dim_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                }
                msg_y += msg_h + 10.0;
            }

            // 正在生成指示器（带动画点）
            if self.ai_panel.is_generating {
                if msg_y < chat_bottom && msg_y + 16.0 > chat_top {
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

            // ===== 输入框区域 =====
            let input_y = y + height - 44.0;
            let input_rect = D2D_RECT_F {
                left: x + margin,
                top: input_y,
                right: x + width - margin,
                bottom: input_y + 34.0,
            };
            target.FillRectangle(&input_rect, &input_bg_brush);
            let input_border = D2D_RECT_F {
                left: x + margin,
                top: input_y,
                right: x + width - margin,
                bottom: input_y + 1.0,
            };
            target.FillRectangle(&input_border, &sep_brush);
            let input_border2 = D2D_RECT_F {
                left: x + margin,
                top: input_y + 33.0,
                right: x + width - margin,
                bottom: input_y + 34.0,
            };
            target.FillRectangle(&input_border2, &sep_brush);

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
                left: x + margin + 8.0,
                top: input_y + 7.0,
                right: x + width - margin - 8.0,
                bottom: input_y + 30.0,
            };
            target.DrawText(
                &input_wide,
                &msg_format,
                &input_text_rect,
                input_color,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 发送提示
            let hint: Vec<u16> = "Enter 发送".encode_utf16().chain(Some(0)).collect();
            let hint_rect = D2D_RECT_F {
                left: x + margin,
                top: y + height - 20.0,
                right: x + width - margin,
                bottom: y + height - 4.0,
            };
            target.DrawText(
                &hint,
                &small_format,
                &hint_rect,
                &dim_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    fn render_settings_sidebar(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        unsafe {
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
                    14.0,
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

            // Title
            let title_text: Vec<u16> = "AI 设置".encode_utf16().chain(Some(0)).collect();
            let title_rect = D2D_RECT_F {
                left: x + 10.0,
                top: y + 10.0,
                right: x + width - 10.0,
                bottom: y + 34.0,
            };
            target.DrawText(
                &title_text,
                &title_format,
                &title_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // Separator
            let sep_color = color_f(0.2, 0.2, 0.2, 1.0);
            let sep_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sep_color)
                .unwrap();
            let sep_rect = D2D_RECT_F {
                left: x,
                top: y + 36.0,
                right: x + width,
                bottom: y + 37.0,
            };
            target.FillRectangle(&sep_rect, &sep_brush);

            self.settings_panel.clear_regions();

            let margin = 10.0;
            let input_w = width - margin * 2.0;
            let label_h = 18.0;
            let input_h = 28.0;
            let gap = 12.0;

            let mut cy = y + 48.0;

            // Provider
            let provider_label: Vec<u16> = "Provider (openai/claude/kimi/azure/custom)"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let provider_label_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + label_h,
            };
            target.DrawText(
                &provider_label,
                &label_format,
                &provider_label_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += label_h;
            let provider_bg = color_f(0.18, 0.18, 0.18, 1.0);
            let provider_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &provider_bg)
                .unwrap();
            let provider_border = if self.settings_panel.active_field
                == Some(crate::settings::SettingsField::Provider)
            {
                color_f(0.0, 0.47, 0.83, 1.0)
            } else {
                color_f(0.3, 0.3, 0.3, 1.0)
            };
            let provider_border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &provider_border)
                .unwrap();
            let provider_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + input_w,
                bottom: cy + input_h,
            };
            target.FillRectangle(&provider_rect, &provider_bg_brush);
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
            target.FillRectangle(&border_top, &provider_border_brush);
            target.FillRectangle(&border_bottom, &provider_border_brush);
            target.FillRectangle(&border_left, &provider_border_brush);
            target.FillRectangle(&border_right, &provider_border_brush);
            let provider_text: Vec<u16> = self
                .settings_panel
                .provider
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let provider_text_rect = D2D_RECT_F {
                left: x + margin + 6.0,
                top: cy,
                right: x + margin + input_w - 6.0,
                bottom: cy + input_h,
            };
            target.DrawText(
                &provider_text,
                &input_format,
                &provider_text_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_field_region(
                crate::settings::SettingsField::Provider,
                x + margin,
                cy,
                input_w,
                input_h,
            );
            cy += input_h + gap;

            // API Key
            let apikey_label: Vec<u16> = "API Key".encode_utf16().chain(Some(0)).collect();
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
            let apikey_bg = color_f(0.18, 0.18, 0.18, 1.0);
            let apikey_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &apikey_bg)
                .unwrap();
            let apikey_border = if self.settings_panel.active_field
                == Some(crate::settings::SettingsField::ApiKey)
            {
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
            target.FillRectangle(&border_top, &apikey_border_brush);
            target.FillRectangle(&border_bottom, &apikey_border_brush);
            target.FillRectangle(&border_left, &apikey_border_brush);
            target.FillRectangle(&border_right, &apikey_border_brush);
            let display_key = self.settings_panel.masked_api_key();
            let apikey_text: Vec<u16> = display_key.encode_utf16().chain(Some(0)).collect();
            let apikey_text_rect = D2D_RECT_F {
                left: x + margin + 6.0,
                top: cy,
                right: x + margin + input_w - 6.0,
                bottom: cy + input_h,
            };
            target.DrawText(
                &apikey_text,
                &input_format,
                &apikey_text_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_field_region(
                crate::settings::SettingsField::ApiKey,
                x + margin,
                cy,
                input_w,
                input_h,
            );
            cy += input_h + gap;

            // Base URL
            let baseurl_label: Vec<u16> = "Base URL (optional)"
                .encode_utf16()
                .chain(Some(0))
                .collect();
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

            // Model
            let model_label: Vec<u16> = "Model".encode_utf16().chain(Some(0)).collect();
            let model_label_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + label_h,
            };
            target.DrawText(
                &model_label,
                &label_format,
                &model_label_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += label_h;
            let model_bg = color_f(0.18, 0.18, 0.18, 1.0);
            let model_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &model_bg)
                .unwrap();
            let model_border = if self.settings_panel.active_field
                == Some(crate::settings::SettingsField::Model)
            {
                color_f(0.0, 0.47, 0.83, 1.0)
            } else {
                color_f(0.3, 0.3, 0.3, 1.0)
            };
            let model_border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &model_border)
                .unwrap();
            let model_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + input_w,
                bottom: cy + input_h,
            };
            target.FillRectangle(&model_rect, &model_bg_brush);
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
            target.FillRectangle(&border_top, &model_border_brush);
            target.FillRectangle(&border_bottom, &model_border_brush);
            target.FillRectangle(&border_left, &model_border_brush);
            target.FillRectangle(&border_right, &model_border_brush);
            let model_text: Vec<u16> = self
                .settings_panel
                .model
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let model_text_rect = D2D_RECT_F {
                left: x + margin + 6.0,
                top: cy,
                right: x + margin + input_w - 6.0,
                bottom: cy + input_h,
            };
            target.DrawText(
                &model_text,
                &input_format,
                &model_text_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_field_region(
                crate::settings::SettingsField::Model,
                x + margin,
                cy,
                input_w,
                input_h,
            );
            cy += input_h + gap + 8.0;

            let btn_w = input_w;
            let btn_h = 32.0;

            // Save button
            let save_bg = if self.settings_panel.hover_button
                == Some(crate::settings::SettingsButton::Save)
            {
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
                left: x + margin,
                top: cy,
                right: x + margin + btn_w,
                bottom: cy + btn_h,
            };
            target.FillRectangle(&save_rect, &save_bg_brush);
            let save_text: Vec<u16> = "保存设置".encode_utf16().chain(Some(0)).collect();
            let save_text_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + btn_w,
                bottom: cy + btn_h,
            };
            let btn_text_color = color_f(1.0, 1.0, 1.0, 1.0);
            let btn_text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_text_color)
                .unwrap();
            target.DrawText(
                &save_text,
                &button_format,
                &save_text_rect,
                &btn_text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_button_region(
                crate::settings::SettingsButton::Save,
                x + margin,
                cy,
                btn_w,
                btn_h,
            );
            cy += btn_h + gap;

            // Test Connection button
            let test_bg = if self.settings_panel.hover_button
                == Some(crate::settings::SettingsButton::TestConnection)
            {
                color_f(0.25, 0.25, 0.25, 1.0)
            } else {
                color_f(0.18, 0.18, 0.18, 1.0)
            };
            let test_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &test_bg)
                .unwrap();
            let test_border = color_f(0.3, 0.3, 0.3, 1.0);
            let test_border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &test_border)
                .unwrap();
            let test_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + btn_w,
                bottom: cy + btn_h,
            };
            target.FillRectangle(&test_rect, &test_bg_brush);
            let test_border_top = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + btn_w,
                bottom: cy + 1.0,
            };
            let test_border_bottom = D2D_RECT_F {
                left: x + margin,
                top: cy + btn_h - 1.0,
                right: x + margin + btn_w,
                bottom: cy + btn_h,
            };
            let test_border_left = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + 1.0,
                bottom: cy + btn_h,
            };
            let test_border_right = D2D_RECT_F {
                left: x + margin + btn_w - 1.0,
                top: cy,
                right: x + margin + btn_w,
                bottom: cy + btn_h,
            };
            target.FillRectangle(&test_border_top, &test_border_brush);
            target.FillRectangle(&test_border_bottom, &test_border_brush);
            target.FillRectangle(&test_border_left, &test_border_brush);
            target.FillRectangle(&test_border_right, &test_border_brush);
            let test_text: Vec<u16> = "测试连接".encode_utf16().chain(Some(0)).collect();
            let test_text_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + btn_w,
                bottom: cy + btn_h,
            };
            let test_text_color = color_f(0.85, 0.85, 0.85, 1.0);
            let test_text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &test_text_color)
                .unwrap();
            target.DrawText(
                &test_text,
                &button_format,
                &test_text_rect,
                &test_text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_button_region(
                crate::settings::SettingsButton::TestConnection,
                x + margin,
                cy,
                btn_w,
                btn_h,
            );
            cy += btn_h + 8.0;

            // Status message
            if !self.settings_panel.test_status.is_empty() {
                let status_color = if self.settings_panel.is_testing {
                    color_f(0.8, 0.8, 0.4, 1.0)
                } else if self.settings_panel.test_status.starts_with("成功") {
                    color_f(0.2, 0.8, 0.2, 1.0)
                } else {
                    color_f(0.9, 0.3, 0.3, 1.0)
                };
                let status_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &status_color)
                    .unwrap();
                let status_format = self
                    .render_ctx
                    .text_format_cache
                    .get_format(
                        11.0,
                        DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                        DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                        DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                    )
                    .unwrap();
                let status_text: Vec<u16> = self
                    .settings_panel
                    .test_status
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let status_rect = D2D_RECT_F {
                    left: x + margin,
                    top: cy,
                    right: x + width - margin,
                    bottom: cy + 40.0,
                };
                target.DrawText(
                    &status_text,
                    &status_format,
                    &status_rect,
                    &status_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    fn render_tree_nodes(
        &self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        tree: &FileTree,
        parent_idx: u32,
        base_x: f32,
        current_y: &mut f32,
        clip_y: f32,
        clip_height: f32,
        sidebar_width: f32,
        format: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        dir_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        sel_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        hover_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        let mut child_idx = if parent_idx == u32::MAX {
            tree.first_root_node()
        } else {
            tree.get_node(parent_idx)
                .map(|n| n.first_child)
                .filter(|&c| c != u32::MAX)
        };

        while let Some(idx) = child_idx {
            if let Some(node) = tree.get_node(idx) {
                let next_sibling = if node.next_sibling != u32::MAX {
                    Some(node.next_sibling)
                } else {
                    None
                };

                if *current_y > clip_y + clip_height {
                    break;
                }

                if *current_y + 20.0 < clip_y {
                    *current_y += 20.0;
                    if node.kind == FileKind::Directory && node.is_expanded {
                        self.skip_tree_nodes(tree, idx, current_y);
                    }
                    child_idx = next_sibling;
                    continue;
                }

                // 根节点（parent_idx == u32::MAX）不缩进，子节点正常缩进
                let indent = if node.parent_idx == u32::MAX {
                    0.0
                } else {
                    node.depth as f32 * 16.0
                };
                let name = tree.get_name(node);

                let icon = if node.kind == FileKind::Directory {
                    if node.is_expanded {
                        "📂"
                    } else {
                        "📁"
                    }
                } else {
                    self.get_file_icon(name)
                };

                let arrow = if node.kind == FileKind::Directory {
                    if node.is_expanded {
                        "▼ "
                    } else {
                        "▶ "
                    }
                } else {
                    ""
                };

                let display = format!("{}{} {}", arrow, icon, name);

                let item_left = base_x + indent;
                let item_right = base_x + sidebar_width - 10.0;

                // 绘制悬停背景
                let is_hover = self.hover_file_node == Some(idx);
                if is_hover {
                    let hover_rect = D2D_RECT_F {
                        left: item_left - 4.0,
                        top: *current_y,
                        right: item_right,
                        bottom: *current_y + 20.0,
                    };
                    unsafe {
                        target.FillRectangle(&hover_rect, hover_brush);
                    }
                }

                // 绘制选中高亮背景
                let is_selected =
                    self.selected_file_node == Some(idx) && node.kind == FileKind::File;
                if is_selected {
                    let sel_rect = D2D_RECT_F {
                        left: item_left - 4.0,
                        top: *current_y,
                        right: item_right,
                        bottom: *current_y + 20.0,
                    };
                    unsafe {
                        target.FillRectangle(&sel_rect, sel_brush);
                    }
                }

                let brush = if node.kind == FileKind::Directory {
                    dir_brush
                } else {
                    text_brush
                };

                unsafe {
                    let wide: Vec<u16> = display.encode_utf16().chain(Some(0)).collect();
                    let text_rect = D2D_RECT_F {
                        left: item_left,
                        top: *current_y,
                        right: item_right,
                        bottom: *current_y + 20.0,
                    };
                    target.DrawText(
                        &wide,
                        format,
                        &text_rect,
                        brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }

                *current_y += 20.0;

                if node.kind == FileKind::Directory && node.is_expanded {
                    self.render_tree_nodes(
                        target,
                        tree,
                        idx,
                        base_x,
                        current_y,
                        clip_y,
                        clip_height,
                        sidebar_width,
                        format,
                        text_brush,
                        dir_brush,
                        sel_brush,
                        hover_brush,
                    );
                }

                child_idx = next_sibling;
            } else {
                break;
            }
        }
    }

    fn skip_tree_nodes(&self, tree: &FileTree, parent_idx: u32, current_y: &mut f32) {
        let mut child_idx = tree
            .get_node(parent_idx)
            .map(|n| n.first_child)
            .filter(|&c| c != u32::MAX);
        while let Some(idx) = child_idx {
            if let Some(node) = tree.get_node(idx) {
                *current_y += 20.0;
                if node.kind == FileKind::Directory && node.is_expanded {
                    self.skip_tree_nodes(tree, idx, current_y);
                }
                child_idx = if node.next_sibling != u32::MAX {
                    Some(node.next_sibling)
                } else {
                    None
                };
            } else {
                break;
            }
        }
    }

    fn get_file_icon(&self, name: &str) -> &'static str {
        let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
        match ext.as_str() {
            "rs" => "🦀",
            "js" => "📜",
            "ts" => "📘",
            "tsx" => "⚛",
            "jsx" => "⚛",
            "json" => "📋",
            "html" | "htm" => "🌐",
            "css" | "scss" | "sass" | "less" => "🎨",
            "md" | "markdown" => "📝",
            "py" | "pyw" | "pyi" => "🐍",
            "c" | "cpp" | "h" | "hpp" | "cc" | "cxx" => "🔧",
            "toml" => "⚙",
            "yaml" | "yml" => "⚙",
            "lock" => "🔒",
            "ps1" | "sh" | "bash" | "zsh" => "📜",
            "exe" | "dll" => "⚙",
            "java" | "kt" => "☕",
            "go" => "🐹",
            "rb" => "💎",
            "php" => "🐘",
            "swift" => "🍎",
            "sql" => "🗄",
            "lua" => "🌙",
            "xml" => "📃",
            "csv" => "📊",
            "dockerfile" => "🐳",
            "vue" => "🌿",
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" => "🖼",
            _ => "📄",
        }
    }

    fn render_editor(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        let line_height = self.text_renderer.line_height();
        let char_width = self.text_renderer.char_width();
        let line_number_width = 60.0;

        unsafe {
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.editor_bg)
                .unwrap();
            let ln_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.line_number_bg)
                .unwrap();
            let sep_color = color_f(0.3, 0.3, 0.3, 1.0);
            let sep_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sep_color)
                .unwrap();
            let sel_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.selection_bg)
                .unwrap();
            let hl_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.line_highlight_bg)
                .unwrap();
            let ln_fg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.line_number_fg)
                .unwrap();
            let cursor_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.cursor_color)
                .unwrap();

            let font_size = self.text_renderer.font_size();
            let ln_format = self
                .render_ctx
                .text_format_cache
                .get_line_number_format(font_size)
                .unwrap();
            let code_format = self
                .render_ctx
                .text_format_cache
                .get_code_format(font_size)
                .unwrap();

            // 绘制背景
            let bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&bg_rect, &bg_brush);
            let ln_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + line_number_width,
                bottom: y + height,
            };
            target.FillRectangle(&ln_rect, &ln_bg_brush);
            let sep_rect = D2D_RECT_F {
                left: x + line_number_width - 1.0,
                top: y,
                right: x + line_number_width,
                bottom: y + height,
            };
            target.FillRectangle(&sep_rect, &sep_brush);

            let total_lines = self.cached_lines.len().max(1);
            let start_line = (self.scroll_y / line_height) as usize;
            let visible_lines = (height / line_height) as usize + 2;
            let end_line = (start_line + visible_lines).min(total_lines);

            for line_idx in start_line..end_line {
                let line_y = y + (line_idx - start_line) as f32 * line_height
                    - (self.scroll_y % line_height);
                if line_y > y + height {
                    break;
                }
                if line_y + line_height < y {
                    continue;
                }

                // 优先使用缓存的行文本，避免重复调用 buffer.get_line()
                let cached_line = if line_idx < self.cached_lines.len() {
                    Some(self.cached_lines[line_idx].as_str())
                } else {
                    None
                };

                // Selection highlight — Glass 模式下使用柔和光晕
                if let (Some((sel_start_line, sel_start_col)), Some((sel_end_line, sel_end_col))) =
                    (self.selection_start, self.selection_end)
                {
                    let (first_line, first_col) = if sel_start_line <= sel_end_line {
                        (sel_start_line, sel_start_col)
                    } else {
                        (sel_end_line, sel_end_col)
                    };
                    let (last_line, last_col) = if sel_start_line <= sel_end_line {
                        (sel_end_line, sel_end_col)
                    } else {
                        (sel_start_line, sel_start_col)
                    };

                    if line_idx >= first_line && line_idx <= last_line {
                        let sel_start_char = if let Some(text) = cached_line {
                            let col = if line_idx == first_line { first_col } else { 0 };
                            text[..col.min(text.len())].chars().count()
                        } else {
                            0
                        };
                        let sel_end_char = if let Some(text) = cached_line {
                            let col = if line_idx == last_line {
                                last_col
                            } else {
                                text.len()
                            };
                            text[..col.min(text.len())].chars().count()
                        } else {
                            0
                        };
                        let sel_start_x =
                            x + line_number_width + 5.0 + sel_start_char as f32 * char_width;
                        let sel_end_x =
                            x + line_number_width + 5.0 + sel_end_char as f32 * char_width;
                        let sel_rect = D2D_RECT_F {
                            left: sel_start_x,
                            top: line_y,
                            right: sel_end_x,
                            bottom: line_y + line_height,
                        };
                        if self.theme.glass_enabled {
                            let _ = glass::draw_glow_selection(
                                target,
                                &mut self.render_ctx.brush_cache,
                                &sel_rect,
                                &self.theme.glow_selection,
                                2.0,
                            );
                        } else {
                            target.FillRectangle(&sel_rect, &sel_brush);
                        }
                    }
                }

                // 当前行高亮
                if line_idx == self.cursor_line {
                    let hl_rect = D2D_RECT_F {
                        left: x + line_number_width,
                        top: line_y,
                        right: x + width,
                        bottom: line_y + line_height,
                    };
                    target.FillRectangle(&hl_rect, &hl_brush);
                }

                // 行号（DrawText）—— 使用预缓存的 UTF-16 编码，避免每帧 format! + encode_utf16
                let ln_wide: &[u16] = if line_idx < self.cached_line_numbers.len()
                    && !self.cached_line_numbers[line_idx].is_empty()
                {
                    &self.cached_line_numbers[line_idx]
                } else {
                    &[]
                };
                // 如果缓存未命中，回退到动态生成
                let fallback_ln: Vec<u16>;
                let ln_wide_final: &[u16] = if ln_wide.is_empty() {
                    fallback_ln = format!("{}", line_idx + 1)
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                    &fallback_ln
                } else {
                    ln_wide
                };
                let ln_rect_draw = D2D_RECT_F {
                    left: x + 5.0,
                    top: line_y,
                    right: x + line_number_width - 5.0,
                    bottom: line_y + line_height,
                };
                target.DrawText(
                    ln_wide_final,
                    &ln_format,
                    &ln_rect_draw,
                    &ln_fg_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // 代码文本（使用缓存的 tokens + DrawText）
                if let Some(line_text) = cached_line {
                    let tokens = &self.cached_tokens[line_idx];
                    let text_x = x + line_number_width + 5.0;

                    let mut current_byte = 0usize;
                    let mut current_char = 0usize;
                    let mut token_idx = 0;

                    while current_byte < line_text.len() {
                        let mut token_color = self.theme.text_default;
                        let token_len: usize;

                        if token_idx < tokens.len() {
                            let token = &tokens[token_idx];
                            if token.start <= current_byte && current_byte < token.start + token.len
                            {
                                token_color = self.theme.color_for_token(token.kind);
                                token_len = (token.start + token.len - current_byte)
                                    .min(line_text.len() - current_byte);
                                if current_byte + token_len >= token.start + token.len {
                                    token_idx += 1;
                                }
                            } else if token.start > current_byte {
                                token_len = (token.start - current_byte)
                                    .min(line_text.len() - current_byte);
                            } else {
                                token_idx += 1;
                                continue;
                            }
                        } else {
                            token_len = line_text.len() - current_byte;
                        }

                        let segment = &line_text
                            [current_byte..(current_byte + token_len).min(line_text.len())];
                        if !segment.is_empty() {
                            let brush = self
                                .render_ctx
                                .brush_cache
                                .get_brush(target, &token_color)
                                .unwrap();
                            // 复用 UTF-16 缓冲区，避免每 token 分配 Vec<u16>
                            self.text_utf16_buf.clear();
                            self.text_utf16_buf.extend(segment.encode_utf16());
                            self.text_utf16_buf.push(0);
                            let seg_rect = D2D_RECT_F {
                                left: text_x + current_char as f32 * char_width,
                                top: line_y,
                                right: text_x + width,
                                bottom: line_y + line_height,
                            };
                            target.DrawText(
                                &self.text_utf16_buf,
                                &code_format,
                                &seg_rect,
                                &brush,
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );
                            // 按字符数推进，而非字节数
                            current_char += segment.chars().count();
                        }

                        current_byte += token_len;
                    }
                }
            }

            // 光标：将字节列转换为字符列计算x坐标
            // 优先使用缓存的行文本，避免重复调用 buffer.get_line()
            let cursor_char_col = if let Some(text) = self.cached_lines.get(self.cursor_line) {
                text[..self.cursor_col.min(text.len())].chars().count()
            } else {
                0
            };
            let cursor_x = x + line_number_width + 5.0 + cursor_char_col as f32 * char_width;
            let cursor_y = y + (self.cursor_line.saturating_sub(start_line)) as f32 * line_height
                - (self.scroll_y % line_height);
            if cursor_y >= y && cursor_y <= y + height {
                let cursor_rect = D2D_RECT_F {
                    left: cursor_x,
                    top: cursor_y,
                    right: cursor_x + 2.0,
                    bottom: cursor_y + line_height,
                };
                target.FillRectangle(&cursor_rect, &cursor_brush);
            }
        }
    }

    fn render_find_replace(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
    ) {
        unsafe {
            let bg_color = if self.theme.glass_enabled {
                color_f(0.18, 0.18, 0.18, 0.95)
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
            let text_color = color_f(0.9, 0.9, 0.9, 1.0);
            let text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &text_color)
                .unwrap();
            let dim_color = color_f(0.5, 0.5, 0.5, 1.0);
            let dim_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &dim_color)
                .unwrap();
            let input_bg_color = color_f(0.12, 0.12, 0.12, 1.0);
            let input_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &input_bg_color)
                .unwrap();
            let match_color = color_f(0.2, 0.8, 0.3, 1.0);
            let match_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &match_color)
                .unwrap();
            let btn_bg_color = color_f(0.25, 0.25, 0.25, 1.0);
            let _btn_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_bg_color)
                .unwrap();
            let btn_hover_color = color_f(0.35, 0.35, 0.35, 1.0);
            let _btn_hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_hover_color)
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

            let panel_height = if self.replace_visible { 72.0 } else { 40.0 };
            let panel_width = width.min(600.0);
            let panel_x = x + width - panel_width - 10.0;

            let panel_rect = D2D_RECT_F {
                left: panel_x,
                top: y,
                right: panel_x + panel_width,
                bottom: y + panel_height,
            };
            target.FillRectangle(&panel_rect, &bg_brush);
            let border_rect = D2D_RECT_F {
                left: panel_x,
                top: y,
                right: panel_x + panel_width,
                bottom: y + 1.0,
            };
            target.FillRectangle(&border_rect, &border_brush);

            let mut cy = y + 8.0;
            let input_h = 24.0;
            let input_w = panel_width - 120.0;

            // 查找标签
            let find_label: Vec<u16> = "查找:".encode_utf16().chain(Some(0)).collect();
            let find_label_rect = D2D_RECT_F {
                left: panel_x + 10.0,
                top: cy,
                right: panel_x + 50.0,
                bottom: cy + input_h,
            };
            target.DrawText(
                &find_label,
                &label_format,
                &find_label_rect,
                &text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 查找输入框
            let find_input_rect = D2D_RECT_F {
                left: panel_x + 50.0,
                top: cy,
                right: panel_x + 50.0 + input_w,
                bottom: cy + input_h,
            };
            target.FillRectangle(&find_input_rect, &input_bg_brush);
            // 焦点边框
            if self.find_focus == crate::editor::FindReplaceFocus::FindQuery {
                let focus_border = D2D_RECT_F {
                    left: panel_x + 50.0,
                    top: cy,
                    right: panel_x + 50.0 + input_w,
                    bottom: cy + 1.0,
                };
                target.FillRectangle(&focus_border, &border_brush);
                let focus_border2 = D2D_RECT_F {
                    left: panel_x + 50.0,
                    top: cy + input_h - 1.0,
                    right: panel_x + 50.0 + input_w,
                    bottom: cy + input_h,
                };
                target.FillRectangle(&focus_border2, &border_brush);
            }
            let find_text = if self.find_query.is_empty() {
                "输入查找内容..."
            } else {
                &self.find_query
            };
            let find_text_color = if self.find_query.is_empty() {
                &dim_brush
            } else {
                &text_brush
            };
            let find_wide: Vec<u16> = find_text.encode_utf16().chain(Some(0)).collect();
            let find_text_rect = D2D_RECT_F {
                left: panel_x + 54.0,
                top: cy + 2.0,
                right: panel_x + 46.0 + input_w,
                bottom: cy + input_h - 2.0,
            };
            target.DrawText(
                &find_wide,
                &input_format,
                &find_text_rect,
                find_text_color,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 匹配计数
            let match_text = if !self.find_results.is_empty() {
                format!("{}/{}", self.find_active_index + 1, self.find_results.len())
            } else if !self.find_query.is_empty() {
                "0/0".to_string()
            } else {
                String::new()
            };
            if !match_text.is_empty() {
                let match_wide: Vec<u16> = match_text.encode_utf16().chain(Some(0)).collect();
                let match_rect = D2D_RECT_F {
                    left: panel_x + 52.0 + input_w,
                    top: cy,
                    right: panel_x + panel_width - 10.0,
                    bottom: cy + input_h,
                };
                target.DrawText(
                    &match_wide,
                    &label_format,
                    &match_rect,
                    &match_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            cy += input_h + 8.0;

            // 替换输入框（如果可见）
            if self.replace_visible {
                let replace_label: Vec<u16> = "替换:".encode_utf16().chain(Some(0)).collect();
                let replace_label_rect = D2D_RECT_F {
                    left: panel_x + 10.0,
                    top: cy,
                    right: panel_x + 50.0,
                    bottom: cy + input_h,
                };
                target.DrawText(
                    &replace_label,
                    &label_format,
                    &replace_label_rect,
                    &text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                let replace_input_rect = D2D_RECT_F {
                    left: panel_x + 50.0,
                    top: cy,
                    right: panel_x + 50.0 + input_w,
                    bottom: cy + input_h,
                };
                target.FillRectangle(&replace_input_rect, &input_bg_brush);
                // 焦点边框
                if self.find_focus == crate::editor::FindReplaceFocus::ReplaceText {
                    let focus_border = D2D_RECT_F {
                        left: panel_x + 50.0,
                        top: cy,
                        right: panel_x + 50.0 + input_w,
                        bottom: cy + 1.0,
                    };
                    target.FillRectangle(&focus_border, &border_brush);
                    let focus_border2 = D2D_RECT_F {
                        left: panel_x + 50.0,
                        top: cy + input_h - 1.0,
                        right: panel_x + 50.0 + input_w,
                        bottom: cy + input_h,
                    };
                    target.FillRectangle(&focus_border2, &border_brush);
                }
                let replace_text = if self.replace_text.is_empty() {
                    "输入替换内容..."
                } else {
                    &self.replace_text
                };
                let replace_text_color = if self.replace_text.is_empty() {
                    &dim_brush
                } else {
                    &text_brush
                };
                let replace_wide: Vec<u16> = replace_text.encode_utf16().chain(Some(0)).collect();
                let replace_text_rect = D2D_RECT_F {
                    left: panel_x + 54.0,
                    top: cy + 2.0,
                    right: panel_x + 46.0 + input_w,
                    bottom: cy + input_h - 2.0,
                };
                target.DrawText(
                    &replace_wide,
                    &input_format,
                    &replace_text_rect,
                    replace_text_color,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    /// 在 render 之前更新标签栏布局缓存
    fn update_tab_layouts(&mut self, x: f32, width: f32, _height: f32) {
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

    fn render_tab_bar(
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
            let close_color = color_f(0.6, 0.6, 0.6, 1.0);
            let close_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &close_color)
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
                let name = tab.file_name();
                let display = if tab.is_dirty {
                    format!("{} ●", name)
                } else {
                    name
                };
                let name_wide: Vec<u16> = display.encode_utf16().chain(Some(0)).collect();
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

                // 关闭按钮（×）
                let close_x = tab_x + tw - close_btn_width + 4.0;
                let close_rect = D2D_RECT_F {
                    left: close_x,
                    top: y + 6.0,
                    right: close_x + 16.0,
                    bottom: y + height - 6.0,
                };
                let close_wide: Vec<u16> = "×".encode_utf16().chain(Some(0)).collect();
                let close_format = self
                    .render_ctx
                    .text_format_cache
                    .get_format(
                        14.0,
                        DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                        DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                        DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                    )
                    .unwrap();
                target.DrawText(
                    &close_wide,
                    &close_format,
                    &close_rect,
                    &close_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                tab_x += tw + gap;
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

    fn render_statusbar(
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
            status.update_cursor_position(self.cursor_line, self.cursor_col);
            status.update_status(&self.status_message);
            let lang_name = match self.language {
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

            // 绘制各区域
            let regions = status.section_regions(width);
            for (i, (rx, _ry, rw, _rh)) in regions.iter().enumerate() {
                if i < status.sections.len() {
                    let section = &status.sections[i];
                    let wide: Vec<u16> = section.label.encode_utf16().chain(Some(0)).collect();
                    let text_rect = D2D_RECT_F {
                        left: x + rx,
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

                    // 分隔线
                    if i > 0 && i < 3 {
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

    fn render_menu_bar(
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

    fn render_title_bar(
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
            let btn_width = 46.0;
            let btn_height = height;
            let close_x = x + width - btn_width;
            let maximize_x = close_x - btn_width;
            let minimize_x = maximize_x - btn_width;

            // 面板切换按钮（在最小化按钮左侧）
            let panel_btn_width = 32.0;
            let right_panel_btn_x = minimize_x - panel_btn_width;
            let bottom_panel_btn_x = right_panel_btn_x - panel_btn_width;

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
            // 关闭图标（X）
            let cx = close_x + btn_width / 2.0;
            let cy = y + height / 2.0;
            // 左上-右下对角线
            for i in 0..10 {
                let t = i as f32 / 9.0;
                let px = cx - 5.0 + t * 10.0;
                let py = cy - 5.0 + t * 10.0;
                let dot = D2D_RECT_F {
                    left: px - 0.5,
                    top: py - 0.5,
                    right: px + 0.5,
                    bottom: py + 0.5,
                };
                target.FillRectangle(&dot, &icon_brush);
            }
            // 右上-左下对角线
            for i in 0..10 {
                let t = i as f32 / 9.0;
                let px = cx + 5.0 - t * 10.0;
                let py = cy - 5.0 + t * 10.0;
                let dot = D2D_RECT_F {
                    left: px - 0.5,
                    top: py - 0.5,
                    right: px + 0.5,
                    bottom: py + 0.5,
                };
                target.FillRectangle(&dot, &icon_brush);
            }

            // 右侧面板切换按钮
            let right_panel_btn_bg = if self.titlebar_hover_button == Some(3) {
                &hover_min_bg
            } else {
                &default_bg
            };
            let right_panel_btn_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, right_panel_btn_bg)
                .unwrap();
            let right_panel_btn_rect = D2D_RECT_F {
                left: right_panel_btn_x,
                top: y,
                right: right_panel_btn_x + panel_btn_width,
                bottom: y + btn_height,
            };
            target.FillRectangle(&right_panel_btn_rect, &right_panel_btn_brush);
            // 右侧面板图标（竖条）
            let right_panel_icon_brush = if self.layout.right_panel_visible {
                &active_icon_brush
            } else {
                &icon_brush
            };
            let rp_rect1 = D2D_RECT_F {
                left: right_panel_btn_x + 10.0,
                top: y + 10.0,
                right: right_panel_btn_x + 13.0,
                bottom: y + height - 10.0,
            };
            target.FillRectangle(&rp_rect1, right_panel_icon_brush);
            let rp_rect2 = D2D_RECT_F {
                left: right_panel_btn_x + 16.0,
                top: y + 10.0,
                right: right_panel_btn_x + 22.0,
                bottom: y + height - 10.0,
            };
            target.FillRectangle(&rp_rect2, right_panel_icon_brush);

            // 底部面板切换按钮
            let bottom_panel_btn_bg = if self.titlebar_hover_button == Some(4) {
                &hover_min_bg
            } else {
                &default_bg
            };
            let bottom_panel_btn_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, bottom_panel_btn_bg)
                .unwrap();
            let bottom_panel_btn_rect = D2D_RECT_F {
                left: bottom_panel_btn_x,
                top: y,
                right: bottom_panel_btn_x + panel_btn_width,
                bottom: y + btn_height,
            };
            target.FillRectangle(&bottom_panel_btn_rect, &bottom_panel_btn_brush);
            // 底部面板图标（横条）
            let bottom_panel_icon_brush = if self.layout.bottom_panel_visible {
                &active_icon_brush
            } else {
                &icon_brush
            };
            let bp_rect1 = D2D_RECT_F {
                left: bottom_panel_btn_x + 8.0,
                top: y + 10.0,
                right: bottom_panel_btn_x + panel_btn_width - 8.0,
                bottom: y + 13.0,
            };
            target.FillRectangle(&bp_rect1, bottom_panel_icon_brush);
            let bp_rect2 = D2D_RECT_F {
                left: bottom_panel_btn_x + 8.0,
                top: y + 16.0,
                right: bottom_panel_btn_x + panel_btn_width - 8.0,
                bottom: y + 22.0,
            };
            target.FillRectangle(&bp_rect2, bottom_panel_icon_brush);
        }
    }

    fn render_submenu(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        menu_item: &crate::menu_bar::MenuBarItem,
    ) {
        unsafe {
            let bg_color = if self.theme.glass_enabled {
                self.theme.submenu_bg
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

            let menu_width = 220.0;
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

    fn render_activity_bar(
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
            let inactive_color = color_f(0.5, 0.5, 0.5, 1.0);
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

            let icon_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    20.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();

            let icon_size = 48.0;
            for (i, item) in self.activity_bar.items.iter().enumerate() {
                let icon_y = y + i as f32 * icon_size;
                let is_active = i == self.activity_bar.active_index;
                let is_hover = self.activity_bar.hover_index == Some(i);

                if is_active {
                    let active_rect = D2D_RECT_F {
                        left: x,
                        top: icon_y,
                        right: x + width,
                        bottom: icon_y + icon_size,
                    };
                    target.FillRectangle(&active_rect, &hover_brush);

                    // 左侧高亮条
                    let indicator_rect = D2D_RECT_F {
                        left: x,
                        top: icon_y + 8.0,
                        right: x + 2.0,
                        bottom: icon_y + icon_size - 8.0,
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

                let icon_text: Vec<u16> = item.view.icon().encode_utf16().chain(Some(0)).collect();
                let icon_rect = D2D_RECT_F {
                    left: x,
                    top: icon_y,
                    right: x + width,
                    bottom: icon_y + icon_size,
                };
                let brush = if is_active {
                    &active_brush
                } else {
                    &inactive_brush
                };
                target.DrawText(
                    &icon_text,
                    &icon_format,
                    &icon_rect,
                    brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    /// 渲染图片预览
    fn render_image_preview(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        unsafe {
            // 背景
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &self.theme.editor_bg)
                .unwrap();
            let bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&bg_rect, &bg_brush);

            let title_format = self
                .render_ctx
                .text_format_cache
                .get_center_format(20.0, DWRITE_FONT_WEIGHT_BOLD.0 as u32)
                .unwrap();
            let info_format = self
                .render_ctx
                .text_format_cache
                .get_center_format(14.0, DWRITE_FONT_WEIGHT_NORMAL.0 as u32)
                .unwrap();

            let title_color = color_f(0.83, 0.83, 0.83, 1.0);
            let title_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &title_color)
                .unwrap();
            let info_color = color_f(0.5, 0.5, 0.5, 1.0);
            let info_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &info_color)
                .unwrap();
            let icon_color = color_f(0.3, 0.7, 1.0, 1.0);
            let icon_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &icon_color)
                .unwrap();

            let center_y = y + height / 2.0;

            // 图片图标
            let icon_text: Vec<u16> = "🖼️".encode_utf16().chain(Some(0)).collect();
            let icon_rect = D2D_RECT_F {
                left: x,
                top: center_y - 60.0,
                right: x + width,
                bottom: center_y - 20.0,
            };
            target.DrawText(
                &icon_text,
                &title_format,
                &icon_rect,
                &icon_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 标题
            let title = "图片预览";
            let title_wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
            let title_rect = D2D_RECT_F {
                left: x,
                top: center_y - 20.0,
                right: x + width,
                bottom: center_y + 10.0,
            };
            target.DrawText(
                &title_wide,
                &title_format,
                &title_rect,
                &title_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 文件路径
            if let Some(path) = &self.file_path {
                let path_text = format!("{}", path.display());
                let path_wide: Vec<u16> = path_text.encode_utf16().chain(Some(0)).collect();
                let path_rect = D2D_RECT_F {
                    left: x + 20.0,
                    top: center_y + 20.0,
                    right: x + width - 20.0,
                    bottom: center_y + 50.0,
                };
                target.DrawText(
                    &path_wide,
                    &info_format,
                    &path_rect,
                    &info_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    /// 渲染命令面板
    fn render_command_palette(
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
                    let label_wide: Vec<u16> = item.label.encode_utf16().chain(Some(0)).collect();
                    let label_rect = D2D_RECT_F {
                        left: x + 16.0,
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
                            left: x + 16.0,
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
