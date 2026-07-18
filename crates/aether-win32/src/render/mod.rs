pub(crate) use aether_core::char_width::char_width as unicode_char_width;
pub(crate) use aether_core::lexer::Language;
pub(crate) use aether_core::workspace::file_tree::{FileKind, FileTree};
pub(crate) use aether_render::d2d::factory::color_f;
pub(crate) use aether_render::d2d::glass;
pub(crate) use windows::Win32::Graphics::Direct2D::Common::{D2D_POINT_2F, D2D_RECT_F};
pub(crate) use windows::Win32::Graphics::Direct2D::{
    ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_NONE,
};
pub(crate) use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING,
};

pub(crate) use crate::editor::{BottomPanelTab, EditorState};
pub(crate) use crate::layout::{Region, ACTIVITY_BAR_BUTTON_SIZE};
pub(crate) use crate::settings::ProviderTemplateButton;

/// 绘制输入框的四条边框
pub(crate) unsafe fn draw_input_borders(
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
        // 多会话并发：轮询所有会话（活动 + 后台），对本帧刚完成的每个会话处理 Agent 动作
        let ai_current_folder = self.current_folder.clone();
        let ai_completed = self
            .ai_panel
            .poll_all_background(ai_current_folder.as_deref());
        self.ai_panel.sync_active_title();
        for conv_idx in ai_completed {
            self.process_ai_agent_actions_for(conv_idx);
        }

        // 设置面板：轮询测试连接结果
        match self.settings_panel.poll_test_result() {
            crate::settings::TestPollResult::SuccessWithPendingSave => {
                self.save_ai_settings();
                // 保存成功后退出模型编辑态，返回模型列表
                self.settings_panel.model_editing = false;
                self.dirty_tracker.mark_full_window();
            }
            crate::settings::TestPollResult::Success
            | crate::settings::TestPollResult::Failed
            | crate::settings::TestPollResult::FailedWithPendingSave => {
                self.dirty_tracker.mark_full_window();
            }
            crate::settings::TestPollResult::Pending => {}
        }

        // LSP: 轮询诊断事件，更新 diagnostics 字段
        self.poll_lsp_events();

        // 终端输出轮询：从读取线程拉取子进程 stdout/stderr 并写入输出缓存。
        // 此前未调用 flush_output 导致 shell 输出无法显示，现在每帧轮询保证实时性。
        if self.terminal_panel.running {
            self.terminal_panel.poll_startup();
            self.terminal_panel.flush_output();
            // AI Agent 排队命令：终端就绪后自动发送执行
            self.terminal_panel.flush_pending_commands();
        }

        // 懒加载预扫描：确保所有 is_expanded 但未加载的目录节点子项已就绪
        // 这样渲染文件树时不会因目录未加载而显示空
        self.preload_expanded_dirs();

        // AI 终端命令后：在监视窗口内检测工作区根目录变化，变化则轻量刷新资源管理器，
        // 使 AI 通过命令（如 Remove-Item / New-Item）删除或新建的文件即时同步显示。
        if let Some(until) = self.fs_watch_until {
            if std::time::Instant::now() >= until {
                self.fs_watch_until = None;
            } else {
                let sig = self.workspace_root_signature();
                if sig != self.fs_last_root_sig {
                    self.fs_last_root_sig = sig;
                    self.refresh_file_tree_light();
                }
            }
        }

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
}

mod account;
mod ai;
mod chrome;
mod dialogs;
mod editor_view;
mod find;
mod menus;
mod remote;
mod remote_dialogs;
mod settings_ai;
mod settings_general;
mod settings_models;
mod sidebar;
mod sidebar_files;
mod sidebar_scm;
mod tabs;
mod terminal;
