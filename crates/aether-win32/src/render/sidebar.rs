use super::*;

impl EditorState {
    pub(super) fn render_right_panel(
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

    pub(super) fn render_sidebar(
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
    pub(super) fn render_loading_spinner(
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
}
