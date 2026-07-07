#![allow(clippy::items_after_test_module, clippy::useless_vec)]

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ROUNDED_RECT};
use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_CENTER;
use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_LEADING;

use crate::editor::EditorState;

#[derive(Clone, Debug, PartialEq)]
pub enum WelcomeAction {
    OpenFolder,
    NewProject,
    CloneRepo,
    OpenRemote,
    OpenRecentProject(String),
    MoreRecentProjects,
}

struct WelcomeActionItem {
    icon_kind: crate::icons::IconKind,
    label: &'static str,
    shortcut: &'static str,
    action: WelcomeAction,
}

#[derive(Debug)]
pub struct WelcomeLayout {
    pub left_col_x: f32,
    pub left_col_width: f32,
    pub right_col_x: f32,
    pub right_col_width: f32,
    pub top_margin: f32,
    pub action_start_y: f32,
    pub action_item_h: f32,
    pub action_gap: f32,
    pub project_start_y: f32,
    pub project_item_h: f32,
    pub more_y: Option<f32>,
    pub more_height: f32,
    /// 空状态"打开文件夹"按钮 rect (left, top, right, bottom)，仅在最近项目为空时为 Some
    pub empty_state_button_rect: Option<(f32, f32, f32, f32)>,
}

impl WelcomeLayout {
    pub fn compute(x: f32, y: f32, width: f32, height: f32, project_count: usize) -> Self {
        let top_margin = height * 0.10;
        // 提升信息密度：使用 88% 的窗口宽度（原 70%），列间距收窄到 8%
        let content_scale = 0.88f32;
        let left_col_ratio = 0.42f32;
        let right_col_ratio = 0.50f32;
        let gap_ratio = 0.08f32;
        let total_content_width = width * content_scale;
        let left_col_width = total_content_width * left_col_ratio;
        let right_col_width = total_content_width * right_col_ratio;
        let col_gap = total_content_width * gap_ratio;
        let left_col_x = x + width * 0.06;
        let right_col_x = left_col_x + left_col_width + col_gap;

        // 收窄品牌区与操作列表之间的间距（原 +100 → +76）
        let action_start_y = y + top_margin + 76.0;
        let action_item_h = 44.0f32;
        let action_gap = 4.0f32;

        let project_start_y = y + top_margin + 40.0;
        let project_item_h = 52.0f32;

        let more_y = if project_count > 0 {
            Some(project_start_y + project_count as f32 * (project_item_h + 8.0) + 12.0)
        } else {
            None
        };
        let more_height = 22.0f32;

        // 空状态按钮：仅在无最近项目时计算
        // 布局：图标(48) + 12 间距 + 主文案(20) + 6 间距 + 副文案(16) + 16 间距 → 按钮顶部
        let empty_state_button_rect = if project_count == 0 {
            let center_x = right_col_x + right_col_width / 2.0;
            let button_w = 120.0f32;
            let button_h = 32.0f32;
            let button_y = project_start_y + 48.0 + 12.0 + 20.0 + 6.0 + 16.0 + 16.0;
            Some((
                center_x - button_w / 2.0,
                button_y,
                center_x + button_w / 2.0,
                button_y + button_h,
            ))
        } else {
            None
        };

        Self {
            left_col_x,
            left_col_width,
            right_col_x,
            right_col_width,
            top_margin,
            action_start_y,
            action_item_h,
            action_gap,
            project_start_y,
            project_item_h,
            more_y,
            more_height,
            empty_state_button_rect,
        }
    }
}

impl EditorState {
    pub fn show_welcome(&self) -> bool {
        self.content.file_path.is_none()
            && self.current_folder.is_none()
            && self.file_tree.is_none()
            && !self.content.is_dirty
            && self.content.buffer.get_all_text().is_empty()
    }

    fn welcome_actions() -> [WelcomeActionItem; 4] {
        [
            WelcomeActionItem {
                icon_kind: crate::icons::IconKind::OpenFolder,
                label: "打开文件夹",
                shortcut: "Ctrl+K",
                action: WelcomeAction::OpenFolder,
            },
            WelcomeActionItem {
                icon_kind: crate::icons::IconKind::NewFile,
                label: "新建项目",
                shortcut: "Ctrl+N",
                action: WelcomeAction::NewProject,
            },
            WelcomeActionItem {
                icon_kind: crate::icons::IconKind::Clone,
                label: "克隆仓库",
                shortcut: "",
                action: WelcomeAction::CloneRepo,
            },
            WelcomeActionItem {
                icon_kind: crate::icons::IconKind::Ssh,
                label: "通过 SSH 连接",
                shortcut: "",
                action: WelcomeAction::OpenRemote,
            },
        ]
    }

    pub(crate) fn hit_test_welcome_action(
        &self,
        mouse_x: f32,
        mouse_y: f32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Option<WelcomeAction> {
        let recent_projects = self.recent_projects.list();
        let layout = WelcomeLayout::compute(x, y, width, height, recent_projects.len());
        let actions = Self::welcome_actions();

        for (i, action) in actions.iter().enumerate() {
            let ay = layout.action_start_y + i as f32 * (layout.action_item_h + layout.action_gap);
            if mouse_x >= layout.left_col_x
                && mouse_x <= layout.left_col_x + layout.left_col_width
                && mouse_y >= ay
                && mouse_y <= ay + layout.action_item_h
            {
                return Some(action.action.clone());
            }
        }

        for (i, project) in recent_projects.iter().enumerate() {
            let py = layout.project_start_y + i as f32 * (layout.project_item_h + 8.0);
            if mouse_x >= layout.right_col_x
                && mouse_x <= layout.right_col_x + layout.right_col_width
                && mouse_y >= py
                && mouse_y <= py + layout.project_item_h
            {
                return Some(WelcomeAction::OpenRecentProject(project.path.clone()));
            }
        }

        if let Some(more_y) = layout.more_y {
            if mouse_x >= layout.right_col_x
                && mouse_x <= layout.right_col_x + 60.0
                && mouse_y >= more_y
                && mouse_y <= more_y + layout.more_height
            {
                return Some(WelcomeAction::MoreRecentProjects);
            }
        }

        // 空状态"打开文件夹"按钮
        if let Some((bl, bt, br, bb)) = layout.empty_state_button_rect {
            if mouse_x >= bl && mouse_x <= br && mouse_y >= bt && mouse_y <= bb {
                return Some(WelcomeAction::OpenFolder);
            }
        }

        None
    }

    pub(crate) fn render_welcome_page(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        // 确保矢量图标几何已创建（懒加载，仅首次调用时执行）
        self.icons.ensure_created_from_target(target);
        let dwrite = self.text_renderer.dwrite_factory();
        let actions = Self::welcome_actions();
        let recent_projects = self.recent_projects.list();
        let has_recent_projects = !recent_projects.is_empty();
        let layout = WelcomeLayout::compute(x, y, width, height, recent_projects.len());

        unsafe {
            // 欢迎页背景：半透明深灰，让 DWM Mica/Acrylic 透出
            // alpha=0.55 在 Mica Alt 上呈现为带色调的毛玻璃
            let bg_brush = target
                .CreateSolidColorBrush(&color_f(0.08, 0.08, 0.10, 0.55), None)
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let title_brush = target
                .CreateSolidColorBrush(&color_f(0.9, 0.9, 0.9, 1.0), None)
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let subtitle_brush = target
                .CreateSolidColorBrush(&color_f(0.6, 0.6, 0.6, 1.0), None)
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let heading_brush = target
                .CreateSolidColorBrush(&color_f(0.85, 0.85, 0.85, 1.0), None)
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let text_brush = target
                .CreateSolidColorBrush(&color_f(0.7, 0.7, 0.7, 1.0), None)
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let text_light_brush = target
                .CreateSolidColorBrush(&color_f(0.5, 0.5, 0.5, 1.0), None)
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let link_brush = target
                .CreateSolidColorBrush(&color_f(0.25, 0.65, 0.95, 1.0), None)
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let normal_bg_brush = target
                .CreateSolidColorBrush(&color_f(0.15, 0.15, 0.15, 1.0), None)
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let hover_bg_brush = target
                .CreateSolidColorBrush(&color_f(0.22, 0.22, 0.22, 1.0), None)
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let hover_text_brush = target
                .CreateSolidColorBrush(&color_f(0.9, 0.9, 0.9, 1.0), None)
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let separator_brush = target
                .CreateSolidColorBrush(&color_f(0.25, 0.25, 0.25, 1.0), None)
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });

            let full_bg = D2D_RECT_F {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
            target.FillRectangle(&full_bg, &bg_brush);

            // UI-UX: 使用矢量 EmojiSheep 图标替代 emoji 字符，保持视觉一致性
            let logo_size = 60.0f32;
            let logo_x = layout.left_col_x;
            let logo_y = y + layout.top_margin;
            self.icons.draw(
                target,
                crate::icons::IconKind::EmojiSheep,
                logo_x,
                logo_y,
                logo_size,
                logo_size,
                &title_brush,
            );

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
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let brand_title: Vec<u16> = "牧羊人编辑器".encode_utf16().chain(Some(0)).collect();
            let brand_title_rect = D2D_RECT_F {
                left: layout.left_col_x + 70.0,
                top: y + layout.top_margin + 5.0,
                right: layout.left_col_x + layout.left_col_width,
                bottom: y + layout.top_margin + 45.0,
            };
            target.DrawText(
                &brand_title,
                &brand_title_format,
                &brand_title_rect,
                &title_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
            );

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
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let brand_sub: Vec<u16> = "Aether Editor — 纯 Rust 原生编辑器"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let brand_sub_rect = D2D_RECT_F {
                left: layout.left_col_x + 70.0,
                top: y + layout.top_margin + 42.0,
                right: layout.left_col_x + layout.left_col_width,
                bottom: y + layout.top_margin + 65.0,
            };
            target.DrawText(
                &brand_sub,
                &brand_sub_format,
                &brand_sub_rect,
                &subtitle_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
            );

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
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
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
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });

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
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let _ = action_shortcut_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);

            // 焦点边框画刷（键盘导航时显示）
            let focus_border_brush = target
                .CreateSolidColorBrush(&color_f(0.25, 0.65, 0.95, 1.0), None)
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });

            for (i, action) in actions.iter().enumerate() {
                let ay =
                    layout.action_start_y + i as f32 * (layout.action_item_h + layout.action_gap);
                let is_hovered = self.welcome_hover_action.as_ref() == Some(&action.action);
                let is_focused = self.welcome_focus_action.as_ref() == Some(&action.action);

                let item_bg = D2D_RECT_F {
                    left: layout.left_col_x,
                    top: ay,
                    right: layout.left_col_x + layout.left_col_width,
                    bottom: ay + layout.action_item_h,
                };
                target.FillRectangle(
                    &item_bg,
                    if is_hovered || is_focused {
                        &hover_bg_brush
                    } else {
                        &normal_bg_brush
                    },
                );

                // 键盘焦点边框
                if is_focused {
                    target.DrawRectangle(&item_bg, &focus_border_brush, 1.5, None);
                }

                // 矢量图标（替代 emoji）
                let icon_color = if is_hovered || is_focused {
                    color_f(0.95, 0.95, 0.95, 1.0)
                } else {
                    color_f(0.75, 0.75, 0.75, 1.0)
                };
                let icon_brush = target
                    .CreateSolidColorBrush(&icon_color, None)
                    .unwrap_or_else(|e| {
                        eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                        panic!("D2D device lost")
                    });
                self.icons.draw(
                    target,
                    action.icon_kind,
                    layout.left_col_x + 8.0,
                    ay + 8.0,
                    action_icon_w - 8.0,
                    layout.action_item_h - 16.0,
                    &icon_brush,
                );

                let label_text: Vec<u16> = action.label.encode_utf16().chain(Some(0)).collect();
                let label_rect = D2D_RECT_F {
                    left: layout.left_col_x + action_icon_w + 8.0,
                    top: ay + 10.0,
                    right: layout.left_col_x + layout.left_col_width - 80.0,
                    bottom: ay + layout.action_item_h - 10.0,
                };
                target.DrawText(
                    &label_text,
                    &action_label_format,
                    &label_rect,
                    if is_hovered {
                        &hover_text_brush
                    } else {
                        &text_brush
                    },
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );

                if !action.shortcut.is_empty() {
                    let shortcut_text: Vec<u16> =
                        action.shortcut.encode_utf16().chain(Some(0)).collect();
                    let shortcut_rect = D2D_RECT_F {
                        left: layout.left_col_x + layout.left_col_width - 75.0,
                        top: ay + 14.0,
                        right: layout.left_col_x + layout.left_col_width - 8.0,
                        bottom: ay + layout.action_item_h - 14.0,
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

            let tip_y = layout.action_start_y
                + actions.len() as f32 * (layout.action_item_h + layout.action_gap)
                + 30.0;
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
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let tip_text: Vec<u16> = "💡 提示：按 Ctrl+K 快速打开文件夹，Ctrl+N 新建项目"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let tip_rect = D2D_RECT_F {
                left: layout.left_col_x,
                top: tip_y,
                right: layout.left_col_x + layout.left_col_width,
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

            let col_gap = layout.right_col_x - layout.left_col_x - layout.left_col_width;
            let sep_x = layout.right_col_x - col_gap / 2.0;
            let sep_rect = D2D_RECT_F {
                left: sep_x,
                top: y + layout.top_margin,
                right: sep_x + 1.0,
                bottom: y + height - layout.top_margin,
            };
            target.FillRectangle(&sep_rect, &separator_brush);

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
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
            let recent_heading: Vec<u16> = "最近项目".encode_utf16().chain(Some(0)).collect();
            let recent_heading_rect = D2D_RECT_F {
                left: layout.right_col_x,
                top: y + layout.top_margin,
                right: layout.right_col_x + layout.right_col_width,
                bottom: y + layout.top_margin + 28.0,
            };
            target.DrawText(
                &recent_heading,
                &recent_heading_format,
                &recent_heading_rect,
                &heading_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
            );

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
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });
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
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });

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
                .unwrap_or_else(|e| {
                    eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                    panic!("D2D device lost")
                });

            for (i, project) in recent_projects.iter().enumerate() {
                let py = layout.project_start_y + i as f32 * (layout.project_item_h + 8.0);
                let project_action = WelcomeAction::OpenRecentProject(project.path.clone());
                let is_hovered = self.welcome_hover_action.as_ref() == Some(&project_action);
                let is_focused = self.welcome_focus_action.as_ref() == Some(&project_action);

                let proj_bg = D2D_RECT_F {
                    left: layout.right_col_x,
                    top: py,
                    right: layout.right_col_x + layout.right_col_width,
                    bottom: py + layout.project_item_h,
                };
                target.FillRectangle(
                    &proj_bg,
                    if is_hovered || is_focused {
                        &hover_bg_brush
                    } else {
                        &normal_bg_brush
                    },
                );

                // 键盘焦点边框
                if is_focused {
                    target.DrawRectangle(&proj_bg, &focus_border_brush, 1.5, None);
                }

                let folder_brush = if is_hovered {
                    &hover_text_brush
                } else {
                    &text_brush
                };
                self.icons.draw(
                    target,
                    crate::icons::IconKind::Folder,
                    layout.right_col_x + 8.0,
                    py + 12.0,
                    32.0,
                    layout.project_item_h - 24.0,
                    folder_brush,
                );

                let name_text: Vec<u16> = project.name.encode_utf16().chain(Some(0)).collect();
                let name_rect = D2D_RECT_F {
                    left: layout.right_col_x + 44.0,
                    top: py + 8.0,
                    right: layout.right_col_x + layout.right_col_width - 8.0,
                    bottom: py + 28.0,
                };
                target.DrawText(
                    &name_text,
                    &project_name_format,
                    &name_rect,
                    if is_hovered {
                        &hover_text_brush
                    } else {
                        &text_brush
                    },
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );

                let path_str = ellipsize_path(
                    &project.path,
                    &project_path_format,
                    dwrite,
                    layout.right_col_width - 52.0,
                );
                let path_text: Vec<u16> = path_str.encode_utf16().chain(Some(0)).collect();
                let path_rect = D2D_RECT_F {
                    left: layout.right_col_x + 44.0,
                    top: py + 28.0,
                    right: layout.right_col_x + layout.right_col_width - 8.0,
                    bottom: py + layout.project_item_h - 8.0,
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

            if !has_recent_projects {
                let center_x = layout.right_col_x + layout.right_col_width / 2.0;

                // 1. 大图标 Folder 48x48 居中，灰色柔和
                let icon_size = 48.0f32;
                let icon_x = center_x - icon_size / 2.0;
                let icon_y = layout.project_start_y;
                let empty_icon_brush = target
                    .CreateSolidColorBrush(&color_f(150.0 / 255.0, 150.0 / 255.0, 150.0 / 255.0, 200.0 / 255.0), None)
                    .unwrap_or_else(|e| {
                        eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                        panic!("D2D device lost")
                    });
                self.icons.draw(
                    target,
                    crate::icons::IconKind::Folder,
                    icon_x,
                    icon_y,
                    icon_size,
                    icon_size,
                    &empty_icon_brush,
                );

                // 2. 主文案 "暂无最近项目" 14pt 居中
                let empty_main_format = dwrite
                    .CreateTextFormat(
                        windows::core::w!("Segoe UI"),
                        None,
                        windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_NORMAL,
                        windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                        windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                        14.0,
                        windows::core::w!("zh-CN"),
                    )
                    .unwrap_or_else(|e| {
                        eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                        panic!("D2D device lost")
                    });
                let _ = empty_main_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let main_y = icon_y + icon_size + 12.0;
                let main_text: Vec<u16> = "暂无最近项目".encode_utf16().chain(Some(0)).collect();
                let main_rect = D2D_RECT_F {
                    left: layout.right_col_x,
                    top: main_y,
                    right: layout.right_col_x + layout.right_col_width,
                    bottom: main_y + 20.0,
                };
                let empty_main_brush = target
                    .CreateSolidColorBrush(&color_f(180.0 / 255.0, 180.0 / 255.0, 180.0 / 255.0, 1.0), None)
                    .unwrap_or_else(|e| {
                        eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                        panic!("D2D device lost")
                    });
                target.DrawText(
                    &main_text,
                    &empty_main_format,
                    &main_rect,
                    &empty_main_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );

                // 3. 副文案 "打开文件夹开始编辑" 12pt 居中
                let empty_sub_format = dwrite
                    .CreateTextFormat(
                        windows::core::w!("Segoe UI"),
                        None,
                        windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_NORMAL,
                        windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                        windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                        12.0,
                        windows::core::w!("zh-CN"),
                    )
                    .unwrap_or_else(|e| {
                        eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                        panic!("D2D device lost")
                    });
                let _ = empty_sub_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let sub_y = main_y + 20.0 + 6.0;
                let sub_text: Vec<u16> = "打开文件夹开始编辑".encode_utf16().chain(Some(0)).collect();
                let sub_rect = D2D_RECT_F {
                    left: layout.right_col_x,
                    top: sub_y,
                    right: layout.right_col_x + layout.right_col_width,
                    bottom: sub_y + 16.0,
                };
                let empty_sub_brush = target
                    .CreateSolidColorBrush(&color_f(140.0 / 255.0, 140.0 / 255.0, 140.0 / 255.0, 1.0), None)
                    .unwrap_or_else(|e| {
                        eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                        panic!("D2D device lost")
                    });
                target.DrawText(
                    &sub_text,
                    &empty_sub_format,
                    &sub_rect,
                    &empty_sub_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );

                // 4. "打开文件夹" 按钮：圆角矩形，hover 变亮
                if let Some((bl, bt, br, bb)) = layout.empty_state_button_rect {
                    let is_btn_hovered =
                        self.welcome_hover_action.as_ref() == Some(&WelcomeAction::OpenFolder);
                    let btn_color = if is_btn_hovered {
                        color_f(80.0 / 255.0, 120.0 / 255.0, 200.0 / 255.0, 1.0)
                    } else {
                        color_f(60.0 / 255.0, 100.0 / 255.0, 180.0 / 255.0, 1.0)
                    };
                    let btn_brush = target
                        .CreateSolidColorBrush(&btn_color, None)
                        .unwrap_or_else(|e| {
                            eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                            panic!("D2D device lost")
                        });
                    let btn_rect = D2D_RECT_F {
                        left: bl,
                        top: bt,
                        right: br,
                        bottom: bb,
                    };
                    let rounded = D2D1_ROUNDED_RECT {
                        rect: btn_rect,
                        radiusX: 4.0,
                        radiusY: 4.0,
                    };
                    target.FillRoundedRectangle(&rounded, &btn_brush);

                    // 按钮文本 "打开文件夹" 白色居中
                    let btn_text: Vec<u16> =
                        "打开文件夹".encode_utf16().chain(Some(0)).collect();
                    let btn_text_rect = D2D_RECT_F {
                        left: bl,
                        top: bt + 6.0,
                        right: br,
                        bottom: bb - 6.0,
                    };
                    let white_brush = target
                        .CreateSolidColorBrush(&color_f(1.0, 1.0, 1.0, 1.0), None)
                        .unwrap_or_else(|e| {
                            eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                            panic!("D2D device lost")
                        });
                    target.DrawText(
                        &btn_text,
                        &empty_main_format,
                        &btn_text_rect,
                        &white_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }

            if let Some(more_y) = layout.more_y {
                let is_more_hovered =
                    self.welcome_hover_action.as_ref() == Some(&WelcomeAction::MoreRecentProjects);
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
                    .unwrap_or_else(|e| {
                        eprintln!("[H-14] D2D 操作失败 (设备丢失?): {:?}", e);
                        panic!("D2D device lost")
                    });
                let more_text: Vec<u16> = "更多...".encode_utf16().chain(Some(0)).collect();
                let more_rect = D2D_RECT_F {
                    left: layout.right_col_x,
                    top: more_y,
                    right: layout.right_col_x + 60.0,
                    bottom: more_y + layout.more_height,
                };
                target.DrawText(
                    &more_text,
                    &more_format,
                    &more_rect,
                    if is_more_hovered {
                        &hover_text_brush
                    } else {
                        &link_brush
                    },
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    pub(crate) fn handle_welcome_click(
        &self,
        mouse_x: f32,
        mouse_y: f32,
        welcome_x: f32,
        welcome_y: f32,
        welcome_width: f32,
        welcome_height: f32,
    ) -> Option<WelcomeAction> {
        self.hit_test_welcome_action(
            mouse_x,
            mouse_y,
            welcome_x,
            welcome_y,
            welcome_width,
            welcome_height,
        )
    }

    /// 获取欢迎页所有可聚焦项的有序列表（先左列 actions，后右列 recent projects + 更多）
    pub fn welcome_focusable_items(&self) -> Vec<WelcomeAction> {
        let mut items: Vec<WelcomeAction> = Self::welcome_actions()
            .iter()
            .map(|a| a.action.clone())
            .collect();
        for p in self.recent_projects.list() {
            items.push(WelcomeAction::OpenRecentProject(p.path.clone()));
        }
        // 若有"更多..."链接也加入
        if !self.recent_projects.list().is_empty() {
            items.push(WelcomeAction::MoreRecentProjects);
        }
        items
    }

    /// Tab/↓ 推进到下一个可聚焦项
    pub fn welcome_focus_next(&mut self) {
        let items = self.welcome_focusable_items();
        if items.is_empty() {
            self.welcome_focus_action = None;
            return;
        }
        let new = match &self.welcome_focus_action {
            None => items.first().cloned(),
            Some(cur) => {
                let idx = items.iter().position(|a| a == cur);
                match idx {
                    Some(i) => Some(items[(i + 1) % items.len()].clone()),
                    None => items.first().cloned(),
                }
            }
        };
        self.welcome_focus_action = new;
    }

    /// Shift+Tab/↑ 退回到上一个可聚焦项
    pub fn welcome_focus_prev(&mut self) {
        let items = self.welcome_focusable_items();
        if items.is_empty() {
            self.welcome_focus_action = None;
            return;
        }
        let new = match &self.welcome_focus_action {
            None => items.last().cloned(),
            Some(cur) => {
                let idx = items.iter().position(|a| a == cur);
                match idx {
                    Some(i) => {
                        let len = items.len();
                        Some(items[(i + len - 1) % len].clone())
                    }
                    None => items.last().cloned(),
                }
            }
        };
        self.welcome_focus_action = new;
    }
}

fn color_f(
    r: f32,
    g: f32,
    b: f32,
    a: f32,
) -> windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
    windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r, g, b, a }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_welcome_layout_compute() {
        let layout = WelcomeLayout::compute(0.0, 0.0, 1000.0, 600.0, 3);
        assert_eq!(layout.action_item_h, 44.0);
        assert_eq!(layout.project_item_h, 52.0);
        assert!(layout.left_col_x >= 0.0);
        assert!(layout.right_col_x > layout.left_col_x);
        assert!(layout.more_y.is_some());
    }

    #[test]
    fn test_welcome_layout_no_projects() {
        let layout = WelcomeLayout::compute(0.0, 0.0, 1000.0, 600.0, 0);
        assert!(layout.more_y.is_none());
    }

    #[test]
    fn test_empty_state_button_rect_some_when_no_projects() {
        let layout = WelcomeLayout::compute(0.0, 0.0, 1000.0, 600.0, 0);
        assert!(
            layout.empty_state_button_rect.is_some(),
            "空状态按钮 rect 应在 project_count=0 时为 Some"
        );
        let (left, top, right, bottom) = layout.empty_state_button_rect.unwrap();
        // 按钮宽度 120，高度 32
        assert_eq!(right - left, 120.0, "按钮宽度应为 120px");
        assert_eq!(bottom - top, 32.0, "按钮高度应为 32px");
        // 按钮应水平居中于右侧列
        let center_x = layout.right_col_x + layout.right_col_width / 2.0;
        assert!(
            (left + right) / 2.0 - center_x < 0.01,
            "按钮应居中于右侧列"
        );
        // 按钮应在 project_start_y 下方
        assert!(
            top > layout.project_start_y,
            "按钮应在 project_start_y 下方"
        );
    }

    #[test]
    fn test_empty_state_button_rect_none_when_has_projects() {
        let layout = WelcomeLayout::compute(0.0, 0.0, 1000.0, 600.0, 3);
        assert!(
            layout.empty_state_button_rect.is_none(),
            "空状态按钮 rect 应在 project_count>0 时为 None"
        );
    }

    #[test]
    fn test_welcome_action_equality() {
        let a = WelcomeAction::OpenFolder;
        let b = WelcomeAction::OpenFolder;
        let c = WelcomeAction::NewProject;
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_welcome_layout_zero_and_small_size() {
        let layout = WelcomeLayout::compute(0.0, 0.0, 0.0, 0.0, 0);
        assert_eq!(layout.action_item_h, 44.0);
        assert!(layout.more_y.is_none());

        let layout = WelcomeLayout::compute(0.0, 0.0, 100.0, 100.0, 5);
        assert!(layout.more_y.is_some());
        assert!(layout.left_col_width > 0.0);
        assert!(layout.right_col_width > 0.0);
    }

    #[test]
    fn test_welcome_action_variants() {
        let actions = vec![
            WelcomeAction::OpenFolder,
            WelcomeAction::NewProject,
            WelcomeAction::CloneRepo,
            WelcomeAction::OpenRemote,
            WelcomeAction::OpenRecentProject("/p".to_string()),
            WelcomeAction::OpenRecentProject("/p".to_string()),
            WelcomeAction::MoreRecentProjects,
        ];
        assert_eq!(actions[0], actions[0]);
        assert_eq!(actions[4], actions[5]);
        assert_ne!(actions[0], actions[1]);
    }
}

/// 长路径中段省略：测量文本宽度，若超过 max_width 则折叠为 `D:\…\project_name` 形式。
/// 使用 IDWriteTextLayout 测量真实宽度，避免按字符数估算不准。
fn ellipsize_path(
    path: &str,
    format: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
    dwrite: &windows::Win32::Graphics::DirectWrite::IDWriteFactory,
    max_width: f32,
) -> String {
    use windows::Win32::Graphics::DirectWrite::{IDWriteTextLayout, DWRITE_TEXT_METRICS};

    // 测量文本宽度的辅助闭包
    let measure = |text: &str| -> f32 {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let layout: Result<IDWriteTextLayout, _> =
            unsafe { dwrite.CreateTextLayout(&wide, format, max_width * 2.0, 100.0) };
        match layout {
            Ok(layout) => {
                let mut metrics = DWRITE_TEXT_METRICS::default();
                unsafe {
                    let _ = layout.GetMetrics(&mut metrics);
                }
                metrics.width
            }
            Err(_) => text.len() as f32 * 6.0,
        }
    };

    if measure(path) <= max_width {
        return path.to_string();
    }

    // 取末尾两段（如 `\project_name`）作为右半部，前缀用 `…` 折叠
    let path_obj = std::path::Path::new(path);
    let last_seg = path_obj.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let parent_seg = path_obj
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");

    // 获取盘符或根
    let root = path_obj
        .components()
        .next()
        .and_then(|c| {
            use std::path::Component;
            match c {
                Component::Prefix(p) => p.as_os_str().to_str().map(|s| s.to_string()),
                Component::RootDir => Some("\\".to_string()),
                _ => None,
            }
        })
        .unwrap_or_default();

    let candidates = [
        format!("{}\\…\\{}", root, last_seg),
        format!("…\\{}", last_seg),
        if !parent_seg.is_empty() {
            format!("…\\{}\\{}", parent_seg, last_seg)
        } else {
            String::new()
        },
    ];

    for candidate in candidates.iter() {
        if candidate.is_empty() {
            continue;
        }
        if measure(candidate) <= max_width {
            return candidate.clone();
        }
    }

    // 都不行就强制截断
    format!("…\\{}", last_seg)
}
