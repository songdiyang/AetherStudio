use std::path::PathBuf;

use windows::core::Result;
use windows::Win32::Foundation::HWND;

use aether_core::buffer::history::{CursorPosition, History, OpType};
use aether_core::buffer::piece_table::PieceTable;
use aether_core::buffer::text_buffer::MultiCursorState;
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
use crate::input::KeyMap;
use crate::layout::{ActivityBarView, LayoutManager, SidebarContent};
use crate::menu_bar::MenuBar;
use crate::settings::SettingsPanel;
use crate::ssh::{CloneRepoDialog, RemoteFileTree, RemoteSession, SshConnectionDialog};
use crate::status_bar::StatusBar;
use crate::tabs::{Tab, TabLayout};
use crate::terminal::TerminalPanel;
use aether_shared::settings::AppSettings;

/// 查找替换焦点状态
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindReplaceFocus {
    None,
    FindQuery,
    ReplaceText,
}

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
    /// 选中的远程文件节点
    pub selected_remote_node: Option<usize>,
    /// 悬停的远程文件节点
    pub hover_remote_node: Option<usize>,
    /// 远程文件树滚动偏移
    pub remote_scroll_y: f32,
    /// 克隆仓库对话框
    pub clone_dialog: CloneRepoDialog,
    /// 窗口是否最大化
    pub is_maximized: bool,
    /// 标题栏按钮悬停状态 (0=最小化, 1=最大化, 2=关闭)
    pub titlebar_hover_button: Option<usize>,
    /// 文件树中选中的节点索引
    pub selected_file_node: Option<u32>,
    /// 文件树中鼠标悬停的节点索引
    pub hover_file_node: Option<u32>,
    /// 侧边栏滚动偏移（用于文件树虚拟滚动）
    pub sidebar_scroll_y: f32,
    /// 应用设置
    pub app_settings: aether_shared::settings::AppSettings,
    /// 设置面板
    pub settings_panel: crate::settings::SettingsPanel,
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
    /// 上一帧的右侧面板可见性（用于检测右侧面板变化）
    pub last_right_panel_visible: bool,
    /// 上一帧的底部面板可见性（用于检测底部面板变化）
    pub last_bottom_panel_visible: bool,
    /// 上一帧的状态消息（用于检测状态栏变化）
    pub last_status_message: String,
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
    pub fn handle_tab_bar_click(&mut self, mouse_x: f32, mouse_y: f32, editor_x: f32) -> bool {
        let tab_bar_height = if self.tabs.len() > 1 { 30.0 } else { 0.0 };
        if tab_bar_height == 0.0 {
            return false;
        }
        if mouse_y < 0.0 || mouse_y > tab_bar_height {
            return false;
        }
        if mouse_x < editor_x {
            return false;
        }
        let rel_x = mouse_x - editor_x + self.tab_scroll_x;
        for layout in &self.tab_layouts {
            if rel_x >= layout.x && rel_x < layout.x + layout.width {
                // 检测关闭按钮点击
                if rel_x >= layout.close_x && rel_x < layout.close_x + layout.close_width {
                    if layout.index == self.active_tab {
                        self.close_current_tab();
                    } else {
                        // 关闭非活动标签页
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
        let tab_bar_height = if self.tabs.len() > 1 { 30.0 } else { 0.0 };
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

    pub fn new(hwnd: HWND) -> Result<Self> {
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
            app_settings,
            ssh_dialog: SshConnectionDialog::new(),
            remote_session: None,
            remote_file_tree: None,
            selected_remote_node: None,
            hover_remote_node: None,
            remote_scroll_y: 0.0,
            clone_dialog: CloneRepoDialog::new(),
            is_maximized: false,
            titlebar_hover_button: None,
            selected_file_node: None,
            hover_file_node: None,
            sidebar_scroll_y: 0.0,
            git_panel: crate::git::GitIntegration::new(),
            dirty_tracker: crate::dirty_rect::DirtyRectTracker::new(1280.0, 800.0),
            last_cursor_line: 0,
            last_cursor_col: 0,
            last_scroll_y: 0.0,
            last_selection_start: None,
            last_selection_end: None,
            last_sidebar_content: crate::layout::SidebarContent::FileTree,
            last_right_panel_visible: false,
            last_bottom_panel_visible: false,
            last_status_message: "就绪".to_string(),
        };
        // 创建第一个标签页并同步
        state.tabs.push(Tab::new());
        state.sync_from_tab();
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
                self.status_message = format!("打开文件失败: {}", e);
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
            match std::fs::write(path, text) {
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
        match std::fs::write(&path, text) {
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
        let editor_height = self.window_height as f32 - 24.0;
        let max_scroll = (total_height - editor_height).max(0.0);
        self.scroll_y = (self.scroll_y + delta_y).clamp(0.0, max_scroll);
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
                let estimated_nodes = if let Some(tree) = &self.remote_file_tree {
                    tree.nodes.len() as f32
                } else {
                    0.0
                };
                let total_height = estimated_nodes * node_height + 40.0;
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
    fn set_clipboard_text(text: &str) {
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
                return;
            }
            let _ = EmptyClipboard();

            let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
            let byte_size = wide.len() * 2;

            if let Ok(hglobal) = GlobalAlloc(GMEM_MOVEABLE, byte_size) {
                let ptr = GlobalLock(hglobal);
                if !ptr.is_null() {
                    let dst = ptr as *mut u16;
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
                    let _ = GlobalUnlock(hglobal);
                    let _ = SetClipboardData(CF_UNICODETEXT, HANDLE(hglobal.0));
                }
            }
            let _ = CloseClipboard();
        }
    }

    /// 获取剪贴板文本
    fn get_clipboard_text() -> Option<String> {
        use windows::Win32::Foundation::{HANDLE, HGLOBAL};
        use windows::Win32::System::DataExchange::{
            CloseClipboard, GetClipboardData, OpenClipboard,
        };
        use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
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
                    let mut len = 0;
                    while *wide_ptr.add(len) != 0 {
                        len += 1;
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
                self.layout.toggle_bottom_panel();
                if self.layout.bottom_panel_visible && !self.terminal_panel.running {
                    let _ = self.terminal_panel.start();
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
        let mut tree = FileTree::new();
        if let Err(e) = self.populate_file_tree(&mut tree, &path, u32::MAX, 0) {
            self.status_message = format!("打开文件夹失败: {}", e);
            return;
        }
        self.file_tree = Some(tree);
        self.current_folder = Some(path.clone());
        // 检测 Git 仓库
        self.git.detect(&path);
        if let Some(branch) = self.git.current_branch_name() {
            self.status_bar.update_git_branch(Some(&branch));
        } else {
            self.status_bar.update_git_branch(None);
        }
        self.status_message = format!("已打开文件夹: {}", path.display());
        // 记录到最近项目列表
        self.recent_projects.add(&path);
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

        let mut current_y = 10.0;
        let result = Self::find_tree_click_target(tree, u32::MAX, mouse_y, &mut current_y);

        if let Some((node_idx, kind)) = result {
            match kind {
                FileKind::Directory => {
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
                        self.load_file(path);
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
        let tree = match self.remote_file_tree.as_ref() {
            Some(t) => t,
            None => return false,
        };

        let mut current_y = 10.0 - self.remote_scroll_y;
        for (i, node) in tree.nodes.iter().enumerate() {
            if mouse_y >= current_y && mouse_y < current_y + 20.0 {
                if node.is_dir {
                    // 展开/折叠目录
                    if let Some(tree) = self.remote_file_tree.as_mut() {
                        if let Some(n) = tree.nodes.get_mut(i) {
                            n.is_expanded = !n.is_expanded;
                        }
                    }
                } else {
                    // 打开远程文件
                    self.selected_remote_node = Some(i);
                    if let Some(session) = &self.remote_session {
                        let remote_path = node.path.clone();
                        match session.read_remote_file(&remote_path) {
                            Ok(content) => {
                                let text = String::from_utf8_lossy(&content).to_string();
                                let tab = crate::tabs::Tab {
                                    file_path: Some(PathBuf::from(format!(
                                        "remote:{}",
                                        remote_path
                                    ))),
                                    buffer: PieceTable::from_string(text),
                                    cursor_line: 0,
                                    cursor_col: 0,
                                    selection_start: None,
                                    selection_end: None,
                                    scroll_y: 0.0,
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
                }
                return true;
            }
            current_y += 20.0;
        }
        false
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

        let mut current_y = 10.0;
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

        let mut current_y = 10.0 - self.remote_scroll_y;
        let mut new_hover = None;
        for (i, _node) in tree.nodes.iter().enumerate() {
            if mouse_y >= current_y && mouse_y < current_y + 20.0 {
                new_hover = Some(i);
                break;
            }
            current_y += 20.0;
        }
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
        match self.ssh_dialog.focus_field {
            0 => self.ssh_dialog.host.push(ch),
            1 => {
                if ch.is_ascii_digit() {
                    self.ssh_dialog.port.push(ch);
                }
            }
            2 => self.ssh_dialog.username.push(ch),
            3 => match self.ssh_dialog.auth_type {
                crate::ssh::SshAuthType::Password => self.ssh_dialog.password.push(ch),
                crate::ssh::SshAuthType::Key => self.ssh_dialog.key_path.push(ch),
                crate::ssh::SshAuthType::Agent => {}
            },
            4 => self.ssh_dialog.key_passphrase.push(ch),
            _ => {}
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

    fn populate_file_tree(
        &self,
        tree: &mut FileTree,
        path: &PathBuf,
        parent_idx: u32,
        depth: u8,
    ) -> std::io::Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(path)?.filter_map(|e| e.ok()).collect();
        entries.sort_by(|a, b| {
            let a_is_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let b_is_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.file_name().cmp(&b.file_name()),
            }
        });

        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type()?.is_dir();
            let kind = if is_dir {
                FileKind::Directory
            } else {
                FileKind::File
            };
            let idx = tree.add_node(&name, kind, parent_idx, depth);

            if is_dir && depth < 5 {
                let _ = self.populate_file_tree(tree, &entry.path(), idx, depth + 1);
            }
        }

        Ok(())
    }

    pub fn insert_char(&mut self, ch: char) {
        let pos = self.cursor_byte_pos();
        let before_pieces = self.buffer.get_pieces();
        let before_add_len = self.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

        let text = ch.to_string();
        self.buffer.insert(pos, &text);
        self.cursor_col += ch.len_utf8();
        self.is_dirty = true;
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

    pub fn insert_tab(&mut self) {
        let pos = self.cursor_byte_pos();
        let before_pieces = self.buffer.get_pieces();
        let before_add_len = self.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.cursor_line, self.cursor_col);

        let tab_text = "    ";
        self.buffer.insert(pos, tab_text);
        self.cursor_col += tab_text.len();
        self.is_dirty = true;
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

        let rel_x = mouse_x - editor_x - line_number_width - 5.0;
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

    fn find_prev_char_boundary(&self, pos: usize) -> usize {
        if pos == 0 {
            return 0;
        }
        let mut p = pos - 1;
        while p > 0 && (self.buffer.get_text(p, p + 1).as_bytes()[0] & 0xC0) == 0x80 {
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
        while p < total && (self.buffer.get_text(p, p + 1).as_bytes()[0] & 0xC0) == 0x80 {
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
            self.cursor_line = line;
            self.cursor_col = col;
            // 选中匹配文本
            self.selection_start = Some((line, col));
            self.selection_end = Some((line, col + self.find_query.len()));
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
            self.cursor_line = line;
            self.cursor_col = col;
            self.selection_start = Some((line, col));
            self.selection_end = Some((line, col + self.find_query.len()));
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
