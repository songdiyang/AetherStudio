use aether_core::char_width::char_width as unicode_char_width;
use aether_core::lexer::Language;
use aether_core::workspace::file_tree::{FileKind, FileTree};
use aether_render::d2d::factory::color_f;
use aether_render::d2d::glass;
use windows::Win32::Graphics::Direct2D::Common::{D2D_POINT_2F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_NONE,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING,
};

use crate::editor::{BottomPanelTab, EditorState};
use crate::layout::{Region, ACTIVITY_BAR_BUTTON_SIZE};
use crate::settings::ProviderTemplateButton;

/// 绘制输入框的四条边框
unsafe fn draw_input_borders(
    target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    brush: &ID2D1SolidColorBrush,
) {
    let top = D2D_RECT_F {
        left: x,
        top: y,
        right: x + w,
        bottom: y + 1.0,
    };
    let bottom = D2D_RECT_F {
        left: x,
        top: y + h - 1.0,
        right: x + w,
        bottom: y + h,
    };
    let left = D2D_RECT_F {
        left: x,
        top: y,
        right: x + 1.0,
        bottom: y + h,
    };
    let right = D2D_RECT_F {
        left: x + w - 1.0,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    target.FillRectangle(&top, brush);
    target.FillRectangle(&bottom, brush);
    target.FillRectangle(&left, brush);
    target.FillRectangle(&right, brush);
}

impl EditorState {
    pub fn render(&mut self) {
        // 避免0尺寸渲染
        if self.window_width == 0 || self.window_height == 0 {
            return;
        }

        // TEST: 每帧开始清除上一帧命中区域
        crate::hit_test::clear_hit_regions();

        // AI-H01: 轮询后台 AI 请求结果，不阻塞 UI 线程
        // 传入工作区目录，供 Edit/Agent 模式解析编辑相对路径生成正确 diff
        let ai_current_folder = self.current_folder.clone();
        self.ai_panel
            .check_background_result(ai_current_folder.as_deref());

        // 设置面板：轮询测试连接结果
        if self.settings_panel.poll_test_result() {
            self.dirty_tracker.mark_full_window();
        }

        // LSP: 轮询诊断事件，更新 diagnostics 字段
        self.poll_lsp_events();

        // 终端输出轮询：从读取线程拉取子进程 stdout/stderr 并写入输出缓存。
        // 此前未调用 flush_output 导致 shell 输出无法显示，现在每帧轮询保证实时性。
        if self.terminal_panel.running {
            self.terminal_panel.poll_startup();
            self.terminal_panel.flush_output();
        }

        // 懒加载预扫描：确保所有 is_expanded 但未加载的目录节点子项已就绪
        // 这样渲染文件树时不会因目录未加载而显示空
        self.preload_expanded_dirs();

        // UI-L07: 降级为 trace，避免生产环境每帧日志噪声
        tracing::trace!(
            win_w = self.window_width,
            win_h = self.window_height,
            "render() start"
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
        let show_tab_bar = self.show_tab_bar();
        let editor_content_region = self.layout.editor_content_region(show_tab_bar);
        let line_height = self.text_renderer.line_height();
        let total_lines = self.content.buffer.len_lines().max(1);
        let visible_start = (self.content.scroll_y / line_height) as usize;
        let visible_lines = (editor_content_region.height / line_height) as usize + 2;
        let visible_end = (visible_start + visible_lines).min(total_lines);

        self.rebuild_cache(visible_start, visible_end);

        // 使用布局管理器计算各区域
        let titlebar_region = self.layout.title_bar_region();
        let menu_region = self.layout.menu_bar_region();
        let activity_region = self.layout.activity_bar_region();
        let sidebar_region = self.layout.sidebar_region();
        let editor_region = self.layout.editor_region();
        let tab_region = self.layout.tab_bar_region(show_tab_bar);
        let status_region = self.layout.status_bar_region();
        let right_panel_region = self.layout.right_panel_region();

        // 预计算标签栏布局
        if show_tab_bar {
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

        // P1.2: 先把事件队列中累积的事件转换为脏矩形
        self.flush_events_to_dirty_tracker();

        // 脏矩形检测：对比上一帧状态，标记变化区域（兼容层）
        let cursor_moved = self.content.cursor_line != self.last_cursor_line
            || self.content.cursor_col != self.last_cursor_col;
        let scroll_changed = (self.content.scroll_y - self.last_scroll_y).abs() > 0.01;
        let selection_changed = self.content.selection_start != self.last_selection_start
            || self.content.selection_end != self.last_selection_end;
        let sidebar_changed = self.sidebar_content != self.last_sidebar_content;
        let sidebar_visible_changed = self.layout.sidebar_visible != self.last_sidebar_visible;
        let activity_bar_visible_changed =
            self.layout.activity_bar_visible != self.last_activity_bar_visible;
        let right_panel_changed = self.layout.right_panel_visible != self.last_right_panel_visible;
        let bottom_panel_changed =
            self.layout.bottom_panel_visible != self.last_bottom_panel_visible;
        let status_changed = self.status_message != self.last_status_message;
        let active_tab_changed = self.active_tab != self.last_active_tab;
        let dialog_visible =
            self.ssh_dialog.visible || self.clone_dialog.visible || self.command_palette.visible;

        // 标签页切换会改变标签栏高亮、编辑器内容、状态栏等多个区域，
        // 局部裁剪容易遗漏旧像素导致重影，强制全量重绘。
        if active_tab_changed {
            self.dirty_tracker.mark_full_window();
        }

        // 底部面板可见性变化属于重大布局变更，强制全量重绘以保证编辑器区域正确刷新
        if bottom_panel_changed {
            self.dirty_tracker.mark_full_window();
        }
        // REQ-P0-06: 侧边栏/活动栏可见性变化改为精确区域标记，
        // 避免不必要的全窗口重绘。标记侧边栏、活动栏和编辑器区域（布局位移）
        if sidebar_visible_changed || activity_bar_visible_changed {
            let activity_region = self.layout.activity_bar_region();
            self.dirty_tracker.mark_region(
                activity_region.x,
                activity_region.y,
                activity_region.width,
                activity_region.height,
                crate::dirty_rect::DirtyRegionType::ActivityBar,
            );
            let sidebar_region = self.layout.sidebar_region();
            self.dirty_tracker.mark_region(
                sidebar_region.x,
                sidebar_region.y,
                sidebar_region.width,
                sidebar_region.height,
                crate::dirty_rect::DirtyRegionType::Sidebar,
            );
            // 编辑器区域因布局位移需要重绘
            let editor_region = self.layout.editor_region();
            self.dirty_tracker.mark_region(
                editor_region.x,
                editor_region.y,
                editor_region.width,
                editor_region.height,
                crate::dirty_rect::DirtyRegionType::EditorContent,
            );
        }
        // REQ-P0-06: 侧边栏内容切换/右侧面板可见性变化改为精确区域标记
        if sidebar_changed {
            let sidebar_region = self.layout.sidebar_region();
            self.dirty_tracker.mark_region(
                sidebar_region.x,
                sidebar_region.y,
                sidebar_region.width,
                sidebar_region.height,
                crate::dirty_rect::DirtyRegionType::Sidebar,
            );
        }
        if right_panel_changed {
            let right_panel_region = self.layout.right_panel_region();
            self.dirty_tracker.mark_region(
                right_panel_region.x,
                right_panel_region.y,
                right_panel_region.width,
                right_panel_region.height,
                crate::dirty_rect::DirtyRegionType::RightPanel,
            );
            // 编辑器区域因布局位移需要重绘
            let editor_region = self.layout.editor_region();
            self.dirty_tracker.mark_region(
                editor_region.x,
                editor_region.y,
                editor_region.width,
                editor_region.height,
                crate::dirty_rect::DirtyRegionType::EditorContent,
            );
        }

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
                    let editor_content_region = self.layout.editor_content_region(show_tab_bar);
                    let cursor_y = editor_content_region.y
                        + self.content.cursor_line as f32 * line_height
                        - self.content.scroll_y;
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
                // REQ-P0-06: 无状态变化时不标记任何脏区域
                crate::dirty_rect::RenderCommand::None => {}
            }
        }

        // REQ-P0-06: 如果没有脏区域，跳过渲染（避免无变化时的全窗口重绘）
        if !self.dirty_tracker.has_dirty() {
            // 仍需更新上一帧状态追踪，避免下一帧误检测到变化
            self.last_cursor_line = self.content.cursor_line;
            self.last_cursor_col = self.content.cursor_col;
            self.last_scroll_y = self.content.scroll_y;
            self.last_selection_start = self.content.selection_start;
            self.last_selection_end = self.content.selection_end;
            self.last_sidebar_content = self.sidebar_content.clone();
            self.last_sidebar_visible = self.layout.sidebar_visible;
            self.last_activity_bar_visible = self.layout.activity_bar_visible;
            self.last_right_panel_visible = self.layout.right_panel_visible;
            self.last_bottom_panel_visible = self.layout.bottom_panel_visible;
            self.last_status_message.clone_from(&self.status_message);
            self.last_active_tab = self.active_tab;
            return;
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
        // REQ-P3-03: 使用多矩形并集裁剪，避免合并为单一包围盒导致的重绘面积膨胀
        let mut use_layer = false;
        if use_clip {
            let rects = self.dirty_tracker.rects();
            if !rects.is_empty() {
                let rect_tuples: Vec<(f32, f32, f32, f32)> = rects
                    .iter()
                    .map(|r| (r.x, r.y, r.width, r.height))
                    .collect();
                use_layer = self
                    .render_ctx
                    .push_multi_clip(self.d2d_factory.factory(), &rect_tuples);
            }
        }

        // 全窗口清除只在全窗口重绘时执行
        if self.dirty_tracker.is_full_window() || !use_clip {
            // 欢迎页状态下使用深色背景（而非透明），避免面板区域出现黑色空洞
            // 透明色虽能让 DWM Mica/Acrylic 透出，但会导致未覆盖区域显示为黑色
            if self.show_welcome() {
                self.render_ctx
                    .clear(&windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
                        r: 0.09,
                        g: 0.09,
                        b: 0.09,
                        a: 1.0,
                    });
            } else {
                self.render_ctx.clear(&self.theme.editor_bg);
            }
        }

        // 欢迎页 + 脏矩形裁剪时，侧边栏/活动栏/右侧面板/底部面板区域不会被全窗口 clear 覆盖，
        // 且欢迎页逻辑跳过这些面板的渲染，导致这些区域显示为黑色。
        // 手动填充这些区域以保证背景色正确。
        if self.show_welcome() && use_clip {
            if self.layout.activity_bar_visible && activity_region.width > 0.0 {
                self.render_ctx.fill_rect(
                    activity_region.x,
                    activity_region.y,
                    activity_region.width,
                    activity_region.height,
                    &self.theme.activity_bar_bg,
                );
            }
            if self.layout.sidebar_visible && sidebar_region.width > 0.0 {
                self.render_ctx.fill_rect(
                    sidebar_region.x,
                    sidebar_region.y,
                    sidebar_region.width,
                    sidebar_region.height,
                    &self.theme.sidebar_bg,
                );
            }
            if self.layout.right_panel_visible && right_panel_region.width > 0.0 {
                self.render_ctx.fill_rect(
                    right_panel_region.x,
                    right_panel_region.y,
                    right_panel_region.width,
                    right_panel_region.height,
                    &self.theme.sidebar_bg,
                );
            }
            if self.layout.bottom_panel_visible {
                let bottom_region = self.layout.bottom_panel_region();
                if bottom_region.height > 0.0 {
                    // 欢迎页状态下，底部面板需要覆盖整个窗口宽度（包括右侧面板下方），
                    // 避免右侧面板下方出现黑色空洞
                    let full_width = self.window_width as f32;
                    self.render_ctx.fill_rect(
                        0.0,
                        bottom_region.y,
                        full_width,
                        bottom_region.height,
                        &self.theme.statusbar_bg,
                    );
                }
            }
        }

        // 预提取菜单栏数据，避免借用冲突
        let item_x_positions = self.menu_bar.item_x_positions.clone();
        let item_widths = self.menu_bar.item_widths.clone();

        let showing_welcome = self.show_welcome();

        // 0. 标题栏（最先渲染，作为背景）
        if self.layout.title_bar_visible {
            self.render_title_bar(&target, &titlebar_region);
        }

        // 1. 菜单栏
        if self.layout.menu_bar_visible {
            self.render_menu_bar(&item_x_positions, &item_widths, &target, &menu_region);
        }

        // 2. 活动栏（欢迎页不渲染）
        if self.layout.activity_bar_visible && !showing_welcome {
            self.render_activity_bar(&target, &activity_region);
        }

        // 3. 侧边栏（欢迎页不渲染）
        if self.layout.sidebar_visible && !showing_welcome {
            self.render_sidebar(&target, &sidebar_region);
        }

        // 4. 标签栏
        if show_tab_bar && !showing_welcome {
            self.render_tab_bar(
                &target,
                tab_region.x,
                tab_region.y,
                tab_region.width,
                tab_region.height,
            );
        }

        // 5. 编辑器内容/欢迎页/空占位页/图片预览/设置页
        let showing_empty_placeholder = self.show_empty_placeholder();
        if showing_welcome {
            tracing::trace!("render: before welcome_page");
            // 欢迎页：全屏居中，不受侧边栏和活动栏影响
            // 但当右侧面板或底部面板打开时，欢迎页需要避让
            let welcome_x = 0.0;
            let mut welcome_width = self.window_width as f32;
            if self.layout.right_panel_visible {
                welcome_width -= self.layout.right_panel_width;
            }
            let welcome_y = self.layout.top_offset();
            let mut welcome_height = self.window_height as f32 - welcome_y;
            if self.layout.status_bar_visible {
                welcome_height -= self.layout.status_bar_height;
            }
            if self.layout.bottom_panel_visible {
                welcome_height -= self.layout.bottom_panel_height;
            }
            welcome_height = welcome_height.max(200.0);
            self.render_welcome_page(&target, welcome_x, welcome_y, welcome_width, welcome_height);
            tracing::trace!("render: after welcome_page");
        } else if showing_empty_placeholder {
            // 空占位页：标签栏为空 + 文件夹已打开时，在编辑区居中显示 logo
            // 侧边栏/活动栏/状态栏均保持可见
            self.render_empty_placeholder(
                &target,
                editor_content_region.x,
                editor_content_region.y,
                editor_content_region.width,
                editor_content_region.height,
            );
        } else if self.active_tab_is_settings() {
            // 设置页面：在编辑器内容区域渲染左侧导航+右侧内容
            let text_brush = match self
                .render_ctx
                .brush_cache
                .get_brush(&target, &self.theme.text_default)
            {
                Ok(b) => b,
                Err(_) => return,
            };
            self.render_settings_sidebar(
                &target,
                editor_content_region.x,
                editor_content_region.y,
                editor_content_region.width,
                editor_content_region.height,
                &text_brush,
            );
        } else if self.content.language == Language::Image {
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
        if self.layout.bottom_panel_visible {
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
        // REQ-P3-02: 测量并缓存子菜单宽度，hit_test 时复用
        let submenu_data = self.menu_bar.active_index.and_then(|active_idx| {
            self.menu_bar
                .items
                .get(active_idx)
                .filter(|item| item.expanded)
                .and_then(|item| {
                    let submenu_x = self.menu_bar.item_x_positions.get(active_idx).copied()?;
                    Some((active_idx, submenu_x, item.clone()))
                })
        });
        if let Some((active_idx, submenu_x, item)) = submenu_data {
            // REQ-P3-02: 测量子菜单内容宽度并写回缓存，hit_test 时使用
            let measured = self.measure_submenu_width(&item);
            if let Some(item_ref) = self.menu_bar.items.get_mut(active_idx) {
                item_ref.submenu_width = measured;
            }
            let item_for_render = crate::menu_bar::MenuBarItem {
                submenu_width: measured,
                ..item
            };
            // 子菜单从标题栏下方弹出
            let submenu_y = titlebar_region.y + titlebar_region.height;
            self.render_submenu(&target, submenu_x, submenu_y, &item_for_render);
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

        // 11. 新建项目对话框
        if self.new_project_dialog.visible {
            self.render_new_project_dialog(&target);
        }

        // 12. 用户下拉菜单（最后渲染，确保在所有 UI 之上）
        if self.user_menu.is_open {
            let titlebar_h = self.layout.title_bar_height;
            let window_w = self.window_width as f32;
            let btn_width = 40.0;
            let close_x = window_w - btn_width;
            let maximize_x = close_x - btn_width;
            let minimize_x = maximize_x - btn_width;
            let user_btn_size = 26.0;
            let user_btn_x = minimize_x - 28.0 * 3.0 - user_btn_size - 4.0;
            let user_btn_y = (titlebar_h - user_btn_size) / 2.0;
            self.render_user_menu(&target, user_btn_x, user_btn_y + user_btn_size + 4.0);
        }

        // 13. 资源管理器空白区域上下文菜单（最上层渲染，覆盖所有内容）
        if self.explorer_context_menu.is_open {
            self.render_explorer_context_menu(&target);
        }

        // 14. 标签右键上下文菜单（最顶层渲染，覆盖所有内容）
        if self.tab_context_menu.visible {
            self.render_tab_context_menu(&target);
        }

        // 15. 活动栏右键上下文菜单（最顶层渲染，覆盖所有内容）
        if self.activity_bar_context_menu.visible {
            self.render_activity_bar_context_menu(&target);
        }

        // 弹出裁剪区域（如果设置了）——必须在 end_draw 之前
        // REQ-P3-03: 根据 use_layer 标志选择 PopLayer 或 PopAxisAlignedClip
        if use_clip {
            self.render_ctx.pop_multi_clip(use_layer);
        }

        match self.render_ctx.end_draw() {
            Ok(()) => {}
            Err(e) => {
                // 设备丢失（D2DERR_RECREATE_TARGET = 0x8899000C），需要重建渲染目标
                if e.code().0 as u32 == 0x8899000C {
                    self.render_ctx.handle_device_lost();
                    // P4-4: 同时清理 IconCache，确保下次绘制时从新 factory 重建几何
                    self.icons.clear();
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
        self.last_cursor_line = self.content.cursor_line;
        self.last_cursor_col = self.content.cursor_col;
        self.last_scroll_y = self.content.scroll_y;
        self.last_selection_start = self.content.selection_start;
        self.last_selection_end = self.content.selection_end;
        self.last_sidebar_content = self.sidebar_content.clone();
        self.last_sidebar_visible = self.layout.sidebar_visible;
        self.last_activity_bar_visible = self.layout.activity_bar_visible;
        self.last_right_panel_visible = self.layout.right_panel_visible;
        self.last_bottom_panel_visible = self.layout.bottom_panel_visible;
        self.last_status_message.clone_from(&self.status_message);
        self.last_active_tab = self.active_tab;

        // P3.4: 渲染 hover tooltip（在最上层，覆盖所有内容）
        self.render_hover_tooltip(&target);

        // UI Tooltip：500ms 延迟显示的活动栏/标题栏悬停提示（最上层）
        if let Ok(tooltip_format) = self.render_ctx.text_format_cache.get_format(
            12.0,
            DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
            DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
            DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
        ) {
            self.render_tooltip(&target, &tooltip_format);
        }

        // 清除脏矩形标记（渲染完成）
        self.dirty_tracker.clear();

        // TEST: 将本帧命中区域写入文件
        crate::hit_test::flush_hit_regions_to_file();
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

            // 调整手柄：悬停或拖拽时在右边缘叠加蓝色高亮
            if self.hover_sidebar_resize || self.layout.sidebar_resizing {
                let handle_color = color_f(0.0, 0.47, 0.83, 1.0);
                let handle_brush =
                    match self.render_ctx.brush_cache.get_brush(target, &handle_color) {
                        Ok(b) => b,
                        Err(_) => return,
                    };
                let handle_rect = D2D_RECT_F {
                    left: x + width - 1.0,
                    top: y,
                    right: x + width + 1.0,
                    bottom: y + height,
                };
                target.FillRectangle(&handle_rect, &handle_brush);
            }

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
                    if self.is_loading_folder {
                        self.render_loading_spinner(target, x, y, width, height, &text_brush);
                    } else {
                        self.render_file_tree_sidebar(target, x, y, width, height, &text_brush);
                    }
                }
                crate::layout::SidebarContent::SourceControlPanel => {
                    self.render_source_control_sidebar(target, x, y, width, height, &text_brush);
                }
                crate::layout::SidebarContent::AiAssistantPanel => {
                    // AI 面板已迁移到右侧面板，左侧栏不再渲染 AI 内容
                }
                crate::layout::SidebarContent::RemoteManagerPanel => {
                    self.render_ssh_manager_sidebar(target, x, y, width, height, &text_brush);
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

    /// 渲染加载中提示（spinner + 文字）
    fn render_loading_spinner(
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
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();

            // 居中显示"正在扫描文件夹..."
            let cx = x + width / 2.0;
            let cy = y + height / 3.0;
            let spinner_radius = 12.0f32;

            let ring_color = color_f(0.3, 0.3, 0.3, 1.0);
            let ring_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &ring_color)
                .unwrap();
            let dot_color = color_f(0.25, 0.65, 0.95, 1.0);
            let dot_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &dot_color)
                .unwrap();

            // 用 GetTickCount 做简单的旋转动画相位
            let phase = (windows::Win32::System::SystemInformation::GetTickCount() as f32 / 200.0)
                % (std::f32::consts::TAU);
            let dot_x = cx + phase.cos() * spinner_radius;
            let dot_y = cy + phase.sin() * spinner_radius;

            // 画底环
            let ring_ellipse = windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                point: windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F { x: cx, y: cy },
                radiusX: spinner_radius,
                radiusY: spinner_radius,
            };
            target.DrawEllipse(&ring_ellipse, &ring_brush, 1.5, None);

            // 画旋转的小圆点
            let dot_ellipse = windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                point: windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                    x: dot_x,
                    y: dot_y,
                },
                radiusX: 3.0,
                radiusY: 3.0,
            };
            target.FillEllipse(&dot_ellipse, &dot_brush);

            // 文字提示
            let loading_text: Vec<u16> =
                "正在扫描文件夹...".encode_utf16().chain(Some(0)).collect();
            let text_rect = D2D_RECT_F {
                left: x,
                top: cy + spinner_radius + 12.0,
                right: x + width,
                bottom: cy + spinner_radius + 40.0,
            };
            target.DrawText(
                &loading_text,
                &ui_format,
                &text_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 强制下一帧重绘以驱动动画
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(self.hwnd, None, false);
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
        let s = self.dpi_scale;
        unsafe {
            // 确保矢量图标几何已创建（FilePython / FileJava / FileText）
            self.icons.ensure_created_from_target(target);
            let ui_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0 * s,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            // 章节标题：11px 加粗，与"源代码管理"侧栏保持一致
            let header_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    11.0 * s,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();
            let tree_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    10.0 * s,
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
            // 章节分隔线颜色
            let sep_color = color_f(0.2, 0.2, 0.2, 1.0);
            let sep_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sep_color)
                .unwrap();
            let btn_bg_color = color_f(0.18, 0.18, 0.18, 1.0);
            let btn_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_bg_color)
                .unwrap();
            let btn_hover_color = color_f(0.28, 0.28, 0.28, 1.0);
            let btn_hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_hover_color)
                .unwrap();

            // 章节标题栏（与"源代码管理"风格一致，约 28px 高）
            let header_h = 28.0f32 * s;
            let header_text: Vec<u16> = "资源管理器".encode_utf16().chain(Some(0)).collect();
            let header_text_rect = D2D_RECT_F {
                left: x + 10.0 * s,
                top: y,
                right: x + width - 68.0 * s,
                bottom: y + header_h,
            };
            target.DrawText(
                &header_text,
                &header_format,
                &header_text_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 标题栏右侧：新建文件 / 新建文件夹按钮
            let btn_size = 20.0f32 * s;
            let btn_margin = 4.0f32 * s;
            let new_file_rect = D2D_RECT_F {
                left: x + width - btn_size * 2.0 - btn_margin * 2.0,
                top: y + (header_h - btn_size) / 2.0,
                right: x + width - btn_size - btn_margin * 2.0,
                bottom: y + (header_h + btn_size) / 2.0,
            };
            let new_folder_rect = D2D_RECT_F {
                left: x + width - btn_size - btn_margin,
                top: y + (header_h - btn_size) / 2.0,
                right: x + width - btn_margin,
                bottom: y + (header_h + btn_size) / 2.0,
            };
            // 保存按钮区域供 hit test 使用
            self.file_tree_new_file_btn = Some(crate::layout::Region::new(
                new_file_rect.left,
                new_file_rect.top,
                new_file_rect.right - new_file_rect.left,
                new_file_rect.bottom - new_file_rect.top,
            ));
            self.file_tree_new_folder_btn = Some(crate::layout::Region::new(
                new_folder_rect.left,
                new_folder_rect.top,
                new_folder_rect.right - new_folder_rect.left,
                new_folder_rect.bottom - new_folder_rect.top,
            ));

            let nf_hover = self
                .file_tree_new_file_btn
                .as_ref()
                .map(|r| r.contains(self.hover_last_mouse_x, self.hover_last_mouse_y))
                .unwrap_or(false);
            let nfo_hover = self
                .file_tree_new_folder_btn
                .as_ref()
                .map(|r| r.contains(self.hover_last_mouse_x, self.hover_last_mouse_y))
                .unwrap_or(false);

            target.FillRectangle(
                &new_file_rect,
                if nf_hover {
                    &btn_hover_brush
                } else {
                    &btn_bg_brush
                },
            );
            target.FillRectangle(
                &new_folder_rect,
                if nfo_hover {
                    &btn_hover_brush
                } else {
                    &btn_bg_brush
                },
            );

            let btn_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0 * s,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();
            let new_file_text: Vec<u16> = "\u{2795}".encode_utf16().chain(Some(0)).collect();
            let new_folder_text: Vec<u16> = "\u{1F4C1}".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &new_file_text,
                &btn_format,
                &new_file_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            target.DrawText(
                &new_folder_text,
                &btn_format,
                &new_folder_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 标题下方的分隔线
            let sep_rect = D2D_RECT_F {
                left: x,
                top: y + header_h,
                right: x + width,
                bottom: y + header_h + 1.0 * s,
            };
            target.FillRectangle(&sep_rect, &sep_brush);

            // 文件树内联输入框（新建文件/文件夹时显示）
            // 该输入框的 y 偏移会通过 file_tree_list_start_y() 自动包含，
            // 此处仍需渲染输入框 UI。
            if let Some(input) = &self.file_tree_input {
                let input_y = y + header_h + 6.0 * s;
                let input_h = 26.0f32 * s;
                let input_rect = D2D_RECT_F {
                    left: x + 10.0 * s,
                    top: input_y,
                    right: x + width - 10.0 * s,
                    bottom: input_y + input_h,
                };
                let input_bg = color_f(0.12, 0.12, 0.12, 1.0);
                let input_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &input_bg)
                    .unwrap();
                let cursor_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &self.theme.cursor_color)
                    .unwrap();
                target.FillRectangle(&input_rect, &input_bg_brush);
                target.DrawRectangle(&input_rect, &sep_brush, 1.0 * s, None);

                let value_text: Vec<u16> = input.value.encode_utf16().collect();
                let value_rect = D2D_RECT_F {
                    left: input_rect.left + 6.0 * s,
                    top: input_rect.top + 2.0 * s,
                    right: input_rect.right - 6.0 * s,
                    bottom: input_rect.bottom - 2.0 * s,
                };
                target.DrawText(
                    &value_text,
                    &ui_format,
                    &value_rect,
                    text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // 精确测量 value 文本宽度（支持 CJK 双宽字符）
                let ui_font_size = 13.0f32 * s;
                let value_width = self
                    .render_ctx
                    .text_format_cache
                    .measure_text_width(
                        &input.value,
                        ui_font_size,
                        DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    )
                    .unwrap_or(0.0);

                // IME 合成串（pre-edit text）显示在 value 之后
                let mut comp_width = 0.0f32;
                if let Some(comp) = &input.composition {
                    if !comp.is_empty() {
                        let comp_text: Vec<u16> = comp.encode_utf16().collect();
                        let comp_x = value_rect.left + value_width;
                        let comp_rect = D2D_RECT_F {
                            left: comp_x,
                            top: value_rect.top,
                            right: value_rect.right,
                            bottom: value_rect.bottom,
                        };
                        // 合成串用稍暗的颜色，带下划线效果
                        let comp_brush = self
                            .render_ctx
                            .brush_cache
                            .get_brush(target, &color_f(1.0, 0.9, 0.4, 1.0))
                            .unwrap();
                        target.DrawText(
                            &comp_text,
                            &ui_format,
                            &comp_rect,
                            &comp_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        comp_width = self
                            .render_ctx
                            .text_format_cache
                            .measure_text_width(
                                comp,
                                ui_font_size,
                                DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                            )
                            .unwrap_or(0.0);
                    }
                }

                // 光标：使用精确测量的文本宽度定位
                if input.caret_visible {
                    let caret_x = value_rect.left + value_width + comp_width;
                    let caret_rect = D2D_RECT_F {
                        left: caret_x,
                        top: value_rect.top + 2.0 * s,
                        right: caret_x + 1.0 * s,
                        bottom: value_rect.bottom - 2.0 * s,
                    };
                    target.FillRectangle(&caret_rect, &cursor_brush);
                }
            }

            if let Some(tree) = &self.file_tree {
                // 与 handle_file_tree_click / update_local_tree_hover 共用同一公式
                //（避免 dpi_scale / scroll / inline input 不一致时焦点错位）
                let mut current_y = y + self.file_tree_list_start_y();
                self.render_tree_nodes(
                    target,
                    tree,
                    u32::MAX,
                    x + 10.0 * s,
                    &mut current_y,
                    y,
                    height,
                    width,
                    &tree_format,
                    text_brush,
                    &dir_brush,
                    &sel_brush,
                    &hover_brush,
                );
            } else if self.file_tree_input.is_none() {
                let text: Vec<u16> = "按 Ctrl+K 打开文件夹"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let text_rect = D2D_RECT_F {
                    left: x + 10.0 * s,
                    top: y + header_h + 6.0 * s,
                    right: x + width - 10.0 * s,
                    bottom: y + header_h + 26.0 * s,
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
            let s = self.dpi_scale;
            let ui_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0 * s,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let tree_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    11.0 * s,
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
                left: x + 10.0 * s,
                top: y + 10.0 * s,
                right: x + width - 10.0 * s,
                bottom: y + 30.0 * s,
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
                let node_height = 16.0_f32 * s;
                let mut current_y = y + 40.0 * s - self.remote_scroll_y;
                let hover = self.hover_remote_node.as_ref();
                let selected = self.selected_remote_node.as_ref();
                Self::draw_remote_nodes_recursive(
                    target,
                    &tree.nodes,
                    x,
                    width,
                    y,
                    height,
                    node_height,
                    s,
                    &mut current_y,
                    hover,
                    selected,
                    &dir_brush,
                    text_brush,
                    &hover_brush,
                    &sel_brush,
                    &tree_format,
                    &self.render_ctx.text_layout_cache,
                );
            } else {
                let msg: Vec<u16> = "未连接远程服务器".encode_utf16().chain(Some(0)).collect();
                let msg_rect = D2D_RECT_F {
                    left: x + 10.0 * s,
                    top: y + 40.0 * s,
                    right: x + width - 10.0 * s,
                    bottom: y + 60.0 * s,
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

    /// P0-1: 递归绘制远程文件树节点（含展开目录的子节点）
    #[allow(clippy::too_many_arguments)]
    fn draw_remote_nodes_recursive(
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        nodes: &[crate::ssh::RemoteFileNode],
        x: f32,
        width: f32,
        clip_top: f32,
        clip_bottom: f32,
        node_height: f32,
        scale: f32,
        current_y: &mut f32,
        hover: Option<&String>,
        selected: Option<&String>,
        dir_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        hover_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        sel_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        tree_format: &IDWriteTextFormat,
        text_layout_cache: &aether_render::d2d::brush_cache::TextLayoutCache,
    ) {
        let s = scale;
        for node in nodes {
            // 超出可见区域底部：停止（节点按顺序排列）
            if *current_y > clip_bottom {
                break;
            }
            // 跳过完全在顶部以上的节点（但需推进 current_y）
            let visible = *current_y + node_height >= clip_top;
            let indent = node.depth as f32 * 16.0 * s;
            let item_left = x + 10.0 * s + indent;
            let item_right = x + width - 10.0 * s;

            if visible {
                // P0-1: Direct2D 绘制调用需在 unsafe 块中执行
                unsafe {
                    let is_hover = hover == Some(&node.path);
                    if is_hover {
                        let hover_rect = D2D_RECT_F {
                            left: item_left - 4.0 * s,
                            top: *current_y,
                            right: item_right,
                            bottom: *current_y + node_height,
                        };
                        target.FillRectangle(&hover_rect, hover_brush);
                    }

                    let is_selected = selected == Some(&node.path) && !node.is_dir;
                    if is_selected {
                        let sel_rect = D2D_RECT_F {
                            left: item_left - 4.0 * s,
                            top: *current_y,
                            right: item_right,
                            bottom: *current_y + node_height,
                        };
                        target.FillRectangle(&sel_rect, sel_brush);
                    }

                    let icon = if node.is_dir {
                        if node.is_expanded {
                            "📂"
                        } else {
                            "📁"
                        }
                    } else {
                        "📄"
                    };
                    // P0-1: 正在加载子目录时显示 ⏳ 指示器
                    let arrow = if node.is_dir {
                        if node.is_loading {
                            "⏳ "
                        } else if node.is_expanded {
                            "▼ "
                        } else {
                            "▶ "
                        }
                    } else {
                        ""
                    };
                    let display = format!("{}{} {}", arrow, icon, node.name);
                    let brush = if node.is_dir { dir_brush } else { text_brush };
                    // 单行 + 字符级"…"省略号：与文件资源管理器一致，避免长名换行堆叠
                    let max_text_w = (item_right - item_left).max(1.0);
                    let layout = text_layout_cache
                        .create_ellipsis_layout(&display, tree_format, max_text_w, node_height)
                        .unwrap();
                    let point = D2D_POINT_2F {
                        x: item_left,
                        y: *current_y,
                    };
                    target.DrawTextLayout(point, &layout, brush, D2D1_DRAW_TEXT_OPTIONS_CLIP);
                }
            }

            *current_y += node_height;
            // 仅展开的目录才递归绘制子节点
            if node.is_expanded {
                Self::draw_remote_nodes_recursive(
                    target,
                    &node.children,
                    x,
                    width,
                    clip_top,
                    clip_bottom,
                    node_height,
                    scale,
                    current_y,
                    hover,
                    selected,
                    dir_brush,
                    text_brush,
                    hover_brush,
                    sel_brush,
                    tree_format,
                    text_layout_cache,
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
                    // H-22: 使用 char 计数而非字节计数，避免多字节 UTF-8 泄漏长度
                    let hidden: String = self.ssh_dialog.password.chars().map(|_| '*').collect();
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

    fn render_new_project_dialog(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
    ) {
        unsafe {
            let scale = self.dpi_scale.max(1.0);
            let width = 460.0f32 / scale;
            let height = 220.0f32 / scale;
            let x = (self.window_width as f32 / scale - width) / 2.0;
            let y = (self.window_height as f32 / scale - height) / 2.0;

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
            let small_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    11.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
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
            let title: Vec<u16> = "新建项目".encode_utf16().chain(Some(0)).collect();
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
            cy += 36.0;

            // 基础路径显示
            let base_label: Vec<u16> = "项目位置:".encode_utf16().chain(Some(0)).collect();
            let base_label_rect = D2D_RECT_F {
                left: x + 16.0,
                top: cy,
                right: x + 80.0,
                bottom: cy + 18.0,
            };
            target.DrawText(
                &base_label,
                &format,
                &base_label_rect,
                &dim_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let base_path_text: Vec<u16> = self
                .new_project_dialog
                .base_path
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let base_path_rect = D2D_RECT_F {
                left: x + 80.0,
                top: cy,
                right: x + width - 16.0,
                bottom: cy + 18.0,
            };
            target.DrawText(
                &base_path_text,
                &small_format,
                &base_path_rect,
                &dim_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 28.0;

            // 项目名称输入
            let label_text: Vec<u16> = "项目名称:".encode_utf16().chain(Some(0)).collect();
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
                .new_project_dialog
                .project_name
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

            if self.new_project_dialog.focus_field == 0 {
                // 绘制输入框光标
                if self.new_project_dialog.caret_visible {
                    let char_width = self.text_renderer.char_width();
                    let text_width: f32 = self
                        .new_project_dialog
                        .project_name
                        .chars()
                        .map(|ch| char_width * unicode_char_width(ch) as f32)
                        .sum();
                    let caret_x = (x + 84.0 + text_width).min(x + width - 22.0);
                    let caret_rect = D2D_RECT_F {
                        left: caret_x,
                        top: cy + 1.0,
                        right: caret_x + 1.0,
                        bottom: cy + 17.0,
                    };
                    let caret_color = color_f(0.9, 0.9, 0.9, 1.0);
                    let caret_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &caret_color)
                        .unwrap();
                    target.FillRectangle(&caret_rect, &caret_brush);
                }

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
            cy += 40.0;

            // 错误消息
            if let Some(err) = &self.new_project_dialog.error_message {
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

            cy += 8.0;

            // 按钮：确认 和 取消
            let btn_w = 80.0;
            let btn_h = 28.0;

            let confirm_btn_rect = D2D_RECT_F {
                left: x + width - 16.0 - btn_w * 2.0 - 8.0,
                top: cy,
                right: x + width - 16.0 - btn_w - 8.0,
                bottom: cy + btn_h,
            };
            let is_confirm_hover = self.new_project_dialog.hover_button == Some(0);
            target.FillRectangle(
                &confirm_btn_rect,
                if is_confirm_hover {
                    &btn_hover_brush
                } else {
                    &btn_bg_brush
                },
            );
            let confirm_text: Vec<u16> = "确认".encode_utf16().chain(Some(0)).collect();
            let confirm_text_rect = D2D_RECT_F {
                left: confirm_btn_rect.left,
                top: cy + 4.0,
                right: confirm_btn_rect.right,
                bottom: cy + btn_h - 2.0,
            };
            target.DrawText(
                &confirm_text,
                &format,
                &confirm_text_rect,
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
            let is_cancel_hover = self.new_project_dialog.hover_button == Some(1);
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

            // 存储区域用于点击检测
            self.new_project_dialog.input_rect = Some(crate::layout::Region::new(
                input_rect.left,
                input_rect.top,
                input_rect.right - input_rect.left,
                input_rect.bottom - input_rect.top,
            ));
            self.new_project_dialog.confirm_btn_rect = Some(crate::layout::Region::new(
                confirm_btn_rect.left,
                confirm_btn_rect.top,
                confirm_btn_rect.right - confirm_btn_rect.left,
                confirm_btn_rect.bottom - confirm_btn_rect.top,
            ));
            self.new_project_dialog.cancel_btn_rect = Some(crate::layout::Region::new(
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

            // ConPTY 模式：输入回显由 shell 处理，无需本地渲染输入行
        }
    }

    // 保留 render_central_terminal 方法定义，但不再被调用
    // 终端已迁移到底部面板 (render_bottom_panel)
    #[allow(dead_code)]
    fn render_central_terminal(
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

    /// 渲染 SSH 远程管理面板（侧边栏）
    /// 显示已保存的服务器列表、连接状态、添加/编辑/删除/连接操作
    #[allow(clippy::too_many_lines)]
    fn render_ssh_manager_sidebar(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        // 先快照所需状态，避免与 panel 的可变借用冲突
        let active_count = self.active_ssh_count();
        let servers: Vec<aether_shared::settings::SshServerConfig> = self.ssh_servers().to_vec();
        let ssh_connecting = self.ssh_connecting;
        // 预计算每个服务器的连接状态
        let connected_states: Vec<bool> = (0..servers.len())
            .map(|i| self.is_ssh_connected(i))
            .collect();
        let connecting_states: Vec<bool> = (0..servers.len())
            .map(|i| self.is_ssh_connecting() && self.active_ssh_index == Some(i))
            .collect();

        let panel = &mut self.ssh_manager_panel;
        // 清除上一帧的按钮区域
        panel.item_btn_rects.clear();

        unsafe {
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
            let label_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            let btn_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();

            let dim_color = color_f(0.55, 0.55, 0.55, 1.0);
            let dim_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &dim_color)
                .unwrap();
            let green_color = color_f(0.3, 0.85, 0.4, 1.0);
            let green_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &green_color)
                .unwrap();
            let red_color = color_f(0.85, 0.3, 0.3, 1.0);
            let red_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &red_color)
                .unwrap();
            let hover_color = color_f(0.2, 0.2, 0.2, 1.0);
            let hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_color)
                .unwrap();
            let sel_color = color_f(0.0, 0.47, 0.83, 0.3);
            let sel_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sel_color)
                .unwrap();
            let btn_bg_color = color_f(0.15, 0.15, 0.15, 1.0);
            let btn_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_bg_color)
                .unwrap();
            let btn_hover_color = color_f(0.25, 0.25, 0.25, 1.0);
            let btn_hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &btn_hover_color)
                .unwrap();
            let input_bg_color = color_f(0.12, 0.12, 0.12, 1.0);
            let input_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &input_bg_color)
                .unwrap();
            let focus_color = color_f(0.0, 0.47, 0.83, 1.0);
            let focus_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &focus_color)
                .unwrap();

            let margin = 10.0_f32;
            let item_h = 32.0_f32;
            let mut cy = y + 10.0;

            // 标题 + 活跃连接数（active_count 已在 panel 借用前快照）
            let title_text: Vec<u16> = format!("SSH 远程管理  ({active_count} 连接中)")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let title_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 22.0,
            };
            target.DrawText(
                &title_text,
                &title_format,
                &title_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 28.0;

            if panel.editing {
                // ===== 编辑/添加表单 =====
                let fields: [(&str, &str); 5] = [
                    ("名称", &panel.form_name),
                    ("主机", &panel.form_host),
                    ("端口", &panel.form_port),
                    ("用户名", &panel.form_username),
                    ("密钥路径", &panel.form_key_path),
                ];
                let field_height = 30.0_f32;
                let form_width = width - margin * 2.0;

                for (i, (label, value)) in fields.iter().enumerate() {
                    // 认证方式字段特殊处理
                    let actual_label =
                        if i == 4 && panel.form_auth_type != crate::ssh::SshAuthType::Key {
                            "（密钥路径不可用）"
                        } else {
                            label
                        };

                    // 标签
                    let label_text: Vec<u16> = actual_label.encode_utf16().chain(Some(0)).collect();
                    let label_rect = D2D_RECT_F {
                        left: x + margin,
                        top: cy,
                        right: x + width - margin,
                        bottom: cy + 16.0,
                    };
                    target.DrawText(
                        &label_text,
                        &ui_format,
                        &label_rect,
                        &dim_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    cy += 18.0;

                    // 输入框背景
                    let input_rect = D2D_RECT_F {
                        left: x + margin,
                        top: cy,
                        right: x + margin + form_width,
                        bottom: cy + field_height - 4.0,
                    };
                    let is_key_field = i == 4;
                    let draw_input =
                        !(is_key_field && panel.form_auth_type != crate::ssh::SshAuthType::Key);
                    if draw_input {
                        target.FillRectangle(&input_rect, &input_bg_brush);
                        // 焦点边框
                        if panel.focus_field == i {
                            target.DrawRectangle(&input_rect, &focus_brush, 1.0, None);
                        }
                        // 值
                        let val_text: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
                        let val_rect = D2D_RECT_F {
                            left: input_rect.left + 6.0,
                            top: input_rect.top + 2.0,
                            right: input_rect.right - 4.0,
                            bottom: input_rect.bottom - 2.0,
                        };
                        target.DrawText(
                            &val_text,
                            &label_format,
                            &val_rect,
                            text_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                    cy += field_height;

                    // 在密钥路径字段后显示认证方式选择
                    if i == 3 {
                        let auth_label = match panel.form_auth_type {
                            crate::ssh::SshAuthType::Agent => "认证: Agent（点击切换）",
                            crate::ssh::SshAuthType::Key => "认证: 密钥（点击切换）",
                            // P1-2: 密码认证已禁用，此分支理论上不可达（cycle 不再进入 Password）
                            crate::ssh::SshAuthType::Password => {
                                "认证: 密码（已禁用，点击切换为 Agent）"
                            }
                        };
                        let auth_text: Vec<u16> =
                            auth_label.encode_utf16().chain(Some(0)).collect();
                        let auth_rect = D2D_RECT_F {
                            left: x + margin,
                            top: cy,
                            right: x + width - margin,
                            bottom: cy + 20.0,
                        };
                        target.FillRectangle(&auth_rect, &btn_bg_brush);
                        target.DrawText(
                            &auth_text,
                            &ui_format,
                            &auth_rect,
                            text_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        panel.item_btn_rects.push((
                            999,
                            0,
                            crate::layout::Region::new(
                                auth_rect.left,
                                auth_rect.top,
                                auth_rect.right - auth_rect.left,
                                auth_rect.bottom - auth_rect.top,
                            ),
                        ));
                        cy += 24.0;
                    }
                }

                // 错误消息
                if let Some(err) = &panel.error_message {
                    let err_text: Vec<u16> = err.encode_utf16().chain(Some(0)).collect();
                    let err_rect = D2D_RECT_F {
                        left: x + margin,
                        top: cy,
                        right: x + width - margin,
                        bottom: cy + 18.0,
                    };
                    target.DrawText(
                        &err_text,
                        &ui_format,
                        &err_rect,
                        &red_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    cy += 22.0;
                }

                // 保存 / 取消按钮
                let btn_w = 80.0_f32;
                let btn_h = 24.0_f32;
                let save_rect = D2D_RECT_F {
                    left: x + margin,
                    top: cy,
                    right: x + margin + btn_w,
                    bottom: cy + btn_h,
                };
                let cancel_rect = D2D_RECT_F {
                    left: x + margin + btn_w + 8.0,
                    top: cy,
                    right: x + margin + btn_w * 2.0 + 8.0,
                    bottom: cy + btn_h,
                };
                let save_hover = panel.hover_action == Some((998, 0));
                target.FillRectangle(
                    &save_rect,
                    if save_hover {
                        &btn_hover_brush
                    } else {
                        &btn_bg_brush
                    },
                );
                let save_text: Vec<u16> = "保存".encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &save_text,
                    &btn_format,
                    &save_rect,
                    text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                panel.save_btn_rect = Some(crate::layout::Region::new(
                    save_rect.left,
                    save_rect.top,
                    save_rect.right - save_rect.left,
                    save_rect.bottom - save_rect.top,
                ));

                let cancel_hover = panel.hover_action == Some((998, 1));
                target.FillRectangle(
                    &cancel_rect,
                    if cancel_hover {
                        &btn_hover_brush
                    } else {
                        &btn_bg_brush
                    },
                );
                let cancel_text: Vec<u16> = "取消".encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &cancel_text,
                    &btn_format,
                    &cancel_rect,
                    text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                panel.cancel_btn_rect = Some(crate::layout::Region::new(
                    cancel_rect.left,
                    cancel_rect.top,
                    cancel_rect.right - cancel_rect.left,
                    cancel_rect.bottom - cancel_rect.top,
                ));
            } else {
                // ===== 服务器列表视图（servers 已在 panel 借用前克隆） =====
                if servers.is_empty() {
                    let empty_text: Vec<u16> = "暂无 SSH 服务器配置\n点击下方按钮添加"
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                    let empty_rect = D2D_RECT_F {
                        left: x + margin,
                        top: cy,
                        right: x + width - margin,
                        bottom: cy + 40.0,
                    };
                    target.DrawText(
                        &empty_text,
                        &ui_format,
                        &empty_rect,
                        &dim_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    cy += 44.0;
                } else {
                    for (i, server) in servers.iter().enumerate() {
                        if cy > y + height {
                            break;
                        }

                        // 服务器条目背景
                        let is_hover = panel.hover == Some(i);
                        let is_selected = panel.selected == Some(i);
                        let item_rect = D2D_RECT_F {
                            left: x + 4.0,
                            top: cy,
                            right: x + width - 4.0,
                            bottom: cy + item_h,
                        };
                        if is_selected {
                            target.FillRectangle(&item_rect, &sel_brush);
                        } else if is_hover {
                            target.FillRectangle(&item_rect, &hover_brush);
                        }

                        // 连接状态指示灯
                        let dot_x = x + margin;
                        let dot_y = cy + item_h / 2.0 - 4.0;
                        let is_connected = connected_states[i];
                        let is_connecting = connecting_states[i];
                        let dot_color = if is_connected {
                            green_color
                        } else if is_connecting {
                            color_f(0.85, 0.7, 0.2, 1.0)
                        } else {
                            dim_color
                        };
                        let dot_brush = self
                            .render_ctx
                            .brush_cache
                            .get_brush(target, &dot_color)
                            .unwrap();
                        let ellipse = windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                            point: windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                                x: dot_x + 4.0,
                                y: dot_y + 4.0,
                            },
                            radiusX: 4.0,
                            radiusY: 4.0,
                        };
                        target.FillEllipse(&ellipse, &dot_brush);

                        // 服务器名称 + 主机
                        let name_text: Vec<u16> = format!(
                            "{}  ({}@{}:{})",
                            server.name, server.username, server.host, server.port
                        )
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                        let name_rect = D2D_RECT_F {
                            left: x + margin + 16.0,
                            top: cy + 4.0,
                            right: x + width - margin - 80.0,
                            bottom: cy + 20.0,
                        };
                        target.DrawText(
                            &name_text,
                            &label_format,
                            &name_rect,
                            text_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        // 认证方式
                        let auth_text: Vec<u16> = match server.auth_type.as_str() {
                            "key" => format!("🔑 {}", server.key_path),
                            // P1-2: 密码认证已禁用，加载时已迁移为 agent，此分支仅作兜底
                            "password" => "密码（已禁用，已迁移为 Agent）".to_string(),
                            _ => "Agent".to_string(),
                        }
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                        let auth_rect = D2D_RECT_F {
                            left: x + margin + 16.0,
                            top: cy + 18.0,
                            right: x + width - margin - 80.0,
                            bottom: cy + 32.0,
                        };
                        target.DrawText(
                            &auth_text,
                            &ui_format,
                            &auth_rect,
                            &dim_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        // 操作按钮: 连接/断开, 编辑, 删除
                        let btn_size = 20.0_f32;
                        let btn_gap = 4.0_f32;
                        let mut btn_x = x + width - margin - btn_size * 3.0 - btn_gap * 2.0;

                        // 按钮 0: 连接/断开
                        let connect_label = if is_connected { "⏹" } else { "▶" };
                        let connect_rect = D2D_RECT_F {
                            left: btn_x,
                            top: cy + (item_h - btn_size) / 2.0,
                            right: btn_x + btn_size,
                            bottom: cy + (item_h + btn_size) / 2.0,
                        };
                        let connect_hover = panel.hover_action == Some((i, 0));
                        target.FillRectangle(
                            &connect_rect,
                            if connect_hover {
                                &btn_hover_brush
                            } else {
                                &btn_bg_brush
                            },
                        );
                        let connect_text: Vec<u16> =
                            connect_label.encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &connect_text,
                            &btn_format,
                            &connect_rect,
                            if is_connected {
                                &red_brush
                            } else {
                                &green_brush
                            },
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        panel.item_btn_rects.push((
                            i,
                            0,
                            crate::layout::Region::new(
                                connect_rect.left,
                                connect_rect.top,
                                connect_rect.right - connect_rect.left,
                                connect_rect.bottom - connect_rect.top,
                            ),
                        ));
                        btn_x += btn_size + btn_gap;

                        // 按钮 1: 编辑
                        let edit_rect = D2D_RECT_F {
                            left: btn_x,
                            top: cy + (item_h - btn_size) / 2.0,
                            right: btn_x + btn_size,
                            bottom: cy + (item_h + btn_size) / 2.0,
                        };
                        let edit_hover = panel.hover_action == Some((i, 1));
                        target.FillRectangle(
                            &edit_rect,
                            if edit_hover {
                                &btn_hover_brush
                            } else {
                                &btn_bg_brush
                            },
                        );
                        let edit_text: Vec<u16> = "✎".encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &edit_text,
                            &btn_format,
                            &edit_rect,
                            text_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        panel.item_btn_rects.push((
                            i,
                            1,
                            crate::layout::Region::new(
                                edit_rect.left,
                                edit_rect.top,
                                edit_rect.right - edit_rect.left,
                                edit_rect.bottom - edit_rect.top,
                            ),
                        ));
                        btn_x += btn_size + btn_gap;

                        // 按钮 2: 删除
                        let del_rect = D2D_RECT_F {
                            left: btn_x,
                            top: cy + (item_h - btn_size) / 2.0,
                            right: btn_x + btn_size,
                            bottom: cy + (item_h + btn_size) / 2.0,
                        };
                        let del_hover = panel.hover_action == Some((i, 2));
                        target.FillRectangle(
                            &del_rect,
                            if del_hover {
                                &btn_hover_brush
                            } else {
                                &btn_bg_brush
                            },
                        );
                        let del_text: Vec<u16> = "✕".encode_utf16().chain(Some(0)).collect();
                        target.DrawText(
                            &del_text,
                            &btn_format,
                            &del_rect,
                            &red_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        panel.item_btn_rects.push((
                            i,
                            2,
                            crate::layout::Region::new(
                                del_rect.left,
                                del_rect.top,
                                del_rect.right - del_rect.left,
                                del_rect.bottom - del_rect.top,
                            ),
                        ));

                        cy += item_h + 2.0;
                    }
                }

                // 添加按钮
                let add_btn_w = width - margin * 2.0;
                let add_btn_h = 28.0_f32;
                let add_rect = D2D_RECT_F {
                    left: x + margin,
                    top: cy + 8.0,
                    right: x + margin + add_btn_w,
                    bottom: cy + 8.0 + add_btn_h,
                };
                let add_hover = panel.hover_action == Some((997, 0));
                target.FillRectangle(
                    &add_rect,
                    if add_hover {
                        &btn_hover_brush
                    } else {
                        &btn_bg_brush
                    },
                );
                let add_text: Vec<u16> = "+ 添加服务器".encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &add_text,
                    &btn_format,
                    &add_rect,
                    text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                panel.add_btn_rect = Some(crate::layout::Region::new(
                    add_rect.left,
                    add_rect.top,
                    add_rect.right - add_rect.left,
                    add_rect.bottom - add_rect.top,
                ));

                // 底部提示（ssh_connecting 已快照）
                cy += 8.0 + add_btn_h + 10.0;
                if ssh_connecting {
                    let hint_text: Vec<u16> = "正在连接...".encode_utf16().chain(Some(0)).collect();
                    let hint_rect = D2D_RECT_F {
                        left: x + margin,
                        top: cy,
                        right: x + width - margin,
                        bottom: cy + 18.0,
                    };
                    target.DrawText(
                        &hint_text,
                        &ui_format,
                        &hint_rect,
                        &dim_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                } else if let Some(err) = &panel.error_message {
                    let err_text: Vec<u16> = err.encode_utf16().chain(Some(0)).collect();
                    let err_rect = D2D_RECT_F {
                        left: x + margin,
                        top: cy,
                        right: x + width - margin,
                        bottom: cy + 18.0,
                    };
                    target.DrawText(
                        &err_text,
                        &ui_format,
                        &err_rect,
                        &red_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }

            // 让 btn_hover_brush 等不被优化掉
            let _ = (&btn_hover_brush, &hover_brush);
        }
    }

    #[allow(dead_code)]
    fn render_open_tabs_sidebar(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
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

            // 标题
            let title_text: Vec<u16> = "打开的标签页".encode_utf16().chain(Some(0)).collect();
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

            // 分隔线
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

            self.tabs_panel.clear_regions();

            let item_h = 28.0;
            let close_btn_w = 20.0;
            let mut cy = y + 44.0;
            for (idx, tab) in self.tabs.iter().enumerate() {
                if cy + item_h > y + height {
                    break;
                }
                let is_active = idx == self.active_tab;
                let is_hover = self.tabs_panel.hover_tab == Some(idx);

                let item_bg = if is_active {
                    color_f(0.16, 0.16, 0.18, 1.0)
                } else if is_hover {
                    color_f(0.20, 0.20, 0.22, 1.0)
                } else {
                    color_f(0.14, 0.14, 0.14, 1.0)
                };
                let item_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &item_bg)
                    .unwrap();
                let item_rect = D2D_RECT_F {
                    left: x + 4.0,
                    top: cy,
                    right: x + width - 4.0,
                    bottom: cy + item_h,
                };
                target.FillRectangle(&item_rect, &item_bg_brush);

                // 文件名
                // REQ-P1-09: 活动标签页的状态在 self.content 中
                let file_name = if is_active {
                    self.content.file_name()
                } else {
                    tab.file_name()
                };
                let file_text: Vec<u16> = file_name.encode_utf16().chain(Some(0)).collect();
                let file_text_rect = D2D_RECT_F {
                    left: x + 10.0,
                    top: cy,
                    right: x + width - 10.0 - close_btn_w,
                    bottom: cy + item_h,
                };
                target.DrawText(
                    &file_text,
                    &label_format,
                    &file_text_rect,
                    text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // 关闭按钮
                let close_x = x + width - 10.0 - close_btn_w;
                let close_y = cy + (item_h - 14.0) / 2.0;
                let close_hover = self.tabs_panel.hover_close == Some(idx);
                let close_color = if close_hover {
                    color_f(1.0, 1.0, 1.0, 1.0)
                } else {
                    color_f(0.5, 0.5, 0.5, 1.0)
                };
                let close_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &close_color)
                    .unwrap();
                // 画 X
                let cx = close_x + close_btn_w / 2.0;
                let cy_c = close_y + 7.0;
                let _line1 = D2D_RECT_F {
                    left: cx - 4.0,
                    top: cy_c - 4.0,
                    right: cx + 4.0,
                    bottom: cy_c + 4.0,
                };
                let _line2 = D2D_RECT_F {
                    left: cx - 4.0,
                    top: cy_c + 4.0,
                    right: cx + 4.0,
                    bottom: cy_c - 4.0,
                };
                // 简化为小矩形表示关闭按钮区域
                let close_rect = D2D_RECT_F {
                    left: close_x,
                    top: close_y,
                    right: close_x + close_btn_w,
                    bottom: close_y + 14.0,
                };
                target.DrawText(
                    &"×".encode_utf16().chain(Some(0)).collect::<Vec<u16>>(),
                    &label_format,
                    &close_rect,
                    &close_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                self.tabs_panel
                    .add_tab_region(idx, x + 4.0, cy, width - 8.0 - close_btn_w, item_h);
                self.tabs_panel
                    .add_close_region(idx, close_x, close_y, close_btn_w, 14.0);

                cy += item_h + 2.0;
            }
        }
    }

    /// 渲染设置面板：左侧导航 + 右侧内容
    fn render_settings_sidebar(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        unsafe {
            // 公共文本格式
            let nav_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
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
            let title_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    18.0,
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

            // 整体背景（右侧内容区）
            let content_bg = color_f(0.12, 0.12, 0.12, 1.0);
            let content_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &content_bg)
                .unwrap();
            let content_bg_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&content_bg_rect, &content_bg_brush);

            // 左侧导航栏布局（宽度可由用户拖拽调整）
            let nav_w = self.settings_panel.nav_width;
            let nav_x = x;
            let nav_y = y;
            let nav_h = height;

            // 导航栏背景（稍亮，与右侧区分）
            let nav_bg = color_f(0.10, 0.10, 0.10, 1.0);
            let nav_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &nav_bg)
                .unwrap();
            let nav_bg_rect = D2D_RECT_F {
                left: nav_x,
                top: nav_y,
                right: nav_x + nav_w,
                bottom: nav_y + nav_h,
            };
            target.FillRectangle(&nav_bg_rect, &nav_bg_brush);

            // 右侧分隔线
            let sep_color = color_f(0.2, 0.2, 0.2, 1.0);
            let sep_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sep_color)
                .unwrap();
            let sep_rect = D2D_RECT_F {
                left: nav_x + nav_w,
                top: nav_y,
                right: nav_x + nav_w + 1.0,
                bottom: nav_y + nav_h,
            };
            target.FillRectangle(&sep_rect, &sep_brush);

            // 调整手柄：悬停或拖拽时高亮
            if self.settings_panel.hover_nav_resize || self.settings_panel.nav_resizing {
                let handle_color = color_f(0.0, 0.47, 0.83, 1.0);
                let handle_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &handle_color)
                    .unwrap();
                let handle_rect = D2D_RECT_F {
                    left: nav_x + nav_w - 1.0,
                    top: nav_y,
                    right: nav_x + nav_w + 1.0,
                    bottom: nav_y + nav_h,
                };
                target.FillRectangle(&handle_rect, &handle_brush);
            }

            // 导航标题
            let nav_title: Vec<u16> = "设置".encode_utf16().chain(Some(0)).collect();
            let nav_title_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    16.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();
            let nav_title_rect = D2D_RECT_F {
                left: nav_x,
                top: nav_y + 16.0,
                right: nav_x + nav_w,
                bottom: nav_y + 48.0,
            };
            target.DrawText(
                &nav_title,
                &nav_title_format,
                &nav_title_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 导航项
            self.settings_panel.clear_regions();
            let tabs = crate::settings::SettingsTab::ALL;
            let nav_item_h = 36.0;
            let nav_item_start_y = nav_y + 60.0;
            for (i, tab) in tabs.iter().enumerate() {
                let item_y = nav_item_start_y + i as f32 * nav_item_h;
                let is_active = self.settings_panel.active_tab == *tab;
                let is_hover = self.settings_panel.hover_tab == Some(*tab);

                let item_bg = if is_active {
                    color_f(0.18, 0.30, 0.45, 1.0)
                } else if is_hover {
                    color_f(0.20, 0.20, 0.22, 1.0)
                } else {
                    color_f(0.10, 0.10, 0.10, 0.0)
                };
                let item_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &item_bg)
                    .unwrap();
                let item_rect = D2D_RECT_F {
                    left: nav_x,
                    top: item_y,
                    right: nav_x + nav_w,
                    bottom: item_y + nav_item_h,
                };
                target.FillRectangle(&item_rect, &item_bg_brush);

                // 激活状态左侧高亮条
                if is_active {
                    let accent = color_f(0.0, 0.47, 0.83, 1.0);
                    let accent_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &accent)
                        .unwrap();
                    let accent_rect = D2D_RECT_F {
                        left: nav_x,
                        top: item_y,
                        right: nav_x + 3.0,
                        bottom: item_y + nav_item_h,
                    };
                    target.FillRectangle(&accent_rect, &accent_brush);
                }

                let item_text_color = if is_active {
                    color_f(1.0, 1.0, 1.0, 1.0)
                } else {
                    color_f(0.75, 0.75, 0.75, 1.0)
                };
                let item_text_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &item_text_color)
                    .unwrap();
                let item_text: Vec<u16> = tab.label().encode_utf16().chain(Some(0)).collect();
                let item_text_rect = D2D_RECT_F {
                    left: nav_x + 20.0,
                    top: item_y,
                    right: nav_x + nav_w - 8.0,
                    bottom: item_y + nav_item_h,
                };
                target.DrawText(
                    &item_text,
                    &nav_format,
                    &item_text_rect,
                    &item_text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                self.settings_panel
                    .add_tab_region(*tab, nav_x, item_y, nav_w, nav_item_h);
            }

            // 右侧内容区域
            let content_x = nav_x + nav_w + 1.0;
            let content_y = nav_y;
            let content_w = width - nav_w - 1.0;
            let content_h = height;

            // 标题栏
            let page_title = match self.settings_panel.active_tab {
                crate::settings::SettingsTab::Account => "账号",
                crate::settings::SettingsTab::General => "通用",
                crate::settings::SettingsTab::Models => "模型",
                crate::settings::SettingsTab::Ai => "AI",
                crate::settings::SettingsTab::Appearance => "外观",
                crate::settings::SettingsTab::Remote => "远程",
            };
            let page_title_wide: Vec<u16> = page_title.encode_utf16().chain(Some(0)).collect();
            let page_title_rect = D2D_RECT_F {
                left: content_x + 24.0,
                top: content_y + 24.0,
                right: content_x + content_w - 24.0,
                bottom: content_y + 56.0,
            };
            target.DrawText(
                &page_title_wide,
                &title_format,
                &page_title_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 标题下方分隔线
            let title_sep_rect = D2D_RECT_F {
                left: content_x + 24.0,
                top: content_y + 64.0,
                right: content_x + content_w - 24.0,
                bottom: content_y + 65.0,
            };
            target.FillRectangle(&title_sep_rect, &sep_brush);

            // 渲染当前激活页面的内容
            let page_x = content_x + 24.0;
            let page_y = content_y + 80.0;
            let page_w = content_w - 48.0;

            match self.settings_panel.active_tab {
                crate::settings::SettingsTab::Account => {
                    self.render_account_page(
                        target,
                        page_x,
                        page_w,
                        page_y,
                        content_h - 80.0,
                        title_format,
                        label_format,
                        text_brush,
                    );
                }
                crate::settings::SettingsTab::General => {
                    self.render_general_settings(
                        target,
                        page_x,
                        page_w,
                        page_y,
                        0.0,
                        label_format,
                        text_brush,
                    );
                }
                crate::settings::SettingsTab::Models => {
                    let label_format_clone = label_format.clone();
                    let input_format_clone = input_format.clone();
                    let button_format_clone = button_format.clone();
                    let title_format_clone = title_format.clone();
                    self.render_models_management(
                        target,
                        page_x,
                        page_w,
                        page_y,
                        0.0,
                        label_format_clone,
                        input_format_clone,
                        button_format_clone,
                        title_format_clone,
                        text_brush,
                    );
                    if self.settings_panel.add_model_dialog.visible {
                        self.render_add_model_dialog(
                            target,
                            page_x,
                            page_w,
                            page_y,
                            label_format,
                            input_format,
                            button_format,
                            title_format,
                            text_brush,
                        );
                    }
                }
                crate::settings::SettingsTab::Ai => {
                    let input_w = page_w.min(460.0);
                    self.render_ai_settings_fields(
                        target,
                        page_x,
                        page_w,
                        page_y,
                        0.0,
                        input_w,
                        20.0,
                        32.0,
                        12.0,
                        label_format,
                        input_format,
                        button_format,
                        text_brush,
                    );
                }
                _ => {}
            }
        }
    }

    /// 渲染 AI 接口设置字段（provider / key / url / model / 保存 / 测试连接）
    #[allow(clippy::too_many_arguments)]
    fn render_ai_settings_fields(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        width: f32,
        start_y: f32,
        margin: f32,
        input_w: f32,
        label_h: f32,
        input_h: f32,
        gap: f32,
        label_format: IDWriteTextFormat,
        input_format: IDWriteTextFormat,
        button_format: IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        let mut cy = start_y;
        unsafe {
            // AI 能力说明
            let info_text = "配置 API 密钥后，AI 助手可通过 Agent 模式新建、修改、删除文件。请先点击测试连接按钮验证密钥有效性。";
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
                bottom: cy + 36.0,
            };
            target.DrawText(
                &info_wide,
                &label_format,
                &info_rect,
                &info_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 36.0 + gap;

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

            // API Key
            let apikey_label: Vec<u16> = "API 密钥".encode_utf16().chain(Some(0)).collect();
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

            // 判断是否为自定义模式（预制模式自动填充 base_url 和 model）
            let is_custom = self.settings_panel.provider == "custom";

            // Base URL（仅自定义模式显示，预制模式自动填充）
            if is_custom {
                let baseurl_label: Vec<u16> =
                    "基础地址".encode_utf16().chain(Some(0)).collect();
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
                .map(|(id, name)| name)
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

            // Temperature
            let temp_label: Vec<u16> = "温度 (0.0-2.0)".encode_utf16().chain(Some(0)).collect();
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
            cy += label_h;
            let temp_bg = color_f(0.18, 0.18, 0.18, 1.0);
            let temp_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &temp_bg)
                .unwrap();
            let temp_border = if self.settings_panel.active_field
                == Some(crate::settings::SettingsField::Temperature)
            {
                color_f(0.0, 0.47, 0.83, 1.0)
            } else {
                color_f(0.3, 0.3, 0.3, 1.0)
            };
            let temp_border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &temp_border)
                .unwrap();
            let temp_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + input_w,
                bottom: cy + input_h,
            };
            target.FillRectangle(&temp_rect, &temp_bg_brush);
            draw_input_borders(target, x + margin, cy, input_w, input_h, &temp_border_brush);
            let temp_text: Vec<u16> = self
                .settings_panel
                .temperature
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let temp_text_rect = D2D_RECT_F {
                left: x + margin + 6.0,
                top: cy,
                right: x + margin + input_w - 6.0,
                bottom: cy + input_h,
            };
            target.DrawText(
                &temp_text,
                &input_format,
                &temp_text_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_field_region(
                crate::settings::SettingsField::Temperature,
                x + margin,
                cy,
                input_w,
                input_h,
            );
            cy += input_h + gap;

            // Max Tokens
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
            let maxtok_bg = color_f(0.18, 0.18, 0.18, 1.0);
            let maxtok_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &maxtok_bg)
                .unwrap();
            let maxtok_border = if self.settings_panel.active_field
                == Some(crate::settings::SettingsField::MaxTokens)
            {
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
            let maxtok_text: Vec<u16> = self
                .settings_panel
                .max_tokens
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let maxtok_text_rect = D2D_RECT_F {
                left: x + margin + 6.0,
                top: cy,
                right: x + margin + input_w - 6.0,
                bottom: cy + input_h,
            };
            target.DrawText(
                &maxtok_text,
                &input_format,
                &maxtok_text_rect,
                text_brush,
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
            cy += input_h + gap;

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
                } else if self.settings_panel.test_status.starts_with('✓') {
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

    /// 渲染模型管理区（标题、说明、添加模型按钮、已有模型列表）
    #[allow(clippy::too_many_arguments)]
    fn render_models_management(
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
            // 标题：模型管理
            let title_text = "模型管理";
            let title_wide: Vec<u16> = title_text.encode_utf16().chain(Some(0)).collect();
            let title_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
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
            cy += 32.0;

            // 说明
            let info_text = "配置 API 密钥添加更多可用模型，预置模型默认使用稳定版本。";
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

            // + 添加模型 按钮
            let add_btn_w = 110.0f32;
            let add_btn_h = 28.0f32;
            let is_hover =
                self.settings_panel.hover_model_button == Some(crate::settings::ModelButton::Add);
            let add_bg = if is_hover {
                color_f(0.25, 0.25, 0.25, 1.0)
            } else {
                color_f(0.18, 0.18, 0.18, 1.0)
            };
            let add_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &add_bg)
                .unwrap();
            let add_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + margin + add_btn_w,
                bottom: cy + add_btn_h,
            };
            target.FillRectangle(&add_rect, &add_bg_brush);
            let add_text_color = color_f(0.85, 0.85, 0.85, 1.0);
            let add_text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &add_text_color)
                .unwrap();
            let add_text: Vec<u16> = "+ 添加模型".encode_utf16().chain(Some(0)).collect();
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
                x + margin,
                cy,
                add_btn_w,
                add_btn_h,
            );
            cy += add_btn_h + 16.0;

            // 模型列表表头
            let row_h = 28.0f32;
            let col_model_w = width * 0.45;
            let col_provider_w = width * 0.30;
            let col_op_w = width * 0.20;
            let header_bg = color_f(0.20, 0.20, 0.22, 1.0);
            let header_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &header_bg)
                .unwrap();
            let header_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + row_h,
            };
            target.FillRectangle(&header_rect, &header_bg_brush);
            let headers = [
                ("模型", col_model_w),
                ("服务商", col_provider_w),
                ("操作", col_op_w),
            ];
            let mut hx = x + margin + 12.0;
            for (label, _) in &headers {
                let label_wide: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &label_wide,
                    &label_format,
                    &D2D_RECT_F {
                        left: hx,
                        top: cy,
                        right: hx + 200.0,
                        bottom: cy + row_h,
                    },
                    text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                hx += width * 0.25;
            }
            cy += row_h;

            // 模型列表项
            let item_text_color = color_f(0.85, 0.85, 0.85, 1.0);
            let item_text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &item_text_color)
                .unwrap();
            let op_color = color_f(0.9, 0.3, 0.3, 1.0);
            let op_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &op_color)
                .unwrap();
            let models_clone: Vec<_> = self.settings_panel.models.iter().cloned().collect();
            for (i, model) in models_clone.iter().enumerate() {
                let is_selected = self.settings_panel.selected_model_id.as_ref() == Some(&model.id);
                let is_hover = self.settings_panel.hover_model_id.as_ref() == Some(&model.id);
                let item_bg = if is_selected {
                    color_f(0.15, 0.35, 0.55, 1.0)
                } else if is_hover {
                    color_f(0.22, 0.22, 0.24, 1.0)
                } else {
                    color_f(0.16, 0.16, 0.18, 1.0)
                };
                let item_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &item_bg)
                    .unwrap();
                let item_rect = D2D_RECT_F {
                    left: x + margin,
                    top: cy,
                    right: x + width - margin,
                    bottom: cy + row_h,
                };
                target.FillRectangle(&item_rect, &item_bg_brush);

                // 模型名
                let name_text: Vec<u16> = model.name.encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &name_text,
                    &input_format,
                    &D2D_RECT_F {
                        left: x + margin + 12.0,
                        top: cy,
                        right: x + margin + col_model_w,
                        bottom: cy + row_h,
                    },
                    text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // 服务商
                let provider_text: Vec<u16> =
                    model.provider.encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &provider_text,
                    &input_format,
                    &D2D_RECT_F {
                        left: x + margin + col_model_w + 12.0,
                        top: cy,
                        right: x + margin + col_model_w + col_provider_w,
                        bottom: cy + row_h,
                    },
                    &item_text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                // 操作：激活/删除
                let op_label = if self.settings_panel.active_model_id.as_ref() == Some(&model.id) {
                    "已激活"
                } else {
                    "激活"
                };
                let op_text: Vec<u16> = op_label.encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &op_text,
                    &label_format,
                    &D2D_RECT_F {
                        left: x + margin + col_model_w + col_provider_w + 12.0,
                        top: cy,
                        right: x + margin + col_model_w + col_provider_w + 50.0,
                        bottom: cy + row_h,
                    },
                    if self.settings_panel.active_model_id.as_ref() == Some(&model.id) {
                        &item_text_brush
                    } else {
                        &op_brush
                    },
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                self.settings_panel.add_model_button_region(
                    crate::settings::ModelButton::Activate,
                    x + margin + col_model_w + col_provider_w + 12.0,
                    cy,
                    40.0,
                    row_h,
                );

                let del_text: Vec<u16> = "删除".encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &del_text,
                    &label_format,
                    &D2D_RECT_F {
                        left: x + margin + col_model_w + col_provider_w + 70.0,
                        top: cy,
                        right: x + margin + col_model_w + col_provider_w + 110.0,
                        bottom: cy + row_h,
                    },
                    &op_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                self.settings_panel.add_model_button_region(
                    crate::settings::ModelButton::Delete,
                    x + margin + col_model_w + col_provider_w + 70.0,
                    cy,
                    40.0,
                    row_h,
                );

                self.settings_panel.add_model_item_region(
                    model.id.clone(),
                    x + margin,
                    cy,
                    width - margin * 2.0,
                    row_h,
                );
                cy += row_h;
                if i >= 7 {
                    // 最多显示 8 行，避免撑满页面
                    break;
                }
            }
        }
    }

    /// 渲染添加模型弹窗
    #[allow(clippy::too_many_arguments)]
    fn render_add_model_dialog(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        page_x: f32,
        page_w: f32,
        page_y: f32,
        label_format: IDWriteTextFormat,
        input_format: IDWriteTextFormat,
        button_format: IDWriteTextFormat,
        title_format: IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        unsafe {
            // 半透明遮罩
            let overlay_color = color_f(0.0, 0.0, 0.0, 0.5);
            let overlay_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &overlay_color)
                .unwrap();
            let overlay_rect = D2D_RECT_F {
                left: page_x,
                top: page_y,
                right: page_x + page_w,
                bottom: page_y + 600.0,
            };
            target.FillRectangle(&overlay_rect, &overlay_brush);

            // 弹窗尺寸
            let dialog_w = 460.0f32;
            let dialog_h = 400.0f32;
            let dx = page_x + (page_w - dialog_w) / 2.0;
            let dy = page_y + 60.0;
            let margin = 20.0f32;
            let gap = 10.0f32;
            let label_h = 16.0f32;
            let input_h = 26.0f32;
            let input_w = dialog_w - margin * 2.0;

            // 弹窗背景
            let bg_color = color_f(0.24, 0.24, 0.26, 1.0);
            let bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bg_color)
                .unwrap();
            let dialog_rect = D2D_RECT_F {
                left: dx,
                top: dy,
                right: dx + dialog_w,
                bottom: dy + dialog_h,
            };
            target.FillRectangle(&dialog_rect, &bg_brush);

            // 弹窗边框
            let border_color = color_f(0.35, 0.35, 0.37, 1.0);
            let border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &border_color)
                .unwrap();
            let border_top = D2D_RECT_F {
                left: dx,
                top: dy,
                right: dx + dialog_w,
                bottom: dy + 1.0,
            };
            let border_bottom = D2D_RECT_F {
                left: dx,
                top: dy + dialog_h - 1.0,
                right: dx + dialog_w,
                bottom: dy + dialog_h,
            };
            let border_left = D2D_RECT_F {
                left: dx,
                top: dy,
                right: dx + 1.0,
                bottom: dy + dialog_h,
            };
            let border_right = D2D_RECT_F {
                left: dx + dialog_w - 1.0,
                top: dy,
                right: dx + dialog_w,
                bottom: dy + dialog_h,
            };
            target.FillRectangle(&border_top, &border_brush);
            target.FillRectangle(&border_bottom, &border_brush);
            target.FillRectangle(&border_left, &border_brush);
            target.FillRectangle(&border_right, &border_brush);

            let mut cy = dy + margin;

            // 标题
            let title_text = "添加模型";
            let title_wide: Vec<u16> = title_text.encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &title_wide,
                &title_format,
                &D2D_RECT_F {
                    left: dx + margin,
                    top: cy,
                    right: dx + dialog_w - margin,
                    bottom: cy + 24.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 关闭按钮
            let close_size = 24.0f32;
            let close_x = dx + dialog_w - margin - close_size;
            let close_y = cy;
            let close_color = if self.settings_panel.add_model_dialog.hover_button
                == Some(crate::settings::AddModelDialogButton::Close)
            {
                color_f(0.9, 0.3, 0.3, 1.0)
            } else {
                color_f(0.6, 0.6, 0.6, 1.0)
            };
            let close_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &close_color)
                .unwrap();
            let close_text: Vec<u16> = "×".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &close_text,
                &title_format,
                &D2D_RECT_F {
                    left: close_x,
                    top: close_y,
                    right: close_x + close_size,
                    bottom: close_y + close_size,
                },
                &close_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_model_dialog.close_region =
                Some((close_x, close_y, close_size, close_size));
            cy += 32.0;

            // 标签页：模型服务商 / 自定义配置
            let tabs = crate::settings::AddModelDialogTab::ALL;
            let tab_w = (input_w - gap) / 2.0;
            let tab_h = 28.0f32;
            for (i, tab) in tabs.iter().enumerate() {
                let tx = dx + margin + i as f32 * (tab_w + gap);
                let ty = cy;
                let is_active = self.settings_panel.add_model_dialog.active_tab == *tab;
                let is_hover = self.settings_panel.add_model_dialog.hover_tab == Some(*tab);
                let tab_bg = if is_active {
                    color_f(0.20, 0.20, 0.22, 1.0)
                } else if is_hover {
                    color_f(0.28, 0.28, 0.30, 1.0)
                } else {
                    color_f(0.32, 0.32, 0.34, 1.0)
                };
                let tab_bg_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &tab_bg)
                    .unwrap();
                let tab_rect = D2D_RECT_F {
                    left: tx,
                    top: ty,
                    right: tx + tab_w,
                    bottom: ty + tab_h,
                };
                target.FillRectangle(&tab_rect, &tab_bg_brush);
                let tab_text: Vec<u16> = tab.label().encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &tab_text,
                    &button_format,
                    &tab_rect,
                    text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                self.settings_panel
                    .add_model_dialog
                    .tab_regions
                    .push((*tab, tx, ty, tab_w, tab_h));
            }
            cy += tab_h + 12.0;

            self.settings_panel.add_model_dialog.field_regions.clear();
            self.settings_panel
                .add_model_dialog
                .provider_template_regions
                .clear();
            self.settings_panel.add_model_dialog.button_regions.clear();

            // 标签页内容
            self.settings_panel.add_model_dialog.field_regions.clear();
            self.settings_panel
                .add_model_dialog
                .provider_template_regions
                .clear();
            self.settings_panel.add_model_dialog.button_regions.clear();
            self.settings_panel
                .add_model_dialog
                .dropdown_trigger_regions
                .clear();
            self.settings_panel
                .add_model_dialog
                .dropdown_item_regions
                .clear();
            self.settings_panel.add_model_dialog.advanced_toggle_region = None;

            match self.settings_panel.add_model_dialog.active_tab {
                crate::settings::AddModelDialogTab::Provider => {
                    // 服务商下拉
                    let provider_label = self.settings_panel.add_model_dialog.provider_label();
                    cy = self.render_add_model_dialog_dropdown(
                        target,
                        dx,
                        cy,
                        margin,
                        input_w,
                        label_h,
                        input_h,
                        gap,
                        "服务商",
                        &provider_label,
                        true,
                        crate::settings::AddModelDropdownKind::Provider,
                        &label_format,
                        &input_format,
                        text_brush,
                    );

                    // 模型下拉
                    let model_label = if self
                        .settings_panel
                        .add_model_dialog
                        .selected_model_id
                        .is_empty()
                    {
                        "选择模型".to_string()
                    } else {
                        self.settings_panel
                            .add_model_dialog
                            .selected_model_id
                            .clone()
                    };
                    cy = self.render_add_model_dialog_dropdown(
                        target,
                        dx,
                        cy,
                        margin,
                        input_w,
                        label_h,
                        input_h,
                        gap,
                        "模型",
                        &model_label,
                        false,
                        crate::settings::AddModelDropdownKind::Model,
                        &label_format,
                        &input_format,
                        text_brush,
                    );

                    // 服务商/模型规格提示（依据官方 API 文档）
                    cy = self.render_provider_model_hint(
                        target,
                        dx,
                        cy,
                        margin,
                        input_w,
                        &label_format,
                        text_brush,
                    );

                    // API 密钥
                    let api_key_value = self.settings_panel.add_model_dialog.masked_api_key();
                    cy = self.render_add_model_dialog_field(
                        target,
                        dx,
                        cy,
                        margin,
                        input_w,
                        label_h,
                        input_h,
                        gap,
                        "API 密钥",
                        &api_key_value,
                        crate::settings::SettingsField::ApiKey,
                        &label_format,
                        &input_format,
                        text_brush,
                    );

                    // 高级配置（可折叠）
                    cy = self.render_add_model_dialog_advanced(
                        target,
                        dx,
                        cy,
                        margin,
                        input_w,
                        label_h,
                        input_h,
                        gap,
                        &label_format,
                        &input_format,
                        text_brush,
                    );
                }
                crate::settings::AddModelDialogTab::Custom => {
                    // 服务提供商
                    let provider_value = self.settings_panel.add_model_dialog.provider.clone();
                    cy = self.render_add_model_dialog_field(
                        target,
                        dx,
                        cy,
                        margin,
                        input_w,
                        label_h,
                        input_h,
                        gap,
                        "服务提供商",
                        &provider_value,
                        crate::settings::SettingsField::Provider,
                        &label_format,
                        &input_format,
                        text_brush,
                    );

                    // 基础地址
                    let base_url_value = self.settings_panel.add_model_dialog.base_url.clone();
                    cy = self.render_add_model_dialog_field(
                        target,
                        dx,
                        cy,
                        margin,
                        input_w,
                        label_h,
                        input_h,
                        gap,
                        "基础地址",
                        &base_url_value,
                        crate::settings::SettingsField::BaseUrl,
                        &label_format,
                        &input_format,
                        text_brush,
                    );

                    // 模型
                    let model_value = self.settings_panel.add_model_dialog.model.clone();
                    cy = self.render_add_model_dialog_field(
                        target,
                        dx,
                        cy,
                        margin,
                        input_w,
                        label_h,
                        input_h,
                        gap,
                        "模型",
                        &model_value,
                        crate::settings::SettingsField::Model,
                        &label_format,
                        &input_format,
                        text_brush,
                    );

                    // API 密钥
                    let api_key_value = self.settings_panel.add_model_dialog.masked_api_key();
                    cy = self.render_add_model_dialog_field(
                        target,
                        dx,
                        cy,
                        margin,
                        input_w,
                        label_h,
                        input_h,
                        gap,
                        "API 密钥",
                        &api_key_value,
                        crate::settings::SettingsField::ApiKey,
                        &label_format,
                        &input_format,
                        text_brush,
                    );
                }
            }

            // 添加模型按钮
            cy += 8.0;
            let add_btn_h = 32.0f32;
            let add_btn_bg = color_f(0.85, 0.85, 0.90, 1.0);
            let add_btn_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &add_btn_bg)
                .unwrap();
            let add_btn_rect = D2D_RECT_F {
                left: dx + margin,
                top: cy,
                right: dx + dialog_w - margin,
                bottom: cy + add_btn_h,
            };
            target.FillRectangle(&add_btn_rect, &add_btn_bg_brush);
            let add_btn_text_color = color_f(0.12, 0.12, 0.12, 1.0);
            let add_btn_text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &add_btn_text_color)
                .unwrap();
            let add_btn_text: Vec<u16> = "添加模型".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &add_btn_text,
                &button_format,
                &add_btn_rect,
                &add_btn_text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_model_dialog.button_regions.push((
                crate::settings::AddModelDialogButton::AddModel,
                dx + margin,
                cy,
                dialog_w - margin * 2.0,
                add_btn_h,
            ));
        }
    }

    /// 辅助：渲染添加模型弹窗中的单个字段
    #[allow(clippy::too_many_arguments)]
    fn render_add_model_dialog_field(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        dx: f32,
        cy: f32,
        margin: f32,
        input_w: f32,
        label_h: f32,
        input_h: f32,
        _gap: f32,
        label: &str,
        value: &str,
        field: crate::settings::SettingsField,
        label_format: &IDWriteTextFormat,
        input_format: &IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) -> f32 {
        unsafe {
            let is_active = self.settings_panel.add_model_dialog.active_field == Some(field);
            let label_wide: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &label_wide,
                label_format,
                &D2D_RECT_F {
                    left: dx + margin,
                    top: cy,
                    right: dx + margin + input_w,
                    bottom: cy + label_h,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let mut cy = cy + label_h + 4.0;
            let input_bg = color_f(0.18, 0.18, 0.18, 1.0);
            let input_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &input_bg)
                .unwrap();
            let input_border = if is_active {
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
                left: dx + margin,
                top: cy,
                right: dx + margin + input_w,
                bottom: cy + input_h,
            };
            target.FillRectangle(&input_rect, &input_bg_brush);
            let border_top = D2D_RECT_F {
                left: dx + margin,
                top: cy,
                right: dx + margin + input_w,
                bottom: cy + 1.0,
            };
            let border_bottom = D2D_RECT_F {
                left: dx + margin,
                top: cy + input_h - 1.0,
                right: dx + margin + input_w,
                bottom: cy + input_h,
            };
            let border_left = D2D_RECT_F {
                left: dx + margin,
                top: cy,
                right: dx + margin + 1.0,
                bottom: cy + input_h,
            };
            let border_right = D2D_RECT_F {
                left: dx + margin + input_w - 1.0,
                top: cy,
                right: dx + margin + input_w,
                bottom: cy + input_h,
            };
            target.FillRectangle(&border_top, &input_border_brush);
            target.FillRectangle(&border_bottom, &input_border_brush);
            target.FillRectangle(&border_left, &input_border_brush);
            target.FillRectangle(&border_right, &input_border_brush);
            let value_wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &value_wide,
                input_format,
                &D2D_RECT_F {
                    left: dx + margin + 6.0,
                    top: cy,
                    right: dx + margin + input_w - 6.0,
                    bottom: cy + input_h,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_model_dialog.field_regions.push((
                field,
                dx + margin,
                cy,
                input_w,
                input_h,
            ));
            cy + input_h + 10.0
        }
    }

    /// 渲染添加模型弹窗中的下拉字段（服务商 / 模型）
    #[allow(clippy::too_many_arguments)]
    fn render_add_model_dialog_dropdown(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        dx: f32,
        cy: f32,
        margin: f32,
        input_w: f32,
        label_h: f32,
        input_h: f32,
        gap: f32,
        label: &str,
        value: &str,
        required: bool,
        kind: crate::settings::AddModelDropdownKind,
        label_format: &IDWriteTextFormat,
        input_format: &IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) -> f32 {
        unsafe {
            let is_open = self.settings_panel.add_model_dialog.open_dropdown == Some(kind);
            // 标签（含红色 *）
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
            // 渲染 * 和 label
            let label_y = cy;
            if required {
                target.DrawText(
                    &prefix,
                    label_format,
                    &D2D_RECT_F {
                        left: dx + margin,
                        top: label_y,
                        right: dx + margin + 10.0,
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
                    left: dx + margin + (if required { 12.0 } else { 0.0 }),
                    top: label_y,
                    right: dx + margin + input_w,
                    bottom: label_y + label_h,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let mut cy = cy + label_h + 4.0;

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
                left: dx + margin,
                top: cy,
                right: dx + margin + input_w,
                bottom: cy + input_h,
            };
            target.FillRectangle(&input_rect, &input_bg_brush);
            // 边框
            for (b_left, b_top, b_right, b_bottom) in [
                (dx + margin, cy, dx + margin + input_w, cy + 1.0),
                (
                    dx + margin,
                    cy + input_h - 1.0,
                    dx + margin + input_w,
                    cy + input_h,
                ),
                (dx + margin, cy, dx + margin + 1.0, cy + input_h),
                (
                    dx + margin + input_w - 1.0,
                    cy,
                    dx + margin + input_w,
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
            let value_color = if value.starts_with("选择") {
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
                    left: dx + margin + 10.0,
                    top: cy,
                    right: dx + margin + input_w - 28.0,
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
                    left: dx + margin + input_w - 24.0,
                    top: cy,
                    right: dx + margin + input_w - 6.0,
                    bottom: cy + input_h,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 保存触发区域
            self.settings_panel
                .add_model_dialog
                .dropdown_trigger_regions
                .push((kind, dx + margin, cy, input_w, input_h));

            let mut next_cy = cy + input_h + gap;

            // 如果展开，渲染下拉项
            if is_open {
                let items: Vec<String> = match kind {
                    crate::settings::AddModelDropdownKind::Provider => self
                        .settings_panel
                        .dropdown_items(crate::settings::AddModelDropdownKind::Provider),
                    crate::settings::AddModelDropdownKind::Model => {
                        self.settings_panel.add_model_dialog.model_options()
                    }
                };
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
                    let is_hover = self.settings_panel.add_model_dialog.hover_dropdown
                        == Some(kind)
                        && self.settings_panel.add_model_dialog.hover_dropdown_index == Some(i);
                    let is_selected = match kind {
                        crate::settings::AddModelDropdownKind::Provider => {
                            let selected = self
                                .settings_panel
                                .add_model_dialog
                                .selected_provider_button;
                            match (selected, i) {
                                (Some(ProviderTemplateButton::DeepSeek), 0) => true,
                                (Some(ProviderTemplateButton::Kimi), 1) => true,
                                (Some(ProviderTemplateButton::Custom), 2) => true,
                                _ => false,
                            }
                        }
                        crate::settings::AddModelDropdownKind::Model => {
                            self.settings_panel.add_model_dialog.selected_model_id == *item_label
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
                        left: dx + margin,
                        top: iy,
                        right: dx + margin + input_w,
                        bottom: iy + item_h,
                    };
                    target.FillRectangle(&item_rect, brush);
                    let item_wide: Vec<u16> = item_label.encode_utf16().chain(Some(0)).collect();
                    target.DrawText(
                        &item_wide,
                        input_format,
                        &D2D_RECT_F {
                            left: dx + margin + 14.0,
                            top: iy,
                            right: dx + margin + input_w - 14.0,
                            bottom: iy + item_h,
                        },
                        text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    self.settings_panel
                        .add_model_dialog
                        .dropdown_item_regions
                        .push((kind, i, dx + margin, iy, input_w, item_h));
                }
                next_cy += items.len() as f32 * item_h + gap;
            }
            next_cy
        }
    }

    /// 渲染设置面板主编辑区的下拉字段（厂商 / 模型）
    /// 与 `render_add_model_dialog_dropdown` 不同：使用 `SettingsDropdownKind`，
    /// 下拉项列表由调用方传入（settings_panel 上有多个下拉，items 集合各异）。
    #[allow(clippy::too_many_arguments)]
    fn render_settings_dropdown(
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
            let mut cy = cy + label_h + 4.0;

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
                            match (self.settings_panel.current_provider_button(), i) {
                                (Some(ProviderTemplateButton::DeepSeek), 0) => true,
                                (Some(ProviderTemplateButton::Kimi), 1) => true,
                                (Some(ProviderTemplateButton::Custom), 2) => true,
                                _ => false,
                            }
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

    /// 渲染添加模型弹窗中的高级配置可折叠区
    #[allow(clippy::too_many_arguments)]
    fn render_add_model_dialog_advanced(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        dx: f32,
        cy: f32,
        margin: f32,
        input_w: f32,
        label_h: f32,
        input_h: f32,
        gap: f32,
        label_format: &IDWriteTextFormat,
        input_format: &IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) -> f32 {
        unsafe {
            let mut cy = cy;
            // 折叠/展开切换行
            let toggle_h = 22.0f32;
            let arrow = if self.settings_panel.add_model_dialog.advanced_expanded {
                "▾ 高级配置"
            } else {
                "▸ 高级配置"
            };
            let arrow_wide: Vec<u16> = arrow.encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &arrow_wide,
                label_format,
                &D2D_RECT_F {
                    left: dx + margin,
                    top: cy,
                    right: dx + margin + input_w,
                    bottom: cy + toggle_h,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_model_dialog.advanced_toggle_region =
                Some((dx + margin, cy, input_w, toggle_h));
            cy += toggle_h + 4.0;

            if !self.settings_panel.add_model_dialog.advanced_expanded {
                return cy;
            }

            // 简介行
            let intro: Vec<u16> =
                "包含模型系列（优化的 Prompt 和超参）、展示名称、上下文窗口等配置。"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
            let intro_color = color_f(0.55, 0.55, 0.55, 1.0);
            let intro_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &intro_color)
                .unwrap();
            target.DrawText(
                &intro,
                label_format,
                &D2D_RECT_F {
                    left: dx + margin,
                    top: cy,
                    right: dx + margin + input_w,
                    bottom: cy + 28.0,
                },
                &intro_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 32.0;

            // 模型展示名称 + 字数计数
            let label: Vec<u16> = "模型展示名称".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &label,
                label_format,
                &D2D_RECT_F {
                    left: dx + margin,
                    top: cy,
                    right: dx + margin + input_w,
                    bottom: cy + label_h,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += label_h + 4.0;
            // 描述
            let desc: Vec<u16> = "在模型列表中展示的名称，未设置时默认显示 Model ID。"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            target.DrawText(
                &desc,
                label_format,
                &D2D_RECT_F {
                    left: dx + margin,
                    top: cy,
                    right: dx + margin + input_w,
                    bottom: cy + label_h,
                },
                &intro_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += label_h + 4.0;
            // 输入框 + 计数
            let dn_value = self.settings_panel.add_model_dialog.display_name.clone();
            let dn_count = format!(
                "{}/32",
                self.settings_panel
                    .add_model_dialog
                    .display_name
                    .chars()
                    .count()
            );
            self.render_input_with_suffix(
                target,
                dx,
                cy,
                margin,
                input_w,
                input_h,
                crate::settings::SettingsField::DisplayName,
                &dn_value,
                &dn_count,
                label_format,
                input_format,
                text_brush,
            );
            cy += input_h + 10.0;

            // 上下文窗口
            let ctx_label: Vec<u16> = "上下文窗口".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &ctx_label,
                label_format,
                &D2D_RECT_F {
                    left: dx + margin,
                    top: cy,
                    right: dx + margin + input_w,
                    bottom: cy + label_h,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += label_h + 4.0;
            let half_w = (input_w - gap) / 2.0;
            // 输入
            let in_label: Vec<u16> = "输入".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &in_label,
                label_format,
                &D2D_RECT_F {
                    left: dx + margin,
                    top: cy,
                    right: dx + margin + 28.0,
                    bottom: cy + input_h,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let ci_value = self.settings_panel.add_model_dialog.context_input.clone();
            self.render_input_only(
                target,
                dx + margin + 32.0,
                cy,
                half_w - 32.0,
                input_h,
                crate::settings::SettingsField::ContextInput,
                &ci_value,
                label_format,
                input_format,
                text_brush,
            );
            // 输出
            let out_label: Vec<u16> = "输出".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &out_label,
                label_format,
                &D2D_RECT_F {
                    left: dx + margin + half_w + gap,
                    top: cy,
                    right: dx + margin + half_w + gap + 28.0,
                    bottom: cy + input_h,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let co_value = self.settings_panel.add_model_dialog.context_output.clone();
            self.render_input_only(
                target,
                dx + margin + half_w + gap + 32.0,
                cy,
                half_w - 32.0,
                input_h,
                crate::settings::SettingsField::ContextOutput,
                &co_value,
                label_format,
                input_format,
                text_brush,
            );
            cy += input_h + 10.0;

            // 工具调用轮次
            let tc_label: Vec<u16> = "工具调用轮次".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &tc_label,
                label_format,
                &D2D_RECT_F {
                    left: dx + margin,
                    top: cy,
                    right: dx + margin + input_w,
                    bottom: cy + label_h,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += label_h + 4.0;
            let tr_value = self
                .settings_panel
                .add_model_dialog
                .tool_call_rounds
                .clone();
            self.render_input_only(
                target,
                dx + margin,
                cy,
                input_w,
                input_h,
                crate::settings::SettingsField::ToolCallRounds,
                &tr_value,
                label_format,
                input_format,
                text_brush,
            );
            cy += input_h + 10.0;
            cy
        }
    }

    /// 辅助：渲染服务商/模型规格提示行
    /// 内容来源于各服务商官方 API 文档（https://api-docs.deepseek.com/zh-cn/ 等）
    #[allow(clippy::too_many_arguments)]
    fn render_provider_model_hint(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        dx: f32,
        cy: f32,
        margin: f32,
        input_w: f32,
        label_format: &IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) -> f32 {
        unsafe {
            // 根据当前选中的服务商生成提示文本
            let provider = self
                .settings_panel
                .add_model_dialog
                .selected_provider_button;
            let hint: String = match provider {
                Some(crate::settings::ProviderTemplateButton::DeepSeek) => {
                    // 来源：https://api-docs.deepseek.com/zh-cn/quick_start/pricing
                    "deepseek-v4-flash/pro：上下文 1M · 输出最大 384K · 兼容 OpenAI 与 Anthropic"
                        .to_string()
                }
                Some(crate::settings::ProviderTemplateButton::Kimi) => {
                    "kimi-code：上下文支持，Moonshot API 兼容".to_string()
                }
                Some(crate::settings::ProviderTemplateButton::Custom) => {
                    "自定义 API 端点，需填写完整 base_url".to_string()
                }
                None => "请选择服务商".to_string(),
            };
            // 链接：用户可点击跳转到 DeepSeek 官方文档
            let link = match provider {
                Some(crate::settings::ProviderTemplateButton::DeepSeek) => {
                    Some("https://api-docs.deepseek.com/zh-cn/")
                }
                _ => None,
            };
            let hint_color = color_f(0.60, 0.60, 0.60, 1.0);
            let hint_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hint_color)
                .unwrap();
            let hint_wide: Vec<u16> = hint.encode_utf16().chain(Some(0)).collect();
            let hint_rect = D2D_RECT_F {
                left: dx + margin,
                top: cy,
                right: dx + margin + input_w,
                bottom: cy + 16.0,
            };
            target.DrawText(
                &hint_wide,
                label_format,
                &hint_rect,
                &hint_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let mut next_cy = cy + 20.0;
            // 链接行（仅 DeepSeek 显示官方文档链接）
            if let Some(url) = link {
                let link_color = color_f(0.30, 0.60, 0.95, 1.0);
                let link_brush = self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &link_color)
                    .unwrap();
                let prefix: Vec<u16> = "📖 官方文档：".encode_utf16().chain(Some(0)).collect();
                let link_wide: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
                target.DrawText(
                    &prefix,
                    label_format,
                    &D2D_RECT_F {
                        left: dx + margin,
                        top: next_cy,
                        right: dx + margin + 80.0,
                        bottom: next_cy + 16.0,
                    },
                    &link_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                target.DrawText(
                    &link_wide,
                    label_format,
                    &D2D_RECT_F {
                        left: dx + margin + 80.0,
                        top: next_cy,
                        right: dx + margin + input_w,
                        bottom: next_cy + 16.0,
                    },
                    &link_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                next_cy += 20.0;
            }
            next_cy + 4.0
        }
    }

    /// 辅助：渲染带后缀计数的输入框
    #[allow(clippy::too_many_arguments)]
    fn render_input_with_suffix(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        dx: f32,
        cy: f32,
        margin: f32,
        input_w: f32,
        input_h: f32,
        field: crate::settings::SettingsField,
        value: &str,
        suffix: &str,
        label_format: &IDWriteTextFormat,
        input_format: &IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        unsafe {
            let is_active = self.settings_panel.add_model_dialog.active_field == Some(field);
            let input_bg = color_f(0.18, 0.18, 0.18, 1.0);
            let input_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &input_bg)
                .unwrap();
            let input_border = if is_active {
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
                left: dx + margin,
                top: cy,
                right: dx + margin + input_w,
                bottom: cy + input_h,
            };
            target.FillRectangle(&input_rect, &input_bg_brush);
            for (b_left, b_top, b_right, b_bottom) in [
                (dx + margin, cy, dx + margin + input_w, cy + 1.0),
                (
                    dx + margin,
                    cy + input_h - 1.0,
                    dx + margin + input_w,
                    cy + input_h,
                ),
                (dx + margin, cy, dx + margin + 1.0, cy + input_h),
                (
                    dx + margin + input_w - 1.0,
                    cy,
                    dx + margin + input_w,
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
            let placeholder_color = color_f(0.55, 0.55, 0.55, 1.0);
            let placeholder_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &placeholder_color)
                .unwrap();
            let value_to_show = if value.is_empty() {
                "请输入模型展示名称"
            } else {
                value
            };
            let value_wide: Vec<u16> = value_to_show.encode_utf16().chain(Some(0)).collect();
            let value_brush = if value.is_empty() {
                &placeholder_brush
            } else {
                text_brush
            };
            target.DrawText(
                &value_wide,
                input_format,
                &D2D_RECT_F {
                    left: dx + margin + 8.0,
                    top: cy,
                    right: dx + margin + input_w - 40.0,
                    bottom: cy + input_h,
                },
                value_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            // 右侧计数
            let suffix_wide: Vec<u16> = suffix.encode_utf16().chain(Some(0)).collect();
            let right_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &placeholder_color)
                .unwrap();
            target.DrawText(
                &suffix_wide,
                input_format,
                &D2D_RECT_F {
                    left: dx + margin + input_w - 38.0,
                    top: cy,
                    right: dx + margin + input_w - 6.0,
                    bottom: cy + input_h,
                },
                &right_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel.add_model_dialog.field_regions.push((
                field,
                dx + margin,
                cy,
                input_w,
                input_h,
            ));
        }
    }

    /// 辅助：渲染纯输入框（无前缀无计数）
    #[allow(clippy::too_many_arguments)]
    fn render_input_only(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        field: crate::settings::SettingsField,
        value: &str,
        _label_format: &IDWriteTextFormat,
        input_format: &IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        unsafe {
            let is_active = self.settings_panel.add_model_dialog.active_field == Some(field);
            let input_bg = color_f(0.18, 0.18, 0.18, 1.0);
            let input_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &input_bg)
                .unwrap();
            let input_border = if is_active {
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
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };
            target.FillRectangle(&input_rect, &input_bg_brush);
            for (b_left, b_top, b_right, b_bottom) in [
                (x, y, x + w, y + 1.0),
                (x, y + h - 1.0, x + w, y + h),
                (x, y, x + 1.0, y + h),
                (x + w - 1.0, y, x + w, y + h),
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
            let value_wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &value_wide,
                input_format,
                &D2D_RECT_F {
                    left: x + 6.0,
                    top: y,
                    right: x + w - 6.0,
                    bottom: y + h,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.settings_panel
                .add_model_dialog
                .field_regions
                .push((field, x, y, w, h));
        }
    }

    /// 渲染"通用"标签页内容（主题 / 字体大小 / 自动保存等只读概览）
    #[allow(clippy::too_many_arguments)]
    fn render_general_settings(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        width: f32,
        start_y: f32,
        margin: f32,
        label_format: IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        unsafe {
            let mut cy = start_y;

            // 主题
            let theme_label = if self.app_settings.ui.theme.is_empty() {
                "默认深色".to_string()
            } else {
                self.app_settings.ui.theme.clone()
            };
            let theme_text: Vec<u16> = format!("主题：{}", theme_label)
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let theme_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 20.0,
            };
            target.DrawText(
                &theme_text,
                &label_format,
                &theme_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 30.0;

            // 字体大小
            let font_size = if self.app_settings.ui.font_size == 0 {
                14
            } else {
                self.app_settings.ui.font_size
            };
            let font_text: Vec<u16> = format!("编辑器字体大小：{} px", font_size)
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let font_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 20.0,
            };
            target.DrawText(
                &font_text,
                &label_format,
                &font_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 30.0;

            // 自动保存
            let auto_save = &self.app_settings.auto_save;
            let auto_save_text: Vec<u16> = format!(
                "自动保存：{}（防抖 {} ms）",
                if auto_save.enabled {
                    "已启用"
                } else {
                    "已禁用"
                },
                auto_save.debounce_ms
            )
            .encode_utf16()
            .chain(Some(0))
            .collect();
            let auto_save_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 20.0,
            };
            target.DrawText(
                &auto_save_text,
                &label_format,
                &auto_save_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 30.0;

            // 失焦保存
            let focus_loss = if auto_save.focus_loss_save {
                "是"
            } else {
                "否"
            };
            let focus_text: Vec<u16> = format!("失焦自动保存：{}", focus_loss)
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let focus_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 20.0,
            };
            target.DrawText(
                &focus_text,
                &label_format,
                &focus_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 40.0;

            // 分隔线
            let sep_color = color_f(0.2, 0.2, 0.2, 1.0);
            let sep_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sep_color)
                .unwrap();
            let sep_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 1.0,
            };
            target.FillRectangle(&sep_rect, &sep_brush);
            cy += 16.0;

            // 提示
            let hint_text: Vec<u16> = "更多通用选项（主题切换、字体调整等）将在后续版本提供"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let hint_color = color_f(0.55, 0.55, 0.55, 1.0);
            let hint_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hint_color)
                .unwrap();
            let hint_rect = D2D_RECT_F {
                left: x + margin,
                top: cy,
                right: x + width - margin,
                bottom: cy + 18.0,
            };
            target.DrawText(
                &hint_text,
                &label_format,
                &hint_rect,
                &hint_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// 渲染"账号"标签页内容（账户信息 / 速通套餐 / 速通用量 / 隐私模式）
    #[allow(clippy::too_many_arguments)]
    fn render_account_page(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        width: f32,
        start_y: f32,
        _height: f32,
        _title_format: IDWriteTextFormat,
        label_format: IDWriteTextFormat,
        text_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        unsafe {
            let margin = 24.0_f32;
            let card_x = x + margin;
            let card_w = width - margin * 2.0;
            let mut cy = start_y + 8.0;

            // ============ 账户信息 ============
            let account_label: Vec<u16> = "账户信息".encode_utf16().chain(Some(0)).collect();
            let section_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    14.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            target.DrawText(
                &account_label,
                &section_format,
                &D2D_RECT_F {
                    left: card_x,
                    top: cy,
                    right: card_x + card_w,
                    bottom: cy + 22.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 30.0;

            // 账户信息卡片
            let card_bg = color_f(0.16, 0.16, 0.18, 1.0);
            let card_bg_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &card_bg)
                .unwrap();
            let card_radius = 6.0_f32;
            let card_h = 168.0_f32;
            let card_rect = D2D_RECT_F {
                left: card_x,
                top: cy,
                right: card_x + card_w,
                bottom: cy + card_h,
            };
            target.FillRectangle(&card_rect, &card_bg_brush);

            // 头像占位（左侧）
            let avatar_size = 40.0_f32;
            let avatar_x = card_x + 20.0;
            let avatar_y = cy + 20.0;
            let avatar_bg = color_f(0.30, 0.30, 0.32, 1.0);
            let avatar_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &avatar_bg)
                .unwrap();
            let avatar_rect = D2D_RECT_F {
                left: avatar_x,
                top: avatar_y,
                right: avatar_x + avatar_size,
                bottom: avatar_y + avatar_size,
            };
            target.FillRectangle(&avatar_rect, &avatar_brush);
            let initial: Vec<u16> = "U".encode_utf16().chain(Some(0)).collect();
            let initial_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    18.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();
            target.DrawText(
                &initial,
                &initial_format,
                &avatar_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 姓名
            let name_x = avatar_x + avatar_size + 12.0;
            let name_y = avatar_y + 2.0;
            let name_text: Vec<u16> = "未登录".encode_utf16().chain(Some(0)).collect();
            let name_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    14.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            target.DrawText(
                &name_text,
                &name_format,
                &D2D_RECT_F {
                    left: name_x,
                    top: name_y,
                    right: card_x + card_w - 200.0,
                    bottom: name_y + 20.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            // 邮箱/手机占位
            let phone_text: Vec<u16> = "未关联手机号".encode_utf16().chain(Some(0)).collect();
            let phone_color = color_f(0.55, 0.55, 0.55, 1.0);
            let phone_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &phone_color)
                .unwrap();
            target.DrawText(
                &phone_text,
                &label_format,
                &D2D_RECT_F {
                    left: name_x,
                    top: name_y + 20.0,
                    right: card_x + card_w - 200.0,
                    bottom: name_y + 40.0,
                },
                &phone_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 右侧按钮：管理账号 / ...
            let btn_h = 28.0_f32;
            let btn_w = 90.0_f32;
            let btn_gap = 8.0_f32;
            let btn_y = avatar_y + (avatar_size - btn_h) / 2.0;
            let manage_btn_x = card_x + card_w - 16.0 - btn_w * 2.0 - btn_gap;
            let manage_btn_rect = D2D_RECT_F {
                left: manage_btn_x,
                top: btn_y,
                right: manage_btn_x + btn_w,
                bottom: btn_y + btn_h,
            };
            let manage_btn_bg = color_f(0.25, 0.25, 0.27, 1.0);
            let manage_btn_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &manage_btn_bg)
                .unwrap();
            target.FillRectangle(&manage_btn_rect, &manage_btn_brush);
            let manage_text: Vec<u16> = "管理账号".encode_utf16().chain(Some(0)).collect();
            let btn_text_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_CENTER.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();
            target.DrawText(
                &manage_text,
                &btn_text_format,
                &manage_btn_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            let more_btn_x = manage_btn_x + btn_w + btn_gap;
            let more_btn_rect = D2D_RECT_F {
                left: more_btn_x,
                top: btn_y,
                right: more_btn_x + btn_w,
                bottom: btn_y + btn_h,
            };
            target.FillRectangle(&more_btn_rect, &manage_btn_brush);
            let more_text: Vec<u16> = "···".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &more_text,
                &btn_text_format,
                &more_btn_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 卡片内分隔线
            let sep_y = cy + 76.0;
            let sep_color = color_f(0.22, 0.22, 0.24, 1.0);
            let sep_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sep_color)
                .unwrap();
            let sep_rect = D2D_RECT_F {
                left: card_x + 16.0,
                top: sep_y,
                right: card_x + card_w - 16.0,
                bottom: sep_y + 1.0,
            };
            target.FillRectangle(&sep_rect, &sep_brush);

            // 速通 Pro 行
            let pro_y = sep_y + 10.0;
            let bolt_color = color_f(0.20, 0.80, 0.50, 1.0);
            let bolt_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &bolt_color)
                .unwrap();
            let bolt_text: Vec<u16> = "⚡ 速通 Pro".encode_utf16().chain(Some(0)).collect();
            let pro_label_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    13.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            target.DrawText(
                &bolt_text,
                &pro_label_format,
                &D2D_RECT_F {
                    left: card_x + 20.0,
                    top: pro_y,
                    right: card_x + 200.0,
                    bottom: pro_y + 20.0,
                },
                &bolt_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let pro_sub: Vec<u16> = "尚未开通 · 享更快 AI 回复"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            target.DrawText(
                &pro_sub,
                &label_format,
                &D2D_RECT_F {
                    left: card_x + 20.0,
                    top: pro_y + 22.0,
                    right: card_x + card_w - 200.0,
                    bottom: pro_y + 42.0,
                },
                &phone_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            let sub_btn_w = 90.0_f32;
            let sub_btn_x = card_x + card_w - 16.0 - sub_btn_w;
            let sub_btn_rect = D2D_RECT_F {
                left: sub_btn_x,
                top: pro_y - 4.0,
                right: sub_btn_x + sub_btn_w,
                bottom: pro_y + 24.0,
            };
            let sub_btn_bg = color_f(0.25, 0.25, 0.27, 1.0);
            let sub_btn_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &sub_btn_bg)
                .unwrap();
            target.FillRectangle(&sub_btn_rect, &sub_btn_brush);
            let sub_text: Vec<u16> = "立即订阅".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &sub_text,
                &btn_text_format,
                &sub_btn_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            cy += card_h + 24.0;

            // ============ 速通用量 ============
            let usage_label: Vec<u16> = "速通用量".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &usage_label,
                &section_format,
                &D2D_RECT_F {
                    left: card_x,
                    top: cy,
                    right: card_x + card_w,
                    bottom: cy + 22.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 30.0;

            let usage_card_h = 96.0_f32;
            let usage_card_rect = D2D_RECT_F {
                left: card_x,
                top: cy,
                right: card_x + card_w,
                bottom: cy + usage_card_h,
            };
            target.FillRectangle(&usage_card_rect, &card_bg_brush);

            // 速通可用次数
            let usage_text: Vec<u16> = "⚡ 速通可用 0 次".encode_utf16().chain(Some(0)).collect();
            let usage_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    14.0,
                    DWRITE_FONT_WEIGHT_BOLD.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_LEADING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR.0 as u32,
                )
                .unwrap();
            target.DrawText(
                &usage_text,
                &usage_format,
                &D2D_RECT_F {
                    left: card_x + 20.0,
                    top: cy + 16.0,
                    right: card_x + card_w - 80.0,
                    bottom: cy + 36.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 右侧刷新按钮
            let refresh_size = 28.0_f32;
            let refresh_x = card_x + card_w - 16.0 - refresh_size;
            let refresh_y = cy + 12.0;
            let refresh_rect = D2D_RECT_F {
                left: refresh_x,
                top: refresh_y,
                right: refresh_x + refresh_size,
                bottom: refresh_y + refresh_size,
            };
            let refresh_bg = color_f(0.25, 0.25, 0.27, 1.0);
            let refresh_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &refresh_bg)
                .unwrap();
            target.FillRectangle(&refresh_rect, &refresh_brush);
            let refresh_text: Vec<u16> = "↻".encode_utf16().chain(Some(0)).collect();
            let refresh_format = self
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
                &refresh_text,
                &refresh_format,
                &refresh_rect,
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 卡片内分隔线
            let usage_sep_y = cy + 50.0;
            let usage_sep_rect = D2D_RECT_F {
                left: card_x + 16.0,
                top: usage_sep_y,
                right: card_x + card_w - 16.0,
                bottom: usage_sep_y + 1.0,
            };
            target.FillRectangle(&usage_sep_rect, &sep_brush);

            // 速通次数行
            let count_text: Vec<u16> = "› 速通次数".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &count_text,
                &usage_format,
                &D2D_RECT_F {
                    left: card_x + 20.0,
                    top: usage_sep_y + 8.0,
                    right: card_x + 200.0,
                    bottom: usage_sep_y + 38.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let count_value: Vec<u16> = "0 次".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &count_value,
                &usage_format,
                &D2D_RECT_F {
                    left: card_x + card_w - 120.0,
                    top: usage_sep_y + 8.0,
                    right: card_x + card_w - 20.0,
                    bottom: usage_sep_y + 38.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            cy += usage_card_h + 24.0;

            // ============ 隐私模式 ============
            let privacy_label: Vec<u16> = "隐私模式".encode_utf16().chain(Some(0)).collect();
            target.DrawText(
                &privacy_label,
                &section_format,
                &D2D_RECT_F {
                    left: card_x,
                    top: cy,
                    right: card_x + card_w,
                    bottom: cy + 22.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            cy += 30.0;

            let privacy_card_h = 56.0_f32;
            let privacy_card_rect = D2D_RECT_F {
                left: card_x,
                top: cy,
                right: card_x + card_w,
                bottom: cy + privacy_card_h,
            };
            target.FillRectangle(&privacy_card_rect, &card_bg_brush);
            let privacy_text: Vec<u16> = "开启后 AI 助手不会保留对话历史"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            target.DrawText(
                &privacy_text,
                &label_format,
                &D2D_RECT_F {
                    left: card_x + 20.0,
                    top: cy + 18.0,
                    right: card_x + card_w - 80.0,
                    bottom: cy + 38.0,
                },
                text_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
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
        let s = self.dpi_scale;
        let mut display_buf = String::with_capacity(64);
        let node_height = 16.0f32 * s;
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

                if *current_y + node_height < clip_y {
                    *current_y += node_height;
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
                    node.depth as f32 * 16.0 * s
                };
                let name = tree.get_name(node);

                // 优先使用矢量图标（.py/.java/.txt），未命中时回退到 emoji
                let vector_icon = if node.kind == FileKind::File {
                    self.get_file_vector_icon(name)
                } else {
                    None
                };

                let icon = if node.kind == FileKind::Directory {
                    if node.is_expanded {
                        "📂"
                    } else {
                        "📁"
                    }
                } else if vector_icon.is_some() {
                    // 矢量图标位置由下方单独绘制，文本中不再占位
                    ""
                } else {
                    self.get_file_icon(name)
                };

                let arrow = if node.kind == FileKind::Directory {
                    if node.is_expanded {
                        "v "
                    } else {
                        "> "
                    }
                } else {
                    ""
                };

                display_buf.clear();
                display_buf.push_str(arrow);
                if vector_icon.is_none() {
                    display_buf.push_str(icon);
                    display_buf.push(' ');
                }
                display_buf.push_str(name);

                let item_left = base_x + indent;
                let item_right = base_x + sidebar_width - 10.0 * s;

                // 绘制悬停背景
                let is_hover = self.hover_file_node == Some(idx);
                if is_hover {
                    let hover_rect = D2D_RECT_F {
                        left: item_left - 4.0 * s,
                        top: *current_y,
                        right: item_right,
                        bottom: *current_y + node_height,
                    };
                    unsafe {
                        target.FillRectangle(&hover_rect, hover_brush);
                    }
                }

                // 绘制选中高亮背景（文件 + 目录都支持选中显示）
                let is_selected = self.selected_file_node == Some(idx);
                if is_selected {
                    let sel_rect = D2D_RECT_F {
                        left: item_left - 4.0 * s,
                        top: *current_y,
                        right: item_right,
                        bottom: *current_y + node_height,
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

                let text_left = if vector_icon.is_some() {
                    // 矢量图标占 14px 宽 + 2px 间距，文字右移避免被图标遮挡
                    item_left + 16.0 * s
                } else {
                    item_left
                };

                unsafe {
                    // 单行 + 字符级"…"省略号：直接 IDWriteTextLayout 处理超长文件名
                    //（旧版用 DrawText 会在 text_rect 宽度不够时按字符换行，出现
                    // "project.private.config.js" 重叠堆叠成一坨的 bug）。
                    // 每次重绘重新创建 layout：节点数少、且 layout 轻量，
                    // 副作用是侧边栏拖动时省略号即时刷新（无缓存滞后）。
                    let max_text_w = (item_right - text_left).max(1.0);
                    let layout = self
                        .render_ctx
                        .text_layout_cache
                        .create_ellipsis_layout(&display_buf, format, max_text_w, node_height)
                        .unwrap();
                    let point = D2D_POINT_2F {
                        x: text_left,
                        y: *current_y,
                    };
                    target.DrawTextLayout(point, &layout, brush, D2D1_DRAW_TEXT_OPTIONS_CLIP);
                }

                // 矢量文件图标：在文本前绘制 14x14 矢量图标（命中 .py/.java/.txt）
                if let Some(kind) = vector_icon {
                    let icon_size = 14.0_f32 * s;
                    let icon_left = item_left;
                    let icon_top = *current_y + (node_height - icon_size) / 2.0;
                    self.icons.draw(
                        target, kind, icon_left, icon_top, icon_size, icon_size, text_brush,
                    );
                }

                *current_y += node_height;

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
        let s = self.dpi_scale;
        let node_height = 16.0f32 * s;
        let mut child_idx = tree
            .get_node(parent_idx)
            .map(|n| n.first_child)
            .filter(|&c| c != u32::MAX);
        while let Some(idx) = child_idx {
            if let Some(node) = tree.get_node(idx) {
                *current_y += node_height;
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

    /// 为常用文件类型返回矢量图标（避免 emoji 字体差异）。
    /// 命中 .py/.java/.txt 等常见扩展时返回对应 IconKind，渲染时将替代 emoji 占位。
    fn get_file_vector_icon(&self, name: &str) -> Option<crate::icons::IconKind> {
        use crate::icons::IconKind;
        // Dockerfile 无扩展名特殊处理
        if name.eq_ignore_ascii_case("Dockerfile") || name.eq_ignore_ascii_case("dockerfile") {
            return Some(IconKind::FileDocker);
        }
        let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
        match ext.as_str() {
            "py" | "pyw" | "pyi" => Some(IconKind::FilePython),
            "java" => Some(IconKind::FileJava),
            "kt" | "kts" => Some(IconKind::FileKotlin),
            "txt" => Some(IconKind::FileText),
            "c" | "h" => Some(IconKind::FileC),
            "cpp" | "cc" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++" => Some(IconKind::FileCpp),
            "cs" => Some(IconKind::FileCSharp),
            "go" => Some(IconKind::FileGo),
            "rs" => Some(IconKind::FileRust),
            "js" | "mjs" | "cjs" | "jsx" => Some(IconKind::FileJs),
            "ts" | "tsx" => Some(IconKind::FileTs),
            "html" | "htm" | "shtml" => Some(IconKind::FileHtml),
            "css" | "scss" | "sass" | "less" => Some(IconKind::FileCss),
            "json" | "jsonc" | "json5" => Some(IconKind::FileJson),
            "yml" | "yaml" => Some(IconKind::FileYaml),
            "toml" => Some(IconKind::FileToml),
            "md" | "markdown" => Some(IconKind::FileMarkdown),
            "sh" | "bash" | "zsh" | "ksh" => Some(IconKind::FileShell),
            "sql" => Some(IconKind::FileSql),
            "rb" | "ruby" | "erb" => Some(IconKind::FileRuby),
            "php" | "php5" | "phtml" => Some(IconKind::FilePhp),
            "lua" => Some(IconKind::FileLua),
            "swift" => Some(IconKind::FileSwift),
            "dart" => Some(IconKind::FileSwift), // Dart 与 Swift 风格相似，暂用 Swift 图标
            _ => None,
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
        let line_number_width = 40.0;

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

            let (start_line, end_line) = self.visible_line_range();

            for line_idx in start_line..end_line {
                let line_y = y + (line_idx - start_line) as f32 * line_height
                    - (self.content.scroll_y % line_height);
                if line_y > y + height {
                    break;
                }
                if line_y + line_height < y {
                    continue;
                }

                // 优先使用缓存的行文本，避免重复调用 buffer.get_line()
                let cached_line = if line_idx < self.content.cached_lines.len() {
                    Some(self.content.cached_lines[line_idx].as_str())
                } else {
                    None
                };

                // Selection highlight — Glass 模式下使用柔和光晕
                if let (Some((sel_start_line, sel_start_col)), Some((sel_end_line, sel_end_col))) =
                    (self.content.selection_start, self.content.selection_end)
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
                            let safe_col = text.floor_char_boundary(col.min(text.len()));
                            text[..safe_col]
                                .chars()
                                .map(unicode_char_width)
                                .sum::<usize>()
                        } else {
                            0
                        };
                        let sel_end_char = if let Some(text) = cached_line {
                            let col = if line_idx == last_line {
                                last_col
                            } else {
                                text.len()
                            };
                            let safe_col = text.floor_char_boundary(col.min(text.len()));
                            text[..safe_col]
                                .chars()
                                .map(unicode_char_width)
                                .sum::<usize>()
                        } else {
                            0
                        };
                        // P0-3: 选区高亮 x 减去水平滚动偏移
                        let sel_start_x = x + line_number_width + 5.0 - self.content.scroll_x
                            + sel_start_char as f32 * char_width;
                        let sel_end_x = x + line_number_width + 5.0 - self.content.scroll_x
                            + sel_end_char as f32 * char_width;
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
                if line_idx == self.content.cursor_line {
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
                // 优化：合并相邻同色 token 段，减少 DrawText 调用次数
                if let Some(line_text) = cached_line {
                    let tokens = &self.content.cached_tokens[line_idx];
                    // P0-3: 应用水平滚动偏移；用 PushAxisAlignedClip 裁剪文本区域，
                    // 防止横向滚动后文本溢出到行号区域
                    let text_x = x + line_number_width + 5.0 - self.content.scroll_x;
                    let text_clip = D2D_RECT_F {
                        left: x + line_number_width,
                        top: line_y,
                        right: x + width,
                        bottom: line_y + line_height,
                    };
                    target.PushAxisAlignedClip(&text_clip, D2D1_ANTIALIAS_MODE_ALIASED);

                    let mut current_byte = 0usize;
                    let mut current_char = 0usize;
                    let mut token_idx = 0;

                    // 当前合并段的起始位置和颜色
                    let mut seg_start_byte = 0usize;
                    let mut seg_start_char = 0usize;
                    let mut seg_color = self.theme.text_default;
                    let mut seg_active = false;

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

                        if !seg_active {
                            // 开始新段
                            seg_start_byte = current_byte;
                            seg_start_char = current_char;
                            seg_color = token_color;
                            seg_active = true;
                        } else if seg_color != token_color {
                            // 颜色变化：flush 前一段，开始新段
                            let segment = &line_text[seg_start_byte..current_byte];
                            if !segment.is_empty() {
                                let brush = self
                                    .render_ctx
                                    .brush_cache
                                    .get_brush(target, &seg_color)
                                    .unwrap();
                                let layout = self
                                    .render_ctx
                                    .text_layout_cache
                                    .get_or_create(segment, &code_format, line_height, font_size)
                                    .unwrap();
                                let point = D2D_POINT_2F {
                                    x: text_x + seg_start_char as f32 * char_width,
                                    y: line_y,
                                };
                                target.DrawTextLayout(
                                    point,
                                    &layout,
                                    &brush,
                                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                                );
                            }
                            seg_start_byte = current_byte;
                            seg_start_char = current_char;
                            seg_color = token_color;
                        }
                        // else: 颜色相同，继续累积当前段（无需 DrawText）

                        current_char += line_text[current_byte..current_byte + token_len]
                            .chars()
                            .map(unicode_char_width)
                            .sum::<usize>();
                        current_byte += token_len;
                    }

                    // flush 最后一段
                    if seg_active {
                        let segment = &line_text[seg_start_byte..current_byte];
                        if !segment.is_empty() {
                            let brush = self
                                .render_ctx
                                .brush_cache
                                .get_brush(target, &seg_color)
                                .unwrap();
                            let layout = self
                                .render_ctx
                                .text_layout_cache
                                .get_or_create(segment, &code_format, line_height, font_size)
                                .unwrap();
                            let point = D2D_POINT_2F {
                                x: text_x + seg_start_char as f32 * char_width,
                                y: line_y,
                            };
                            target.DrawTextLayout(
                                point,
                                &layout,
                                &brush,
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                            );
                        }
                    }
                    // P0-3: 配对 PopAxisAlignedClip，恢复渲染范围
                    target.PopAxisAlignedClip();
                }

                // ===== LSP 诊断波浪线 =====
                // 根据当前文件路径查找诊断，line_idx 0-based vs DiagnosticItem.line 1-based
                if let Some(path) = &self.content.file_path {
                    let path_str = path.to_string_lossy().to_string();
                    if let Some(diags) = self.diagnostics.get(&path_str) {
                        for diag in diags.iter() {
                            // 当前行（1-based -> 0-based 比较）
                            if diag.line.saturating_sub(1) != line_idx {
                                continue;
                            }
                            // 颜色：错误红色、警告黄色、信息蓝色、提示灰色
                            let wave_color = match diag.severity {
                                1 => color_f(0.9, 0.25, 0.25, 1.0),
                                2 => color_f(0.9, 0.75, 0.2, 1.0),
                                3 => color_f(0.35, 0.6, 0.95, 1.0),
                                _ => color_f(0.55, 0.55, 0.55, 1.0),
                            };
                            let wave_brush = self
                                .render_ctx
                                .brush_cache
                                .get_brush(target, &wave_color)
                                .unwrap();
                            // 起始/结束字符列（1-based -> 0-based）
                            let start_char = diag.col.saturating_sub(1);
                            let end_char = if diag.end_line == diag.line && diag.end_col > diag.col
                            {
                                diag.end_col.saturating_sub(1)
                            } else {
                                // 跨行或无 end_col：取到行尾
                                cached_line
                                    .map(|t| t.chars().count())
                                    .unwrap_or(start_char + 1)
                            };
                            // 至少给 1 个字符宽度，避免空诊断不可见
                            let end_char = end_char.max(start_char + 1);
                            let wave_left = x + line_number_width + 5.0 - self.content.scroll_x
                                + start_char as f32 * char_width;
                            let wave_right = x + line_number_width + 5.0 - self.content.scroll_x
                                + end_char as f32 * char_width;
                            // 波浪线位于行底部，3px 高度区域
                            let wave_top = line_y + line_height - 3.0;
                            // 限制在可见区域
                            if wave_right <= x + line_number_width || wave_left >= x + width {
                                continue;
                            }
                            let clip_left = wave_left.max(x + line_number_width);
                            let clip_right = wave_right.min(x + width);
                            // 绘制简单波浪线（用小矩形拼接成锯齿状）
                            let seg_count =
                                ((clip_right - clip_left) / (char_width * 0.5)).ceil() as i32;
                            if seg_count <= 0 {
                                continue;
                            }
                            let seg_w = (clip_right - clip_left) / seg_count as f32;
                            for i in 0..seg_count {
                                let sx = clip_left + i as f32 * seg_w;
                                // 上下交替形成波浪
                                let offset = if i % 2 == 0 { 0.0 } else { 2.0 };
                                let seg_rect = D2D_RECT_F {
                                    left: sx,
                                    top: wave_top + offset,
                                    right: sx + seg_w + 0.5,
                                    bottom: wave_top + offset + 1.0,
                                };
                                target.FillRectangle(&seg_rect, &wave_brush);
                            }
                        }
                    }
                }
            }

            // P3.2: 在光标之前渲染内联补全幽灵文本
            self.render_inline_completion(
                target,
                x,
                y,
                width,
                height,
                start_line,
                line_height,
                char_width,
                line_number_width,
                &code_format,
            );

            // 光标：将字节列转换为字符列计算x坐标
            // UI-H04: 使用字符宽度累加而非简单 char count * char_width，
            // 支持 CJK 等双宽度字符的正确光标定位
            let cursor_char_col = if let Some(text) =
                self.content.cached_lines.get(self.content.cursor_line)
            {
                let byte_pos = text.floor_char_boundary(self.content.cursor_col.min(text.len()));
                text[..byte_pos]
                    .chars()
                    .map(unicode_char_width)
                    .sum::<usize>()
            } else {
                0
            };
            // P0-3: 光标 x 减去水平滚动偏移
            let cursor_x = x + line_number_width + 5.0 - self.content.scroll_x
                + cursor_char_col as f32 * char_width;
            let cursor_y = y
                + (self.content.cursor_line.saturating_sub(start_line)) as f32 * line_height
                - (self.content.scroll_y % line_height);
            // UI-L02: 更新 IME 候选窗口位置到光标处
            // 文件树输入框激活时，IME 候选窗口定位到输入框附近而非编辑器光标
            // 终端聚焦时，定位到终端光标，否则用户看不到合成窗口会以为删除无效
            if self.terminal_panel.focused {
                let term_region = self.layout.bottom_panel_region();
                let (t_row, t_col) = self.terminal_panel.cursor_position();
                // 光标位置使用 DirectWrite HitTestTextPosition 获取精确前缀坐标（逻辑像素，最后再乘 DPI）
                let cell_w_logical = self
                    .render_ctx
                    .text_format_cache
                    .measure_text_width("M", 11.0, DWRITE_FONT_WEIGHT_NORMAL.0 as u32)
                    .unwrap_or(7.0);
                let prefix_x_logical =
                    if let Some(line) = self.terminal_panel.output_lines.get(t_row) {
                        let char_count = line.chars().count();
                        let take = t_col.min(char_count);
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
                            .unwrap_or(t_col as f32 * cell_w_logical);
                        let extra = (t_col.saturating_sub(char_count)) as f32 * cell_w_logical;
                        prefix_x + extra
                    } else {
                        t_col as f32 * cell_w_logical
                    };
                let line_h_logical = 14.0;
                let term_x_logical = term_region.x + 8.0 + prefix_x_logical;
                let term_y_logical = term_region.y + 24.0 + t_row as f32 * line_h_logical;
                self.ime.set_composition_window_position(
                    (term_x_logical * self.dpi_scale) as i32,
                    (term_y_logical * self.dpi_scale) as i32,
                );
                self.ime.set_candidate_window_position(
                    (term_x_logical * self.dpi_scale) as i32,
                    ((term_y_logical + line_h_logical) * self.dpi_scale) as i32,
                );
            } else if self.file_tree_input.is_some() {
                let sidebar = self.layout.sidebar_region();
                let ft_input_y = sidebar.y + 28.0 + 6.0; // header_h + margin
                let ft_value_x = sidebar.x + 10.0 + 6.0; // input_rect.left + padding
                                                         // 估算 value 宽度（近似，IME 候选窗口只需大致位置）
                let value_chars = self
                    .file_tree_input
                    .as_ref()
                    .map(|i| i.value.chars().count())
                    .unwrap_or(0);
                let ft_cursor_x = ft_value_x + value_chars as f32 * 7.0;
                self.ime.set_candidate_window_position(
                    (ft_cursor_x * self.dpi_scale) as i32,
                    ((ft_input_y + 26.0) * self.dpi_scale) as i32, // input_h
                );
            } else {
                self.ime.set_composition_window_position(
                    (cursor_x * self.dpi_scale) as i32,
                    (cursor_y * self.dpi_scale) as i32,
                );
                self.ime.set_candidate_window_position(
                    (cursor_x * self.dpi_scale) as i32,
                    ((cursor_y + line_height) * self.dpi_scale) as i32,
                );
            }
            if cursor_y >= y && cursor_y <= y + height {
                // P0-2: 若存在 IME 合成串，渲染合成串文本 + 下划线，光标隐藏
                if let Some(comp) = self.composition.as_ref() {
                    if !comp.is_empty() {
                        // 合成串宽度（按字符宽度累加，CJK 字符 2 倍宽）
                        let comp_char_width: usize = comp.chars().map(unicode_char_width).sum();
                        let comp_pixel_width = comp_char_width as f32 * char_width;

                        // 渲染合成串文本（与代码格式一致）
                        let comp_utf16: Vec<u16> = comp.encode_utf16().collect();
                        let comp_rect = D2D_RECT_F {
                            left: cursor_x,
                            top: cursor_y,
                            right: cursor_x + comp_pixel_width + 4.0,
                            bottom: cursor_y + line_height,
                        };
                        target.DrawText(
                            &comp_utf16,
                            &code_format,
                            &comp_rect,
                            &cursor_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        // 渲染下划线（光标颜色，距底部 2px，1px 高）
                        let underline_y = cursor_y + line_height - 2.0;
                        let underline_rect = D2D_RECT_F {
                            left: cursor_x,
                            top: underline_y,
                            right: cursor_x + comp_pixel_width,
                            bottom: underline_y + 1.0,
                        };
                        target.FillRectangle(&underline_rect, &cursor_brush);
                    } else {
                        // 合成串为空时显示普通光标
                        let cursor_rect = D2D_RECT_F {
                            left: cursor_x,
                            top: cursor_y,
                            right: cursor_x + 2.0,
                            bottom: cursor_y + line_height,
                        };
                        target.FillRectangle(&cursor_rect, &cursor_brush);
                    }
                } else {
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
    }

    /// P3.2: 渲染内联补全幽灵文本
    #[allow(clippy::too_many_arguments)]
    fn render_inline_completion(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        _width: f32,
        _height: f32,
        start_line: usize,
        line_height: f32,
        char_width: f32,
        line_number_width: f32,
        code_format: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
    ) {
        let Some(comp) = self.content.inline_completion.as_ref() else {
            return;
        };

        // 仅当建议触发位置与当前光标位置匹配时渲染，避免错位
        if comp.trigger_line != self.content.cursor_line
            || comp.trigger_col != self.content.cursor_col
        {
            return;
        }

        unsafe {
            let ghost_color = color_f(0.5, 0.5, 0.5, 0.6);
            let Ok(ghost_brush) = self.render_ctx.brush_cache.get_brush(target, &ghost_color)
            else {
                return;
            };

            let cursor_char_col = if let Some(text) =
                self.content.cached_lines.get(self.content.cursor_line)
            {
                let byte_pos = text.floor_char_boundary(self.content.cursor_col.min(text.len()));
                text[..byte_pos]
                    .chars()
                    .map(unicode_char_width)
                    .sum::<usize>()
            } else {
                0
            };

            let ghost_x = x + line_number_width + 5.0 - self.content.scroll_x
                + cursor_char_col as f32 * char_width;
            let ghost_y = y
                + (self.content.cursor_line.saturating_sub(start_line)) as f32 * line_height
                - (self.content.scroll_y % line_height);

            let text_utf16: Vec<u16> = comp.text.encode_utf16().collect();
            let text_rect = windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
                left: ghost_x,
                top: ghost_y,
                right: ghost_x + comp.text.len() as f32 * char_width + 10.0,
                bottom: ghost_y + line_height,
            };
            target.DrawText(
                &text_utf16,
                code_format,
                &text_rect,
                &ghost_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// P3.4: 渲染 hover tooltip（鼠标悬停提示框）
    ///
    /// 在鼠标附近绘制一个深色背景的提示框，显示文件树节点的完整路径。
    /// 后续可扩展为 LSP hover 信息显示。
    fn render_hover_tooltip(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
    ) {
        let Some(tooltip) = self.hover_tooltip.as_ref() else {
            return;
        };
        if tooltip.is_empty() {
            return;
        }

        unsafe {
            // 估算文本尺寸：每行高度 16px，字符宽度约 7px
            let char_width = 7.0_f32;
            let line_height = 16.0_f32;
            let padding = 8.0_f32;
            let lines: Vec<&str> = tooltip.text.split('\n').collect();
            let max_line_chars = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
            // 限制最大宽度
            let max_w = tooltip.max_width.min(400.0);
            let text_w = (max_line_chars as f32 * char_width).min(max_w);
            let text_h = lines.len() as f32 * line_height;
            let box_w = text_w + padding * 2.0;
            let box_h = text_h + padding * 2.0;

            // 钳制到窗口范围内，避免 tooltip 超出右/下边界
            let win_w = self.window_width as f32;
            let win_h = self.window_height as f32;
            let tx = if tooltip.x + box_w > win_w {
                (win_w - box_w).max(0.0)
            } else {
                tooltip.x
            };
            let ty = if tooltip.y + box_h > win_h {
                (win_h - box_h).max(0.0)
            } else {
                tooltip.y
            };

            // 背景：半透明深色
            let bg_color = color_f(0.12, 0.12, 0.15, 0.95);
            let Ok(bg_brush) = self.render_ctx.brush_cache.get_brush(target, &bg_color) else {
                return;
            };
            // 边框：浅色
            let border_color = color_f(0.4, 0.4, 0.45, 1.0);
            let Ok(border_brush) = self.render_ctx.brush_cache.get_brush(target, &border_color)
            else {
                return;
            };
            // 文本：浅色
            let text_color = color_f(0.9, 0.9, 0.9, 1.0);
            let Ok(text_brush) = self.render_ctx.brush_cache.get_brush(target, &text_color) else {
                return;
            };

            let box_rect = windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
                left: tx,
                top: ty,
                right: tx + box_w,
                bottom: ty + box_h,
            };
            target.FillRectangle(&box_rect, &bg_brush);
            target.DrawRectangle(&box_rect, &border_brush, 1.0, None);

            // 绘制文本（逐行）
            // DWRITE_TEXT_ALIGNMENT_LEADING=0, DWRITE_PARAGRAPH_ALIGNMENT_NEAR=0
            let font_size = self.text_renderer.font_size();
            let tf = match self
                .render_ctx
                .text_format_cache
                .get_format(font_size, 400, 0, 0)
            {
                Ok(tf) => tf,
                Err(_) => return,
            };

            for (i, line) in lines.iter().enumerate() {
                let line_y = ty + padding + i as f32 * line_height;
                let line_rect = windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
                    left: tx + padding,
                    top: line_y,
                    right: tx + box_w - padding,
                    bottom: line_y + line_height,
                };
                let utf16: Vec<u16> = line.encode_utf16().collect();
                target.DrawText(
                    &utf16,
                    &tf,
                    &line_rect,
                    &text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );
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
                // REQ-P1-09: 活动标签页的状态在 self.content 中，需从中读取
                // SubTask 7.4: 不再在文件名中拼接 "●"，改为独立小圆点
                let (name, is_dirty) = if is_active {
                    (self.content.file_name(), self.content.is_dirty)
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

    fn render_user_menu(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
    ) {
        unsafe {
            let menu_width = self.user_menu.menu_width();
            let menu_height = self.user_menu.menu_height();

            // 边界检查：确保菜单不超出窗口右边界
            let max_x = (self.window_width as f32 - menu_width).max(4.0);
            let menu_x = x.min(max_x);

            // 菜单背景
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
            let menu_rect = D2D_RECT_F {
                left: menu_x,
                top: y,
                right: menu_x + menu_width,
                bottom: y + menu_height,
            };

            // 绘制阴影（右侧和底部）
            let shadow_color = color_f(0.0, 0.0, 0.0, 0.35);
            let shadow_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &shadow_color)
                .unwrap();
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

            target.FillRectangle(&menu_rect, &bg_brush);

            // 菜单边框
            let border_color = color_f(0.3, 0.3, 0.3, 1.0);
            let border_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &border_color)
                .unwrap();
            target.DrawRectangle(&menu_rect, &border_brush, 1.0, None);

            // 保存菜单区域用于点击检测（使用调整后的位置）
            self.user_menu.menu_rect = Some(crate::layout::Region::new(
                menu_x,
                y,
                menu_width,
                menu_height,
            ));

            let text_color = color_f(0.85, 0.85, 0.85, 1.0);
            let text_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &text_color)
                .unwrap();
            let hover_bg = color_f(0.0, 0.47, 0.83, 1.0);
            let hover_brush = self
                .render_ctx
                .brush_cache
                .get_brush(target, &hover_bg)
                .unwrap();
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
            let shortcut_format = self
                .render_ctx
                .text_format_cache
                .get_format(
                    12.0,
                    DWRITE_FONT_WEIGHT_NORMAL.0 as u32,
                    DWRITE_TEXT_ALIGNMENT_TRAILING.0 as u32,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER.0 as u32,
                )
                .unwrap();

            // 绘制用户名头部（蓝色背景）
            let header_height = 40.0;
            let header_rect = D2D_RECT_F {
                left: menu_x,
                top: y,
                right: menu_x + menu_width,
                bottom: y + header_height,
            };
            target.FillRectangle(&header_rect, &hover_brush);
            let username_wide: Vec<u16> = self
                .user_menu
                .username
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let username_rect = D2D_RECT_F {
                left: menu_x + 12.0,
                top: y,
                right: menu_x + menu_width - 12.0,
                bottom: y + header_height,
            };
            target.DrawText(
                &username_wide,
                &text_format,
                &username_rect,
                &self
                    .render_ctx
                    .brush_cache
                    .get_brush(target, &color_f(1.0, 1.0, 1.0, 1.0))
                    .unwrap(),
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 绘制菜单项
            let item_height = 32.0;
            let separator_height = 9.0;
            let mut current_y = y + header_height;

            for (i, item) in self.user_menu.items.iter().enumerate() {
                if item.is_separator() {
                    // 分隔线
                    let sep_rect = D2D_RECT_F {
                        left: menu_x + 8.0,
                        top: current_y + 4.0,
                        right: menu_x + menu_width - 8.0,
                        bottom: current_y + 5.0,
                    };
                    let sep_color = color_f(0.3, 0.3, 0.3, 1.0);
                    let sep_brush = self
                        .render_ctx
                        .brush_cache
                        .get_brush(target, &sep_color)
                        .unwrap();
                    target.FillRectangle(&sep_rect, &sep_brush);
                    current_y += separator_height;
                } else {
                    let is_hover = self.user_menu.hover_index == Some(i);
                    if is_hover {
                        let item_rect = D2D_RECT_F {
                            left: menu_x + 4.0,
                            top: current_y,
                            right: menu_x + menu_width - 4.0,
                            bottom: current_y + item_height,
                        };
                        target.FillRectangle(&item_rect, &hover_brush);
                    }

                    let label_wide: Vec<u16> = item.label().encode_utf16().chain(Some(0)).collect();
                    let label_rect = D2D_RECT_F {
                        left: menu_x + 16.0,
                        top: current_y,
                        right: menu_x + menu_width - 80.0,
                        bottom: current_y + item_height,
                    };
                    target.DrawText(
                        &label_wide,
                        &text_format,
                        &label_rect,
                        &text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    // 快捷键
                    if let Some(shortcut) = item.shortcut() {
                        let shortcut_wide: Vec<u16> =
                            shortcut.encode_utf16().chain(Some(0)).collect();
                        let shortcut_rect = D2D_RECT_F {
                            left: menu_x + menu_width - 78.0,
                            top: current_y,
                            right: menu_x + menu_width - 16.0,
                            bottom: current_y + item_height,
                        };
                        let shortcut_color = color_f(0.6, 0.6, 0.6, 1.0);
                        let shortcut_brush = self
                            .render_ctx
                            .brush_cache
                            .get_brush(target, &shortcut_color)
                            .unwrap();
                        target.DrawText(
                            &shortcut_wide,
                            &shortcut_format,
                            &shortcut_rect,
                            &shortcut_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }

                    current_y += item_height;
                }
            }
        }
    }

    /// 渲染资源管理器空白区域上下文菜单。
    ///
    /// 复用 user_menu 的视觉风格（背景、阴影、边框、hover 高亮），
    /// 但无用户名头部，菜单从顶部 padding 开始直接排列菜单项。
    fn render_explorer_context_menu(
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
    fn render_tab_context_menu(
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
    fn render_activity_bar_context_menu(
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
    fn measure_submenu_width(&mut self, menu_item: &crate::menu_bar::MenuBarItem) -> f32 {
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

    fn render_submenu(
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
            if let Some(path) = &self.content.file_path {
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
