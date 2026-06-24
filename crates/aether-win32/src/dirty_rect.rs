/// 脏矩形追踪系统
///
/// 优化策略：
/// - 记录需要重绘的矩形区域，避免每帧全量清除+重绘
/// - 合并重叠的脏矩形，减少绘制调用次数
/// - 支持按区域类型标记（编辑器、侧边栏、状态栏等），实现局部重绘

/// 脏矩形区域类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirtyRegionType {
    /// 标题栏
    TitleBar,
    /// 菜单栏
    MenuBar,
    /// 活动栏
    ActivityBar,
    /// 侧边栏
    Sidebar,
    /// 编辑器内容区
    EditorContent,
    /// 标签栏
    TabBar,
    /// 状态栏
    StatusBar,
    /// 右侧面板
    RightPanel,
    /// 底部面板
    BottomPanel,
    /// 查找替换面板
    FindReplace,
    /// 对话框（SSH、克隆等）
    Dialog,
    /// 全窗口（resize、DPI变化等）
    FullWindow,
}

/// 脏矩形
#[derive(Clone, Copy, Debug)]
pub struct DirtyRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub region_type: DirtyRegionType,
}

impl DirtyRect {
    pub fn new(x: f32, y: f32, width: f32, height: f32, region_type: DirtyRegionType) -> Self {
        Self {
            x,
            y,
            width,
            height,
            region_type,
        }
    }

    /// 是否与另一个矩形重叠
    pub fn intersects(&self, other: &DirtyRect) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }

    /// 合并两个矩形
    pub fn merge(&self, other: &DirtyRect) -> DirtyRect {
        let min_x = self.x.min(other.x);
        let min_y = self.y.min(other.y);
        let max_x = (self.x + self.width).max(other.x + other.width);
        let max_y = (self.y + self.height).max(other.y + other.height);
        DirtyRect {
            x: min_x,
            y: min_y,
            width: max_x - min_x,
            height: max_y - min_y,
            region_type: self.region_type,
        }
    }

    /// 检查点是否在矩形内
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }
}

/// 脏矩形追踪器
pub struct DirtyRectTracker {
    /// 当前帧的脏矩形列表
    rects: Vec<DirtyRect>,
    /// 是否有全窗口重绘请求
    full_window_dirty: bool,
    /// 窗口尺寸（用于全窗口重绘）
    window_width: f32,
    window_height: f32,
    /// 合并阈值：当脏矩形数量超过此值时，合并为全窗口重绘
    merge_threshold: usize,
    /// 最大脏矩形数量，超过则降级为全窗口重绘
    max_rects: usize,
}

impl DirtyRectTracker {
    pub fn new(window_width: f32, window_height: f32) -> Self {
        Self {
            rects: Vec::with_capacity(16),
            full_window_dirty: true, // 首帧全量重绘
            window_width,
            window_height,
            merge_threshold: 8,
            max_rects: 16,
        }
    }

    /// 标记整个窗口为脏（resize、DPI变化等）
    pub fn mark_full_window(&mut self) {
        self.full_window_dirty = true;
        self.rects.clear();
    }

    /// 标记指定区域为脏
    pub fn mark_region(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        region_type: DirtyRegionType,
    ) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        // 如果已经有全窗口标记，忽略局部标记
        if self.full_window_dirty {
            return;
        }

        let new_rect = DirtyRect::new(x, y, width, height, region_type);

        // 检查是否与已有矩形重叠，如果重叠则合并
        let mut merged = false;
        for rect in &mut self.rects {
            if rect.region_type == region_type && rect.intersects(&new_rect) {
                *rect = rect.merge(&new_rect);
                merged = true;
                break;
            }
        }

        if !merged {
            self.rects.push(new_rect);
        }

        // 如果脏矩形数量超过合并阈值，触发合并
        if self.rects.len() > self.merge_threshold {
            self.merge_all_rects();
        }

        // 如果脏矩形数量过多，降级为全窗口重绘
        if self.rects.len() > self.max_rects {
            self.mark_full_window();
        }
    }

    /// 标记编辑器中的单行区域为脏
    pub fn mark_editor_line(
        &mut self,
        line_idx: usize,
        line_height: f32,
        editor_x: f32,
        editor_y: f32,
        editor_width: f32,
    ) {
        let line_y = editor_y + line_idx as f32 * line_height;
        self.mark_region(
            editor_x,
            line_y,
            editor_width,
            line_height,
            DirtyRegionType::EditorContent,
        );
    }

    /// 标记光标区域为脏
    pub fn mark_cursor(
        &mut self,
        cursor_x: f32,
        cursor_y: f32,
        cursor_width: f32,
        line_height: f32,
    ) {
        self.mark_region(
            cursor_x,
            cursor_y,
            cursor_width,
            line_height,
            DirtyRegionType::EditorContent,
        );
    }

    /// 标记状态栏为脏
    pub fn mark_status_bar(&mut self, x: f32, y: f32, width: f32, height: f32) {
        self.mark_region(x, y, width, height, DirtyRegionType::StatusBar);
    }

    /// 获取当前脏矩形列表
    pub fn rects(&self) -> &[DirtyRect] {
        &self.rects
    }

    /// 是否需要全窗口重绘
    pub fn is_full_window(&self) -> bool {
        self.full_window_dirty
    }

    /// 是否有脏区域
    pub fn has_dirty(&self) -> bool {
        self.full_window_dirty || !self.rects.is_empty()
    }

    /// 获取全窗口矩形
    pub fn full_window_rect(&self) -> DirtyRect {
        DirtyRect::new(
            0.0,
            0.0,
            self.window_width,
            self.window_height,
            DirtyRegionType::FullWindow,
        )
    }

    /// 检查指定区域类型是否需要重绘
    /// 在全窗口重绘时返回 true，否则检查是否有对应类型的脏矩形
    pub fn is_region_dirty(&self, region_type: DirtyRegionType) -> bool {
        if self.full_window_dirty {
            return true;
        }
        self.rects.iter().any(|r| r.region_type == region_type)
    }

    /// 检查编辑器内容区域是否需要重绘（包括 EditorContent、FullWindow）
    pub fn is_editor_dirty(&self) -> bool {
        if self.full_window_dirty {
            return true;
        }
        self.rects.iter().any(|r| {
            matches!(
                r.region_type,
                DirtyRegionType::EditorContent
                    | DirtyRegionType::TabBar
                    | DirtyRegionType::FindReplace
                    | DirtyRegionType::FullWindow
            )
        })
    }

    /// 检查侧边栏区域是否需要重绘
    pub fn is_sidebar_dirty(&self) -> bool {
        if self.full_window_dirty {
            return true;
        }
        self.rects.iter().any(|r| {
            matches!(
                r.region_type,
                DirtyRegionType::Sidebar
                    | DirtyRegionType::ActivityBar
                    | DirtyRegionType::FullWindow
            )
        })
    }

    /// 检查状态栏是否需要重绘
    pub fn is_status_bar_dirty(&self) -> bool {
        if self.full_window_dirty {
            return true;
        }
        self.rects.iter().any(|r| {
            matches!(
                r.region_type,
                DirtyRegionType::StatusBar | DirtyRegionType::FullWindow
            )
        })
    }

    /// 检查右侧面板是否需要重绘
    pub fn is_right_panel_dirty(&self) -> bool {
        if self.full_window_dirty {
            return true;
        }
        self.rects.iter().any(|r| {
            matches!(
                r.region_type,
                DirtyRegionType::RightPanel | DirtyRegionType::FullWindow
            )
        })
    }

    /// 检查底部面板是否需要重绘
    pub fn is_bottom_panel_dirty(&self) -> bool {
        if self.full_window_dirty {
            return true;
        }
        self.rects.iter().any(|r| {
            matches!(
                r.region_type,
                DirtyRegionType::BottomPanel | DirtyRegionType::FullWindow
            )
        })
    }

    /// 检查标题栏/菜单栏是否需要重绘
    pub fn is_title_bar_dirty(&self) -> bool {
        if self.full_window_dirty {
            return true;
        }
        self.rects.iter().any(|r| {
            matches!(
                r.region_type,
                DirtyRegionType::TitleBar | DirtyRegionType::MenuBar | DirtyRegionType::FullWindow
            )
        })
    }

    /// 更新窗口尺寸
    pub fn resize(&mut self, width: f32, height: f32) {
        self.window_width = width;
        self.window_height = height;
        self.mark_full_window();
    }

    /// 清除所有脏标记（渲染完成后调用）
    pub fn clear(&mut self) {
        self.full_window_dirty = false;
        self.rects.clear();
    }

    /// 合并所有重叠的脏矩形
    pub fn merge_all_rects(&mut self) {
        if self.rects.len() < 2 {
            return;
        }
        let mut merged: Vec<DirtyRect> = Vec::with_capacity(self.rects.len());
        for rect in self.rects.drain(..) {
            let mut found = false;
            for m in &mut merged {
                if m.region_type == rect.region_type && m.intersects(&rect) {
                    *m = m.merge(&rect);
                    found = true;
                    break;
                }
            }
            if !found {
                merged.push(rect);
            }
        }
        self.rects = merged;
    }

    /// 获取需要重绘的矩形数量（用于调试）
    pub fn dirty_count(&self) -> usize {
        if self.full_window_dirty {
            1
        } else {
            self.rects.len()
        }
    }
}

/// 渲染命令类型，用于优化渲染管线
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderCommand {
    /// 只重绘编辑器内容（光标闪烁、输入字符等）
    EditorOnly,
    /// 重绘编辑器 + 状态栏（光标移动、选择变化等）
    EditorAndStatus,
    /// 重绘侧边栏（文件树变化、Git状态变化等）
    SidebarOnly,
    /// 重绘右侧面板（AI面板更新等）
    RightPanelOnly,
    /// 重绘底部面板（终端输出更新等）
    BottomPanelOnly,
    /// 全量重绘（窗口resize、标签切换等）
    FullRedraw,
}

impl RenderCommand {
    /// 根据当前状态推断最优渲染命令
    pub fn infer_from_state(
        cursor_moved: bool,
        selection_changed: bool,
        text_edited: bool,
        scroll_changed: bool,
        sidebar_changed: bool,
        right_panel_changed: bool,
        bottom_panel_changed: bool,
        status_changed: bool,
        dialog_visible: bool,
    ) -> Self {
        if dialog_visible {
            return RenderCommand::FullRedraw;
        }
        if scroll_changed || text_edited {
            return RenderCommand::EditorAndStatus;
        }
        if cursor_moved || selection_changed {
            return RenderCommand::EditorAndStatus;
        }
        if sidebar_changed {
            return RenderCommand::SidebarOnly;
        }
        if right_panel_changed {
            return RenderCommand::RightPanelOnly;
        }
        if bottom_panel_changed {
            return RenderCommand::BottomPanelOnly;
        }
        if status_changed {
            return RenderCommand::EditorAndStatus;
        }
        RenderCommand::FullRedraw
    }
}
