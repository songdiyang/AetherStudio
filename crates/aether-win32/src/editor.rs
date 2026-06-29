use std::path::PathBuf;

use windows::core::Result;
use windows::Win32::Foundation::HWND;

use aether_core::buffer::history::{CursorPosition, History, OpType};
use aether_core::buffer::piece_table::PieceTable;
use aether_core::buffer::text_buffer::{Cursor, MultiCursorState};
use aether_core::lexer::{Language, LexemeSpan};
use aether_core::workspace::file_tree::{FileKind, FileTree};
use aether_render::d2d::factory::D2DFactory;
use aether_render::d2d::text::TextRenderer;
use aether_render::theme::Theme;

use crate::activity_bar::ActivityBar;
use crate::ai_panel::AiPanel;
use crate::command_palette::CommandPalette;
use crate::dialogs::Dialogs;
use crate::git::GitIntegration;
use crate::input::{KeyMap, PressTarget};
use crate::layout::{ActivityBarView, LayoutManager, SidebarContent, TAB_BAR_HEIGHT};
use crate::menu_bar::MenuBar;
use crate::settings::SettingsPanel;
use crate::ssh::{
    CloneRepoDialog, RemoteFileTree, RemoteSession, SshConnectionDialog, SshManagerPanel,
};
use crate::status_bar::StatusBar;
use crate::tabs::{Tab, TabLayout};
use crate::terminal::TerminalPanel;
use aether_shared::settings::AppSettings;
// P0-1: RemoteFs trait 为 SshRemoteFs::list_dir 等方法提供作用域
use aether_remote::RemoteFs;

/// 查找替换焦点状态
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindReplaceFocus {
    None,
    FindQuery,
    ReplaceText,
}

/// 文件夹异步扫描结果（由后台线程通过 PostMessage 发送回 UI 线程）
struct FolderScanResult {
    path: PathBuf,
    tree: Option<FileTree>,
    error: Option<String>,
}

/// C-09: SSH 异步连接结果（由后台线程通过 PostMessage 发送回 UI 线程）
struct SshConnectResult {
    session: Option<RemoteSession>,
    entries: Option<Vec<aether_remote::RemoteDirEntry>>,
    error: Option<String>,
}

/// C-09: Git 异步克隆结果（由后台线程通过 PostMessage 发送回 UI 线程）
struct GitCloneResult {
    target_path: PathBuf,
    error: Option<String>,
}

/// P0-1: 远程子目录异步列目录结果（由后台线程通过 PostMessage WM_APP+6 发送回 UI 线程）
struct SshListDirResult {
    /// 被列目录的节点路径
    path: String,
    /// 列目录成功时的条目
    entries: Option<Vec<aether_remote::RemoteDirEntry>>,
    /// 列目录失败时的错误
    error: Option<String>,
}

/// HWND 的 Send 包装（HWND 本质是指针，PostMessageW 是线程安全的）
#[derive(Clone, Copy)]
struct SendHwnd(usize);
unsafe impl Send for SendHwnd {}

/// 编辑器应用状态
pub struct EditorState {
    pub hwnd: HWND,
    pub d2d_factory: D2DFactory,
    pub render_ctx: crate::render_context::RenderContext,
    pub text_renderer: TextRenderer,
    pub theme: Theme,
    // 当前活动标签页的编辑状态（直接字段，零开销访问）
    pub buffer: PieceTable,
    pub file_path: Option<PathBuf>,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub selection_start: Option<(usize, usize)>,
    pub selection_end: Option<(usize, usize)>,
    pub is_selecting: bool,
    pub scroll_y: f32,
    /// P0-3: 水平滚动偏移（逻辑像素），用于查看超出编辑器宽度的长行
    pub scroll_x: f32,
    pub history: History,
    pub is_dirty: bool,
    // 渲染缓存
    pub(crate) cached_lines: Vec<String>,
    pub(crate) cached_tokens: Vec<Vec<LexemeSpan>>,
    /// 行级缓存版本号，每行独立追踪
    pub(crate) line_cache_versions: Vec<u64>,
    /// 全局编辑版本号，用于行级缓存失效
    pub(crate) buffer_version: u64,
    /// 行号 UTF-16 预缓存（避免每帧 format! + encode_utf16 分配）
    pub(crate) cached_line_numbers: Vec<Vec<u16>>,
    /// 可复用的 UTF-16 文本缓冲区（避免 render_editor 中每 token 分配 Vec<u16>）
    pub(crate) text_utf16_buf: Vec<u16>,
    // 当前语言
    pub(crate) language: Language,
    /// 标签页系统（后台存储，切换时同步）
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active_tab: usize,
    /// 标签栏布局缓存（用于点击检测）
    pub(crate) tab_layouts: Vec<TabLayout>,
    /// 鼠标悬停的标签索引
    pub(crate) hover_tab: Option<usize>,
    /// 标签栏滚动偏移
    pub(crate) tab_scroll_x: f32,
    // 查找与替换状态
    pub find_visible: bool,
    pub replace_visible: bool,
    pub find_query: String,
    pub replace_text: String,
    pub find_results: Vec<(usize, usize)>, // (line, col) 匹配位置列表
    pub find_active_index: usize,
    /// 查找替换焦点状态
    pub find_focus: FindReplaceFocus,
    /// 查找缓存：避免查询未变时重复全量扫描
    last_find_query: String,
    find_result_version: u64,
    // 全局 UI 状态
    pub file_tree: Option<FileTree>,
    pub current_folder: Option<PathBuf>,
    pub status_message: String,
    pub key_map: KeyMap,
    pub window_width: u32,
    pub window_height: u32,
    /// DPI 缩放因子（1.0 = 100%, 1.5 = 150%）
    pub dpi_scale: f32,
    /// UI-L02: IME 集成，控制 CJK 输入法候选窗口位置
    pub ime: crate::ime::ImeIntegration,
    // 新布局系统
    pub layout: LayoutManager,
    pub menu_bar: MenuBar,
    pub activity_bar: ActivityBar,
    pub status_bar: StatusBar,
    pub activity_view: ActivityBarView,
    pub sidebar_content: SidebarContent,
    /// 最近项目管理器
    pub recent_projects: crate::recent_projects::RecentProjectsManager,
    /// 命令面板
    pub command_palette: CommandPalette,
    /// 多光标状态
    pub multi_cursor: MultiCursorState,
    /// Git 集成
    pub git: GitIntegration,
    /// 终端面板
    pub terminal_panel: TerminalPanel,
    /// AI 助手面板
    pub ai_panel: AiPanel,
    /// SSH 连接对话框
    pub ssh_dialog: SshConnectionDialog,
    /// 远程会话
    pub remote_session: Option<RemoteSession>,
    /// 远程文件树
    pub remote_file_tree: Option<RemoteFileTree>,
    /// 选中的远程文件节点（P0-1: 改为路径标识，适配递归树）
    pub selected_remote_node: Option<String>,
    /// 悬停的远程文件节点（P0-1: 改为路径标识，适配递归树）
    pub hover_remote_node: Option<String>,
    /// 远程文件树滚动偏移
    pub remote_scroll_y: f32,
    /// 克隆仓库对话框
    pub clone_dialog: CloneRepoDialog,
    /// SSH 管理面板（侧边栏服务器管理）
    pub ssh_manager_panel: SshManagerPanel,
    /// 当前连接的服务器配置索引（对应 app_settings.remote.ssh_servers）
    pub active_ssh_index: Option<usize>,
    /// 窗口是否最大化
    pub is_maximized: bool,
    /// P0.2c: 是否为主窗口(无 owner)。仅主窗口在退出时持久化窗口状态。
    pub is_main_window: bool,
    /// 标题栏按钮悬停状态 (0=最小化, 1=最大化, 2=关闭)
    pub titlebar_hover_button: Option<usize>,
    /// 文件树中选中的节点索引
    pub selected_file_node: Option<u32>,
    /// 文件树中鼠标悬停的节点索引
    pub hover_file_node: Option<u32>,
    /// 欢迎页悬停的操作项
    pub welcome_hover_action: Option<crate::welcome::WelcomeAction>,
    /// 欢迎页键盘焦点项
    pub welcome_focus_action: Option<crate::welcome::WelcomeAction>,
    /// 全局矢量图标缓存（欢迎页/状态栏/命令面板等共用）
    pub icons: crate::icons::IconCache,
    /// 文件夹异步加载中（控制 sidebar spinner 显示）
    pub is_loading_folder: bool,
    /// C-09: SSH 后台连接中（防止重复触发，控制状态栏提示）
    pub ssh_connecting: bool,
    /// C-09: Git 后台克隆中（防止重复触发，控制状态栏提示）
    pub git_cloning: bool,
    /// 侧边栏滚动偏移（用于文件树虚拟滚动）
    pub sidebar_scroll_y: f32,
    /// 应用设置
    pub app_settings: aether_shared::settings::AppSettings,
    /// 设置面板
    pub settings_panel: crate::settings::SettingsPanel,
    /// 打开标签页面板
    pub open_tabs_panel: crate::open_tabs::OpenTabsPanel,
    /// Git 面板
    pub git_panel: crate::git::GitIntegration,
    /// 脏矩形追踪器（用于局部重绘优化）
    pub dirty_tracker: crate::dirty_rect::DirtyRectTracker,
    /// 上一帧的光标位置（用于检测光标移动）
    pub last_cursor_line: usize,
    /// 上一帧的光标列（用于检测光标移动）
    pub last_cursor_col: usize,
    /// 上一帧的滚动位置（用于检测滚动变化）
    pub last_scroll_y: f32,
    /// 上一帧的选择状态（用于检测选择变化）
    pub last_selection_start: Option<(usize, usize)>,
    /// 上一帧的选择结束（用于检测选择变化）
    pub last_selection_end: Option<(usize, usize)>,
    /// 上一帧的侧边栏内容类型（用于检测侧边栏变化）
    pub last_sidebar_content: crate::layout::SidebarContent,
    /// 上一帧的侧边栏可见性（用于检测侧边栏显示/隐藏变化）
    pub last_sidebar_visible: bool,
    /// 上一帧的活动栏可见性（用于检测活动栏显示/隐藏变化）
    pub last_activity_bar_visible: bool,
    /// 上一帧的右侧面板可见性（用于检测右侧面板变化）
    pub last_right_panel_visible: bool,
    /// 上一帧的底部面板可见性（用于检测底部面板变化）
    pub last_bottom_panel_visible: bool,
    /// 上一帧的状态消息（用于检测状态栏变化）
    pub last_status_message: String,
    /// 用户菜单
    pub user_menu: crate::user_menu::UserMenu,
    /// 长按检测：按下时刻（None 表示当前未进行长按检测）
    pub lpress_start: Option<std::time::Instant>,
    /// 长按检测起始 x（逻辑像素）
    pub lpress_x: f32,
    /// 长按检测起始 y（逻辑像素）
    pub lpress_y: f32,
    /// 长按检测目标（活动栏/菜单栏）
    pub lpress_target: Option<PressTarget>,
    /// 长按检测目标索引
    pub lpress_index: usize,
    /// 当前鼠标左键是否按下（用于 WM_TIMER 判定）
    pub lbutton_down: bool,
    /// P0-2: IME 合成串（pre-edit text），中文/日文输入过程中显示在光标处
    pub composition: Option<String>,
}

impl EditorState {
    /// 将当前编辑状态保存到后台标签页存储
    fn sync_to_tab(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.buffer =
                std::mem::replace(&mut self.buffer, PieceTable::from_string(String::new()));
            tab.file_path = self.file_path.clone();
            tab.cursor_line = self.cursor_line;
            tab.cursor_col = self.cursor_col;
            tab.selection_start = self.selection_start;
            tab.selection_end = self.selection_end;
            tab.scroll_y = self.scroll_y;
            tab.scroll_x = self.scroll_x;
            tab.history = std::mem::replace(&mut self.history, History::new());
            tab.is_dirty = self.is_dirty;
            tab.cached_lines = std::mem::take(&mut self.cached_lines);
            tab.cached_tokens = std::mem::take(&mut self.cached_tokens);
            tab.line_cache_versions = std::mem::take(&mut self.line_cache_versions);
            tab.buffer_version = self.buffer_version;
            tab.language = self.language;
        }
    }

    /// 从后台标签页存储恢复编辑状态到当前视图
    fn sync_from_tab(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            self.buffer =
                std::mem::replace(&mut tab.buffer, PieceTable::from_string(String::new()));
            self.file_path = tab.file_path.clone();
            self.cursor_line = tab.cursor_line;
            self.cursor_col = tab.cursor_col;
            self.selection_start = tab.selection_start;
            self.selection_end = tab.selection_end;
            self.scroll_y = tab.scroll_y;
            self.scroll_x = tab.scroll_x;
            self.history = std::mem::replace(&mut tab.history, History::new());
            self.is_dirty = tab.is_dirty;
            self.cached_lines = std::mem::take(&mut tab.cached_lines);
            self.cached_tokens = std::mem::take(&mut tab.cached_tokens);
            self.line_cache_versions = std::mem::take(&mut tab.line_cache_versions);
            self.buffer_version = tab.buffer_version;
            self.language = tab.language;
        }
    }

    /// 获取当前活动标签页（只读）
    pub fn current_tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    /// 获取当前标签页数量
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// 切换到指定标签页
    pub fn switch_tab(&mut self, index: usize) {
        if index < self.tabs.len() && index != self.active_tab {
            self.sync_to_tab();
            self.active_tab = index;
            self.sync_from_tab();
            self.is_selecting = false;
            self.sync_file_tree_selection();
            self.status_message = format!("切换到: {}", self.current_tab().file_name());
        }
    }

    /// 关闭当前标签页，返回是否还有标签页
    pub fn close_current_tab(&mut self) -> bool {
        if self.tabs.len() <= 1 {
            // 最后一个标签页，重置为空文件
            self.tabs[0] = Tab::new();
            self.active_tab = 0;
            self.sync_from_tab();
            self.is_selecting = false;
            self.status_message = "已关闭".to_string();
            return true;
        }
        self.tabs.remove(self.active_tab);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
        self.sync_from_tab();
        self.is_selecting = false;
        self.status_message = format!("已关闭，剩余 {} 个文件", self.tabs.len());
        !self.tabs.is_empty()
    }

    /// P2-8: 带保存确认的关闭标签页。
    /// 返回值：true 表示已关闭（用户确认或无需保存），false 表示用户取消。
    pub fn close_current_tab_checked(&mut self) -> bool {
        // self 上的 is_dirty / buffer / file_path 即为当前活动标签页的实时状态
        // （编辑操作直接作用于 self，仅在切换标签页时通过 sync_to_tab/sync_from_tab 交换）
        if self.is_dirty {
            let file_name = self
                .file_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "未命名".to_string());
            let msg = format!("{} 有未保存的修改，是否保存并关闭？", file_name);
            let confirmed = Dialogs::confirm_yes_no(self.hwnd, "关闭标签页", &msg);
            if !confirmed {
                self.status_message = "已取消关闭".to_string();
                return false;
            }
            let saved = self.save_file();
            if !saved {
                self.status_message = "保存失败，已取消关闭".to_string();
                return false;
            }
        }
        self.close_current_tab();
        true
    }

    /// P2-3: 调整字体大小（Ctrl+= 放大 / Ctrl+- 缩小 / Ctrl+0 重置）。
    /// delta 为正放大、为负缩小；传 None 则重置为 14.0。
    pub fn zoom_font(&mut self, delta: Option<f32>) {
        let current = self.text_renderer.font_size();
        let new_size = match delta {
            Some(d) => current + d,
            None => 14.0,
        };
        self.text_renderer.set_font_size(new_size);
        // 重建文本格式缓存（与 set_font_size 同步，避免渲染时使用旧格式）
        let fs = self.text_renderer.font_size();
        self.render_ctx.text_format_cache.init_common_formats(fs);
        self.status_message = format!("字体大小: {:.1} px", fs);
    }

    /// 新建标签页
    pub fn new_tab(&mut self) -> usize {
        self.sync_to_tab();
        let tab = Tab::new();
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.sync_from_tab();
        self.is_selecting = false;
        self.active_tab
    }

    /// 切换到下一个标签页
    pub fn next_tab(&mut self) {
        if self.tabs.len() > 1 {
            let next = (self.active_tab + 1) % self.tabs.len();
            self.switch_tab(next);
        }
    }

    /// 切换到上一个标签页
    pub fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            let prev = (self.active_tab + self.tabs.len() - 1) % self.tabs.len();
            self.switch_tab(prev);
        }
    }

    /// 跳转到指定标签页（1-based index）
    pub fn goto_tab(&mut self, index: usize) {
        if index > 0 && index <= self.tabs.len() {
            self.switch_tab(index - 1);
        }
    }

    /// 处理标签栏点击，返回是否处理了点击
    /// mouse_y 是相对于标签栏的 y 坐标（已由调用方转换）
    pub fn handle_tab_bar_click(&mut self, mouse_x: f32, _mouse_y: f32, editor_x: f32) -> bool {
        // P3-2: 使用 layout 常量而非硬编码 30.0，确保布局常量变更时此处理同步
        let tab_bar_height = if self.tabs.len() > 1 {
            TAB_BAR_HEIGHT
        } else {
            0.0
        };
        if tab_bar_height == 0.0 {
            return false;
        }
        // mouse_y 已由调用方检查，这里只检查 x 坐标
        if mouse_x < editor_x {
            return false;
        }
        let rel_x = mouse_x - editor_x + self.tab_scroll_x;
        for layout in &self.tab_layouts {
            if rel_x >= layout.x && rel_x < layout.x + layout.width {
                // 检测关闭按钮点击
                if rel_x >= layout.close_x && rel_x < layout.close_x + layout.close_width {
                    if layout.index == self.active_tab {
                        // P2-8: 活动标签页走 dirty 检查（可能触发保存对话框）
                        self.close_current_tab_checked();
                    } else {
                        // P2-8: 非活动标签页，检查其 is_dirty
                        let tab_dirty = self
                            .tabs
                            .get(layout.index)
                            .map(|t| t.is_dirty)
                            .unwrap_or(false);
                        if tab_dirty {
                            let tab_name = self
                                .tabs
                                .get(layout.index)
                                .and_then(|t| t.file_path.as_ref())
                                .and_then(|p| p.file_name())
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| "未命名".to_string());
                            let msg = format!("{} 有未保存的修改，是否丢弃修改并关闭？", tab_name);
                            if !Dialogs::confirm_yes_no(self.hwnd, "关闭标签页", &msg) {
                                self.status_message = "已取消关闭".to_string();
                                return true;
                            }
                        }
                        self.tabs.remove(layout.index);
                        if layout.index < self.active_tab {
                            self.active_tab -= 1;
                        }
                        self.status_message = format!("已关闭，剩余 {} 个文件", self.tabs.len());
                    }
                    return true;
                }
                // 切换标签页
                self.switch_tab(layout.index);
                return true;
            }
        }
        false
    }

    /// 更新鼠标悬停标签
    pub fn update_hover_tab(&mut self, mouse_x: f32, mouse_y: f32, editor_x: f32) {
        // P3-2: 使用 layout 常量而非硬编码 30.0
        let tab_bar_height = if self.tabs.len() > 1 {
            TAB_BAR_HEIGHT
        } else {
            0.0
        };
        if tab_bar_height == 0.0 || mouse_y < 0.0 || mouse_y > tab_bar_height || mouse_x < editor_x
        {
            self.hover_tab = None;
            return;
        }
        let rel_x = mouse_x - editor_x + self.tab_scroll_x;
        for layout in &self.tab_layouts {
            if rel_x >= layout.x && rel_x < layout.x + layout.width {
                self.hover_tab = Some(layout.index);
                return;
            }
        }
        self.hover_tab = None;
    }

    pub fn new(hwnd: HWND, is_main_window: bool) -> Result<Self> {
        let d2d_factory = D2DFactory::new()?;
        let text_renderer = TextRenderer::new()?;
        let theme = Theme::glass();
        let buffer = PieceTable::from_string(String::new());
        let key_map = KeyMap::new();
        let app_settings = AppSettings::load();

        let mut state = Self {
            hwnd,
            d2d_factory,
            render_ctx: crate::render_context::RenderContext::new(),
            text_renderer,
            theme,
            buffer,
            file_path: None,
            cursor_line: 0,
            cursor_col: 0,
            selection_start: None,
            selection_end: None,
            is_selecting: false,
            scroll_y: 0.0,
            scroll_x: 0.0,
            history: History::new(),
            is_dirty: false,
            cached_lines: Vec::new(),
            cached_tokens: Vec::new(),
            line_cache_versions: Vec::new(),
            buffer_version: 0,
            cached_line_numbers: Vec::new(),
            text_utf16_buf: Vec::with_capacity(256),
            language: Language::PlainText,
            tabs: Vec::new(),
            active_tab: 0,
            tab_layouts: Vec::new(),
            hover_tab: None,
            tab_scroll_x: 0.0,
            find_visible: false,
            replace_visible: false,
            find_query: String::new(),
            replace_text: String::new(),
            find_results: Vec::new(),
            find_active_index: 0,
            find_focus: FindReplaceFocus::None,
            last_find_query: String::new(),
            find_result_version: 0,
            file_tree: None,
            current_folder: None,
            status_message: "就绪".to_string(),
            key_map,
            window_width: 1280,
            window_height: 800,
            dpi_scale: 1.0,
            // UI-L02: 实例化 IME 集成
            ime: crate::ime::ImeIntegration::new(hwnd),
            layout: LayoutManager::new(1280.0, 800.0),
            menu_bar: MenuBar::new(),
            activity_bar: ActivityBar::new(),
            status_bar: StatusBar::new(),
            activity_view: ActivityBarView::Explorer,
            sidebar_content: SidebarContent::FileTree,
            recent_projects: crate::recent_projects::RecentProjectsManager::new(),
            command_palette: CommandPalette::new(),
            multi_cursor: MultiCursorState::new(),
            git: GitIntegration::new(),
            terminal_panel: TerminalPanel::new(),
            ai_panel: AiPanel::new(),
            settings_panel: SettingsPanel::from_settings(&app_settings),
            open_tabs_panel: crate::open_tabs::OpenTabsPanel::new(),
            app_settings,
            ssh_dialog: SshConnectionDialog::new(),
            remote_session: None,
            remote_file_tree: None,
            selected_remote_node: None,
            hover_remote_node: None,
            remote_scroll_y: 0.0,
            clone_dialog: CloneRepoDialog::new(),
            ssh_manager_panel: SshManagerPanel::new(),
            active_ssh_index: None,
            is_maximized: false,
            is_main_window,
            titlebar_hover_button: None,
            selected_file_node: None,
            hover_file_node: None,
            welcome_hover_action: None,
            welcome_focus_action: None,
            icons: crate::icons::IconCache::new(),
            is_loading_folder: false,
            ssh_connecting: false,
            git_cloning: false,
            sidebar_scroll_y: 0.0,
            git_panel: crate::git::GitIntegration::new(),
            dirty_tracker: crate::dirty_rect::DirtyRectTracker::new(1280.0, 800.0),
            last_cursor_line: 0,
            last_cursor_col: 0,
            last_scroll_y: 0.0,
            last_selection_start: None,
            last_selection_end: None,
            last_sidebar_content: crate::layout::SidebarContent::FileTree,
            last_sidebar_visible: true,
            last_activity_bar_visible: true,
            last_right_panel_visible: false,
            last_bottom_panel_visible: false,
            last_status_message: "就绪".to_string(),
            user_menu: crate::user_menu::UserMenu::new(),
            lpress_start: None,
            lpress_x: 0.0,
            lpress_y: 0.0,
            lpress_target: None,
            lpress_index: 0,
            lbutton_down: false,
            composition: None,
        };
        // 应用持久化的活动栏/菜单栏顺序（空配置使用默认顺序）
        let activity_order = state.app_settings.ui.activity_bar_order.clone();
        let menu_order = state.app_settings.ui.menu_bar_order.clone();
        if !activity_order.is_empty() {
            state.activity_bar.apply_order(&activity_order);
            // 应用顺序后修正当前活动视图
            state.activity_view = state.activity_bar.active_view();
            state.sidebar_content = crate::layout::SidebarContent::from_view(state.activity_view);
        }
        if !menu_order.is_empty() {
            state.menu_bar.apply_order(&menu_order);
        }
        // 创建第一个标签页并同步
        state.tabs.push(Tab::new());
        state.sync_from_tab();

        // P0.2c: 主窗口启动时自动恢复上次打开的工作区。
        // 仅在路径仍然存在时打开,避免引用已删除/移动的目录。
        // 异步扫描结果通过 WM_APP+3 回调到达,此处调用仅触发扫描。
        if is_main_window {
            if let Some(workspace) = state.app_settings.ui.last_workspace.clone() {
                if workspace.exists() {
                    state.open_folder(workspace);
                }
            }
        }

        Ok(state)
    }

    pub fn init_render_target(&mut self) -> Result<()> {
        let _dpi = self.dpi_scale * 96.0;
        let phys_w = (self.window_width as f32 * self.dpi_scale) as u32;
        let phys_h = (self.window_height as f32 * self.dpi_scale) as u32;
        self.render_ctx.init_render_target(
            &self.d2d_factory,
            self.hwnd,
            phys_w,
            phys_h,
            self.dpi_scale,
        )?;
        Ok(())
    }

    /// 调整窗口尺寸 - 接收物理像素，内部转换为逻辑像素(DIP)
    pub fn resize(&mut self, phys_width: u32, phys_height: u32) {
        let log_w = (phys_width as f32 / self.dpi_scale) as u32;
        let log_h = (phys_height as f32 / self.dpi_scale) as u32;
        self.window_width = log_w;
        self.window_height = log_h;
        self.layout.resize_window(log_w as f32, log_h as f32);
        // 更新脏矩形追踪器窗口尺寸，触发全窗口重绘
        self.dirty_tracker.resize(log_w as f32, log_h as f32);
        self.render_ctx.resize(phys_width, phys_height);
    }

    /// 检查当前标签页是否可以重用（空文件且未修改）
    fn can_reuse_current_tab(&self) -> bool {
        self.file_path.is_none() && !self.is_dirty && self.buffer.len_bytes() == 0
    }

    /// 重置当前编辑状态到初始值
    fn reset_editor_state(&mut self) {
        self.cursor_line = 0;
        self.cursor_col = 0;
        self.scroll_y = 0.0;
        self.history.clear();
        self.is_dirty = false;
        self.buffer_version += 1;
        self.clear_selection();
    }

    /// 在新标签页中打开内容
    fn open_in_new_tab(&mut self, tab: Tab) {
        self.sync_to_tab();
        let mut placeholder = tab;
        std::mem::swap(&mut self.tabs[self.active_tab], &mut placeholder);
        self.tabs.push(placeholder);
        self.active_tab = self.tabs.len() - 1;
        self.sync_from_tab();
        self.is_selecting = false;
    }

    pub fn load_file(&mut self, path: PathBuf) {
        let lang = Language::from_path(&path);

        if lang == Language::Image {
            self.load_image_file(path);
            return;
        }

        if !is_text_file(&path) {
            self.show_unsupported_file(&path);
            return;
        }

        match PieceTable::from_file(&path) {
            Ok(buffer) => {
                if self.can_reuse_current_tab() {
                    self.buffer = buffer;
                    self.file_path = Some(path.clone());
                    self.language = lang;
                    self.reset_editor_state();
                    // 重用当前标签页时，直接更新标签页数据，
                    // 不要调用 sync_to_tab() 否则会把 buffer 移走
                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                        tab.buffer = PieceTable::from_string(String::new());
                        tab.file_path = Some(path.clone());
                        tab.language = lang;
                        tab.cursor_line = 0;
                        tab.cursor_col = 0;
                        tab.scroll_y = 0.0;
                        tab.is_dirty = false;
                        tab.buffer_version = 1;
                        tab.cached_lines.clear();
                        tab.cached_tokens.clear();
                        tab.line_cache_versions.clear();
                    }
                    self.status_message = format!("已打开: {}", path.display());
                } else {
                    let tab = Tab {
                        file_path: Some(path.clone()),
                        buffer,
                        cursor_line: 0,
                        cursor_col: 0,
                        selection_start: None,
                        selection_end: None,
                        scroll_y: 0.0,
                        scroll_x: 0.0,
                        history: History::new(),
                        is_dirty: false,
                        cached_lines: Vec::new(),
                        cached_tokens: Vec::new(),
                        line_cache_versions: Vec::new(),
                        buffer_version: 1,
                        language: lang,
                    };
                    self.open_in_new_tab(tab);
                    self.status_message = format!("已打开: {}", path.display());
                }
            }
            Err(e) => {
                let msg = format!("打开文件失败: {}", e);
                self.status_message = msg.clone();
                Dialogs::show_error(self.hwnd, "打开文件", &msg);
            }
        }
    }

    /// 加载图片文件
    fn load_image_file(&mut self, path: PathBuf) {
        let content = format!("[图片预览] {}", path.display());
        if self.can_reuse_current_tab() {
            self.file_path = Some(path.clone());
            self.language = Language::Image;
            self.buffer = PieceTable::from_string(content);
            self.reset_editor_state();
            self.sync_to_tab();
            self.status_message = format!("已打开图片: {}", path.display());
        } else {
            let tab = Tab {
                file_path: Some(path.clone()),
                buffer: PieceTable::from_string(content),
                cursor_line: 0,
                cursor_col: 0,
                selection_start: None,
                selection_end: None,
                scroll_y: 0.0,
                scroll_x: 0.0,
                history: History::new(),
                is_dirty: false,
                cached_lines: Vec::new(),
                cached_tokens: Vec::new(),
                line_cache_versions: Vec::new(),
                buffer_version: 1,
                language: Language::Image,
            };
            self.open_in_new_tab(tab);
            self.status_message = format!("已打开图片: {}", path.display());
        }
    }

    /// 显示不支持的文件提示
    fn show_unsupported_file(&mut self, path: &PathBuf) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown");
        let message = format!("不支持的文件格式: .{}\n文件: {}", ext, path.display());
        if self.can_reuse_current_tab() {
            self.file_path = Some(path.clone());
            self.language = Language::PlainText;
            self.buffer = PieceTable::from_string(message);
            self.reset_editor_state();
            self.sync_to_tab();
            self.status_message = format!("不支持的文件格式: .{}", ext);
        } else {
            let tab = Tab {
                file_path: Some(path.clone()),
                buffer: PieceTable::from_string(message),
                cursor_line: 0,
                cursor_col: 0,
                selection_start: None,
                selection_end: None,
                scroll_y: 0.0,
                scroll_x: 0.0,
                history: History::new(),
                is_dirty: false,
                cached_lines: Vec::new(),
                cached_tokens: Vec::new(),
                line_cache_versions: Vec::new(),
                buffer_version: 1,
                language: Language::PlainText,
            };
            self.open_in_new_tab(tab);
            self.status_message = format!("不支持的文件格式: .{}", ext);
        }
    }

    /// 新建文件
    pub fn new_file(&mut self) {
        if self.can_reuse_current_tab() {
            self.buffer = PieceTable::from_string(String::new());
            self.file_path = None;
            self.reset_editor_state();
            self.sync_to_tab();
            self.status_message = "新文件".to_string();
        } else {
            self.open_in_new_tab(Tab::new());
            self.status_message = "新文件".to_string();
        }
    }

    /// P4-2: 原子写入文件，避免写入中途崩溃导致文件损坏
    /// 先写入同目录的临时文件并 fsync，再原子 rename 替换目标文件
    fn atomic_write(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        use std::path::Path;

        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        let temp_path = dir.join(format!(
            ".aether-save-{}-{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));

        let result = (|| -> std::io::Result<()> {
            let mut file = std::fs::File::create(&temp_path)?;
            file.write_all(data)?;
            file.sync_all()?;
            drop(file); // 关闭句柄后再 rename
            std::fs::rename(&temp_path, path)?;
            Ok(())
        })();

        // 任何步骤失败时清理临时文件
        if result.is_err() {
            let _ = std::fs::remove_file(&temp_path);
        }
        result
    }

    /// 保存文件，返回是否成功
    pub fn save_file(&mut self) -> bool {
        if let Some(path) = &self.file_path.clone() {
            let text = self.buffer.get_all_text();
            // 处理远程文件保存
            if let Some(remote_path) = path.to_str().and_then(|s| s.strip_prefix("remote:")) {
                if let Some(session) = &self.remote_session {
                    match session.write_remote_file(remote_path, text.as_bytes()) {
                        Ok(()) => {
                            self.is_dirty = false;
                            self.sync_to_tab();
                            self.status_message = format!("已保存到远程: {}", remote_path);
                            return true;
                        }
                        Err(e) => {
                            self.status_message = format!("保存远程文件失败: {}", e);
                            return false;
                        }
                    }
                } else {
                    self.status_message = "远程会话未连接".to_string();
                    return false;
                }
            }
            // P4-2: 使用原子写入，避免写入中途崩溃导致文件损坏
            match Self::atomic_write(path, text.as_bytes()) {
                Ok(()) => {
                    self.is_dirty = false;
                    self.sync_to_tab();
                    self.status_message = "已保存".to_string();
                    true
                }
                Err(e) => {
                    self.status_message = format!("保存失败: {}", e);
                    false
                }
            }
        } else {
            self.status_message = "没有文件路径，请使用另存为".to_string();
            false
        }
    }

    /// 另存为
    pub fn save_as(&mut self, path: PathBuf) -> bool {
        let text = self.buffer.get_all_text();
        match Self::atomic_write(&path, text.as_bytes()) {
            Ok(()) => {
                self.file_path = Some(path.clone());
                self.is_dirty = false;
                self.sync_to_tab();
                self.status_message = format!("已保存: {}", path.display());
                true
            }
            Err(e) => {
                self.status_message = format!("保存失败: {}", e);
                false
            }
        }
    }
}

/// 需要跳过的常见大目录（构建输出、依赖缓存等）
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "build",
    "dist",
    "out",
    "vendor",
    ".git",
    ".svn",
    ".hg",
    ".idea",
    ".vscode",
    "__pycache__",
    ".pytest_cache",
    "*.egg-info",
    "bin",
    "obj",
    "Debug",
    "Release",
    "x64",
    "x86",
    "coverage",
    ".nyc_output",
    ".next",
    ".nuxt",
];

/// 扫描单个目录的一层子项并加入文件树（懒加载基础）
/// 不会递归进入子目录。返回扫描到的子项数量。
fn populate_children_one_level(
    tree: &mut FileTree,
    dir_path: &PathBuf,
    parent_idx: u32,
    depth: u8,
) -> std::io::Result<usize> {
    const MAX_ENTRIES_PER_DIR: usize = 1000;

    let mut entries: Vec<_> = std::fs::read_dir(dir_path)?
        .filter_map(|e| e.ok())
        .collect();

    // 限制每层目录扫描数量，避免超大目录卡死
    if entries.len() > MAX_ENTRIES_PER_DIR {
        entries.truncate(MAX_ENTRIES_PER_DIR);
    }

    entries.sort_by(|a, b| {
        let a_is_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_is_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });

    let mut count = 0usize;
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();

        // 跳过常见大目录
        if SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }

        let is_dir = entry.file_type()?.is_dir();
        let kind = if is_dir {
            FileKind::Directory
        } else {
            FileKind::File
        };
        // 子节点深度 = 父深度 + 1
        let _ = tree.add_node(&name, kind, parent_idx, depth);
        count += 1;
    }

    Ok(count)
}

/// 递归构建文件树（已弃用递归逻辑，保留兼容）
/// 现在仅用于 open_folder 加载根层；深层目录由懒加载按需扫描
#[allow(dead_code)]
fn populate_file_tree(
    tree: &mut FileTree,
    path: &PathBuf,
    parent_idx: u32,
    depth: u8,
) -> std::io::Result<()> {
    let _ = populate_children_one_level(tree, path, parent_idx, depth)?;
    Ok(())
}

impl EditorState {
    /// 复制选中文本到剪贴板
    pub fn copy(&mut self) {
        if let Some(text) = self.get_selected_text() {
            Self::set_clipboard_text(&text);
            self.status_message = "已复制".to_string();
        }
    }

    /// 剪切选中文本到剪贴板
    pub fn cut(&mut self) {
        if let Some(text) = self.get_selected_text() {
            Self::set_clipboard_text(&text);
            self.delete_selection();
            self.status_message = "已剪切".to_string();
        }
    }

    /// 从剪贴板粘贴文本
    pub fn paste(&mut self) {
        if let Some(text) = Self::get_clipboard_text() {
            // 如果有选区，先删除选中内容
            if self.selection_start.is_some() && self.selection_end.is_some() {
                self.delete_selection();
            }
            let pos = self.cursor_byte_pos();
            let before_pieces = self.buffer.get_pieces();
            let before_add_len = self.buffer.add_buffer_len();
            let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

            self.buffer.insert(pos, &text);
            self.is_dirty = true;
            self.buffer_version += 1;

            // 更新光标位置
            let line_breaks = text.matches('\n').count();
            if line_breaks == 0 {
                self.cursor_col += text.len();
            } else {
                self.cursor_line += line_breaks;
                self.cursor_col = text
                    .rsplit_once('\n')
                    .map(|(_, last)| last.len())
                    .unwrap_or(0);
            }

            let cursor_after = CursorPosition::new(self.cursor_line, self.cursor_col);
            self.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Insert,
                pos,
            );
            self.clear_selection();
            self.status_message = "已粘贴".to_string();
        }
    }

    /// 删除选中文本
    pub fn delete_selection(&mut self) {
        let (start_line, start_col) = match self.selection_start {
            Some(s) => s,
            None => return,
        };
        let (end_line, end_col) = match self.selection_end {
            Some(e) => e,
            None => return,
        };

        let (first_line, first_col) = if start_line <= end_line {
            (start_line, start_col)
        } else {
            (end_line, end_col)
        };
        let (last_line, last_col) = if start_line <= end_line {
            (end_line, end_col)
        } else {
            (start_line, start_col)
        };

        let start_byte = self.line_byte_start(first_line) + first_col;
        let end_byte = self.line_byte_start(last_line) + last_col;

        if start_byte < end_byte {
            let before_pieces = self.buffer.get_pieces();
            let before_add_len = self.buffer.add_buffer_len();
            let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

            self.buffer.delete(start_byte, end_byte);
            self.is_dirty = true;
            self.buffer_version += 1;

            self.cursor_line = first_line;
            self.cursor_col = first_col;

            let cursor_after = CursorPosition::new(self.cursor_line, self.cursor_col);
            self.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Delete,
                start_byte,
            );
        }
        self.clear_selection();
    }

    /// 全选
    pub fn select_all(&mut self) {
        let last_line = self.buffer.len_lines().saturating_sub(1);
        let last_col = self
            .buffer
            .get_line(last_line)
            .map(|t| t.len())
            .unwrap_or(0);
        self.selection_start = Some((0, 0));
        self.selection_end = Some((last_line, last_col));
        self.cursor_line = last_line;
        self.cursor_col = last_col;
        self.is_selecting = false;
    }

    /// 滚动
    pub fn scroll(&mut self, delta_y: f32) {
        let line_height = self.text_renderer.line_height();
        let total_height = self.buffer.len_lines() as f32 * line_height;
        // UI-M02: 使用实际编辑器区域高度替代硬编码 24.0
        let editor_region = self.layout.editor_region();
        let editor_height = editor_region.height.max(1.0);
        let max_scroll = (total_height - editor_height).max(0.0);
        self.scroll_y = (self.scroll_y + delta_y).clamp(0.0, max_scroll);
    }

    /// P0-3: 水平滚动。
    /// `delta_x` 为正表示向右滚动（查看右侧内容），为负向左。
    /// 最大滚动范围由当前可见行中最长行的像素宽度决定。
    pub fn scroll_horizontal(&mut self, delta_x: f32) {
        let char_width = self.text_renderer.char_width();
        let editor_region = self.layout.editor_region();
        let editor_width = editor_region.width.max(1.0);

        // 计算可见范围内最长行的字符宽度
        let line_height = self.text_renderer.line_height();
        let start_line = (self.scroll_y / line_height) as usize;
        let visible_lines = ((editor_region.height / line_height) as usize + 2).max(1);
        let end_line = (start_line + visible_lines).min(self.cached_lines.len().max(1));

        let mut max_line_chars: usize = 0;
        for line_idx in start_line..end_line {
            if let Some(text) = self.cached_lines.get(line_idx) {
                let chars = text
                    .chars()
                    .map(|ch| if (ch as u32) > 0x7F { 2 } else { 1 })
                    .sum::<usize>();
                if chars > max_line_chars {
                    max_line_chars = chars;
                }
            }
        }

        // 行号宽度 + 5px 内边距，扣除后为文本可视宽度
        let text_visible_width = (editor_width - 60.0 - 5.0).max(1.0);
        let max_content_width = max_line_chars as f32 * char_width;
        let max_scroll_x = (max_content_width - text_visible_width).max(0.0);

        self.scroll_x = (self.scroll_x + delta_x).clamp(0.0, max_scroll_x);
    }

    /// P0-3: 重置水平滚动（光标跳转、文件加载时调用）
    pub fn reset_scroll_x(&mut self) {
        self.scroll_x = 0.0;
    }

    /// P0-3: 确保光标在水平方向可见，必要时调整 scroll_x。
    /// 在光标移动后调用。
    pub fn ensure_cursor_visible_horizontal(&mut self) {
        let char_width = self.text_renderer.char_width();
        let editor_region = self.layout.editor_region();
        let text_visible_width = (editor_region.width - 60.0 - 5.0).max(1.0);

        // 光标在当前行的字符列
        let cursor_char_col = if let Some(text) = self.cached_lines.get(self.cursor_line) {
            let byte_pos = self.cursor_col.min(text.len());
            text[..byte_pos]
                .chars()
                .map(|ch| if (ch as u32) > 0x7F { 2 } else { 1 })
                .sum::<usize>()
        } else {
            0
        };
        let cursor_x = cursor_char_col as f32 * char_width;

        let left = self.scroll_x;
        let right = self.scroll_x + text_visible_width;

        if cursor_x < left {
            // 光标在可视区左侧，向左滚动
            self.scroll_x = cursor_x.max(0.0);
        } else if cursor_x >= right {
            // 光标在可视区右侧，向右滚动（留 1 字符余量）
            self.scroll_x = cursor_x - text_visible_width + char_width;
        }
    }

    /// 侧边栏滚动（文件树虚拟滚动）
    pub fn scroll_sidebar(&mut self, delta_y: f32) {
        match &self.sidebar_content {
            crate::layout::SidebarContent::FileTree => {
                let node_height = 20.0;
                let estimated_nodes = if let Some(tree) = &self.file_tree {
                    tree.len() as f32
                } else {
                    0.0
                };
                let total_height = estimated_nodes * node_height + 20.0;
                let sidebar_region = self.layout.sidebar_region();
                let visible_height = sidebar_region.height;
                let max_scroll = (total_height - visible_height).max(0.0);
                self.sidebar_scroll_y = (self.sidebar_scroll_y + delta_y).clamp(0.0, max_scroll);
            }
            crate::layout::SidebarContent::RemoteFileTree => {
                let node_height = 20.0;
                // P0-1: 按可见节点数（含展开的子节点）估算滚动高度
                let visible_nodes = self
                    .remote_file_tree
                    .as_ref()
                    .map(|t| t.count_visible_nodes())
                    .unwrap_or(0) as f32;
                let total_height = visible_nodes * node_height + 40.0;
                let sidebar_region = self.layout.sidebar_region();
                let visible_height = sidebar_region.height;
                let max_scroll = (total_height - visible_height).max(0.0);
                self.remote_scroll_y = (self.remote_scroll_y + delta_y).clamp(0.0, max_scroll);
            }
            crate::layout::SidebarContent::SourceControlPanel => {
                let item_height = 22.0;
                let staged = self.git.staged_files().len() as f32;
                let unstaged = self.git.unstaged_files().len() as f32;
                let untracked = self.git.untracked_files().len() as f32;
                let total_height = 100.0 + (staged + unstaged + untracked) * item_height + 60.0;
                let sidebar_region = self.layout.sidebar_region();
                let visible_height = sidebar_region.height;
                let max_scroll = (total_height - visible_height).max(0.0);
                self.git.scroll_y = (self.git.scroll_y + delta_y).clamp(0.0, max_scroll);
            }
            crate::layout::SidebarContent::AiAssistantPanel => {
                let msg_height = 60.0;
                let total_height = self.ai_panel.messages.len() as f32 * msg_height + 200.0;
                let sidebar_region = self.layout.sidebar_region();
                let visible_height = sidebar_region.height;
                let max_scroll = (total_height - visible_height).max(0.0);
                self.ai_panel.scroll_y = (self.ai_panel.scroll_y + delta_y).clamp(0.0, max_scroll);
            }
            _ => {}
        }
    }

    /// 设置剪贴板文本
    fn set_clipboard_text(text: &str) -> bool {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::DataExchange::{
            CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
        };
        use windows::Win32::System::Memory::{
            GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
        };
        const CF_UNICODETEXT: u32 = 13;

        unsafe {
            if OpenClipboard(None).is_err() {
                return false;
            }
            let _ = EmptyClipboard();

            let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
            let byte_size = wide.len() * 2;

            let hglobal = match GlobalAlloc(GMEM_MOVEABLE, byte_size) {
                Ok(h) => h,
                Err(_) => {
                    let _ = CloseClipboard();
                    return false;
                }
            };
            let ptr = GlobalLock(hglobal);
            if ptr.is_null() {
                // H-19: GlobalLock 失败时释放 HGLOBAL
                let _ = GlobalUnlock(hglobal);
                let _ = CloseClipboard();
                return false;
            }
            let dst = ptr as *mut u16;
            std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
            let _ = GlobalUnlock(hglobal);
            // H-19: SetClipboardData 失败时释放 HGLOBAL，防止内存泄漏
            if SetClipboardData(CF_UNICODETEXT, HANDLE(hglobal.0)).is_err() {
                // UI-M07: SetClipboardData 失败后 HGLOBAL 所有权未转移，必须手动释放
                extern "system" {
                    fn GlobalFree(hMem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
                }
                GlobalFree(hglobal.0);
                let _ = CloseClipboard();
                return false;
            }
            let _ = CloseClipboard();
            true
        }
    }

    /// 获取剪贴板文本
    pub(crate) fn get_clipboard_text() -> Option<String> {
        use windows::Win32::Foundation::{HANDLE, HGLOBAL};
        use windows::Win32::System::DataExchange::{
            CloseClipboard, GetClipboardData, OpenClipboard,
        };
        use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
        const CF_UNICODETEXT: u32 = 13;

        unsafe {
            if OpenClipboard(None).is_err() {
                return None;
            }
            let result = GetClipboardData(CF_UNICODETEXT)
                .ok()
                .and_then(|handle: HANDLE| {
                    let hglobal = HGLOBAL(handle.0);
                    let ptr = GlobalLock(hglobal);
                    if ptr.is_null() {
                        return None;
                    }
                    let wide_ptr = ptr as *const u16;
                    // UI-C03: 使用 GlobalSize 限制扫描范围，防止越界读
                    let total_bytes = GlobalSize(hglobal) as usize;
                    let max_chars = total_bytes / std::mem::size_of::<u16>();
                    let mut len = 0;
                    while len < max_chars && *wide_ptr.add(len) != 0 {
                        len += 1;
                    }
                    // 如果没有找到 null 终止符，使用全部数据
                    if len >= max_chars {
                        len = max_chars;
                    }
                    let slice = std::slice::from_raw_parts(wide_ptr, len);
                    let _ = GlobalUnlock(hglobal);
                    String::from_utf16(slice).ok()
                });
            let _ = CloseClipboard();
            result
        }
    }

    /// 执行菜单命令
    pub fn execute_command(&mut self, cmd: crate::menu_bar::CommandId, hwnd: HWND) {
        match cmd {
            crate::menu_bar::CommandId::FileNew => {
                self.new_file();
            }
            crate::menu_bar::CommandId::FileNewWindow => {
                // 通过 PostMessage 通知窗口过程创建新窗口
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        hwnd,
                        windows::Win32::UI::WindowsAndMessaging::WM_APP + 2,
                        windows::Win32::Foundation::WPARAM(0),
                        windows::Win32::Foundation::LPARAM(0),
                    );
                }
            }
            crate::menu_bar::CommandId::FileOpen => {
                if let Some(path) = Dialogs::open_file_dialog(hwnd, "打开文件", &[]) {
                    self.load_file(path);
                }
            }
            crate::menu_bar::CommandId::FileOpenFolder => {
                if let Some(path) = Dialogs::open_folder_dialog(hwnd, "打开文件夹") {
                    self.open_folder(path);
                }
            }
            crate::menu_bar::CommandId::FileCloseWorkspace => {
                self.close_workspace();
            }
            crate::menu_bar::CommandId::FileSave => {
                self.save_file();
            }
            crate::menu_bar::CommandId::FileSaveAs => {
                if let Some(path) = Dialogs::save_file_dialog(hwnd, "另存为", "untitled.txt") {
                    self.save_as(path);
                }
            }
            crate::menu_bar::CommandId::FileExit => unsafe {
                windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            },
            crate::menu_bar::CommandId::EditUndo => {
                self.undo();
            }
            crate::menu_bar::CommandId::EditRedo => {
                self.redo();
            }
            crate::menu_bar::CommandId::EditCut => {
                self.cut();
            }
            crate::menu_bar::CommandId::EditCopy => {
                self.copy();
            }
            crate::menu_bar::CommandId::EditPaste => {
                self.paste();
            }
            crate::menu_bar::CommandId::EditFind => {
                self.toggle_find();
            }
            crate::menu_bar::CommandId::EditReplace => {
                self.toggle_replace();
            }
            crate::menu_bar::CommandId::EditSelectAll | crate::menu_bar::CommandId::SelectAll => {
                self.select_all();
            }
            crate::menu_bar::CommandId::ViewToggleSidebar => {
                self.layout.sidebar_visible = !self.layout.sidebar_visible;
            }
            crate::menu_bar::CommandId::ViewToggleActivityBar => {
                self.layout.activity_bar_visible = !self.layout.activity_bar_visible;
            }
            crate::menu_bar::CommandId::ViewToggleStatusBar => {
                self.layout.status_bar_visible = !self.layout.status_bar_visible;
            }
            crate::menu_bar::CommandId::ViewZoomIn => {
                self.status_message = "放大功能即将推出".to_string();
            }
            crate::menu_bar::CommandId::ViewZoomOut => {
                self.status_message = "缩小功能即将推出".to_string();
            }
            crate::menu_bar::CommandId::GotoFile => {
                self.status_message = "转到文件功能即将推出".to_string();
            }
            crate::menu_bar::CommandId::GotoLine => {
                self.status_message = "转到行功能即将推出".to_string();
            }
            crate::menu_bar::CommandId::RunStart => {
                self.status_message = "运行功能即将推出".to_string();
            }
            crate::menu_bar::CommandId::RunDebug => {
                self.status_message = "调试功能即将推出".to_string();
            }
            crate::menu_bar::CommandId::TerminalNew => {
                self.layout.toggle_terminal_panel();
                if self.layout.bottom_panel_visible {
                    self.terminal_panel.focused = true;
                    if !self.terminal_panel.running {
                        let _ = self.terminal_panel.start();
                    }
                    // 启动周期刷新定时器以显示异步 shell 输出
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(
                            self.hwnd, 0xA002, 50, None,
                        );
                    }
                } else {
                    self.terminal_panel.focused = false;
                    unsafe {
                        let _ =
                            windows::Win32::UI::WindowsAndMessaging::KillTimer(self.hwnd, 0xA002);
                    }
                }
                self.status_message = if self.layout.bottom_panel_visible {
                    "终端已打开"
                } else {
                    "终端已关闭"
                }
                .to_string();
            }
            crate::menu_bar::CommandId::HelpAbout => {
                self.status_message = "牧羊人编辑器 v0.1.0".to_string();
            }
            crate::menu_bar::CommandId::None => {}
        }
    }

    pub fn open_folder(&mut self, path: PathBuf) {
        // 异步扫描：先快速同步验证路径可读，再启动后台线程扫描根层
        // 同步预检避免无效路径白白启动线程
        if let Err(e) = std::fs::read_dir(&path) {
            let msg = format!("打开文件夹失败: {}", e);
            self.status_message = msg.clone();
            Dialogs::show_error(self.hwnd, "打开文件夹", &msg);
            return;
        }

        // 工作区信任检查：未信任目录先弹窗询问
        if !crate::dialogs::trusted_folders::is_trusted(&path) {
            let title = "工作区信任";
            let msg = format!(
                "是否信任此文件夹中的代码作者？\n\n{}\n\n\
                 信任后将允许执行 Git 检测、LSP、插件等可能运行该目录中代码的功能。",
                path.display()
            );
            if !Dialogs::confirm_yes_no(self.hwnd, title, &msg) {
                self.status_message = "已取消打开不受信任的工作区".to_string();
                return;
            }
            crate::dialogs::trusted_folders::add_trusted(&path);
        }

        // 设置 loading 状态，立即重绘显示 spinner
        self.is_loading_folder = true;
        self.current_folder = Some(path.clone());
        self.status_message = format!("正在扫描: {}...", path.display());
        self.recent_projects.add(&path);

        let hwnd = self.hwnd;
        let path_clone = path.clone();
        // HWND 不是 Send，但实际只是个指针，PostMessageW 是线程安全的
        // 用 SendHwnd 包装以通过类型检查
        let send_hwnd = SendHwnd(hwnd.0 as usize);
        std::thread::spawn(move || {
            let mut tree = FileTree::new();
            let result = populate_children_one_level(&mut tree, &path_clone, u32::MAX, 0);
            let (tree_opt, error_opt) = match result {
                Ok(_) => (Some(tree), None),
                Err(e) => (None, Some(e.to_string())),
            };
            // 通过 PostMessage 把结果发回 UI 线程：WPARAM 持有 Box raw pointer
            let payload = Box::new(FolderScanResult {
                path: path_clone,
                tree: tree_opt,
                error: error_opt,
            });
            let raw = Box::into_raw(payload) as usize;
            let hwnd = windows::Win32::Foundation::HWND(send_hwnd.0 as *mut std::ffi::c_void);
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::WM_APP + 3,
                    windows::Win32::Foundation::WPARAM(raw),
                    windows::Win32::Foundation::LPARAM(0),
                );
            }
        });
    }

    /// 异步文件夹扫描完成回调（在 UI 线程由 WM_APP+3 调用）
    pub fn on_folder_scan_complete(&mut self, raw: usize) {
        self.is_loading_folder = false;
        // 安全重建 Box
        let payload = unsafe { Box::from_raw(raw as *mut FolderScanResult) };
        match payload.tree {
            Some(tree) => {
                self.file_tree = Some(tree);
                self.git.detect(&payload.path);
                if let Some(branch) = self.git.current_branch_name() {
                    self.status_bar.update_git_branch(Some(&branch));
                } else {
                    self.status_bar.update_git_branch(None);
                }
                self.status_message = format!("已打开文件夹: {}", payload.path.display());
                self.welcome_focus_action = None;
                // 自动打开 README（若存在）
                self.try_open_readme(&payload.path);
            }
            None => {
                let msg = format!(
                    "打开文件夹失败: {}",
                    payload.error.as_deref().unwrap_or("未知错误")
                );
                self.status_message = msg.clone();
                self.current_folder = None;
                Dialogs::show_error(self.hwnd, "打开文件夹", &msg);
            }
        }
    }

    /// C-09: 异步启动 SSH 连接（后台线程执行 connect + list_dir，不阻塞 UI）
    pub fn start_ssh_connect(&mut self, config: aether_remote::ssh::SshConfig) {
        // P0-2: 预检 ssh.exe 是否可用，缺失时引导用户安装并跳转下载页
        if !aether_remote::ssh_available() {
            self.ssh_connecting = false;
            self.ssh_dialog.visible = false;
            self.status_message = format!(
                "未检测到 ssh，请安装 OpenSSH: {}",
                aether_remote::ssh::SSH_DOWNLOAD_URL
            );
            self.ssh_manager_panel.error_message = Some(format!(
                "系统未安装 ssh.exe，请安装 OpenSSH 后重试。\n下载页: {}",
                aether_remote::ssh::SSH_DOWNLOAD_URL
            ));
            return;
        }
        // P0-2: 预检认证方式——密码认证在 shell out 模式下不支持
        if matches!(config.auth, aether_remote::ssh::SshAuth::Password(_)) {
            self.ssh_connecting = false;
            self.ssh_dialog.visible = false;
            self.status_message = "密码认证不支持，请使用密钥或 Agent 认证".to_string();
            self.ssh_manager_panel.error_message =
                Some("密码认证在 shell out 模式下不支持（无 tty 无法交互输入密码），请配置密钥认证或 Agent 认证。".to_string());
            return;
        }
        let host = config.host.clone();
        self.ssh_connecting = true;
        self.ssh_dialog.visible = false;
        self.status_message = format!("正在连接 {}...", host);
        let send_hwnd = SendHwnd(self.hwnd.0 as usize);
        std::thread::spawn(move || {
            let mut session = RemoteSession::new(config);
            let result: SshConnectResult = match session.connect() {
                Ok(()) => match session.list_current_dir() {
                    Ok(entries) => SshConnectResult {
                        session: Some(session),
                        entries: Some(entries),
                        error: None,
                    },
                    Err(e) => SshConnectResult {
                        session: Some(session),
                        entries: None,
                        error: Some(format!("SSH 连接成功，但无法列出目录: {}", e)),
                    },
                },
                Err(e) => SshConnectResult {
                    session: None,
                    entries: None,
                    error: Some(e),
                },
            };
            let raw = Box::into_raw(Box::new(result)) as usize;
            let hwnd = windows::Win32::Foundation::HWND(send_hwnd.0 as *mut std::ffi::c_void);
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::WM_APP + 4,
                    windows::Win32::Foundation::WPARAM(raw),
                    windows::Win32::Foundation::LPARAM(0),
                );
            }
        });
    }

    /// C-09: SSH 连接完成回调（在 UI 线程由 WM_APP+4 调用）
    pub fn on_ssh_connect_complete(&mut self, raw: usize) {
        self.ssh_connecting = false;
        let payload = unsafe { Box::from_raw(raw as *mut SshConnectResult) };
        if let Some(session) = payload.session {
            self.remote_session = Some(session);
            if let Some(entries) = payload.entries {
                self.remote_file_tree = Some(RemoteFileTree::from_entries("/", entries));
                self.sidebar_content = SidebarContent::RemoteFileTree;
                self.status_message = "SSH 连接成功".to_string();
            } else if let Some(e) = payload.error {
                self.status_message = e;
                self.active_ssh_index = None;
            }
        } else if let Some(e) = payload.error {
            // 连接失败：清除活跃索引，在管理面板中显示错误
            self.active_ssh_index = None;
            if self.ssh_dialog.visible {
                self.ssh_dialog.error_message = Some(e);
            } else {
                self.ssh_manager_panel.error_message = Some(e);
            }
            self.status_message = "SSH 连接失败".to_string();
        }
    }

    /// C-09: 异步启动 Git 克隆（后台线程执行 clone_repo，不阻塞 UI）
    pub fn start_git_clone(&mut self, url: String, target_path: PathBuf) {
        // P0-2: 预检 git 是否可用，缺失时引导用户安装并跳转下载页
        if !aether_remote::git_available() {
            self.git_cloning = false;
            self.clone_dialog.visible = true;
            self.clone_dialog.error_message = Some(format!(
                "系统未安装 git，请安装 Git 后重试。\n下载页: {}",
                aether_remote::GIT_DOWNLOAD_URL
            ));
            self.status_message = format!(
                "未检测到 git，请安装 Git: {}",
                aether_remote::GIT_DOWNLOAD_URL
            );
            return;
        }
        self.git_cloning = true;
        self.clone_dialog.visible = false;
        self.status_message = format!("正在克隆 {}...", url);
        let send_hwnd = SendHwnd(self.hwnd.0 as usize);
        std::thread::spawn(move || {
            let result = crate::git::GitIntegration::clone_repo(&url, &target_path);
            let payload = GitCloneResult {
                target_path: target_path.clone(),
                error: result.err(),
            };
            let raw = Box::into_raw(Box::new(payload)) as usize;
            let hwnd = windows::Win32::Foundation::HWND(send_hwnd.0 as *mut std::ffi::c_void);
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::WM_APP + 5,
                    windows::Win32::Foundation::WPARAM(raw),
                    windows::Win32::Foundation::LPARAM(0),
                );
            }
        });
    }

    /// C-09: Git 克隆完成回调（在 UI 线程由 WM_APP+5 调用）
    pub fn on_git_clone_complete(&mut self, raw: usize) {
        self.git_cloning = false;
        let payload = unsafe { Box::from_raw(raw as *mut GitCloneResult) };
        match &payload.error {
            None => {
                self.status_message = format!("克隆成功: {}", payload.target_path.display());
                self.open_folder(payload.target_path.clone());
            }
            Some(e) => {
                // 克隆失败：重新打开对话框并显示错误
                self.clone_dialog.visible = true;
                self.clone_dialog.error_message = Some(e.clone());
                self.status_message = "克隆失败".to_string();
            }
        }
    }

    /// P0-1: 异步加载远程子目录内容（后台线程执行 list_dir，不阻塞 UI）
    ///
    /// 采用与 connect/git_clone 相同的 PostMessage 异步模式。
    /// 因 SshRemoteFs 每次 shell out 调用 ssh（无持久连接），后台线程用
    /// `new_connected` 复用已验证的配置直接列目录，避免重复 connect 探测。
    pub fn start_remote_list_dir(&mut self, path: String) {
        // 取当前活跃会话的配置（克隆后传入后台线程）
        let config = match self.remote_session.as_ref() {
            Some(session) => session.config.clone(),
            None => return,
        };
        // 标记目标节点为 loading（防止重复触发，显示指示器）
        if let Some(tree) = self.remote_file_tree.as_mut() {
            if let Some(node) = tree.find_node_mut(&path) {
                if node.is_loading || (node.children_loaded && node.is_expanded) {
                    // 已在加载或已展开，无需重复触发
                    return;
                }
                node.is_loading = true;
            } else {
                return;
            }
        } else {
            return;
        }
        let send_hwnd = SendHwnd(self.hwnd.0 as usize);
        std::thread::spawn(move || {
            // 复用已验证配置，跳过 connect 探测（主线程已确认连接可用）
            let fs = aether_remote::ssh::SshRemoteFs::new_connected(config);
            let result: SshListDirResult = match fs.list_dir(&path) {
                Ok(entries) => SshListDirResult {
                    path,
                    entries: Some(entries),
                    error: None,
                },
                Err(e) => SshListDirResult {
                    path,
                    entries: None,
                    error: Some(e.to_string()),
                },
            };
            let raw = Box::into_raw(Box::new(result)) as usize;
            let hwnd = windows::Win32::Foundation::HWND(send_hwnd.0 as *mut std::ffi::c_void);
            unsafe {
                let posted = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::WM_APP + 6,
                    windows::Win32::Foundation::WPARAM(raw),
                    windows::Win32::Foundation::LPARAM(0),
                );
                if posted.is_err() {
                    // P2-3: PostMessage 失败（HWND 已销毁），回收内存避免泄漏
                    drop(Box::from_raw(raw as *mut SshListDirResult));
                }
            }
        });
    }

    /// P0-1: 远程子目录列目录完成回调（在 UI 线程由 WM_APP+6 调用）
    pub fn on_ssh_list_dir_complete(&mut self, raw: usize) {
        let payload = unsafe { Box::from_raw(raw as *mut SshListDirResult) };
        // 树可能已被清空（用户断开连接），需防御性检查
        let tree = match self.remote_file_tree.as_mut() {
            Some(t) => t,
            None => return,
        };
        if let Some(entries) = payload.entries {
            tree.expand_node(&payload.path, entries);
            self.status_message = format!("已加载目录: {}", payload.path);
        } else {
            // 加载失败：清除 loading 标志，显示错误
            tree.mark_node_load_failed(&payload.path);
            if let Some(e) = payload.error {
                self.status_message = format!("加载目录失败 {}: {}", payload.path, e);
            }
        }
    }

    // ===== SSH 管理面板方法 =====

    /// 获取已保存的 SSH 服务器配置列表
    pub fn ssh_servers(&self) -> &[aether_shared::settings::SshServerConfig] {
        &self.app_settings.remote.ssh_servers
    }

    /// 当前活跃连接数（0 或 1）
    pub fn active_ssh_count(&self) -> usize {
        if self.active_ssh_index.is_some() && self.remote_session.is_some() {
            1
        } else {
            0
        }
    }

    /// 判断指定索引的服务器是否正在连接中
    pub fn is_ssh_connecting(&self) -> bool {
        self.ssh_connecting
    }

    /// 判断指定索引的服务器是否已连接
    pub fn is_ssh_connected(&self, index: usize) -> bool {
        self.active_ssh_index == Some(index)
            && self
                .remote_session
                .as_ref()
                .map(|s| s.is_connected())
                .unwrap_or(false)
    }

    /// 添加 SSH 服务器配置（从管理面板表单）
    pub fn save_ssh_server_from_form(&mut self) -> std::result::Result<(), String> {
        let config = self.ssh_manager_panel.form_to_config()?;
        if let Some(index) = self.ssh_manager_panel.edit_index {
            // 编辑现有
            if index < self.app_settings.remote.ssh_servers.len() {
                self.app_settings.remote.ssh_servers[index] = config;
            }
        } else {
            // 新增
            self.app_settings.remote.ssh_servers.push(config);
        }
        self.ssh_manager_panel.editing = false;
        self.ssh_manager_panel.edit_index = None;
        // P1-3: 持久化失败时向用户反馈，不再静默丢弃
        if let Err(e) = self.app_settings.save() {
            self.status_message = format!("配置保存失败: {}", e);
        }
        Ok(())
    }

    /// 删除 SSH 服务器配置
    pub fn delete_ssh_server(&mut self, index: usize) {
        if index < self.app_settings.remote.ssh_servers.len() {
            // 如果正在连接该服务器，先断开
            if self.active_ssh_index == Some(index) {
                self.disconnect_ssh();
            }
            // 调整 active_ssh_index（删除后索引偏移）
            if let Some(ai) = self.active_ssh_index {
                if ai > index {
                    self.active_ssh_index = Some(ai - 1);
                } else if ai == index {
                    self.active_ssh_index = None;
                }
            }
            self.app_settings.remote.ssh_servers.remove(index);
            // P1-3: 持久化失败时向用户反馈，不再静默丢弃
            if let Err(e) = self.app_settings.save() {
                self.status_message = format!("配置保存失败: {}", e);
            } else {
                self.status_message = "已删除服务器配置".to_string();
            }
        }
    }

    /// 异步连接指定索引的 SSH 服务器
    pub fn connect_ssh_server(&mut self, index: usize) {
        if self.ssh_connecting {
            return;
        }
        let servers = &self.app_settings.remote.ssh_servers;
        if index >= servers.len() {
            return;
        }
        let config = &servers[index];
        // P0-2: 认证凭证预检——密码认证不支持，在启动后台连接前拦截
        if config.auth_type.as_str() == "password" {
            self.ssh_manager_panel.error_message = Some(
                "密码认证在 shell out 模式下不支持（无 tty 无法交互输入密码），请编辑该服务器配置为密钥认证或 Agent 认证。".to_string(),
            );
            self.status_message = "密码认证不支持，请使用密钥或 Agent 认证".to_string();
            return;
        }
        let ssh_config = crate::ssh::SshManagerPanel::config_to_ssh_config(config);
        let server_name = config.name.clone();
        // 如果已有连接，先断开
        if self.remote_session.is_some() {
            self.disconnect_ssh();
        }
        self.active_ssh_index = Some(index);
        self.start_ssh_connect(ssh_config);
        self.status_message = format!("正在连接 {}...", server_name);
    }

    /// 断开当前 SSH 连接
    pub fn disconnect_ssh(&mut self) {
        if let Some(mut session) = self.remote_session.take() {
            session.disconnect();
        }
        self.active_ssh_index = None;
        self.remote_file_tree = None;
        self.ssh_connecting = false;
        self.status_message = "SSH 已断开".to_string();
    }

    /// 在打开的文件夹根目录查找 README 并自动加载
    /// P2-7: 仅在当前标签页为空且未修改时才自动加载，避免覆盖用户已有内容
    fn try_open_readme(&mut self, folder: &PathBuf) {
        // 当前标签页有内容或未保存的修改时，不自动加载 README
        if self.is_dirty || self.buffer.len_bytes() > 0 || self.file_path.is_some() {
            return;
        }
        let candidates = ["README.md", "README.MD", "README", "readme.md", "Readme.md"];
        for name in candidates {
            let readme_path = folder.join(name);
            if readme_path.is_file() {
                self.load_file(readme_path);
                return;
            }
        }
    }

    pub fn close_workspace(&mut self) {
        self.file_tree = None;
        self.current_folder = None;
        self.file_path = None;
        self.buffer = PieceTable::from_string(String::new());
        self.cursor_line = 0;
        self.cursor_col = 0;
        self.scroll_y = 0.0;
        self.selection_start = None;
        self.selection_end = None;
        self.is_dirty = false;
        self.cached_lines.clear();
        self.cached_tokens.clear();
        self.language = Language::PlainText;
        self.tabs.clear();
        self.tabs.push(crate::tabs::Tab::new());
        self.active_tab = 0;
        self.selected_file_node = None;
        self.welcome_focus_action = None;
        self.git.detect(&std::path::Path::new("."));
        self.status_bar.update_git_branch(None);
        self.status_message = "已关闭工作区".to_string();
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

    fn handle_file_tree_click(&mut self, _mouse_x: f32, mouse_y: f32) -> bool {
        let tree = match self.file_tree.as_ref() {
            Some(t) => t,
            None => return false,
        };

        let mut current_y = 34.0;
        let result = Self::find_tree_click_target(tree, u32::MAX, mouse_y, &mut current_y);

        if let Some((node_idx, kind)) = result {
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
                    return true;
                }
                FileKind::File => {
                    self.selected_file_node = Some(node_idx);
                    if let Some(path) = self.get_node_path(node_idx) {
                        // 检查该文件是否已在某个标签页中打开
                        if let Some(existing_tab) = self
                            .tabs
                            .iter()
                            .position(|tab| tab.file_path.as_ref() == Some(&path))
                        {
                            // 切换到已打开的标签页
                            self.switch_tab(existing_tab);
                        } else {
                            self.load_file(path);
                        }
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn handle_git_panel_click(&mut self, mouse_x: f32, mouse_y: f32) -> bool {
        if !self.git.is_repo() {
            return false;
        }
        // Git 面板布局：分支(30px) + commit输入(30px) + 按钮(30px) + 分隔(5px) + staged + unstaged + untracked
        // 简化实现：根据鼠标位置检测点击的文件或按钮
        let mut current_y = 10.0f32;
        let sidebar_width = self.layout.sidebar_width;
        let item_height = 22.0f32;
        let section_gap = 8.0f32;

        // 跳过标题和分支区域 (约 70px)
        current_y += 70.0;

        // 检测按钮点击 (Commit, Refresh)
        let button_y = current_y;
        if mouse_y >= button_y && mouse_y < button_y + 26.0 {
            if mouse_x >= 10.0 && mouse_x < 70.0 {
                // Commit 按钮
                if !self.git.commit_message.is_empty() {
                    let msg = self.git.commit_message.clone();
                    let _ = self.git.commit(&msg);
                    self.git.commit_message.clear();
                }
                return true;
            } else if mouse_x >= 80.0 && mouse_x < 140.0 {
                // Refresh 按钮
                self.git.refresh();
                return true;
            }
        }
        current_y += 36.0;

        // 检测文件列表点击
        let staged = self.git.staged_files();
        let unstaged = self.git.unstaged_files();
        let untracked = self.git.untracked_files();

        // Staged Changes
        if !staged.is_empty() {
            current_y += section_gap + 20.0; // 标题
            for (file, _status) in &staged {
                if mouse_y >= current_y && mouse_y < current_y + item_height {
                    if mouse_x >= sidebar_width - 30.0 && mouse_x < sidebar_width - 10.0 {
                        // 点击取消暂存
                        let _ = self.git.unstage_file(file);
                    } else {
                        // 点击选择文件，显示 diff
                        self.git.selected_file = Some(file.clone());
                        self.show_git_diff(file, true);
                    }
                    return true;
                }
                current_y += item_height;
            }
            current_y += section_gap;
        }

        // Changes (unstaged)
        if !unstaged.is_empty() {
            current_y += section_gap + 20.0;
            for (file, _status) in &unstaged {
                if mouse_y >= current_y && mouse_y < current_y + item_height {
                    if mouse_x >= sidebar_width - 30.0 && mouse_x < sidebar_width - 10.0 {
                        // 点击暂存
                        let _ = self.git.stage_file(file);
                    } else {
                        self.git.selected_file = Some(file.clone());
                        self.show_git_diff(file, false);
                    }
                    return true;
                }
                current_y += item_height;
            }
            current_y += section_gap;
        }

        // Untracked
        if !untracked.is_empty() {
            current_y += section_gap + 20.0;
            for file in &untracked {
                if mouse_y >= current_y && mouse_y < current_y + item_height {
                    if mouse_x >= sidebar_width - 30.0 && mouse_x < sidebar_width - 10.0 {
                        let _ = self.git.stage_file(file);
                    } else {
                        self.git.selected_file = Some(file.clone());
                    }
                    return true;
                }
                current_y += item_height;
            }
        }

        false
    }

    fn handle_remote_tree_click(&mut self, _mouse_x: f32, mouse_y: f32) -> bool {
        // P0-1: 递归遍历可见节点，按 y 坐标命中目标节点。
        // 在独立作用域内完成对树的只读借用，收集所需信息后释放借用，
        // 避免与后续 &mut self 调用（start_remote_list_dir 等）冲突。
        let (path, is_dir, node_state) = {
            let tree = match self.remote_file_tree.as_ref() {
                Some(t) => t,
                None => return false,
            };
            let node_height = 20.0_f32;
            let mut current_y = 10.0 - self.remote_scroll_y;
            let target =
                Self::find_remote_node_at_y(&tree.nodes, mouse_y, node_height, &mut current_y);
            let (path, is_dir) = match target {
                Some(t) => t,
                None => return false,
            };
            let state = tree
                .find_node(&path)
                .map(|n| (n.is_dir, n.is_expanded, n.children_loaded, n.is_loading));
            (path, is_dir, state)
        };

        if is_dir {
            // P0-1: 目录节点的展开/折叠/懒加载逻辑
            let (is_dir, is_expanded, children_loaded, is_loading) = match node_state {
                Some(s) => s,
                None => return false,
            };
            if !is_dir || is_loading {
                return false;
            }
            if !children_loaded {
                // 首次展开：异步加载子目录
                self.start_remote_list_dir(path);
                return true;
            }
            // 子节点已加载：切换展开/折叠
            if let Some(tree) = self.remote_file_tree.as_mut() {
                if let Some(n) = tree.find_node_mut(&path) {
                    n.is_expanded = !is_expanded;
                }
            }
            true
        } else {
            // 打开远程文件
            self.selected_remote_node = Some(path.clone());
            if let Some(session) = &self.remote_session {
                let remote_path = path.clone();
                match session.read_remote_file(&remote_path) {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content).to_string();
                        let tab = crate::tabs::Tab {
                            file_path: Some(PathBuf::from(format!("remote:{}", remote_path))),
                            buffer: PieceTable::from_string(text),
                            cursor_line: 0,
                            cursor_col: 0,
                            selection_start: None,
                            selection_end: None,
                            scroll_y: 0.0,
                            scroll_x: 0.0,
                            history: History::new(),
                            is_dirty: false,
                            cached_lines: Vec::new(),
                            cached_tokens: Vec::new(),
                            line_cache_versions: Vec::new(),
                            buffer_version: 1,
                            language: Language::PlainText,
                        };
                        self.open_in_new_tab(tab);
                        self.status_message = format!("已打开远程文件: {}", remote_path);
                    }
                    Err(e) => {
                        self.status_message = format!("读取远程文件失败: {}", e);
                    }
                }
            }
            true
        }
    }

    /// P0-1: 递归查找 y 坐标命中的可见远程节点，返回 (path, is_dir)
    ///
    /// `current_y` 从顶部起始（含滚动偏移），逐节点递增 node_height。
    /// 仅遍历展开目录的子节点。
    fn find_remote_node_at_y(
        nodes: &[crate::ssh::RemoteFileNode],
        mouse_y: f32,
        node_height: f32,
        current_y: &mut f32,
    ) -> Option<(String, bool)> {
        for node in nodes {
            if mouse_y >= *current_y && mouse_y < *current_y + node_height {
                return Some((node.path.clone(), node.is_dir));
            }
            *current_y += node_height;
            if node.is_expanded {
                if let Some(found) =
                    Self::find_remote_node_at_y(&node.children, mouse_y, node_height, current_y)
                {
                    return Some(found);
                }
            }
        }
        None
    }

    /// 显示 Git diff 视图
    pub fn show_git_diff(&mut self, file: &str, staged: bool) {
        if let Some(path) = &self.current_folder {
            let args = if staged {
                vec!["diff", "--cached", "--", file]
            } else {
                vec!["diff", "--", file]
            };
            let (stdout, stderr, success) = crate::git::GitCommand::exec(path, &args);
            if success {
                let diff_text = if stdout.is_empty() {
                    format!("// 无差异: {}\n", file)
                } else {
                    stdout
                };
                let tab = crate::tabs::Tab {
                    file_path: Some(PathBuf::from(format!("diff: {}", file))),
                    buffer: PieceTable::from_string(diff_text),
                    cursor_line: 0,
                    cursor_col: 0,
                    selection_start: None,
                    selection_end: None,
                    scroll_y: 0.0,
                    scroll_x: 0.0,
                    history: History::new(),
                    is_dirty: false,
                    cached_lines: Vec::new(),
                    cached_tokens: Vec::new(),
                    line_cache_versions: Vec::new(),
                    buffer_version: 1,
                    language: Language::PlainText,
                };
                self.open_in_new_tab(tab);
                self.status_message = format!("显示 {} 的差异", file);
            } else {
                self.status_message = format!("获取差异失败: {}", stderr);
            }
        }
    }

    /// 更新文件树悬停状态，返回是否需要重绘
    pub fn update_file_tree_hover(&mut self, _mouse_x: f32, mouse_y: f32) -> bool {
        match &self.sidebar_content {
            crate::layout::SidebarContent::FileTree => {
                self.update_local_tree_hover(_mouse_x, mouse_y)
            }
            crate::layout::SidebarContent::RemoteFileTree => self.update_remote_tree_hover(mouse_y),
            _ => {
                let old = self.hover_file_node.take();
                old.is_some()
            }
        }
    }

    fn update_local_tree_hover(&mut self, _mouse_x: f32, mouse_y: f32) -> bool {
        let tree = match self.file_tree.as_ref() {
            Some(t) => t,
            None => {
                let old = self.hover_file_node.take();
                return old.is_some();
            }
        };

        let mut current_y = 34.0;
        let result = Self::find_tree_click_target(tree, u32::MAX, mouse_y, &mut current_y);

        let new_hover = result.map(|(idx, _)| idx);
        let changed = self.hover_file_node != new_hover;
        self.hover_file_node = new_hover;
        changed
    }

    fn update_remote_tree_hover(&mut self, mouse_y: f32) -> bool {
        let tree = match self.remote_file_tree.as_ref() {
            Some(t) => t,
            None => {
                let old = self.hover_remote_node.take();
                return old.is_some();
            }
        };
        // P0-1: 递归遍历可见节点确定悬停目标（按路径标识）
        let node_height = 20.0_f32;
        let mut current_y = 10.0 - self.remote_scroll_y;
        let new_hover =
            match Self::find_remote_node_at_y(&tree.nodes, mouse_y, node_height, &mut current_y) {
                Some((path, _)) => Some(path),
                None => None,
            };
        let changed = self.hover_remote_node != new_hover;
        self.hover_remote_node = new_hover;
        changed
    }

    /// 处理 SSH 对话框点击
    pub fn handle_ssh_dialog_click(
        &mut self,
        mouse_x: f32,
        mouse_y: f32,
    ) -> Option<crate::ssh::DialogAction> {
        if let Some(rect) = &self.ssh_dialog.connect_btn_rect {
            if rect.contains(mouse_x, mouse_y) {
                self.ssh_dialog.hover_button = Some(0);
                return Some(crate::ssh::DialogAction::Connect);
            }
        }
        if let Some(rect) = &self.ssh_dialog.cancel_btn_rect {
            if rect.contains(mouse_x, mouse_y) {
                self.ssh_dialog.hover_button = Some(1);
                return Some(crate::ssh::DialogAction::Cancel);
            }
        }
        self.ssh_dialog.hover_button = None;
        Some(crate::ssh::DialogAction::None)
    }

    /// 处理克隆对话框点击
    pub fn handle_clone_dialog_click(
        &mut self,
        mouse_x: f32,
        mouse_y: f32,
    ) -> Option<crate::ssh::DialogAction> {
        if let Some(rect) = &self.clone_dialog.clone_btn_rect {
            if rect.contains(mouse_x, mouse_y) {
                self.clone_dialog.hover_button = Some(0);
                return Some(crate::ssh::DialogAction::Connect);
            }
        }
        if let Some(rect) = &self.clone_dialog.cancel_btn_rect {
            if rect.contains(mouse_x, mouse_y) {
                self.clone_dialog.hover_button = Some(1);
                return Some(crate::ssh::DialogAction::Cancel);
            }
        }
        self.clone_dialog.hover_button = None;
        Some(crate::ssh::DialogAction::None)
    }

    /// 处理 SSH 对话框键盘输入
    pub fn handle_ssh_dialog_key(&mut self, ch: char) {
        // P2-4: 复用 paste 路径的字段解析逻辑
        // 先读取 focus_field 避免与可变借用冲突
        let focus = self.ssh_dialog.focus_field;
        if let Some(field) = self.ssh_dialog_active_field_mut() {
            // port 字段（focus_field == 1）只接受数字
            let is_port = focus == 1;
            if !is_port || ch.is_ascii_digit() {
                field.push(ch);
            }
        }
    }

    /// P2-4: 返回 SSH 对话框当前 focus_field 对应的可变字段引用
    fn ssh_dialog_active_field_mut(&mut self) -> Option<&mut String> {
        match self.ssh_dialog.focus_field {
            0 => Some(&mut self.ssh_dialog.host),
            1 => Some(&mut self.ssh_dialog.port),
            2 => Some(&mut self.ssh_dialog.username),
            3 => match self.ssh_dialog.auth_type {
                crate::ssh::SshAuthType::Password => Some(&mut self.ssh_dialog.password),
                crate::ssh::SshAuthType::Key => Some(&mut self.ssh_dialog.key_path),
                crate::ssh::SshAuthType::Agent => None,
            },
            4 => Some(&mut self.ssh_dialog.key_passphrase),
            _ => None,
        }
    }

    /// P2-4: 向 SSH 对话框当前字段粘贴剪贴板内容（port 字段过滤非数字）
    pub fn paste_into_ssh_dialog(&mut self) {
        if let Some(text) = Self::get_clipboard_text() {
            // 先读取 focus_field 避免与可变借用冲突
            let focus = self.ssh_dialog.focus_field;
            if let Some(field) = self.ssh_dialog_active_field_mut() {
                let is_port = focus == 1;
                if is_port {
                    // 仅保留数字字符
                    field.extend(text.chars().filter(|c| c.is_ascii_digit()));
                } else {
                    // 移除换行/回车避免破坏单行输入
                    field.extend(text.chars().filter(|c| *c != '\n' && *c != '\r'));
                }
            }
        }
    }

    /// P2-4: 向克隆对话框 URL 字段粘贴剪贴板内容
    pub fn paste_into_clone_dialog(&mut self) {
        if let Some(text) = Self::get_clipboard_text() {
            // 移除换行/回车
            self.clone_dialog
                .url
                .extend(text.chars().filter(|c| *c != '\n' && *c != '\r'));
        }
    }

    /// 处理 SSH 对话框退格
    pub fn handle_ssh_dialog_backspace(&mut self) {
        match self.ssh_dialog.focus_field {
            0 => {
                self.ssh_dialog.host.pop();
            }
            1 => {
                self.ssh_dialog.port.pop();
            }
            2 => {
                self.ssh_dialog.username.pop();
            }
            3 => match self.ssh_dialog.auth_type {
                crate::ssh::SshAuthType::Password => {
                    self.ssh_dialog.password.pop();
                }
                crate::ssh::SshAuthType::Key => {
                    self.ssh_dialog.key_path.pop();
                }
                crate::ssh::SshAuthType::Agent => {}
            },
            4 => {
                self.ssh_dialog.key_passphrase.pop();
            }
            _ => {}
        }
    }

    /// 处理克隆对话框键盘输入
    pub fn handle_clone_dialog_key(&mut self, ch: char) {
        self.clone_dialog.url.push(ch);
    }

    /// 处理克隆对话框退格
    pub fn handle_clone_dialog_backspace(&mut self) {
        self.clone_dialog.url.pop();
    }

    /// 根据当前打开的文件路径同步文件树选中状态
    pub fn sync_file_tree_selection(&mut self) {
        if let Some(ref path) = self.file_path {
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

    fn find_node_by_path(tree: &FileTree, target: &PathBuf, base: &PathBuf) -> Option<u32> {
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

    fn get_node_path(&self, node_idx: u32) -> Option<PathBuf> {
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
    fn ensure_node_loaded(&mut self, node_idx: u32) -> bool {
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

    fn find_tree_click_target(
        tree: &FileTree,
        parent_idx: u32,
        mouse_y: f32,
        current_y: &mut f32,
    ) -> Option<(u32, FileKind)> {
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

                if mouse_y >= *current_y && mouse_y < *current_y + 20.0 {
                    return Some((idx, node.kind));
                }
                *current_y += 20.0;

                // 如果目录展开，递归查找子节点
                if node.kind == FileKind::Directory && node.is_expanded {
                    if let Some(result) =
                        Self::find_tree_click_target(tree, idx, mouse_y, current_y)
                    {
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

    pub fn insert_char(&mut self, ch: char) {
        // P1-4: 自动配对括号
        if self.try_auto_pair(ch) {
            return;
        }

        let pos = self.cursor_byte_pos();
        let before_pieces = self.buffer.get_pieces();
        let before_add_len = self.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

        let text = ch.to_string();
        self.buffer.insert(pos, &text);
        self.cursor_col += ch.len_utf8();
        self.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.is_dirty = true;
        }
        self.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.cursor_line, self.cursor_col);
        self.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
        );
        self.status_message = "已修改".to_string();
    }

    /// P1-4: 尝试自动配对括号。
    /// 返回 true 表示已处理（调用方不应再执行默认插入）。
    /// 规则：
    /// 1. 输入开括号 `( [ { ' "`，且无选区：插入配对，光标居中
    /// 2. 输入开括号且有选区：包裹选区（开括号在选区前，闭括号在选区后）
    /// 3. 输入闭括号 `) ] }`，且光标后已是相同闭括号：跳过插入，光标右移
    fn try_auto_pair(&mut self, ch: char) -> bool {
        // 开括号 → 闭括号映射
        let pair_close = match ch {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            '\'' => Some('\''),
            '"' => Some('"'),
            _ => None,
        };

        // 闭括号跳过逻辑：光标后已是相同闭括号，直接右移光标
        let is_skip_close = matches!(ch, ')' | ']' | '}');
        if is_skip_close {
            if let Some(text) = self.buffer.get_line(self.cursor_line) {
                if self.cursor_col < text.len() {
                    if let Some(next_ch) = text[self.cursor_col..].chars().next() {
                        if next_ch == ch {
                            // 跳过插入，光标右移一个字符
                            self.cursor_col += ch.len_utf8();
                            return true;
                        }
                    }
                }
            }
            return false;
        }

        let close_ch = match pair_close {
            Some(c) => c,
            None => return false,
        };

        // 检查是否有选区
        let has_selection = self
            .selection_start
            .zip(self.selection_end)
            .map(|(s, e)| s != e)
            .unwrap_or(false);

        let pos = self.cursor_byte_pos();
        let before_pieces = self.buffer.get_pieces();
        let before_add_len = self.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

        if has_selection {
            // 包裹选区：在选区开始处插入开括号，在选区结束处插入闭括号
            let (sel_start_line, sel_start_col) = self.selection_start.unwrap();
            let (sel_end_line, sel_end_col) = self.selection_end.unwrap();
            // 确保 start < end
            let (start_line, start_col, end_line, end_col) =
                if (sel_start_line, sel_start_col) <= (sel_end_line, sel_end_col) {
                    (sel_start_line, sel_start_col, sel_end_line, sel_end_col)
                } else {
                    (sel_end_line, sel_end_col, sel_start_line, sel_start_col)
                };

            let start_byte = self.line_col_to_byte(start_line, start_col);
            let end_byte = self.line_col_to_byte(end_line, end_col);

            // 插入闭括号在前（避免位置偏移），开括号在后
            let close_str = close_ch.to_string();
            let open_str = ch.to_string();
            self.buffer.insert(end_byte, &close_str);
            self.buffer.insert(start_byte, &open_str);

            // 更新选区：保持选中文本不变，扩展到包含括号
            self.selection_start = Some((start_line, start_col));
            self.selection_end = Some((end_line, end_col + close_ch.len_utf8()));

            self.is_dirty = true;
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.is_dirty = true;
            }
            self.buffer_version += 1;

            let cursor_after = CursorPosition::new(self.cursor_line, self.cursor_col);
            self.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Insert,
                pos,
            );
            self.status_message = "已修改".to_string();
            return true;
        }

        // 无选区：插入开括号 + 闭括号，光标置于中间
        let pair_text = format!("{}{}", ch, close_ch);
        self.buffer.insert(pos, &pair_text);
        // 光标移动到开括号之后（不前进到闭括号）
        self.cursor_col += ch.len_utf8();

        self.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.is_dirty = true;
        }
        self.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.cursor_line, self.cursor_col);
        self.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
        );
        self.status_message = "已修改".to_string();
        true
    }

    /// P0-2: 设置 IME 合成串（pre-edit text）。
    /// 在 WM_IME_COMPOSITION 收到 GCS_COMPSTR 时调用，
    /// 清空已存在的合成串后写入新值，并触发重绘。
    pub fn set_composition(&mut self, text: String) {
        self.composition = Some(text);
    }

    /// P0-2: 提交合成串为正式文本。
    /// 在 WM_IME_COMPOSITION 收到 GCS_RESULTSTR 或 WM_IME_ENDCOMPOSITION 时调用。
    /// 先清除合成串，再将提交文本逐字符插入到光标处。
    pub fn commit_composition(&mut self, text: String) {
        // 清除合成状态显示
        self.composition = None;
        if text.is_empty() {
            return;
        }
        for ch in text.chars() {
            self.broadcast_insert_char(ch);
        }
    }

    /// P0-2: 清除合成串（用户取消输入或 IME 失焦时调用）。
    pub fn clear_composition(&mut self) {
        self.composition = None;
    }

    pub fn insert_tab(&mut self) {
        let pos = self.cursor_byte_pos();
        let before_pieces = self.buffer.get_pieces();
        let before_add_len = self.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

        let tab_text = "    ";
        self.buffer.insert(pos, tab_text);
        self.cursor_col += tab_text.len();
        self.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.is_dirty = true;
        }
        self.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.cursor_line, self.cursor_col);
        self.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
        );
        self.status_message = "已修改".to_string();
    }

    pub fn insert_newline(&mut self) {
        let pos = self.cursor_byte_pos();
        let before_pieces = self.buffer.get_pieces();
        let before_add_len = self.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

        // 获取当前行的前导空白（用于自动缩进）
        let indent = if let Some(line_text) = self.buffer.get_line(self.cursor_line) {
            let leading_ws: String = line_text
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();
            leading_ws
        } else {
            String::new()
        };

        // 检测是否需要额外缩进（行尾有 { 或 :）
        let extra_indent = if let Some(line_text) = self.buffer.get_line(self.cursor_line) {
            let trimmed = line_text.trim_end();
            if trimmed.ends_with('{') || trimmed.ends_with(':') {
                "    "
            } else {
                ""
            }
        } else {
            ""
        };

        let full_indent = format!("{}{}", indent, extra_indent);
        let insert_text = if full_indent.is_empty() {
            "\n".to_string()
        } else {
            format!("\n{}", full_indent)
        };

        self.buffer.insert(pos, &insert_text);
        self.cursor_line += 1;
        self.cursor_col = full_indent.len();
        self.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.is_dirty = true;
        }
        self.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.cursor_line, self.cursor_col);
        self.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
        );
        self.status_message = "已修改".to_string();
    }

    pub fn delete_char(&mut self) {
        if self.cursor_col > 0 {
            let pos = self.cursor_byte_pos();
            let prev_pos = self.find_prev_char_boundary(pos);
            if prev_pos < pos {
                let before_pieces = self.buffer.get_pieces();
                let before_add_len = self.buffer.add_buffer_len();
                let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

                self.buffer.delete(prev_pos, pos);
                self.cursor_col -= pos - prev_pos;
                self.is_dirty = true;
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    tab.is_dirty = true;
                }
                self.buffer_version += 1;

                let cursor_after = CursorPosition::new(self.cursor_line, self.cursor_col);
                self.history.record(
                    before_pieces,
                    before_add_len,
                    cursor_before,
                    cursor_after,
                    OpType::Delete,
                    prev_pos,
                );
                self.status_message = "已修改".to_string();
            }
        } else if self.cursor_line > 0 {
            let prev_line = self.cursor_line - 1;
            if let Some(prev_text) = self.buffer.get_line(prev_line) {
                let prev_len = prev_text.len();
                if let Some(curr_text) = self.buffer.get_line(self.cursor_line) {
                    let curr_len = curr_text.len();
                    let start = self.line_byte_start(prev_line) + prev_len;
                    let end = start + curr_len + 1;

                    let before_pieces = self.buffer.get_pieces();
                    let before_add_len = self.buffer.add_buffer_len();
                    let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

                    self.buffer.delete(start, end);
                    self.cursor_line = prev_line;
                    self.cursor_col = prev_len;
                    self.is_dirty = true;
                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                        tab.is_dirty = true;
                    }
                    self.buffer_version += 1;

                    let cursor_after = CursorPosition::new(self.cursor_line, self.cursor_col);
                    self.history.record(
                        before_pieces,
                        before_add_len,
                        cursor_before,
                        cursor_after,
                        OpType::Delete,
                        start,
                    );
                    self.status_message = "已修改".to_string();
                }
            }
        }
    }

    pub fn delete_forward(&mut self) {
        let pos = self.cursor_byte_pos();
        let next_pos = self.find_next_char_boundary(pos);
        if next_pos > pos {
            let before_pieces = self.buffer.get_pieces();
            let before_add_len = self.buffer.add_buffer_len();
            let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

            self.buffer.delete(pos, next_pos);
            self.is_dirty = true;
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.is_dirty = true;
            }
            self.buffer_version += 1;

            let cursor_after = CursorPosition::new(self.cursor_line, self.cursor_col);
            self.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Delete,
                pos,
            );
            self.status_message = "已修改".to_string();
        }
    }

    /// 多光标编辑操作广播
    /// 将插入、删除等操作应用到所有光标位置
    /// 从后往前执行，避免位置偏移问题
    pub fn broadcast_insert_char(&mut self, ch: char) {
        if self.multi_cursor.cursor_count() <= 1 {
            // 单光标模式，直接插入
            self.insert_char(ch);
            return;
        }

        // 多光标模式：从后往前插入
        let cursors: Vec<_> = self.multi_cursor.cursors.clone();
        for cursor in cursors.iter().rev() {
            let pos = self.line_col_to_byte(cursor.line, cursor.col);
            self.buffer.insert(pos, &ch.to_string());
        }

        // 更新所有光标位置
        for cursor in &mut self.multi_cursor.cursors {
            cursor.col += ch.len_utf8();
        }

        self.is_dirty = true;
        self.buffer_version += 1;
        self.status_message = format!("已在 {} 个位置插入", self.multi_cursor.cursor_count());
    }

    /// 多光标删除（退格）广播
    pub fn broadcast_delete_char(&mut self) {
        if self.multi_cursor.cursor_count() <= 1 {
            self.delete_char();
            return;
        }

        // 先计算所有需要删除的位置
        let mut delete_positions: Vec<(usize, usize)> = Vec::new();
        for cursor in self.multi_cursor.cursors.iter().rev() {
            if cursor.col > 0 {
                let pos = self.line_col_to_byte(cursor.line, cursor.col);
                let prev_pos = self.find_prev_char_boundary(pos);
                if prev_pos < pos {
                    delete_positions.push((prev_pos, pos));
                }
            }
        }

        // 执行删除
        for (start, end) in delete_positions {
            self.buffer.delete(start, end);
        }

        // 更新所有光标位置（重新计算）
        for i in 0..self.multi_cursor.cursors.len() {
            let cursor = &self.multi_cursor.cursors[i];
            if cursor.col > 0 {
                let pos = self.line_col_to_byte(cursor.line, cursor.col);
                let prev_pos = self.find_prev_char_boundary(pos);
                let new_col = prev_pos - self.line_byte_start(cursor.line);
                self.multi_cursor.cursors[i].col = new_col;
            }
        }

        self.is_dirty = true;
        self.buffer_version += 1;
    }

    /// 多光标插入换行广播
    pub fn broadcast_insert_newline(&mut self) {
        if self.multi_cursor.cursor_count() <= 1 {
            self.insert_newline();
            return;
        }

        let cursors: Vec<_> = self.multi_cursor.cursors.clone();
        for cursor in cursors.iter().rev() {
            let pos = self.line_col_to_byte(cursor.line, cursor.col);
            self.buffer.insert(pos, "\n");
        }

        // 更新所有光标位置
        for cursor in &mut self.multi_cursor.cursors {
            cursor.line += 1;
            cursor.col = 0;
        }

        self.is_dirty = true;
        self.buffer_version += 1;
    }

    /// 撤销
    pub fn undo(&mut self) {
        let current_pieces = self.buffer.get_pieces();
        let current_add_len = self.buffer.add_buffer_len();
        let current_cursor = CursorPosition::new(self.cursor_line, self.cursor_col);

        if let Some((pieces, add_len, cursor)) =
            self.history
                .undo(current_pieces, current_add_len, current_cursor)
        {
            self.buffer.restore(pieces, add_len);
            self.cursor_line = cursor.line;
            self.cursor_col = cursor.column;
            self.is_dirty = true;
            self.buffer_version += 1;
            self.status_message = "已撤销".to_string();
        }
    }

    /// 重做
    pub fn redo(&mut self) {
        let current_pieces = self.buffer.get_pieces();
        let current_add_len = self.buffer.add_buffer_len();
        let current_cursor = CursorPosition::new(self.cursor_line, self.cursor_col);

        if let Some((pieces, add_len, cursor)) =
            self.history
                .redo(current_pieces, current_add_len, current_cursor)
        {
            self.buffer.restore(pieces, add_len);
            self.cursor_line = cursor.line;
            self.cursor_col = cursor.column;
            self.is_dirty = true;
            self.buffer_version += 1;
            self.status_message = "已重做".to_string();
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_col > 0 {
            if let Some(text) = self.buffer.get_line(self.cursor_line) {
                let col = self.cursor_col.min(text.len());
                if let Some(ch) = text[..col].chars().next_back() {
                    self.cursor_col = col - ch.len_utf8();
                } else {
                    self.cursor_col = 0;
                }
            }
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            if let Some(text) = self.buffer.get_line(self.cursor_line) {
                self.cursor_col = text.len();
            }
        }
    }

    pub fn move_cursor_right(&mut self) {
        if let Some(text) = self.buffer.get_line(self.cursor_line) {
            if self.cursor_col < text.len() {
                if let Some(ch) = text[self.cursor_col..].chars().next() {
                    self.cursor_col += ch.len_utf8();
                }
            } else if self.cursor_line + 1 < self.buffer.len_lines() {
                self.cursor_line += 1;
                self.cursor_col = 0;
            }
        }
    }

    pub fn move_cursor_up(&mut self) {
        if self.cursor_line > 0 {
            self.cursor_line -= 1;
            if let Some(text) = self.buffer.get_line(self.cursor_line) {
                self.cursor_col = self.cursor_col.min(text.len());
            }
        }
    }

    pub fn move_cursor_down(&mut self) {
        if self.cursor_line + 1 < self.buffer.len_lines() {
            self.cursor_line += 1;
            if let Some(text) = self.buffer.get_line(self.cursor_line) {
                self.cursor_col = self.cursor_col.min(text.len());
            }
        }
    }

    pub fn move_cursor_home(&mut self) {
        self.cursor_col = 0;
    }

    pub fn move_cursor_end(&mut self) {
        if let Some(text) = self.buffer.get_line(self.cursor_line) {
            self.cursor_col = text.len();
        }
    }

    /// P1-6: Smart Home - 跳到行首首个非空白字符。
    /// 若光标已在首个非空白位置，再按一次跳到行首（col=0）。
    /// 通过传入 `already_at_smart_home` 判断是否为第二次按 Home。
    pub fn move_cursor_smart_home(&mut self, already_at_smart_home: bool) {
        if already_at_smart_home {
            self.cursor_col = 0;
            return;
        }
        if let Some(text) = self.buffer.get_line(self.cursor_line) {
            let first_non_ws = text
                .char_indices()
                .skip_while(|(_, c)| c.is_whitespace())
                .map(|(i, _)| i)
                .next()
                .unwrap_or(text.len());
            self.cursor_col = first_non_ws;
        }
    }

    /// P1-6: 移动到文件首行
    pub fn move_cursor_file_start(&mut self) {
        self.cursor_line = 0;
        self.cursor_col = 0;
    }

    /// P1-6: 移动到文件末行末列
    pub fn move_cursor_file_end(&mut self) {
        let last_line = self.buffer.len_lines().saturating_sub(1);
        self.cursor_line = last_line;
        if let Some(text) = self.buffer.get_line(self.cursor_line) {
            self.cursor_col = text.len();
        }
    }

    /// P1-6: 向左移动一个单词。
    /// 跳过当前空白，再跳到上一个单词边界。
    pub fn move_cursor_word_left(&mut self) {
        if let Some(text) = self.buffer.get_line(self.cursor_line) {
            let chars: Vec<char> = text.chars().collect();
            let mut idx = self
                .cursor_col
                .min(text.len())
                .saturating_sub(if self.cursor_col > 0 { 1 } else { 0 });

            // 跳过空白
            while idx > 0 && chars[idx - 1].is_whitespace() {
                idx -= 1;
            }
            // 跳过当前单词
            let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
            if idx > 0 && is_word_char(chars[idx - 1]) {
                while idx > 0 && is_word_char(chars[idx - 1]) {
                    idx -= 1;
                }
            } else if idx > 0 {
                // 非单词字符：跳过一个符号
                idx -= 1;
            }
            // 转回字节偏移
            let mut byte_col = 0;
            for (i, c) in chars.iter().enumerate() {
                if i >= idx {
                    break;
                }
                byte_col += c.len_utf8();
            }
            self.cursor_col = byte_col;
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.move_cursor_end();
        }
    }

    /// P1-6: 向右移动一个单词。
    pub fn move_cursor_word_right(&mut self) {
        if let Some(text) = self.buffer.get_line(self.cursor_line) {
            let chars: Vec<char> = text.chars().collect();
            let idx = self.cursor_col;
            let mut char_idx = text[..idx.min(text.len())].chars().count();

            // 跳过空白
            while char_idx < chars.len() && chars[char_idx].is_whitespace() {
                char_idx += 1;
            }
            // 跳过当前单词
            let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
            if char_idx < chars.len() && is_word_char(chars[char_idx]) {
                while char_idx < chars.len() && is_word_char(chars[char_idx]) {
                    char_idx += 1;
                }
            } else if char_idx < chars.len() {
                char_idx += 1;
            }
            // 转回字节偏移
            let mut byte_col = 0;
            for (i, c) in chars.iter().enumerate() {
                if i >= char_idx {
                    break;
                }
                byte_col += c.len_utf8();
            }
            self.cursor_col = byte_col;
        } else if self.cursor_line + 1 < self.buffer.len_lines() {
            self.cursor_line += 1;
            self.cursor_col = 0;
        }
    }

    /// P1-6: 切换行注释（按语言决定注释符号）。
    /// 当前行已有注释符号则移除，否则添加。
    pub fn toggle_line_comment(&mut self) {
        let comment_prefix = match self.language {
            Language::Rust
            | Language::C
            | Language::JavaScript
            | Language::TypeScript
            | Language::Json => "// ",
            Language::Python | Language::Toml => "# ",
            _ => return, // 不支持的语言（如 PlainText/Markdown/Html/Css）直接返回
        };

        let line_idx = self.cursor_line;
        let line = match self.buffer.get_line(line_idx) {
            Some(s) => s,
            None => return,
        };

        // 检测是否已有注释前缀
        let stripped = line.strip_prefix(comment_prefix);
        let pos = self.line_byte_start(line_idx);
        let before_pieces = self.buffer.get_pieces();
        let before_add_len = self.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

        if let Some(_rest) = stripped {
            // 已有注释：移除前缀
            let remove_len = comment_prefix.len();
            self.buffer.delete(pos, pos + remove_len);
            // 光标列前移
            self.cursor_col = self.cursor_col.saturating_sub(remove_len);
        } else {
            // 无注释：在行首添加前缀
            self.buffer.insert(pos, comment_prefix);
            // 光标列后移
            self.cursor_col += comment_prefix.len();
        }

        self.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.is_dirty = true;
        }
        self.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.cursor_line, self.cursor_col);
        self.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
        );
        self.status_message = "已切换注释".to_string();
    }

    /// P1-6: 在下一行同一列添加光标（Ctrl+Alt+Down）。
    pub fn add_cursor_line_below(&mut self) {
        let line = self.cursor_line;
        let col = self.cursor_col;
        if line + 1 < self.buffer.len_lines() {
            let new_line = line + 1;
            // 钳制 col 到新行长度
            let max_col = self
                .buffer
                .get_line(new_line)
                .map(|s| s.len())
                .unwrap_or(col);
            self.multi_cursor
                .add_cursor(Cursor::new(new_line, col.min(max_col)));
            self.cursor_line = new_line;
            self.cursor_col = col.min(max_col);
            self.status_message =
                format!("已添加光标（共 {} 处）", self.multi_cursor.cursor_count());
        }
    }

    /// P1-6: 在上一行同一列添加光标（Ctrl+Alt+Up）。
    pub fn add_cursor_line_above(&mut self) {
        let line = self.cursor_line;
        let col = self.cursor_col;
        if line > 0 {
            let new_line = line - 1;
            let max_col = self
                .buffer
                .get_line(new_line)
                .map(|s| s.len())
                .unwrap_or(col);
            self.multi_cursor
                .add_cursor(Cursor::new(new_line, col.min(max_col)));
            self.cursor_line = new_line;
            self.cursor_col = col.min(max_col);
            self.status_message =
                format!("已添加光标（共 {} 处）", self.multi_cursor.cursor_count());
        }
    }

    /// P1-6: 添加下一个相同单词的光标（Ctrl+D）。
    /// 找到当前选中文本或光标所在单词的下一个出现位置，添加光标。
    pub fn add_cursor_at_next_occurrence(&mut self) {
        // 获取当前要查找的文本（来自选区或光标所在单词）
        let search_text = if let (Some((sline, scol)), Some((eline, ecol))) =
            (self.selection_start, self.selection_end)
        {
            if sline == eline {
                let s = self.line_col_to_byte(sline, scol);
                let e = self.line_col_to_byte(eline, ecol);
                if s < e {
                    self.buffer.get_text(s, e)
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            // 取光标所在单词
            if let Some(text) = self.buffer.get_line(self.cursor_line) {
                let chars: Vec<char> = text.chars().collect();
                let char_idx = text[..self.cursor_col.min(text.len())].chars().count();
                let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
                if char_idx < chars.len() && is_word_char(chars[char_idx]) {
                    // 找单词边界
                    let mut start = char_idx;
                    while start > 0 && is_word_char(chars[start - 1]) {
                        start -= 1;
                    }
                    let mut end = char_idx;
                    while end < chars.len() && is_word_char(chars[end]) {
                        end += 1;
                    }
                    let mut byte_start = 0;
                    let mut byte_end = 0;
                    for (i, c) in chars.iter().enumerate() {
                        if i < start {
                            byte_start += c.len_utf8();
                        }
                        if i < end {
                            byte_end += c.len_utf8();
                        }
                    }
                    text[byte_start..byte_end].to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        };

        if search_text.is_empty() {
            return;
        }

        // 从当前光标位置开始向后查找
        let start_byte = self.cursor_byte_pos() + search_text.len();
        let total_bytes = self.buffer.len_bytes();
        let text_after = self.buffer.get_text(start_byte, total_bytes);

        if let Some(rel_pos) = text_after.find(&search_text) {
            let abs_byte = start_byte + rel_pos;
            // 转换为 (line, col)
            let (line, col) = self.byte_to_line_col(abs_byte);
            self.multi_cursor.add_cursor(Cursor::new(line, col));
            self.cursor_line = line;
            self.cursor_col = col;
            self.selection_start = Some((line, col));
            self.selection_end = Some((line, col + search_text.len()));
            self.status_message =
                format!("已添加光标（共 {} 处）", self.multi_cursor.cursor_count());
        }
    }

    pub fn set_cursor_from_mouse(
        &mut self,
        mouse_x: f32,
        mouse_y: f32,
        editor_x: f32,
        editor_y: f32,
    ) {
        let line_height = self.text_renderer.line_height();
        let char_width = self.text_renderer.char_width();
        let line_number_width = 60.0;

        // P0-3: 鼠标 x 加上 scroll_x 抵消，确保点击的字符位置正确
        let rel_x = mouse_x - editor_x - line_number_width - 5.0 + self.scroll_x;
        let rel_y = mouse_y - editor_y + self.scroll_y;

        let line = (rel_y / line_height) as usize;
        let char_col = (rel_x / char_width).max(0.0) as usize;

        let total_lines = self.buffer.len_lines();
        self.cursor_line = line.min(total_lines.saturating_sub(1));

        if let Some(text) = self.buffer.get_line(self.cursor_line) {
            // 将字符列转换为字节偏移，对齐到字符边界
            let mut byte_col = 0usize;
            for (i, ch) in text.chars().enumerate() {
                if i >= char_col {
                    break;
                }
                byte_col += ch.len_utf8();
            }
            self.cursor_col = byte_col.min(text.len());
        } else {
            self.cursor_col = 0;
        }
    }

    pub fn start_selection(&mut self) {
        self.selection_start = Some((self.cursor_line, self.cursor_col));
        self.selection_end = Some((self.cursor_line, self.cursor_col));
        self.is_selecting = true;
    }

    pub fn update_selection(&mut self) {
        if self.is_selecting {
            self.selection_end = Some((self.cursor_line, self.cursor_col));
        }
    }

    pub fn end_selection(&mut self) {
        self.is_selecting = false;
    }

    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
    }

    /// P2-5: 双击选词。基于鼠标位置定位到 (line, byte_col)，然后在当前行
    /// 选择光标下的"词"（连续的字母/数字/下划线为词；否则选单个字符）。
    pub fn select_word_at_mouse(
        &mut self,
        mouse_x: f32,
        mouse_y: f32,
        editor_x: f32,
        editor_y: f32,
    ) {
        // 先把光标定位到点击位置
        self.set_cursor_from_mouse(mouse_x, mouse_y, editor_x, editor_y);
        let line_idx = self.cursor_line;
        let byte_col = self.cursor_col;
        let line_text = match self.buffer.get_line(line_idx) {
            Some(t) => t,
            None => return,
        };
        // 把字节偏移转换为 char 索引
        let mut byte_to_char: Vec<usize> = Vec::with_capacity(line_text.len() + 1);
        let mut acc = 0usize;
        byte_to_char.push(0);
        for ch in line_text.chars() {
            acc += ch.len_utf8();
            byte_to_char.push(acc);
        }
        let total_chars = line_text.chars().count();
        // byte_col 可能等于 line_text.len()（行末）
        let click_char_idx = byte_to_char
            .iter()
            .position(|&b| b >= byte_col)
            .map(|i| i.min(total_chars))
            .unwrap_or(total_chars);
        let chars: Vec<char> = line_text.chars().collect();
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
        let (start_char, end_char) = if click_char_idx >= total_chars {
            // 行末：选择最后一字符（若存在）
            let s = total_chars.saturating_sub(1);
            (s, total_chars)
        } else {
            let c = chars[click_char_idx];
            if is_word_char(c) {
                // 向左扩展
                let mut s = click_char_idx;
                while s > 0 && is_word_char(chars[s - 1]) {
                    s -= 1;
                }
                // 向右扩展（end 为排他边界）
                let mut e = click_char_idx + 1;
                while e < total_chars && is_word_char(chars[e]) {
                    e += 1;
                }
                (s, e)
            } else {
                // 分隔符：选这一字符
                (click_char_idx, click_char_idx + 1)
            }
        };
        // 把字符索引转回字节偏移
        let start_byte = byte_to_char.get(start_char).copied().unwrap_or(0);
        let end_byte = byte_to_char
            .get(end_char)
            .copied()
            .unwrap_or(line_text.len());
        self.selection_start = Some((line_idx, start_byte));
        self.selection_end = Some((line_idx, end_byte));
        self.cursor_col = end_byte;
        self.is_selecting = false;
    }

    pub fn get_selected_text(&self) -> Option<String> {
        let (start_line, start_col) = self.selection_start?;
        let (end_line, end_col) = self.selection_end?;

        if start_line == end_line {
            let line = self.buffer.get_line(start_line)?;
            let start = start_col.min(line.len());
            let end = end_col.min(line.len());
            let (s, e) = if start <= end {
                (start, end)
            } else {
                (end, start)
            };
            return Some(line[s..e].to_string());
        }

        // Multi-line selection (simplified)
        let mut result = String::new();
        let (first_line, first_col) = if start_line <= end_line {
            (start_line, start_col)
        } else {
            (end_line, end_col)
        };
        let (last_line, last_col) = if start_line <= end_line {
            (end_line, end_col)
        } else {
            (start_line, start_col)
        };

        for line_idx in first_line..=last_line {
            if let Some(line) = self.buffer.get_line(line_idx) {
                if line_idx == first_line {
                    result.push_str(&line[first_col.min(line.len())..]);
                } else if line_idx == last_line {
                    result.push_str(&line[..last_col.min(line.len())]);
                } else {
                    result.push_str(&line);
                }
                if line_idx != last_line {
                    result.push('\n');
                }
            }
        }
        Some(result)
    }

    fn cursor_byte_pos(&self) -> usize {
        self.line_byte_start(self.cursor_line) + self.cursor_col
    }

    fn line_byte_start(&self, line_idx: usize) -> usize {
        self.buffer.line_start_byte(line_idx)
    }

    /// 将行号+列号转换为字节偏移 - O(1) 行起始 + O(1) 列偏移
    pub fn line_col_to_byte(&self, line: usize, col: usize) -> usize {
        let start = self.buffer.line_start_byte(line);
        if let Some(text) = self.buffer.get_line(line) {
            start + col.min(text.len())
        } else {
            start
        }
    }

    /// P1-6: 将字节偏移转换为 (line, col) - O(log n) 二分查找行号
    fn byte_to_line_col(&self, byte: usize) -> (usize, usize) {
        let total_lines = self.buffer.len_lines();
        if total_lines == 0 {
            return (0, 0);
        }
        // 二分查找：找到第一个 line_start_byte > byte 的行，则该行前一行为目标行
        let mut lo: usize = 0;
        let mut hi: usize = total_lines;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.buffer.line_start_byte(mid) <= byte {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let line = lo.saturating_sub(1).min(total_lines.saturating_sub(1));
        let line_start = self.buffer.line_start_byte(line);
        let col = byte.saturating_sub(line_start);
        (line, col)
    }

    fn find_prev_char_boundary(&self, pos: usize) -> usize {
        if pos == 0 {
            return 0;
        }
        let mut p = pos - 1;
        // P4-1: 使用 byte_at 替代 get_text(p, p+1).as_bytes()[0]，避免 String 堆分配
        while p > 0 && self.buffer.byte_at(p).map_or(false, |b| (b & 0xC0) == 0x80) {
            p -= 1;
        }
        p
    }

    fn find_next_char_boundary(&self, pos: usize) -> usize {
        let total = self.buffer.len_bytes();
        if pos >= total {
            return total;
        }
        let mut p = pos + 1;
        // P4-1: 使用 byte_at 避免逐字节 String 分配
        while p < total && self.buffer.byte_at(p).map_or(false, |b| (b & 0xC0) == 0x80) {
            p += 1;
        }
        p
    }

    /// 增量重建缓存：只重建可见行范围内的缓存，大幅减少大文件的词法分析开销
    pub(crate) fn rebuild_cache(&mut self, visible_start: usize, visible_end: usize) {
        let total_lines = self.buffer.len_lines().max(1);

        // 如果行数变化，重新调整缓存向量大小
        if self.cached_lines.len() != total_lines {
            self.cached_lines.resize_with(total_lines, || String::new());
            self.cached_tokens.resize_with(total_lines, || Vec::new());
            self.line_cache_versions.resize(total_lines, 0);
        }

        // 调整行号 UTF-16 缓存大小
        if self.cached_line_numbers.len() != total_lines {
            self.cached_line_numbers
                .resize_with(total_lines, || Vec::new());
        }

        // 只重建可见行范围内的缓存（加上前后各2行的缓冲，避免滚动时闪烁）
        let cache_start = visible_start.saturating_sub(2);
        let cache_end = (visible_end + 2).min(total_lines);

        // 延迟创建 lexer：只在发现至少一行需要重建时才创建
        // 避免每帧都创建 lexer（Box 分配 + 初始化开销）
        let mut lexer: Option<Box<dyn aether_core::lexer::Lexer>> = None;

        for i in cache_start..cache_end {
            if self.line_cache_versions[i] != self.buffer_version {
                if lexer.is_none() {
                    lexer = Some(self.language.create_lexer());
                }
                let line = self.buffer.get_line(i).unwrap_or_default();
                let tokens = lexer.as_ref().unwrap().lex_full(&line);
                self.cached_lines[i] = line;
                self.cached_tokens[i] = tokens;
                self.line_cache_versions[i] = self.buffer_version;
            }
            // 行号 UTF-16 缓存：如果为空则构建
            if self.cached_line_numbers[i].is_empty() {
                let num_str = format!("{}", i + 1);
                self.cached_line_numbers[i] = num_str.encode_utf16().chain(Some(0)).collect();
            }
        }
    }

    /// 全量重建缓存（用于初始化或强制刷新）
    #[allow(dead_code)]
    pub(crate) fn rebuild_cache_full(&mut self) {
        let total_lines = self.buffer.len_lines().max(1);
        let lexer = self.language.create_lexer();

        if self.cached_lines.len() != total_lines {
            self.cached_lines.resize_with(total_lines, || String::new());
            self.cached_tokens.resize_with(total_lines, || Vec::new());
            self.line_cache_versions.resize(total_lines, 0);
        }

        for i in 0..total_lines {
            if self.line_cache_versions[i] != self.buffer_version {
                let line = self.buffer.get_line(i).unwrap_or_default();
                let tokens = lexer.lex_full(&line);
                self.cached_lines[i] = line;
                self.cached_tokens[i] = tokens;
                self.line_cache_versions[i] = self.buffer_version;
            }
        }
    }

    /// 标记指定行范围的缓存为失效
    /// 在编辑操作后调用，只标记受影响的行，避免全量重建
    #[allow(dead_code)]
    pub(crate) fn invalidate_line_cache(&mut self, start_line: usize, end_line: usize) {
        let total_lines = self.line_cache_versions.len();
        if total_lines == 0 {
            return;
        }
        let start = start_line.min(total_lines - 1);
        let end = end_line.min(total_lines - 1);
        for i in start..=end {
            self.line_cache_versions[i] = 0; // 0 表示未缓存，强制重建
        }
    }

    /// 处理编辑结果，更新缓存和行版本
    #[allow(dead_code)]
    pub(crate) fn apply_edit_result(&mut self, result: &aether_core::buffer::EditResult) {
        self.buffer_version += 1;
        let total_lines = self.buffer.len_lines().max(1);

        if result.line_delta != 0 {
            // 行数变化，重新调整缓存向量
            self.cached_lines.resize_with(total_lines, || String::new());
            self.cached_tokens.resize_with(total_lines, || Vec::new());
            self.line_cache_versions.resize(total_lines, 0);
        }

        // 标记受影响的行为失效
        let end_line = if result.line_delta > 0 {
            // 插入导致行增加，需要重建从起始行到新增行末尾
            (result.end_line + result.line_delta as usize).min(total_lines - 1)
        } else {
            result.end_line.min(total_lines.saturating_sub(1))
        };
        self.invalidate_line_cache(result.start_line, end_line);
    }

    /// 查找所有匹配位置
    /// 优化：缓存查询结果，避免查询未变且文本未变时重复全量扫描
    pub fn find_all(&mut self) {
        self.find_active_index = 0;
        if self.find_query.is_empty() {
            self.find_results.clear();
            self.last_find_query.clear();
            return;
        }
        // 缓存命中：查询和文本版本都未变，跳过搜索
        if self.find_query == self.last_find_query
            && self.find_result_version == self.buffer_version
            && !self.find_results.is_empty()
        {
            // 结果已有效，无需重新搜索
            return;
        }
        // 缓存未命中：清空并重新搜索
        self.find_results.clear();
        let query = self.find_query.clone();
        let total_lines = self.buffer.len_lines();
        for line_idx in 0..total_lines {
            if let Some(line) = self.buffer.get_line(line_idx) {
                let mut start = 0;
                while let Some(pos) = line[start..].find(&query) {
                    let abs_pos = start + pos;
                    self.find_results.push((line_idx, abs_pos));
                    start = abs_pos + query.len();
                    if start >= line.len() {
                        break;
                    }
                }
            }
        }
        // 更新缓存状态
        self.last_find_query = query;
        self.find_result_version = self.buffer_version;
    }

    /// 跳转到下一个匹配
    pub fn find_next(&mut self) {
        if self.find_results.is_empty() {
            self.find_all();
        }
        if !self.find_results.is_empty() {
            self.find_active_index = (self.find_active_index + 1) % self.find_results.len();
            let (line, col) = self.find_results[self.find_active_index];
            // P2-6: 选区末尾对齐到字符边界；cursor_col 置于匹配末尾以符合编辑器约定
            let end_col = self.clamp_to_char_boundary(line, col + self.find_query.len());
            self.cursor_line = line;
            self.cursor_col = end_col;
            // 选中匹配文本
            self.selection_start = Some((line, col));
            self.selection_end = Some((line, end_col));
        }
    }

    /// 跳转到上一个匹配
    pub fn find_prev(&mut self) {
        if self.find_results.is_empty() {
            self.find_all();
        }
        if !self.find_results.is_empty() {
            if self.find_active_index == 0 {
                self.find_active_index = self.find_results.len() - 1;
            } else {
                self.find_active_index -= 1;
            }
            let (line, col) = self.find_results[self.find_active_index];
            // P2-6: 选区末尾对齐到字符边界
            let end_col = self.clamp_to_char_boundary(line, col + self.find_query.len());
            self.cursor_line = line;
            self.cursor_col = end_col;
            self.selection_start = Some((line, col));
            self.selection_end = Some((line, end_col));
        }
    }

    /// P2-6: 把字节偏移对齐到字符边界（向下取到下一个字符起点）。
    /// 避免 selection_end 落在多字节字符中间导致渲染/截取异常。
    fn clamp_to_char_boundary(&self, line_idx: usize, byte_pos: usize) -> usize {
        if let Some(line) = self.buffer.get_line(line_idx) {
            let max = line.len();
            if byte_pos >= max {
                return max;
            }
            // 向前微调到字符边界（byte_pos 通常已在边界上，此处做防御性对齐）
            let mut p = byte_pos;
            while p > 0 && !line.is_char_boundary(p) {
                p -= 1;
            }
            p
        } else {
            byte_pos
        }
    }

    /// 替换当前匹配
    pub fn replace_current(&mut self) -> bool {
        if self.find_results.is_empty() || self.find_active_index >= self.find_results.len() {
            return false;
        }
        let (line, col) = self.find_results[self.find_active_index];
        let pos = self.line_byte_start(line) + col;
        let end_pos = pos + self.find_query.len();

        let before_pieces = self.buffer.get_pieces();
        let before_add_len = self.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

        self.buffer.delete(pos, end_pos);
        self.buffer.insert(pos, &self.replace_text);
        self.is_dirty = true;
        self.buffer_version += 1;

        self.cursor_line = line;
        self.cursor_col = col + self.replace_text.len();
        let cursor_after = CursorPosition::new(self.cursor_line, self.cursor_col);
        self.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
        );

        // 重新查找
        self.find_all();
        true
    }

    /// 替换所有匹配
    pub fn replace_all(&mut self) -> usize {
        if self.find_query.is_empty() || self.find_query == self.replace_text {
            return 0;
        }
        self.find_all();
        let count = self.find_results.len();
        if count == 0 {
            return 0;
        }

        // 从后往前替换，避免位置偏移
        let replacements = self.find_results.clone();
        let query_len = self.find_query.len();
        let replace_text = self.replace_text.clone();

        for (line, col) in replacements.iter().rev() {
            let pos = self.line_byte_start(*line) + *col;
            let end_pos = pos + query_len;
            self.buffer.delete(pos, end_pos);
            self.buffer.insert(pos, &replace_text);
        }

        self.is_dirty = true;
        self.buffer_version += 1;
        self.find_results.clear();
        self.find_active_index = 0;
        self.status_message = format!("已替换 {} 处", count);
        count
    }

    /// 切换查找面板
    pub fn toggle_find(&mut self) {
        self.find_visible = !self.find_visible;
        if !self.find_visible {
            self.replace_visible = false;
            self.find_focus = FindReplaceFocus::None;
        } else {
            self.find_focus = FindReplaceFocus::FindQuery;
        }
        if self.find_visible && !self.find_query.is_empty() {
            self.find_all();
        }
    }

    /// 切换替换面板
    pub fn toggle_replace(&mut self) {
        self.replace_visible = !self.replace_visible;
        self.find_visible = self.replace_visible || self.find_visible;
        if !self.find_visible {
            self.find_focus = FindReplaceFocus::None;
        } else {
            self.find_focus = if self.replace_visible {
                FindReplaceFocus::FindQuery
            } else {
                FindReplaceFocus::None
            };
        }
        if self.find_visible && !self.find_query.is_empty() {
            self.find_all();
        }
    }

    /// 关闭查找替换面板
    pub fn close_find_replace(&mut self) {
        self.find_visible = false;
        self.replace_visible = false;
        self.find_focus = FindReplaceFocus::None;
    }

    /// 应用 AI 生成的代码到当前编辑器
    pub fn apply_ai_code(&mut self, code: &str) -> bool {
        if code.is_empty() {
            return false;
        }
        // 如果有选区，替换选区内容；否则在当前光标位置插入
        if self.selection_start.is_some() && self.selection_end.is_some() {
            let (start_line, start_col) = self.selection_start.unwrap();
            let (end_line, end_col) = self.selection_end.unwrap();
            let (first_line, first_col) = if start_line <= end_line {
                (start_line, start_col)
            } else {
                (end_line, end_col)
            };
            let (last_line, last_col) = if start_line <= end_line {
                (end_line, end_col)
            } else {
                (start_line, start_col)
            };
            let start_byte = self.line_byte_start(first_line) + first_col;
            let end_byte = self.line_byte_start(last_line) + last_col;

            let before_pieces = self.buffer.get_pieces();
            let before_add_len = self.buffer.add_buffer_len();
            let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

            self.buffer.delete(start_byte, end_byte);
            self.buffer.insert(start_byte, code);

            // 计算新光标位置
            let code_lines: Vec<&str> = code.lines().collect();
            let new_line = first_line + code_lines.len().saturating_sub(1);
            let new_col = if code_lines.len() <= 1 {
                first_col + code.len()
            } else {
                code_lines.last().unwrap_or(&"").len()
            };
            self.cursor_line = new_line;
            self.cursor_col = new_col;
            let cursor_after = CursorPosition::new(self.cursor_line, self.cursor_col);
            self.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Insert,
                start_byte,
            );

            self.clear_selection();
            self.is_dirty = true;
            self.buffer_version += 1;
            self.status_message = "已应用 AI 代码".to_string();
            return true;
        }
        let pos = self.cursor_byte_pos();
        let before_pieces = self.buffer.get_pieces();
        let before_add_len = self.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

        self.buffer.insert(pos, code);

        // 更新光标位置
        let _code_lines: Vec<&str> = code.lines().collect();
        let line_breaks = code.matches('\n').count();
        if line_breaks == 0 {
            self.cursor_col += code.len();
        } else {
            self.cursor_line += line_breaks;
            self.cursor_col = code
                .rsplit_once('\n')
                .map(|(_, last)| last.len())
                .unwrap_or(0);
        }
        let cursor_after = CursorPosition::new(self.cursor_line, self.cursor_col);
        self.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
        );

        self.is_dirty = true;
        self.buffer_version += 1;
        self.status_message = "已插入 AI 代码".to_string();
        true
    }
}

/// 检查文件是否为文本文件
pub(crate) fn is_text_file(path: &std::path::Path) -> bool {
    // 已知的文本文件扩展名
    let text_extensions = [
        "txt",
        "rs",
        "c",
        "h",
        "cpp",
        "hpp",
        "cc",
        "cxx",
        "js",
        "jsx",
        "ts",
        "tsx",
        "json",
        "md",
        "markdown",
        "py",
        "pyw",
        "pyi",
        "toml",
        "yaml",
        "yml",
        "xml",
        "html",
        "htm",
        "css",
        "scss",
        "sass",
        "less",
        "java",
        "kt",
        "go",
        "rb",
        "php",
        "swift",
        "sh",
        "bash",
        "zsh",
        "ps1",
        "bat",
        "cmd",
        "sql",
        "lua",
        "r",
        "pl",
        "pm",
        "t",
        "ini",
        "cfg",
        "conf",
        "properties",
        "log",
        "csv",
        "tsv",
    ];

    if let Some(ext) = path.extension() {
        if let Some(ext_str) = ext.to_str() {
            let ext_lower = ext_str.to_lowercase();
            if text_extensions.contains(&ext_lower.as_str()) {
                return true;
            }
        }
    }

    // 尝试读取文件前 8KB 检测是否为文本
    if let Ok(file) = std::fs::File::open(path) {
        use std::io::Read;
        let mut buffer = [0u8; 8192];
        if let Ok(n) = file.take(8192).read(&mut buffer) {
            let sample = &buffer[..n];
            // 如果包含空字节，则认为是二进制文件
            if sample.contains(&0) {
                return false;
            }
            // 检查是否主要是可打印字符
            let printable_count = sample
                .iter()
                .filter(|&&b| {
                    b.is_ascii_graphic() || b.is_ascii_whitespace() || b == 0x0D || b == 0x0A
                })
                .count();
            if n > 0 && printable_count >= n * 9 / 10 {
                return true;
            }
        }
    }

    false
}
