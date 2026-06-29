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

    pub fn icon(&self) -> &'static str {
        match self {
            ActivityBarView::Explorer => "📁",
            ActivityBarView::SourceControl => "🌿",
            ActivityBarView::Terminal => "⌨",
            ActivityBarView::RemoteManager => "🔌",
            ActivityBarView::AiAssistant => "🤖",
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
    pub fn tab_bar_region(&self, has_multiple_tabs: bool) -> Region {
        let editor = self.editor_region();
        let height = if has_multiple_tabs {
            TAB_BAR_HEIGHT
        } else {
            0.0
        };
        Region::new(editor.x, editor.y, editor.width, height)
    }

    /// 计算编辑器内容区域（排除标签栏）
    pub fn editor_content_region(&self, has_multiple_tabs: bool) -> Region {
        let editor = self.editor_region();
        let tab_height = if has_multiple_tabs {
            TAB_BAR_HEIGHT
        } else {
            0.0
        };
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
    /// 底部面板只占据中间编辑器区域（活动栏和侧边栏之间），不覆盖左右侧边栏
    pub fn bottom_panel_region(&self) -> Region {
        if !self.bottom_panel_visible {
            return Region::new(
                self.editor_region().x,
                self.window_height - self.status_bar_height,
                self.editor_region().width,
                0.0,
            );
        }
        let editor = self.editor_region();
        Region::new(
            editor.x,
            self.window_height - self.status_bar_height - self.bottom_panel_height,
            editor.width,
            self.bottom_panel_height,
        )
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

    /// 内容区域高度（排除标题栏、菜单栏和状态栏）
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
        if self.bottom_panel_visible {
            height -= self.bottom_panel_height;
        }
        // 确保内容区域至少有 0 像素的高度
        height.max(0.0)
    }

    /// 调整侧边栏宽度
    pub fn resize_sidebar(&mut self, delta: f32) {
        let new_width = (self.sidebar_width + delta).clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        self.sidebar_width = new_width;
    }

    /// 调整右侧面板宽度
    pub fn resize_right_panel(&mut self, delta: f32) {
        let new_width = (self.right_panel_width + delta).max(0.0);
        self.right_panel_width = new_width;
    }

    /// 调整底部面板高度
    pub fn resize_bottom_panel(&mut self, delta: f32) {
        let new_height = (self.bottom_panel_height + delta).max(0.0);
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
    pub fn toggle_terminal_panel(&mut self) {
        self.bottom_panel_visible = !self.bottom_panel_visible;
        if self.bottom_panel_visible {
            self.bottom_panel_height = 200.0;
        } else {
            self.bottom_panel_height = 0.0;
        }
    }
}
