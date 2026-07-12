/// 编辑器布局区域定义
#[derive(Clone, Debug)]
pub struct Region {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Region {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }
}

/// 活动栏视图类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityBarView {
    Explorer,
    SourceControl,
    Terminal,
    RemoteManager,
    AiAssistant,
}

impl ActivityBarView {
    pub fn label(&self) -> &'static str {
        match self {
            ActivityBarView::Explorer => "资源管理器",
            ActivityBarView::SourceControl => "源代码管理",
            ActivityBarView::Terminal => "终端",
            ActivityBarView::RemoteManager => "SSH 远程管理",
            ActivityBarView::AiAssistant => "AI 助手",
        }
    }

    pub fn icon(&self) -> crate::icons::IconKind {
        match self {
            ActivityBarView::Explorer => crate::icons::IconKind::Folder,
            ActivityBarView::SourceControl => crate::icons::IconKind::GitBranch,
            ActivityBarView::Terminal => crate::icons::IconKind::Terminal,
            ActivityBarView::RemoteManager => crate::icons::IconKind::Ssh,
            ActivityBarView::AiAssistant => crate::icons::IconKind::Bot,
        }
    }

    /// 稳定字符串标识，用于持久化排序
    pub fn key(&self) -> &'static str {
        match self {
            ActivityBarView::Explorer => "explorer",
            ActivityBarView::SourceControl => "sourceControl",
            ActivityBarView::Terminal => "terminal",
            ActivityBarView::RemoteManager => "remoteManager",
            ActivityBarView::AiAssistant => "aiAssistant",
        }
    }

    /// 默认顺序
    pub fn default_order() -> Vec<ActivityBarView> {
        vec![
            ActivityBarView::Explorer,
            ActivityBarView::SourceControl,
            ActivityBarView::RemoteManager,
        ]
    }

    /// 从字符串键解析
    pub fn from_key(key: &str) -> Option<ActivityBarView> {
        match key {
            "explorer" => Some(ActivityBarView::Explorer),
            "sourceControl" => Some(ActivityBarView::SourceControl),
            "terminal" => None, // 终端已迁移到标题栏按钮
            "remoteManager" => Some(ActivityBarView::RemoteManager),
            "openTabs" => Some(ActivityBarView::RemoteManager), // 兼容旧设置
            "aiAssistant" => None,                              // AI助手已迁移到标题栏按钮
            _ => None,
        }
    }
}

/// 侧边栏内容类型
#[derive(Clone, Debug, PartialEq)]
pub enum SidebarContent {
    FileTree,
    SourceControlPanel,
    TerminalPanel,
    RemoteManagerPanel,
    RemoteFileTree,
    AiAssistantPanel,
}

impl SidebarContent {
    pub fn from_view(view: ActivityBarView) -> Self {
        match view {
            ActivityBarView::Explorer => SidebarContent::FileTree,
            ActivityBarView::SourceControl => SidebarContent::SourceControlPanel,
            ActivityBarView::Terminal => SidebarContent::TerminalPanel,
            ActivityBarView::RemoteManager => SidebarContent::RemoteManagerPanel,
            ActivityBarView::AiAssistant => SidebarContent::AiAssistantPanel,
        }
    }

    pub fn is_ai_assistant(&self) -> bool {
        matches!(self, SidebarContent::AiAssistantPanel)
    }
}

/// 布局常量
pub const TITLE_BAR_HEIGHT: f32 = 32.0;
pub const MENU_BAR_HEIGHT: f32 = 0.0; // 菜单栏合并到标题栏，高度为0
pub const ACTIVITY_BAR_WIDTH: f32 = 48.0;
pub const SIDEBAR_WIDTH: f32 = 250.0;
pub const STATUS_BAR_HEIGHT: f32 = 22.0;
pub const TAB_BAR_HEIGHT: f32 = 30.0;
pub const MIN_SIDEBAR_WIDTH: f32 = 150.0;
pub const MAX_SIDEBAR_WIDTH: f32 = 500.0;
/// 底部面板最小高度
pub const MIN_BOTTOM_PANEL_HEIGHT: f32 = 100.0;
/// 右侧面板最小宽度
pub const MIN_RIGHT_PANEL_WIDTH: f32 = 150.0;

/// 布局管理器 - 计算和管理所有 UI 区域的几何布局
#[derive(Clone, Debug)]
pub struct LayoutManager {
    pub window_width: f32,
    pub window_height: f32,
    // 各区域尺寸
    pub title_bar_height: f32,
    pub menu_bar_height: f32,
    pub activity_bar_width: f32,
    pub sidebar_width: f32,
    pub right_panel_width: f32,
    pub bottom_panel_height: f32,
    pub status_bar_height: f32,
    // 可见性
    pub title_bar_visible: bool,
    pub menu_bar_visible: bool,
    pub activity_bar_visible: bool,
    pub sidebar_visible: bool,
    pub right_panel_visible: bool,
    pub bottom_panel_visible: bool,
    pub status_bar_visible: bool,
    pub right_panel_resizing: bool,
    pub bottom_panel_resizing: bool,
}

impl LayoutManager {
    pub fn new(window_width: f32, window_height: f32) -> Self {
        Self {
            window_width,
            window_height,
            title_bar_height: TITLE_BAR_HEIGHT,
            menu_bar_height: MENU_BAR_HEIGHT,
            activity_bar_width: ACTIVITY_BAR_WIDTH,
            sidebar_width: SIDEBAR_WIDTH,
            right_panel_width: 0.0,
            bottom_panel_height: 0.0,
            status_bar_height: STATUS_BAR_HEIGHT,
            title_bar_visible: true,
            menu_bar_visible: true,
            activity_bar_visible: true,
            sidebar_visible: true,
            right_panel_visible: false,
            bottom_panel_visible: false,
            status_bar_visible: true,
            right_panel_resizing: false,
            bottom_panel_resizing: false,
        }
    }

    /// REQ-P2-07: 应用 DPI 缩放到所有布局常量
    /// 在窗口初始化和 DPI 变化时调用，确保高 DPI 显示器上 UI 元素尺寸正确
    pub fn apply_dpi_scale(&mut self, scale: f32) {
        self.title_bar_height = TITLE_BAR_HEIGHT * scale;
        self.menu_bar_height = MENU_BAR_HEIGHT * scale;
        self.activity_bar_width = ACTIVITY_BAR_WIDTH * scale;
        // 侧边栏宽度保持用户可调，但初始值按 DPI 缩放
        self.sidebar_width = SIDEBAR_WIDTH * scale;
        self.status_bar_height = STATUS_BAR_HEIGHT * scale;
        // 右侧/底部面板高度仅在可见时缩放
        if self.right_panel_width > 0.0 {
            self.right_panel_width = 300.0 * scale;
        }
        if self.bottom_panel_height > 0.0 {
            self.bottom_panel_height = 200.0 * scale;
        }
    }

    /// 计算标题栏区域
    pub fn title_bar_region(&self) -> Region {
        if !self.title_bar_visible {
            return Region::new(0.0, 0.0, self.window_width, 0.0);
        }
        Region::new(0.0, 0.0, self.window_width, self.title_bar_height)
    }

    /// 计算菜单栏区域
    pub fn menu_bar_region(&self) -> Region {
        if !self.menu_bar_visible {
            return Region::new(0.0, self.title_bar_height, self.window_width, 0.0);
        }
        Region::new(
            0.0,
            self.title_bar_height,
            self.window_width,
            self.menu_bar_height,
        )
    }

    /// 计算活动栏区域
    pub fn activity_bar_region(&self) -> Region {
        if !self.activity_bar_visible {
            return Region::new(0.0, self.top_offset(), 0.0, self.content_height());
        }
        Region::new(
            0.0,
            self.top_offset(),
            self.activity_bar_width,
            self.content_height(),
        )
    }

    /// 计算侧边栏区域
    pub fn sidebar_region(&self) -> Region {
        // UI-L06: 活动栏隐藏时侧边栏应从 x=0 开始，而非固定偏移 48px
        let x = if self.activity_bar_visible {
            self.activity_bar_width
        } else {
            0.0
        };
        if !self.sidebar_visible {
            return Region::new(x, self.top_offset(), 0.0, self.content_height());
        }
        Region::new(
            x,
            self.top_offset(),
            self.sidebar_width,
            self.content_height(),
        )
    }

    /// 计算编辑器区域（包含标签栏和编辑器内容）
    pub fn editor_region(&self) -> Region {
        let x = if self.activity_bar_visible {
            self.activity_bar_width
        } else {
            0.0
        } + if self.sidebar_visible {
            self.sidebar_width
        } else {
            0.0
        };
        let right = if self.right_panel_visible {
            self.right_panel_width
        } else {
            0.0
        };
        let width = (self.window_width - x - right).max(0.0);
        Region::new(x, self.top_offset(), width, self.content_height())
    }

    /// 计算标签栏区域
    pub fn tab_bar_region(&self, show_tab_bar: bool) -> Region {
        let editor = self.editor_region();
        let height = if show_tab_bar { TAB_BAR_HEIGHT } else { 0.0 };
        Region::new(editor.x, editor.y, editor.width, height)
    }

    /// 计算编辑器内容区域（排除标签栏）
    pub fn editor_content_region(&self, show_tab_bar: bool) -> Region {
        let editor = self.editor_region();
        let tab_height = if show_tab_bar { TAB_BAR_HEIGHT } else { 0.0 };
        let height = (editor.height - tab_height).max(0.0);
        Region::new(editor.x, editor.y + tab_height, editor.width, height)
    }

    /// 计算右侧面板区域
    pub fn right_panel_region(&self) -> Region {
        if !self.right_panel_visible {
            return Region::new(
                self.window_width,
                self.top_offset(),
                0.0,
                self.content_height(),
            );
        }
        Region::new(
            self.window_width - self.right_panel_width,
            self.top_offset(),
            self.right_panel_width,
            self.content_height(),
        )
    }

    /// 计算底部面板区域
    /// 底部面板横跨整个窗口底部（覆盖左右侧边栏下方的空间），与状态栏一致
    pub fn bottom_panel_region(&self) -> Region {
        let y = self.window_height - self.status_bar_height - self.bottom_panel_height;
        if !self.bottom_panel_visible {
            return Region::new(
                0.0,
                self.window_height - self.status_bar_height,
                self.window_width,
                0.0,
            );
        }
        Region::new(0.0, y, self.window_width, self.bottom_panel_height)
    }

    /// 计算状态栏区域
    pub fn status_bar_region(&self) -> Region {
        if !self.status_bar_visible {
            return Region::new(0.0, self.window_height, self.window_width, 0.0);
        }
        Region::new(
            0.0,
            self.window_height - self.status_bar_height,
            self.window_width,
            self.status_bar_height,
        )
    }

    /// 顶部偏移（标题栏 + 菜单栏）
    pub fn top_offset(&self) -> f32 {
        let mut offset = 0.0;
        if self.title_bar_visible {
            offset += self.title_bar_height;
        }
        if self.menu_bar_visible {
            offset += self.menu_bar_height;
        }
        offset
    }

    /// 内容区域高度（排除标题栏、菜单栏、状态栏和底部面板）
    fn content_height(&self) -> f32 {
        let mut height = self.window_height;
        if self.title_bar_visible {
            height -= self.title_bar_height;
        }
        if self.menu_bar_visible {
            height -= self.menu_bar_height;
        }
        if self.status_bar_visible {
            height -= self.status_bar_height;
        }
        // 无论底部面板是否可见，都预留空间以防止侧边栏/活动栏区域延伸到底部面板
        height -= self.bottom_panel_height;
        // 确保内容区域至少有 0 像素的高度
        height.max(0.0)
    }

    /// 调整侧边栏宽度
    pub fn resize_sidebar(&mut self, delta: f32) {
        let new_width = (self.sidebar_width + delta).clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        self.sidebar_width = new_width;
    }

    /// 调整右侧面板宽度
    /// clamp: 最小 MIN_RIGHT_PANEL_WIDTH，最大 window_width * 0.8
    pub fn resize_right_panel(&mut self, delta: f32) {
        let new_width =
            (self.right_panel_width + delta).clamp(MIN_RIGHT_PANEL_WIDTH, self.window_width * 0.8);
        self.right_panel_width = new_width;
    }

    /// 调整底部面板高度
    /// clamp: 最小 MIN_BOTTOM_PANEL_HEIGHT，最大 window_height * 0.8
    pub fn resize_bottom_panel(&mut self, delta: f32) {
        let new_height = (self.bottom_panel_height + delta)
            .clamp(MIN_BOTTOM_PANEL_HEIGHT, self.window_height * 0.8);
        self.bottom_panel_height = new_height;
    }

    /// 切换侧边栏可见性
    pub fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
    }

    /// 切换活动栏可见性
    pub fn toggle_activity_bar(&mut self) {
        self.activity_bar_visible = !self.activity_bar_visible;
    }

    /// 切换状态栏可见性
    pub fn toggle_status_bar(&mut self) {
        self.status_bar_visible = !self.status_bar_visible;
    }

    /// 更新窗口大小
    pub fn resize_window(&mut self, width: f32, height: f32) {
        self.window_width = width;
        self.window_height = height;
    }

    /// 切换底部面板可见性
    /// REQ-P2-09: 合并原 toggle_bottom_panel 与 toggle_terminal_panel（两者实现完全相同）
    pub fn toggle_bottom_panel(&mut self) {
        self.bottom_panel_visible = !self.bottom_panel_visible;
        if self.bottom_panel_visible {
            self.bottom_panel_height = 200.0;
        } else {
            self.bottom_panel_height = 0.0;
        }
    }

    /// 切换右侧面板可见性
    pub fn toggle_right_panel(&mut self) {
        self.right_panel_visible = !self.right_panel_visible;
        if self.right_panel_visible {
            self.right_panel_width = 300.0;
        } else {
            self.right_panel_width = 0.0;
        }
    }

    /// 切换终端面板可见性。
    /// 终端渲染在底部面板区域，不覆盖主编辑器内容区域。
    /// REQ-P2-09: 合并到 toggle_bottom_panel，保留别名以兼容调用方
    pub fn toggle_terminal_panel(&mut self) {
        self.toggle_bottom_panel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_contains() {
        let r = Region::new(10.0, 20.0, 100.0, 50.0);
        assert!(r.contains(10.0, 20.0));
        assert!(r.contains(109.9, 69.9));
        assert!(!r.contains(110.0, 30.0));
        assert!(!r.contains(50.0, 70.0));
        assert!(!r.contains(9.9, 30.0));
    }

    #[test]
    fn test_region_right_and_bottom() {
        let r = Region::new(5.0, 5.0, 10.0, 20.0);
        assert_eq!(r.right(), 15.0);
        assert_eq!(r.bottom(), 25.0);
    }

    #[test]
    fn test_activity_bar_view_label_icon_key() {
        assert_eq!(ActivityBarView::Explorer.label(), "资源管理器");
        assert_eq!(
            ActivityBarView::Terminal.icon(),
            crate::icons::IconKind::Terminal
        );
        assert_eq!(ActivityBarView::AiAssistant.key(), "aiAssistant");
    }

    #[test]
    fn test_activity_bar_view_default_order() {
        let order = ActivityBarView::default_order();
        assert_eq!(
            order,
            vec![
                ActivityBarView::Explorer,
                ActivityBarView::SourceControl,
                ActivityBarView::RemoteManager,
            ]
        );
    }

    #[test]
    fn test_activity_bar_view_from_key() {
        assert_eq!(
            ActivityBarView::from_key("explorer"),
            Some(ActivityBarView::Explorer)
        );
        assert_eq!(
            ActivityBarView::from_key("sourceControl"),
            Some(ActivityBarView::SourceControl)
        );
        assert_eq!(
            ActivityBarView::from_key("remoteManager"),
            Some(ActivityBarView::RemoteManager)
        );
        assert_eq!(
            ActivityBarView::from_key("openTabs"),
            Some(ActivityBarView::RemoteManager)
        );
        assert_eq!(ActivityBarView::from_key("terminal"), None);
        assert_eq!(ActivityBarView::from_key("aiAssistant"), None);
        assert_eq!(ActivityBarView::from_key("unknown"), None);
    }

    #[test]
    fn test_sidebar_content_from_view() {
        assert_eq!(
            SidebarContent::from_view(ActivityBarView::Explorer),
            SidebarContent::FileTree
        );
        assert_eq!(
            SidebarContent::from_view(ActivityBarView::SourceControl),
            SidebarContent::SourceControlPanel
        );
        assert_eq!(
            SidebarContent::from_view(ActivityBarView::Terminal),
            SidebarContent::TerminalPanel
        );
        assert_eq!(
            SidebarContent::from_view(ActivityBarView::RemoteManager),
            SidebarContent::RemoteManagerPanel
        );
        assert_eq!(
            SidebarContent::from_view(ActivityBarView::AiAssistant),
            SidebarContent::AiAssistantPanel
        );
    }

    #[test]
    fn test_sidebar_content_is_ai_assistant() {
        assert!(SidebarContent::AiAssistantPanel.is_ai_assistant());
        assert!(!SidebarContent::FileTree.is_ai_assistant());
    }

    #[test]
    fn test_layout_manager_default_regions() {
        let layout = LayoutManager::new(1280.0, 800.0);
        let title = layout.title_bar_region();
        assert_eq!(title.x, 0.0);
        assert_eq!(title.height, TITLE_BAR_HEIGHT);

        let activity = layout.activity_bar_region();
        assert_eq!(activity.x, 0.0);
        assert_eq!(activity.width, ACTIVITY_BAR_WIDTH);

        let sidebar = layout.sidebar_region();
        assert_eq!(sidebar.x, ACTIVITY_BAR_WIDTH);
        assert_eq!(sidebar.width, SIDEBAR_WIDTH);

        let editor = layout.editor_region();
        assert_eq!(editor.x, ACTIVITY_BAR_WIDTH + SIDEBAR_WIDTH);
        assert!(editor.width > 0.0);

        let status = layout.status_bar_region();
        assert_eq!(status.y, 800.0 - STATUS_BAR_HEIGHT);
        assert_eq!(status.height, STATUS_BAR_HEIGHT);
    }

    #[test]
    fn test_layout_manager_hidden_activity_bar() {
        let mut layout = LayoutManager::new(1280.0, 800.0);
        layout.toggle_activity_bar();
        assert!(!layout.activity_bar_visible);

        let activity = layout.activity_bar_region();
        assert_eq!(activity.width, 0.0);

        let sidebar = layout.sidebar_region();
        assert_eq!(sidebar.x, 0.0);

        let editor = layout.editor_region();
        assert_eq!(editor.x, SIDEBAR_WIDTH);
    }

    #[test]
    fn test_layout_manager_hidden_sidebar() {
        let mut layout = LayoutManager::new(1280.0, 800.0);
        layout.toggle_sidebar();
        assert!(!layout.sidebar_visible);

        let sidebar = layout.sidebar_region();
        assert_eq!(sidebar.width, 0.0);

        let editor = layout.editor_region();
        assert_eq!(editor.x, ACTIVITY_BAR_WIDTH);
    }

    #[test]
    fn test_layout_manager_right_and_bottom_panels() {
        let mut layout = LayoutManager::new(1280.0, 800.0);
        layout.toggle_right_panel();
        let right = layout.right_panel_region();
        assert_eq!(right.width, 300.0);
        assert_eq!(right.x, 1280.0 - 300.0);

        layout.toggle_bottom_panel();
        let bottom = layout.bottom_panel_region();
        assert_eq!(bottom.height, 200.0);
        assert_eq!(bottom.y, 800.0 - STATUS_BAR_HEIGHT - 200.0);

        let editor = layout.editor_region();
        assert_eq!(
            editor.width,
            1280.0 - ACTIVITY_BAR_WIDTH - SIDEBAR_WIDTH - 300.0
        );
    }

    #[test]
    fn test_layout_manager_tab_bar_and_content() {
        let layout = LayoutManager::new(1280.0, 800.0);
        let tab_bar = layout.tab_bar_region(true);
        assert_eq!(tab_bar.height, TAB_BAR_HEIGHT);

        let content = layout.editor_content_region(true);
        assert_eq!(
            content.height,
            layout.editor_region().height - TAB_BAR_HEIGHT
        );
        assert_eq!(content.y, layout.editor_region().y + TAB_BAR_HEIGHT);
    }

    #[test]
    fn test_layout_manager_resize_sidebar() {
        let mut layout = LayoutManager::new(1280.0, 800.0);
        layout.resize_sidebar(100.0);
        assert_eq!(layout.sidebar_width, SIDEBAR_WIDTH + 100.0);

        layout.resize_sidebar(1000.0);
        assert_eq!(layout.sidebar_width, MAX_SIDEBAR_WIDTH);

        layout.resize_sidebar(-1000.0);
        assert_eq!(layout.sidebar_width, MIN_SIDEBAR_WIDTH);
    }

    #[test]
    fn test_layout_manager_resize_right_panel_clamp() {
        // window_width = 1280.0 → 上限 = 1024.0
        let mut layout = LayoutManager::new(1280.0, 800.0);
        layout.right_panel_width = 300.0;

        // 正常增量
        layout.resize_right_panel(100.0);
        assert_eq!(layout.right_panel_width, 400.0);

        // 超过上限 → clamp 到 window_width * 0.8 = 1024.0
        layout.resize_right_panel(1000.0);
        assert_eq!(layout.right_panel_width, 1280.0 * 0.8);

        // 低于下限 → clamp 到 MIN_RIGHT_PANEL_WIDTH
        layout.resize_right_panel(-10000.0);
        assert_eq!(layout.right_panel_width, MIN_RIGHT_PANEL_WIDTH);

        // 关键：到达最小值后继续拖拽不应使面板更小
        let min_val = layout.right_panel_width;
        layout.resize_right_panel(-50.0);
        assert_eq!(layout.right_panel_width, min_val);
        layout.resize_right_panel(-50.0);
        assert_eq!(layout.right_panel_width, min_val);
    }

    #[test]
    fn test_layout_manager_resize_bottom_panel_clamp() {
        // window_height = 800.0 → 上限 = 640.0
        let mut layout = LayoutManager::new(1280.0, 800.0);
        layout.bottom_panel_height = 200.0;

        // 正常增量
        layout.resize_bottom_panel(100.0);
        assert_eq!(layout.bottom_panel_height, 300.0);

        // 超过上限 → clamp 到 window_height * 0.8 = 640.0
        layout.resize_bottom_panel(1000.0);
        assert_eq!(layout.bottom_panel_height, 800.0 * 0.8);

        // 低于下限 → clamp 到 MIN_BOTTOM_PANEL_HEIGHT
        layout.resize_bottom_panel(-10000.0);
        assert_eq!(layout.bottom_panel_height, MIN_BOTTOM_PANEL_HEIGHT);

        // 关键：到达最小值后继续拖拽不应使面板更小
        let min_val = layout.bottom_panel_height;
        layout.resize_bottom_panel(-50.0);
        assert_eq!(layout.bottom_panel_height, min_val);
        layout.resize_bottom_panel(-50.0);
        assert_eq!(layout.bottom_panel_height, min_val);
    }

    #[test]
    fn test_layout_manager_resize_window() {
        let mut layout = LayoutManager::new(1280.0, 800.0);
        layout.resize_window(1920.0, 1080.0);
        assert_eq!(layout.window_width, 1920.0);
        assert_eq!(layout.window_height, 1080.0);
        assert_eq!(layout.title_bar_region().width, 1920.0);
    }

    #[test]
    fn test_layout_manager_toggle_status_bar() {
        let mut layout = LayoutManager::new(1280.0, 800.0);
        layout.toggle_status_bar();
        assert!(!layout.status_bar_visible);
        let status = layout.status_bar_region();
        assert_eq!(status.height, 0.0);
        assert_eq!(status.y, 800.0);
    }

    #[test]
    fn test_layout_manager_top_offset_and_content_height() {
        let mut layout = LayoutManager::new(800.0, 600.0);
        assert_eq!(layout.top_offset(), TITLE_BAR_HEIGHT + MENU_BAR_HEIGHT);
        assert_eq!(
            layout.content_height(),
            600.0 - TITLE_BAR_HEIGHT - STATUS_BAR_HEIGHT
        );

        layout.toggle_status_bar();
        assert_eq!(layout.content_height(), 600.0 - TITLE_BAR_HEIGHT);

        layout.toggle_bottom_panel();
        assert_eq!(layout.content_height(), 600.0 - TITLE_BAR_HEIGHT - 200.0);
    }
}
