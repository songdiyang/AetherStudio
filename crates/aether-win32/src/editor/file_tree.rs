use super::*;

impl EditorState {
    /// 命中检测：侧边栏右侧的宽度调整手柄
    /// 仅当侧边栏可见、活动栏已渲染（侧边栏真实存在宽度）时返回 true
    pub fn hit_test_sidebar_resize(&self, mouse_x: f32, mouse_y: f32) -> bool {
        if !self.layout.sidebar_visible {
            return false;
        }
        let sidebar = self.layout.sidebar_region();
        if mouse_y < sidebar.y || mouse_y >= sidebar.y + sidebar.height {
            return false;
        }
        let handle_x = sidebar.x + sidebar.width;
        mouse_x >= handle_x - SIDEBAR_RESIZE_GRAB && mouse_x <= handle_x + SIDEBAR_RESIZE_GRAB
    }
    /// 文件树节点列表的起始 Y 坐标（相对侧边栏顶部），与 render_tree_nodes
    /// 中的 `y + header_h + 6.0 * s + input_offset_y - sidebar_scroll_y` 严格一致。
    ///
    /// 之前三处（render、handle_file_tree_click、update_local_tree_hover、rbutton_down）
    /// 各自硬编码 34.0，未考虑 dpi_scale、sidebar_scroll_y 和 file_tree_input，
    /// 导致高 DPI / 滚动 / 内联输入时点击/悬停位置与渲染节点错位，
    /// 表现为"焦点与选中状态分离"。
    pub fn file_tree_list_start_y(&self) -> f32 {
        let s = self.dpi_scale;
        let header_h = 28.0 * s;
        let base = header_h + 6.0 * s;
        let input_offset_y = if self.file_tree_input.is_some() {
            26.0 * s + 10.0 * s
        } else {
            0.0
        };
        base + input_offset_y - self.sidebar_scroll_y
    }
    /// 开始文件树内联输入（新建文件/文件夹）
    pub fn start_file_tree_input(&mut self, kind: FileTreeInputKind) {
        let default_name = match kind {
            FileTreeInputKind::NewFile => "新建文件.txt",
            FileTreeInputKind::NewFolder => "新建文件夹",
        };
        self.file_tree_input = Some(FileTreeInput {
            kind,
            value: default_name.to_string(),
            caret_visible: true,
            composition: None,
        });
        self.dirty_tracker.mark_region(
            self.layout.sidebar_region().x,
            self.layout.sidebar_region().y,
            self.layout.sidebar_region().width,
            self.layout.sidebar_region().height,
            crate::dirty_rect::DirtyRegionType::Sidebar,
        );
    }
    /// 确认文件树内联输入，执行新建操作
    pub fn confirm_file_tree_input(&mut self) {
        let Some(input) = self.file_tree_input.take() else {
            return;
        };
        let Some(base_path) = self.current_folder.clone() else {
            self.status_message = "请先打开文件夹".to_string();
            self.dirty_tracker.mark_region(
                self.layout.sidebar_region().x,
                self.layout.sidebar_region().y,
                self.layout.sidebar_region().width,
                self.layout.sidebar_region().height,
                crate::dirty_rect::DirtyRegionType::Sidebar,
            );
            return;
        };

        let name = input.value.trim();
        if name.is_empty() {
            self.status_message = "名称不能为空".to_string();
            self.dirty_tracker.mark_region(
                self.layout.sidebar_region().x,
                self.layout.sidebar_region().y,
                self.layout.sidebar_region().width,
                self.layout.sidebar_region().height,
                crate::dirty_rect::DirtyRegionType::Sidebar,
            );
            return;
        }

        // 验证文件名不含 Windows 非法字符
        const INVALID_CHARS: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
        if name.contains(INVALID_CHARS) {
            let bad: String = name.chars().filter(|c| INVALID_CHARS.contains(c)).collect();
            self.status_message = format!("文件名不能包含: {}", bad);
            // 验证失败时保留输入框，让用户修改后重试
            self.file_tree_input = Some(input);
            self.dirty_tracker.mark_region(
                self.layout.sidebar_region().x,
                self.layout.sidebar_region().y,
                self.layout.sidebar_region().width,
                self.layout.sidebar_region().height,
                crate::dirty_rect::DirtyRegionType::Sidebar,
            );
            return;
        }

        let target = base_path.join(name);
        if target.exists() {
            self.status_message = format!("{} 已存在", name);
            self.dirty_tracker.mark_region(
                self.layout.sidebar_region().x,
                self.layout.sidebar_region().y,
                self.layout.sidebar_region().width,
                self.layout.sidebar_region().height,
                crate::dirty_rect::DirtyRegionType::Sidebar,
            );
            return;
        }

        match input.kind {
            FileTreeInputKind::NewFile => {
                if let Err(e) = std::fs::write(&target, "") {
                    self.status_message = format!("创建文件失败: {}", e);
                } else {
                    self.status_message = format!("已创建文件: {}", name);
                    self.refresh_file_tree();
                    self.load_file(target);
                }
            }
            FileTreeInputKind::NewFolder => {
                if let Err(e) = std::fs::create_dir(&target) {
                    self.status_message = format!("创建文件夹失败: {}", e);
                } else {
                    self.status_message = format!("已创建文件夹: {}", name);
                    self.refresh_file_tree();
                }
            }
        }
    }
    /// 取消文件树内联输入
    pub fn cancel_file_tree_input(&mut self) {
        if self.file_tree_input.take().is_some() {
            self.dirty_tracker.mark_region(
                self.layout.sidebar_region().x,
                self.layout.sidebar_region().y,
                self.layout.sidebar_region().width,
                self.layout.sidebar_region().height,
                crate::dirty_rect::DirtyRegionType::Sidebar,
            );
        }
    }
    /// 刷新文件树（重新扫描当前文件夹）
    pub fn refresh_file_tree(&mut self) {
        if let Some(path) = self.current_folder.clone() {
            self.open_folder(path);
        }
    }
    /// 在 Windows 文件资源管理器中打开当前工作区文件夹。
    /// 通过 ShellExecuteW 调用系统 explorer.exe，无纯 Rust 依赖。
    pub fn open_in_file_explorer(&mut self) {
        let Some(folder) = self.current_folder.clone() else {
            self.status_message = "请先打开文件夹".to_string();
            return;
        };
        let path_str = folder.to_string_lossy().to_string();
        let wide: Vec<u16> = path_str.encode_utf16().chain(Some(0)).collect();
        unsafe {
            use windows::Win32::UI::Shell::ShellExecuteW;
            let operation: Vec<u16> = "open\0".encode_utf16().collect();
            let _ = ShellExecuteW(
                None,
                windows::core::PCWSTR(operation.as_ptr()),
                windows::core::PCWSTR(wide.as_ptr()),
                windows::core::PCWSTR::null(),
                None,
                windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
            );
        }
        self.status_message = format!("已在文件资源管理器中打开: {}", path_str);
    }
    /// 复制当前工作区文件夹的绝对路径到剪贴板。
    pub fn copy_folder_path(&mut self) {
        let Some(folder) = self.current_folder.clone() else {
            self.status_message = "请先打开文件夹".to_string();
            return;
        };
        let path_str = folder.to_string_lossy().to_string();
        if Self::set_clipboard_text(&path_str) {
            self.status_message = format!("已复制路径: {}", path_str);
        } else {
            self.status_message = "复制路径失败".to_string();
        }
    }
    /// 执行资源管理器空白区域上下文菜单项对应的动作。
    /// 返回 true 表示动作已处理（调用方负责重绘）。
    pub fn execute_explorer_context_action(
        &mut self,
        item: crate::context_menu::ExplorerContextMenuItem,
    ) -> bool {
        use crate::context_menu::ExplorerContextMenuItem as Item;
        match item {
            Item::NewFile => {
                self.start_file_tree_input(FileTreeInputKind::NewFile);
                true
            }
            Item::NewFolder => {
                self.start_file_tree_input(FileTreeInputKind::NewFolder);
                true
            }
            Item::Refresh => {
                self.refresh_file_tree();
                true
            }
            Item::RevealInExplorer => {
                self.open_in_file_explorer();
                true
            }
            Item::CopyPath => {
                self.copy_folder_path();
                true
            }
            _ => false,
        }
    }
    pub fn handle_sidebar_click(&mut self, mouse_x: f32, mouse_y: f32) -> bool {
        match &self.sidebar_content {
            crate::layout::SidebarContent::FileTree => {
                self.handle_file_tree_click(mouse_x, mouse_y)
            }
            crate::layout::SidebarContent::SourceControlPanel => {
                self.handle_git_panel_click(mouse_x, mouse_y)
            }
            crate::layout::SidebarContent::RemoteFileTree => {
                self.handle_remote_tree_click(mouse_x, mouse_y)
            }
            _ => false,
        }
    }
    pub(super) fn handle_file_tree_click(&mut self, mouse_x: f32, mouse_y: f32) -> bool {
        // 优先检测标题栏按钮点击
        if let Some(rect) = self.file_tree_new_file_btn.clone() {
            if rect.contains(mouse_x, mouse_y) {
                self.start_file_tree_input(FileTreeInputKind::NewFile);
                return true;
            }
        }
        if let Some(rect) = self.file_tree_new_folder_btn.clone() {
            if rect.contains(mouse_x, mouse_y) {
                self.start_file_tree_input(FileTreeInputKind::NewFolder);
                return true;
            }
        }

        // 如果正在内联输入，点击其他区域取消输入
        if self.file_tree_input.is_some() {
            self.cancel_file_tree_input();
            return true;
        }

        let tree = match self.file_tree.as_ref() {
            Some(t) => t,
            None => return false,
        };

        let mut current_y = self.file_tree_list_start_y();
        let sidebar_width = self.layout.sidebar_width;
        let dpi_scale = self.dpi_scale;
        let result = Self::find_tree_click_target(
            tree,
            u32::MAX,
            mouse_x,
            mouse_y,
            sidebar_width,
            dpi_scale,
            &mut current_y,
        );

        if let Some((node_idx, kind, part)) = result {
            match kind {
                FileKind::Directory => {
                    // 读取当前展开状态以决定是否需要懒加载
                    let will_expand = self
                        .file_tree
                        .as_ref()
                        .and_then(|t| t.get_node(node_idx))
                        .map(|n| !n.is_expanded)
                        .unwrap_or(false);
                    // 展开前确保子节点已加载
                    if will_expand {
                        let _ = self.ensure_node_loaded(node_idx);
                    }
                    if let Some(tree) = self.file_tree.as_mut() {
                        if let Some(node) = tree.get_node_mut(node_idx) {
                            node.is_expanded = !node.is_expanded;
                        }
                    }
                    // 点击名称/图标区域时同时选中该目录
                    if part == FileTreeClickPart::Label {
                        self.selected_file_node = Some(node_idx);
                    }
                    self.emit_event(crate::events::EditorEvent::SidebarChanged);
                    return true;
                }
                FileKind::File => {
                    // 仅点击文件名称/图标区域才打开文件
                    if part == FileTreeClickPart::Label {
                        self.selected_file_node = Some(node_idx);
                        self.emit_event(crate::events::EditorEvent::SidebarChanged);
                        if let Some(path) = self.get_node_path(node_idx) {
                            // 检查该文件是否已在某个标签页中打开
                            // REQ-P1-09: 活动标签页的 file_path 在 self.content 中
                            let active_path = self.content.file_path.clone();
                            let active_idx = self.active_tab;
                            if let Some(existing_tab) =
                                self.tabs.iter().enumerate().position(|(i, tab)| {
                                    if i == active_idx {
                                        active_path.as_ref() == Some(&path)
                                    } else {
                                        tab.file_path() == Some(&path)
                                    }
                                })
                            {
                                // 切换到已打开的标签页
                                self.switch_tab(existing_tab);
                            } else {
                                self.load_file(path);
                            }
                            return true;
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }
    /// 更新文件树悬停状态，返回是否需要重绘
    pub fn update_file_tree_hover(&mut self, mouse_x: f32, mouse_y: f32) -> bool {
        match &self.sidebar_content {
            crate::layout::SidebarContent::FileTree => {
                self.update_local_tree_hover(mouse_x, mouse_y)
            }
            crate::layout::SidebarContent::RemoteFileTree => self.update_remote_tree_hover(mouse_y),
            _ => {
                let old = self.hover_file_node.take();
                old.is_some()
            }
        }
    }
    pub(super) fn update_local_tree_hover(&mut self, mouse_x: f32, mouse_y: f32) -> bool {
        let tree = match self.file_tree.as_ref() {
            Some(t) => t,
            None => {
                let old = self.hover_file_node.take();
                return old.is_some();
            }
        };

        let mut current_y = self.file_tree_list_start_y();
        let sidebar_width = self.layout.sidebar_width;
        let dpi_scale = self.dpi_scale;
        let result = Self::find_tree_click_target(
            tree,
            u32::MAX,
            mouse_x,
            mouse_y,
            sidebar_width,
            dpi_scale,
            &mut current_y,
        );

        let new_hover = result.map(|(idx, _, _)| idx);
        let changed = self.hover_file_node != new_hover;
        self.hover_file_node = new_hover;
        changed
    }
    /// 根据当前打开的文件路径同步文件树选中状态
    pub fn sync_file_tree_selection(&mut self) {
        if let Some(ref path) = self.content.file_path {
            if let Some(ref folder) = self.current_folder {
                if let Some(ref tree) = self.file_tree {
                    // 尝试找到匹配当前文件路径的节点
                    if let Some(matched) = Self::find_node_by_path(tree, path, folder) {
                        self.selected_file_node = Some(matched);
                    }
                }
            }
        }
    }
    pub(super) fn find_node_by_path(tree: &FileTree, target: &Path, base: &Path) -> Option<u32> {
        // 获取相对于 base 的路径
        let rel_path = target.strip_prefix(base).ok()?;
        let components: Vec<_> = rel_path.components().collect();
        if components.is_empty() {
            return None;
        }

        let mut current_idx = tree.first_root_node()?;
        for (i, comp) in components.iter().enumerate() {
            let comp_name = comp.as_os_str().to_string_lossy();
            let mut found = None;
            let mut child_idx = tree
                .get_node(current_idx)
                .map(|n| n.first_child)
                .filter(|&c| c != u32::MAX);

            while let Some(idx) = child_idx {
                if let Some(node) = tree.get_node(idx) {
                    let name = tree.get_name(node);
                    if name == comp_name.as_ref() {
                        found = Some(idx);
                        break;
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

            if let Some(idx) = found {
                if i == components.len() - 1 {
                    return Some(idx);
                }
                current_idx = idx;
            } else {
                return None;
            }
        }
        None
    }
    pub(super) fn get_node_path(&self, node_idx: u32) -> Option<PathBuf> {
        let folder = self.current_folder.as_ref()?;
        let tree = self.file_tree.as_ref()?;
        let mut path_parts = Vec::new();

        let mut current_idx = Some(node_idx);
        while let Some(idx) = current_idx {
            let node = tree.get_node(idx)?;
            let name = tree.get_name(node).to_string();
            path_parts.push(name);

            if node.parent_idx == u32::MAX {
                break;
            }
            current_idx = Some(node.parent_idx);
        }

        path_parts.reverse();
        let mut path = folder.clone();
        for part in path_parts {
            path = path.join(part);
        }

        Some(path)
    }
    /// 懒加载：确保目录节点的子项已扫描
    /// 若节点未加载（is_loaded=false），扫描其磁盘子目录一层并标记为已加载
    /// 返回 true 表示本次实际执行了加载
    pub(super) fn ensure_node_loaded(&mut self, node_idx: u32) -> bool {
        // 先读取需要的信息，避免跨方法借用
        let (already_loaded, dir_path, depth) = {
            let tree = match self.file_tree.as_ref() {
                Some(t) => t,
                None => return false,
            };
            let node = match tree.get_node(node_idx) {
                Some(n) => n,
                None => return false,
            };
            if node.kind != FileKind::Directory || node.is_loaded {
                return false;
            }
            let path = match self.get_node_path(node_idx) {
                Some(p) => p,
                None => return false,
            };
            (node.is_loaded, path, node.depth)
        };

        let _ = already_loaded; // 已通过上面的判断保证为 false
        let child_depth = depth.saturating_add(1);
        if let Some(tree) = self.file_tree.as_mut() {
            let _ = populate_children_one_level(tree, &dir_path, node_idx, child_depth);
            if let Some(node) = tree.get_node_mut(node_idx) {
                node.is_loaded = true;
            }
            return true;
        }
        false
    }
    /// 渲染前预扫描：加载所有 is_expanded 但未加载的目录节点
    /// 分批处理，避免单帧加载过多目录导致卡顿（每帧最多加载 8 个目录）
    pub(crate) fn preload_expanded_dirs(&mut self) {
        const MAX_LOAD_PER_FRAME: usize = 8;
        let mut to_load: Vec<u32> = Vec::new();

        // 收集需要加载的节点（已展开但未加载的目录）
        if let Some(tree) = self.file_tree.as_ref() {
            for (i, node) in tree.nodes_iter().enumerate() {
                if node.kind == FileKind::Directory && node.is_expanded && !node.is_loaded {
                    to_load.push(i as u32);
                    if to_load.len() >= MAX_LOAD_PER_FRAME {
                        break;
                    }
                }
            }
        }

        for idx in to_load {
            let _ = self.ensure_node_loaded(idx);
        }
    }
    pub(crate) fn find_tree_click_target(
        tree: &FileTree,
        parent_idx: u32,
        mouse_x: f32,
        mouse_y: f32,
        sidebar_width: f32,
        dpi_scale: f32,
        current_y: &mut f32,
    ) -> Option<(u32, FileKind, FileTreeClickPart)> {
        let node_height = 16.0 * dpi_scale;
        let base_x = 10.0;
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

                // 节点按 y 递增排列，鼠标在当前节点上方则后续不可能命中
                if mouse_y < *current_y {
                    return None;
                }

                if mouse_y >= *current_y && mouse_y < *current_y + node_height {
                    // 计算该节点在渲染时的横向范围，与 render_tree_nodes 保持一致
                    let indent = if node.parent_idx == u32::MAX {
                        0.0
                    } else {
                        node.depth as f32 * 16.0 * dpi_scale
                    };
                    let item_left = base_x + indent;
                    let item_right = sidebar_width - 10.0 * dpi_scale;

                    // x 超出节点有效区域视为未命中（避免点击滚动条或空白处误触发）
                    if mouse_x < item_left - 4.0 * dpi_scale || mouse_x > item_right {
                        return None;
                    }

                    // 判断点击的是目录展开箭头还是名称/图标区域
                    let part = if node.kind == FileKind::Directory {
                        // 箭头区域近似为节点左侧约 20px（"▶ " / "▼ "）
                        let arrow_right = item_left + 20.0 * dpi_scale;
                        if mouse_x < arrow_right {
                            FileTreeClickPart::Arrow
                        } else {
                            FileTreeClickPart::Label
                        }
                    } else {
                        FileTreeClickPart::Label
                    };

                    return Some((idx, node.kind, part));
                }
                *current_y += node_height;

                // 如果目录展开，递归查找子节点
                if node.kind == FileKind::Directory && node.is_expanded {
                    if let Some(result) = Self::find_tree_click_target(
                        tree,
                        idx,
                        mouse_x,
                        mouse_y,
                        sidebar_width,
                        dpi_scale,
                        current_y,
                    ) {
                        return Some(result);
                    }
                }

                child_idx = next_sibling;
            } else {
                break;
            }
        }
        None
    }
    pub(super) fn format_file_tree(&self, tree: &FileTree) -> String {
        let mut lines = Vec::new();
        let max_files = 200;
        for (idx, node) in tree.nodes_iter().enumerate() {
            if idx >= max_files {
                lines.push("...".to_string());
                break;
            }
            if node.kind != FileKind::File {
                continue;
            }
            if let Some(path) = file_tree_node_path(tree, idx as u32) {
                lines.push(path);
            }
        }
        if lines.is_empty() {
            "(空)".to_string()
        } else {
            lines.join("\n")
        }
    }
}
