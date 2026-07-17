use super::*;

impl EditorState {
    pub(super) fn render_remote_file_tree_sidebar(
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
    pub(super) fn draw_remote_nodes_recursive(
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

    /// 渲染 SSH 远程管理面板（侧边栏）
    /// 显示已保存的服务器列表、连接状态、添加/编辑/删除/连接操作
    #[allow(clippy::too_many_lines)]
    pub(super) fn render_ssh_manager_sidebar(
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
}
