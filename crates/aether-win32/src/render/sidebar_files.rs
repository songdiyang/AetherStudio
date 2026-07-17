use super::*;

impl EditorState {
    pub(super) fn render_file_tree_sidebar(
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_tree_nodes(
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

    pub(super) fn skip_tree_nodes(&self, tree: &FileTree, parent_idx: u32, current_y: &mut f32) {
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
}
