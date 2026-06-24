use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE;
use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_CENTER;
use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_LEADING;

use crate::editor::EditorState;

/// 欢迎页操作类型
#[derive(Clone, Debug)]
pub enum WelcomeAction {
    OpenFolder,
    NewFile,
    CloneRepo,
    OpenRemote,
    /// 打开最近项目，参数为项目路径
    OpenRecentProject(String),
}

/// 欢迎页操作按钮项
struct WelcomeActionItem {
    icon: &'static str,
    label: &'static str,
    shortcut: &'static str,
    action: WelcomeAction,
}

impl EditorState {
    /// 是否显示欢迎页
    pub fn show_welcome(&self) -> bool {
        self.file_path.is_none()
            && self.current_folder.is_none()
            && self.file_tree.is_none()
            && !self.is_dirty
            && self.buffer.get_all_text().is_empty()
    }

    /// 渲染欢迎页 - VS Code风格双栏布局
    /// 左侧：品牌标题 + 操作按钮列表
    /// 右侧：最近项目列表
    pub(crate) fn render_welcome_page(
        &self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        let dwrite = self.text_renderer.dwrite_factory();

        // 定义操作项列表
        let actions = [
            WelcomeActionItem {
                icon: "📁",
                label: "打开文件夹",
                shortcut: "Ctrl+K",
                action: WelcomeAction::OpenFolder,
            },
            WelcomeActionItem {
                icon: "📄",
                label: "新建文件",
                shortcut: "Ctrl+N",
                action: WelcomeAction::NewFile,
            },
            WelcomeActionItem {
                icon: "🌐",
                label: "克隆仓库",
                shortcut: "",
                action: WelcomeAction::CloneRepo,
            },
            WelcomeActionItem {
                icon: "🔌",
                label: "通过 SSH 连接",
                shortcut: "",
                action: WelcomeAction::OpenRemote,
            },
        ];

        // 获取真实的最近项目数据
        let recent_projects = self.recent_projects.list();
        let has_recent_projects = !recent_projects.is_empty();

        unsafe {
            // 画刷缓存
            let bg_brush = target
                .CreateSolidColorBrush(&color_f(0.118, 0.118, 0.118, 1.0), None)
                .unwrap();
            let title_brush = target
                .CreateSolidColorBrush(&color_f(0.9, 0.9, 0.9, 1.0), None)
                .unwrap();
            let subtitle_brush = target
                .CreateSolidColorBrush(&color_f(0.6, 0.6, 0.6, 1.0), None)
                .unwrap();
            let heading_brush = target
                .CreateSolidColorBrush(&color_f(0.85, 0.85, 0.85, 1.0), None)
                .unwrap();
            let text_brush = target
                .CreateSolidColorBrush(&color_f(0.7, 0.7, 0.7, 1.0), None)
                .unwrap();
            let text_light_brush = target
                .CreateSolidColorBrush(&color_f(0.5, 0.5, 0.5, 1.0), None)
                .unwrap();
            let link_brush = target
                .CreateSolidColorBrush(&color_f(0.25, 0.65, 0.95, 1.0), None)
                .unwrap();
            let hover_bg_brush = target
                .CreateSolidColorBrush(&color_f(0.18, 0.18, 0.18, 1.0), None)
                .unwrap();
            let separator_brush = target
                .CreateSolidColorBrush(&color_f(0.25, 0.25, 0.25, 1.0), None)
                .unwrap();

            // 全屏背景（确保覆盖整个编辑器区域）
            let full_bg = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&full_bg, &bg_brush);

            // 布局参数 - 基于可用宽度的百分比（参考 Qoder 布局）
            let top_margin = height * 0.12;
            // 内容总宽度占可用宽度的 70%，整体偏左（左侧留白 15%）
            let content_scale = 0.70f32;
            let left_col_ratio = 0.45f32; // 左列占内容区的 45%
            let right_col_ratio = 0.35f32; // 右列占内容区的 35%
            let gap_ratio = 0.20f32; // 间距占内容区的 20%
            let total_content_width = width * content_scale;
            let left_col_width = total_content_width * left_col_ratio;
            let right_col_width = total_content_width * right_col_ratio;
            let col_gap = total_content_width * gap_ratio;
            // 左侧留白 15%，让内容偏左（类似 Qoder）
            let left_col_x = x + width * 0.15;
            let right_col_x = left_col_x + left_col_width + col_gap;

            // ===== 左侧列：品牌 + 操作 =====

            // Logo/品牌图标（使用emoji作为临时Logo）
            let logo_format = dwrite
                .CreateTextFormat(
                    windows::core::w!("Segoe UI"),
                    None,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                    48.0,
                    windows::core::w!("zh-CN"),
                )
                .unwrap();
            let logo_text: Vec<u16> = "🐑".encode_utf16().chain(Some(0)).collect();
            let logo_rect = D2D_RECT_F {
                left: left_col_x,
                top: y + top_margin,
                right: left_col_x + 60.0,
                bottom: y + top_margin + 60.0,
            };
            target.DrawText(
                &logo_text,
                &logo_format,
                &logo_rect,
                &title_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
            );

            // 品牌标题
            let brand_title_format = dwrite
                .CreateTextFormat(
                    windows::core::w!("Segoe UI"),
                    None,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_BOLD,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                    32.0,
                    windows::core::w!("zh-CN"),
                )
                .unwrap();
            let brand_title: Vec<u16> = "牧羊人编辑器".encode_utf16().chain(Some(0)).collect();
            let brand_title_rect = D2D_RECT_F {
                left: left_col_x + 70.0,
                top: y + top_margin + 5.0,
                right: left_col_x + left_col_width,
                bottom: y + top_margin + 45.0,
            };
            target.DrawText(
                &brand_title,
                &brand_title_format,
                &brand_title_rect,
                &title_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
            );

            // 英文副标题
            let brand_sub_format = dwrite
                .CreateTextFormat(
                    windows::core::w!("Segoe UI"),
                    None,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                    14.0,
                    windows::core::w!("zh-CN"),
                )
                .unwrap();
            let brand_sub: Vec<u16> = "Aether Editor — 纯 Rust 原生编辑器"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let brand_sub_rect = D2D_RECT_F {
                left: left_col_x + 70.0,
                top: y + top_margin + 42.0,
                right: left_col_x + left_col_width,
                bottom: y + top_margin + 65.0,
            };
            target.DrawText(
                &brand_sub,
                &brand_sub_format,
                &brand_sub_rect,
                &subtitle_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
            );

            // 操作按钮列表
            let action_start_y = y + top_margin + 100.0;
            let action_item_h = 48.0f32;
            let action_gap = 4.0f32;
            let action_icon_w = 40.0f32;

            let action_icon_format = dwrite
                .CreateTextFormat(
                    windows::core::w!("Segoe UI"),
                    None,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                    18.0,
                    windows::core::w!("zh-CN"),
                )
                .unwrap();
            let _ = action_icon_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);

            let action_label_format = dwrite
                .CreateTextFormat(
                    windows::core::w!("Segoe UI"),
                    None,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                    14.0,
                    windows::core::w!("zh-CN"),
                )
                .unwrap();

            let action_shortcut_format = dwrite
                .CreateTextFormat(
                    windows::core::w!("Segoe UI"),
                    None,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                    12.0,
                    windows::core::w!("zh-CN"),
                )
                .unwrap();
            let _ = action_shortcut_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);

            for (i, action) in actions.iter().enumerate() {
                let ay = action_start_y + i as f32 * (action_item_h + action_gap);

                // 悬停背景（简化：始终显示轻微背景）
                let item_bg = D2D_RECT_F {
                    left: left_col_x,
                    top: ay,
                    right: left_col_x + left_col_width,
                    bottom: ay + action_item_h,
                };
                target.FillRectangle(&item_bg, &hover_bg_brush);

                // 图标
                let icon_text: Vec<u16> = action.icon.encode_utf16().chain(Some(0)).collect();
                let icon_rect = D2D_RECT_F {
                    left: left_col_x + 8.0,
                    top: ay + 8.0,
                    right: left_col_x + action_icon_w,
                    bottom: ay + action_item_h - 8.0,
                };
                target.DrawText(
                    &icon_text,
                    &action_icon_format,
                    &icon_rect,
                    &text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );

                // 标签
                let label_text: Vec<u16> = action.label.encode_utf16().chain(Some(0)).collect();
                let label_rect = D2D_RECT_F {
                    left: left_col_x + action_icon_w + 8.0,
                    top: ay + 10.0,
                    right: left_col_x + left_col_width - 80.0,
                    bottom: ay + action_item_h - 10.0,
                };
                target.DrawText(
                    &label_text,
                    &action_label_format,
                    &label_rect,
                    &text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );

                // 快捷键（如果有）
                if !action.shortcut.is_empty() {
                    let shortcut_text: Vec<u16> =
                        action.shortcut.encode_utf16().chain(Some(0)).collect();
                    let shortcut_rect = D2D_RECT_F {
                        left: left_col_x + left_col_width - 75.0,
                        top: ay + 14.0,
                        right: left_col_x + left_col_width - 8.0,
                        bottom: ay + action_item_h - 14.0,
                    };
                    target.DrawText(
                        &shortcut_text,
                        &action_shortcut_format,
                        &shortcut_rect,
                        &text_light_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }

            // 底部提示（更显眼）
            let tip_y = action_start_y + actions.len() as f32 * (action_item_h + action_gap) + 30.0;
            let tip_format = dwrite
                .CreateTextFormat(
                    windows::core::w!("Segoe UI"),
                    None,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                    13.0,
                    windows::core::w!("zh-CN"),
                )
                .unwrap();
            let tip_text: Vec<u16> = "💡 提示：按 Ctrl+K 快速打开文件夹，Ctrl+N 新建文件"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let tip_rect = D2D_RECT_F {
                left: left_col_x,
                top: tip_y,
                right: left_col_x + left_col_width,
                bottom: tip_y + 24.0,
            };
            target.DrawText(
                &tip_text,
                &tip_format,
                &tip_rect,
                &text_light_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
            );

            // ===== 右侧列：最近项目 =====

            // 分隔线
            let sep_x = right_col_x - col_gap / 2.0;
            let sep_rect = D2D_RECT_F {
                left: sep_x,
                top: y + top_margin,
                right: sep_x + 1.0,
                bottom: y + height - top_margin,
            };
            target.FillRectangle(&sep_rect, &separator_brush);

            // "最近项目"标题
            let recent_heading_format = dwrite
                .CreateTextFormat(
                    windows::core::w!("Segoe UI"),
                    None,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_BOLD,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                    16.0,
                    windows::core::w!("zh-CN"),
                )
                .unwrap();
            let recent_heading: Vec<u16> = "最近项目".encode_utf16().chain(Some(0)).collect();
            let recent_heading_rect = D2D_RECT_F {
                left: right_col_x,
                top: y + top_margin,
                right: right_col_x + right_col_width,
                bottom: y + top_margin + 28.0,
            };
            target.DrawText(
                &recent_heading,
                &recent_heading_format,
                &recent_heading_rect,
                &heading_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
            );

            // 项目列表
            let project_start_y = y + top_margin + 40.0;
            let project_item_h = 56.0f32;

            let project_icon_format = dwrite
                .CreateTextFormat(
                    windows::core::w!("Segoe UI"),
                    None,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                    20.0,
                    windows::core::w!("zh-CN"),
                )
                .unwrap();
            let _ = project_icon_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);

            let project_name_format = dwrite
                .CreateTextFormat(
                    windows::core::w!("Segoe UI"),
                    None,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                    14.0,
                    windows::core::w!("zh-CN"),
                )
                .unwrap();

            let project_path_format = dwrite
                .CreateTextFormat(
                    windows::core::w!("Segoe UI"),
                    None,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                    11.0,
                    windows::core::w!("zh-CN"),
                )
                .unwrap();

            for (i, project) in recent_projects.iter().enumerate() {
                let py = project_start_y + i as f32 * (project_item_h + 8.0);

                // 项目项背景
                let proj_bg = D2D_RECT_F {
                    left: right_col_x,
                    top: py,
                    right: right_col_x + right_col_width,
                    bottom: py + project_item_h,
                };
                target.FillRectangle(&proj_bg, &hover_bg_brush);

                // 文件夹图标
                let folder_icon: Vec<u16> = "📁".encode_utf16().chain(Some(0)).collect();
                let folder_rect = D2D_RECT_F {
                    left: right_col_x + 8.0,
                    top: py + 12.0,
                    right: right_col_x + 40.0,
                    bottom: py + project_item_h - 12.0,
                };
                target.DrawText(
                    &folder_icon,
                    &project_icon_format,
                    &folder_rect,
                    &text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );

                // 项目名称
                let name_text: Vec<u16> = project.name.encode_utf16().chain(Some(0)).collect();
                let name_rect = D2D_RECT_F {
                    left: right_col_x + 44.0,
                    top: py + 8.0,
                    right: right_col_x + right_col_width - 8.0,
                    bottom: py + 28.0,
                };
                target.DrawText(
                    &name_text,
                    &project_name_format,
                    &name_rect,
                    &text_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );

                // 项目路径
                let path_text: Vec<u16> = project.path.encode_utf16().chain(Some(0)).collect();
                let path_rect = D2D_RECT_F {
                    left: right_col_x + 44.0,
                    top: py + 28.0,
                    right: right_col_x + right_col_width - 8.0,
                    bottom: py + project_item_h - 8.0,
                };
                target.DrawText(
                    &path_text,
                    &project_path_format,
                    &path_rect,
                    &text_light_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            // 空状态提示
            if !has_recent_projects {
                let empty_text: Vec<u16> = "暂无最近项目".encode_utf16().chain(Some(0)).collect();
                let empty_rect = D2D_RECT_F {
                    left: right_col_x,
                    top: project_start_y + 20.0,
                    right: right_col_x + right_col_width,
                    bottom: project_start_y + 50.0,
                };
                target.DrawText(
                    &empty_text,
                    &project_name_format,
                    &empty_rect,
                    &text_light_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            // "更多"链接（仅当有项目时显示）
            if has_recent_projects {
                let more_y =
                    project_start_y + recent_projects.len() as f32 * (project_item_h + 8.0) + 12.0;
                let more_format = dwrite
                    .CreateTextFormat(
                        windows::core::w!("Segoe UI"),
                        None,
                        windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_NORMAL,
                        windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                        windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                        13.0,
                        windows::core::w!("zh-CN"),
                    )
                    .unwrap();
                let more_text: Vec<u16> = "更多...".encode_utf16().chain(Some(0)).collect();
                let more_rect = D2D_RECT_F {
                    left: right_col_x,
                    top: more_y,
                    right: right_col_x + 60.0,
                    bottom: more_y + 22.0,
                };
                target.DrawText(
                    &more_text,
                    &more_format,
                    &more_rect,
                    &link_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    /// 处理欢迎页点击（布局与 render_welcome_page 保持一致）
    pub(crate) fn handle_welcome_click(
        &self,
        mouse_x: f32,
        mouse_y: f32,
        welcome_x: f32,
        welcome_y: f32,
        welcome_width: f32,
        welcome_height: f32,
    ) -> Option<WelcomeAction> {
        // 🔧 与 render_welcome_page 保持完全一致的布局计算
        let top_margin = welcome_height * 0.12;
        let content_scale = 0.70f32;
        let left_col_ratio = 0.45f32;
        let right_col_ratio = 0.35f32;
        let gap_ratio = 0.20f32;
        let total_content_width = welcome_width * content_scale;
        let left_col_width = total_content_width * left_col_ratio;
        let right_col_width = total_content_width * right_col_ratio;
        let col_gap = total_content_width * gap_ratio;
        let left_col_x = welcome_x + welcome_width * 0.15;
        let right_col_x = left_col_x + left_col_width + col_gap;
        let action_start_y = welcome_y + top_margin + 100.0;
        let action_item_h = 48.0f32;
        let action_gap = 4.0f32;

        let actions = [
            WelcomeActionItem {
                icon: "📁",
                label: "打开文件夹",
                shortcut: "Ctrl+K",
                action: WelcomeAction::OpenFolder,
            },
            WelcomeActionItem {
                icon: "📄",
                label: "新建文件",
                shortcut: "Ctrl+N",
                action: WelcomeAction::NewFile,
            },
            WelcomeActionItem {
                icon: "🌐",
                label: "克隆仓库",
                shortcut: "",
                action: WelcomeAction::CloneRepo,
            },
            WelcomeActionItem {
                icon: "🔌",
                label: "通过 SSH 连接",
                shortcut: "",
                action: WelcomeAction::OpenRemote,
            },
        ];

        // 检测左侧操作按钮点击
        for (i, action) in actions.iter().enumerate() {
            let ay = action_start_y + i as f32 * (action_item_h + action_gap);
            if mouse_x >= left_col_x
                && mouse_x <= left_col_x + left_col_width
                && mouse_y >= ay
                && mouse_y <= ay + action_item_h
            {
                return Some(action.action.clone());
            }
        }

        // 检测右侧最近项目点击
        let project_start_y = welcome_y + top_margin + 40.0;
        let project_item_h = 56.0f32;
        let recent_projects = self.recent_projects.list();

        for (i, project) in recent_projects.iter().enumerate() {
            let py = project_start_y + i as f32 * (project_item_h + 8.0);
            if mouse_x >= right_col_x
                && mouse_x <= right_col_x + right_col_width
                && mouse_y >= py
                && mouse_y <= py + project_item_h
            {
                return Some(WelcomeAction::OpenRecentProject(project.path.clone()));
            }
        }

        None
    }
}

/// 颜色辅助函数（与 aether_render 中的 color_f 等价）
fn color_f(
    r: f32,
    g: f32,
    b: f32,
    a: f32,
) -> windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
    windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r, g, b, a }
}
