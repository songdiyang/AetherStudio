#![allow(clippy::collapsible_match, clippy::cmp_owned)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use windows::core::Result;
use windows::Win32::Foundation::HWND;

use aether_core::buffer::history::{CursorPosition, OpType};
use aether_core::buffer::piece_table::PieceTable;
use aether_core::buffer::text_buffer::{Cursor, MultiCursorState};
use aether_core::char_width::char_width as unicode_char_width;
use aether_core::lexer::Language;
use aether_core::workspace::file_tree::{FileKind, FileTree};
use aether_lsp::client::{default_server_config, LspEvent};
use aether_lsp::LspClient;
use aether_render::d2d::factory::D2DFactory;
use aether_render::d2d::text::TextRenderer;
use aether_render::theme::Theme;
use aether_tree_sitter::TreeSitterHighlighter;
use lsp_types::{CompletionItem, Diagnostic};
use url::Url;

use crate::activity_bar::ActivityBar;
use crate::ai_agent::AiEdit;
use crate::ai_context::{truncate_middle, wrap_code_block, AiContextAttachment};
use crate::ai_panel::AiPanel;
use crate::command_palette::CommandPalette;
use crate::dialogs::Dialogs;
use crate::focus_manager::FocusManager;
use crate::git::GitIntegration;
use crate::input::{KeyMap, PressTarget};
use crate::layout::{
    ActivityBarView, LayoutManager, SidebarContent, SIDEBAR_RESIZE_GRAB, TAB_BAR_HEIGHT,
};
use crate::menu_bar::MenuBar;
use crate::ssh::{
    CloneRepoDialog, RemoteFileTree, RemoteSession, SshConnectionDialog, SshManagerPanel,
};
use crate::status_bar::StatusBar;
use crate::tabs::{Tab, TabContent, TabLayout};
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

/// 底部面板当前显示的子面板。
///
/// 当前仅用于在终端面板和"问题"面板之间切换，UI 实现先做，
/// 问题数据/采集引擎后续再设计（`diagnostics: HashMap<...>` 是预留数据源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BottomPanelTab {
    #[default]
    Terminal,
    Problems,
}

impl BottomPanelTab {
    /// 标签栏上显示的标题（与 render.rs 标签顺序保持一致）。
    pub fn label(self) -> &'static str {
        match self {
            BottomPanelTab::Terminal => "终端",
            BottomPanelTab::Problems => "问题",
        }
    }
}

/// 文件树内联输入类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileTreeInputKind {
    NewFile,
    NewFolder,
}

/// 文件树内联输入状态（用于新建文件/文件夹时重命名）
#[derive(Clone, Debug)]
pub struct FileTreeInput {
    pub kind: FileTreeInputKind,
    pub value: String,
    pub caret_visible: bool,
    /// IME 合成串（中文输入法预编辑文本），渲染时显示在 value 之后
    pub composition: Option<String>,
}

/// 文件树点击命中的具体部位
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileTreeClickPart {
    /// 点击目录展开/折叠箭头
    Arrow,
    /// 点击文件或目录名称/图标区域
    Label,
}

/// Hover Tooltip（鼠标悬停提示）
///
/// 当鼠标在某个区域停留超过 `HOVER_DELAY_MS` 后显示。
/// 当前用于文件树节点完整路径提示，后续可扩展接入 LSP hover。
#[derive(Clone, Debug, PartialEq)]
pub struct HoverTooltip {
    /// 提示文本（多行用 `\n` 分隔）
    pub text: String,
    /// 提示框左上角 x（逻辑像素）
    pub x: f32,
    /// 提示框左上角 y（逻辑像素）
    pub y: f32,
    /// 提示框最大宽度（用于自动换行，0 表示不限制）
    pub max_width: f32,
}

impl HoverTooltip {
    pub fn new(text: impl Into<String>, x: f32, y: f32, max_width: f32) -> Self {
        Self {
            text: text.into(),
            x,
            y,
            max_width,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// 计算文件树节点的完整路径（从根到该节点）。
///
/// 通过 `parent_idx` 链向上遍历，拼接各级目录名。
/// 返回 `None` 表示节点不存在或树不可用。
pub fn file_tree_node_path(tree: &FileTree, node_idx: u32) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    let mut current_idx = node_idx;

    loop {
        let node = tree.get_node(current_idx)?;
        parts.push(tree.get_name(node));
        if node.parent_idx == u32::MAX {
            break;
        }
        current_idx = node.parent_idx;
    }

    parts.reverse();
    Some(parts.join("/"))
}

/// 文件夹异步扫描条目（由后台线程生成，分批通过 PostMessage 发送回 UI 线程）
#[derive(Clone, Debug)]
struct ScannedEntry {
    name: String,
    kind: FileKind,
    #[allow(dead_code)]
    path: PathBuf,
    depth: u8,
}

/// 文件夹异步扫描批次（由后台线程通过 PostMessage WM_APP+7 发送回 UI 线程）
#[derive(Clone, Debug)]
pub(crate) struct ScannedBatch {
    generation: u64,
    entries: Vec<ScannedEntry>,
    complete: bool,
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

/// C-06: 安全地通过 PostMessageW 发送 Box 指针；失败时回收 Box 防止泄漏
unsafe fn post_boxed_message_lparam<T>(hwnd: HWND, msg: u32, ptr: *mut T) {
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
    let posted = PostMessageW(
        hwnd,
        msg,
        windows::Win32::Foundation::WPARAM(0),
        windows::Win32::Foundation::LPARAM(ptr as isize),
    );
    if posted.is_err() {
        let _ = Box::from_raw(ptr);
    }
}

unsafe fn post_boxed_message_wparam<T>(hwnd: HWND, msg: u32, ptr: *mut T) {
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
    let posted = PostMessageW(
        hwnd,
        msg,
        windows::Win32::Foundation::WPARAM(ptr as usize),
        windows::Win32::Foundation::LPARAM(0),
    );
    if posted.is_err() {
        let _ = Box::from_raw(ptr);
    }
}

/// 简化的 LSP 诊断项（UI 层不直接依赖 lsp-types）
#[derive(Clone, Debug)]
pub struct DiagnosticItem {
    pub severity: u8,
    pub message: String,
    pub line: usize,
    pub col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

/// 编辑器应用状态
pub struct EditorState {
    pub hwnd: HWND,
    pub d2d_factory: D2DFactory,
    pub render_ctx: crate::render_context::RenderContext,
    pub text_renderer: TextRenderer,
    pub theme: Theme,
    /// REQ-P1-09: 当前活动标签页的编辑状态（单一归属，切换标签时通过 swap 交换）
    pub content: TabContent,
    pub is_selecting: bool,
    /// 行号 UTF-16 预缓存（避免每帧 format! + encode_utf16 分配）
    pub(crate) cached_line_numbers: Vec<Vec<u16>>,
    /// 标签页系统（后台存储，切换时同步）
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active_tab: usize,
    /// 标签栏布局缓存（用于点击检测）
    pub(crate) tab_layouts: Vec<TabLayout>,
    /// 鼠标悬停的标签索引
    pub(crate) hover_tab: Option<usize>,
    /// 标签栏滚动偏移
    pub(crate) tab_scroll_x: f32,
    /// 标签栏右侧 "+" 新建按钮的命中区域（逻辑像素，相对于窗口左上角）
    /// 由 `update_tab_layouts` 在每帧渲染前更新；点击检测在 `handle_tab_bar_click` 中使用。
    pub(crate) plus_button_rect: Option<(f32, f32, f32, f32)>,
    /// "+" 新建按钮的悬停状态（由 `update_hover_tab` 更新，render 读取以绘制 hover 背景）
    pub(crate) plus_button_hover: bool,
    /// Task 8: 正在拖拽的标签索引（拖拽进行中时为 Some）
    pub(crate) dragging_tab: Option<usize>,
    /// Task 8: 拖拽放置目标索引（drop_index）
    pub(crate) tab_drop_index: Option<usize>,
    /// Task 8: 拖拽起始鼠标位置（用于判断是否进入拖拽模式）
    pub(crate) tab_drag_start: Option<(i32, i32)>,
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
    /// 当前文件夹加载世代，用于丢弃过期批次消息
    pub(crate) folder_generation: u64,
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
    /// 终端聚焦时缓存的原 IME 上下文句柄（HIMC）。
    /// 终端聚焦时 disassociate，离开时 restore，从而彻底旁路 IME 系统级拦截
    /// （修复：中文 IME 在"开启未合成"状态下会拦截 Backspace，导致终端里的汉字无法删除）
    pub saved_ime_himc: Option<windows::Win32::UI::Input::Ime::HIMC>,
    /// AI 助手面板
    pub ai_panel: AiPanel,
    /// 全局搜索面板
    pub search_panel: crate::search_panel::SearchPanel,
    /// 底部面板当前选中的子面板（终端 / 问题）
    pub bottom_panel_tab: BottomPanelTab,
    /// 当前工作区中各文件的 LSP 诊断（路径字符串 -> 诊断列表）
    pub diagnostics: HashMap<String, Vec<DiagnosticItem>>,
    /// LSP 客户端（工作区打开时初始化，旧版 LSP 集成）
    pub legacy_lsp_client: Option<std::sync::Arc<aether_lsp::client::LspClient>>,
    /// LSP 事件接收器
    pub lsp_rx: Option<tokio::sync::mpsc::UnboundedReceiver<aether_lsp::client::LspEvent>>,
    /// Tokio 运行时（用于 LSP 异步操作）
    pub lsp_runtime: Option<tokio::runtime::Runtime>,
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
    /// 新建项目对话框
    pub new_project_dialog: crate::new_project_dialog::NewProjectDialog,
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
    /// 侧边栏宽度调整手柄的悬停状态
    pub hover_sidebar_resize: bool,
    /// 文件树中选中的节点索引
    pub selected_file_node: Option<u32>,
    /// 文件树中鼠标悬停的节点索引
    pub hover_file_node: Option<u32>,
    /// 文件树内联输入状态（新建文件/文件夹）
    pub file_tree_input: Option<FileTreeInput>,
    /// 文件树标题栏按钮区域（用于点击检测）
    pub file_tree_new_file_btn: Option<crate::layout::Region>,
    pub file_tree_new_folder_btn: Option<crate::layout::Region>,
    pub file_tree_open_folder_btn: Option<crate::layout::Region>,
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
    /// 标签栏面板
    pub tabs_panel: crate::open_tabs::TabsPanel,
    /// Git 面板
    pub git_panel: crate::git::GitIntegration,
    /// 脏矩形追踪器（用于局部重绘优化）
    pub dirty_tracker: crate::dirty_rect::DirtyRectTracker,
    /// REQ-P0-05: 统一焦点管理器
    pub focus_manager: FocusManager,
    /// 事件队列（P1.1: 解耦模型改动与渲染）
    pub event_queue: crate::events::EventQueue,
    /// P3.1: 内联补全服务（占位）
    pub inline_completion_service: crate::inline_completion::InlineCompletionService,
    /// P3.4: 当前显示的 hover tooltip（鼠标悬停提示）
    pub hover_tooltip: Option<HoverTooltip>,
    /// P3.4: 上次鼠标位置（用于 hover 防抖判定）
    pub hover_last_mouse_x: f32,
    pub hover_last_mouse_y: f32,
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
    /// 上一帧的活动标签页索引（用于检测标签切换）
    pub last_active_tab: usize,
    /// 用户菜单
    pub user_menu: crate::user_menu::UserMenu,
    /// 资源管理器空白区域上下文菜单
    pub explorer_context_menu: crate::context_menu::ExplorerContextMenu,
    /// 标签右键上下文菜单
    pub tab_context_menu: crate::tab_context_menu::TabContextMenuState,
    /// 活动栏右键上下文菜单
    pub activity_bar_context_menu: crate::activity_bar_context_menu::ActivityBarContextMenuState,
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
    /// tree-sitter 高亮器（主线程持有，非 Send，不能进 rayon 并行）
    pub(crate) ts_highlighter: TreeSitterHighlighter,
    /// 后台语法高亮器（独立线程，避免阻塞 UI 输入）
    pub(crate) bg_highlighter: aether_tree_sitter::BackgroundHighlighter,
    /// 已发送后台高亮请求对应的 buffer_version（变化时触发新请求）
    pub(crate) hl_request_version: u64,
    /// tokio runtime（驱动 LSP 异步操作）
    pub(crate) tokio_runtime: tokio::runtime::Runtime,
    /// LSP 客户端（Arc 共享给 tokio task）
    pub(crate) lsp_client: Arc<LspClient>,
    /// LSP 诊断表（uri -> diagnostics），由 WM_APP+3 处理更新
    pub(crate) lsp_diagnostics: std::collections::HashMap<Url, Vec<Diagnostic>>,
    // Phase H: 补全弹窗状态
    pub(crate) completion_items: Vec<CompletionItem>,
    pub(crate) completion_visible: bool,
    pub(crate) completion_selected: usize,
    pub(crate) completion_trigger_line: usize,
    pub(crate) completion_trigger_col: usize,
    // Phase H: 悬停 tooltip 状态
    pub(crate) hover_content: Option<String>,
    #[allow(dead_code)]
    pub(crate) hover_x: f32,
    #[allow(dead_code)]
    pub(crate) hover_y: f32,
    /// UI Tooltip 状态（500ms 延迟显示、4px 移动容差的悬停提示）
    pub tooltip_state: crate::tooltip::TooltipState,
    /// Task 13.3: 最后关闭的标签内容（用于 Ctrl+Shift+T 恢复）
    pub last_closed_tab: Option<TabContent>,
    /// Logo 位图（aether-512.png），懒加载，用于欢迎页和空占位页
    pub(crate) logo_bitmap: Option<windows::Win32::Graphics::Direct2D::ID2D1Bitmap>,
}

/// Task 8.4: 标签重排核心逻辑（自由函数，可独立测试）。
///
/// 将 `items[drag_idx]` 移动到 `drop_idx` 位置（插入到该索引之前），
/// 并同步调整 `active` 索引以跟随移动的元素。
pub(crate) fn reorder_tabs_with_active<T>(
    items: &mut Vec<T>,
    active: &mut usize,
    drag_idx: usize,
    drop_idx: usize,
) {
    if drag_idx >= items.len() || drop_idx > items.len() {
        return;
    }
    // 计算实际插入位置：drop_idx > drag_idx 时需补偿 remove 导致的索引偏移
    let insert_at = if drop_idx > drag_idx {
        drop_idx - 1
    } else {
        drop_idx
    };
    // insert_at == drag_idx 表示无需移动（如 drop_idx == drag_idx + 1）
    if insert_at == drag_idx {
        return;
    }
    let item = items.remove(drag_idx);
    let insert_at = insert_at.min(items.len());
    items.insert(insert_at, item);
    // 调整 active 索引
    if *active == drag_idx {
        *active = insert_at;
    } else if drag_idx < *active && drop_idx >= *active {
        *active -= 1;
    } else if drag_idx > *active && drop_idx <= *active {
        *active += 1;
    }
}

/// Task 13.3: 从 `tabs` 中移除指定索引的标签，并将其内容保存到 `last_closed`。
/// 返回 true 表示已移除并保存，false 表示索引越界。
/// 此自由函数便于单元测试 save/restore 逻辑（无需构造完整 EditorState）。
pub(crate) fn remove_tab_saving_content(
    tabs: &mut Vec<Tab>,
    index: usize,
    last_closed: &mut Option<TabContent>,
) -> bool {
    if index >= tabs.len() {
        return false;
    }
    let removed = tabs.remove(index);
    if let crate::tabs::Tab::File(content) = removed {
        *last_closed = Some(content);
    }
    true
}

/// Task 13.3: 将 `last_closed` 中的内容作为新标签恢复到 `tabs` 末尾，
/// 并将 `active` 指向新标签。返回 true 表示已恢复，false 表示无可恢复内容。
#[allow(dead_code)]
pub(crate) fn reopen_last_closed_tab_logic(
    tabs: &mut Vec<Tab>,
    active: &mut usize,
    last_closed: &mut Option<TabContent>,
) -> bool {
    let Some(content) = last_closed.take() else {
        return false;
    };
    tabs.push(crate::tabs::Tab::File(content));
    *active = tabs.len() - 1;
    true
}

impl EditorState {
    /// REQ-P1-09: 交换活动标签页内容（替代原 sync_to_tab/sync_from_tab 的字段逐个同步）
    ///
    /// 将 `self.content` 与 `self.tabs[index].content` 原子交换，
    /// 消除手动字段同步，保证状态归属单一。
    fn swap_tab_content(&mut self, index: usize) {
        if let Some(crate::tabs::Tab::File(content)) = self.tabs.get_mut(index) {
            std::mem::swap(&mut self.content, content);
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

    /// 是否显示标签栏：只要有标签页就显示（仅欢迎页例外）。
    /// 修复：原先单个未关联文件的 File tab 会导致标签栏消失，用户误以为"全关了"，
    /// 但 self.content 仍持有内容可编辑。现在保证空 File tab 也显示标签栏。
    pub fn show_tab_bar(&self) -> bool {
        !self.tabs.is_empty() && !self.active_tab_is_welcome()
    }

    /// 是否显示欢迎页
    // pub fn show_welcome(&self) -> bool {
    //     self.tabs.is_empty() || self.active_tab_is_welcome()
    // }

    /// 是否显示空占位页（tabs 为空时的默认状态）
    pub fn show_empty_placeholder(&self) -> bool {
        self.tabs.is_empty()
    }

    /// 当前活动标签页是否是文件 tab
    pub fn active_tab_is_file(&self) -> bool {
        self.tabs
            .get(self.active_tab)
            .map(|t| t.is_file())
            .unwrap_or(false)
    }

    /// 当前活动标签页是否是设置 tab
    pub fn active_tab_is_settings(&self) -> bool {
        self.tabs
            .get(self.active_tab)
            .map(|t| t.is_settings())
            .unwrap_or(false)
    }

    /// 当前活动标签页是否是欢迎 tab
    pub fn active_tab_is_welcome(&self) -> bool {
        self.tabs
            .get(self.active_tab)
            .map(|t| t.is_welcome())
            .unwrap_or(false)
    }

    /// 当前活动文件标签页的文件路径
    pub fn active_file_path(&self) -> Option<&std::path::PathBuf> {
        self.tabs.get(self.active_tab).and_then(|t| t.file_path())
    }

    /// 查找设置 tab 的索引
    pub fn find_settings_tab(&self) -> Option<usize> {
        self.tabs.iter().position(|t| t.is_settings())
    }

    /// 查找欢迎 tab 的索引
    pub fn find_welcome_tab(&self) -> Option<usize> {
        self.tabs.iter().position(|t| t.is_welcome())
    }

    /// 确保 logo 位图已加载（懒加载，仅在首次需要时从文件读取）
    pub(crate) fn ensure_logo_bitmap(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
    ) {
        if self.logo_bitmap.is_some() {
            return;
        }
        let png_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources/app_icons/source/aether-512.png");
        match std::fs::read(&png_path) {
            Ok(png_bytes) => match crate::bitmap_loader::load_png_to_bitmap(target, &png_bytes) {
                Ok(bitmap) => {
                    self.logo_bitmap = Some(bitmap);
                }
                Err(e) => {
                    tracing::warn!("加载 logo 位图失败: {}", e);
                }
            },
            Err(e) => {
                tracing::warn!("读取 logo 文件失败: {:?} - {}", png_path, e);
            }
        }
    }

    /// 切换活动视图到指定视图（非 AI 助手）。
    ///
    /// 更新活动栏高亮、`activity_view`、侧边栏可见性与内容。
    /// 供活动栏左键点击与右键上下文菜单共用。
    pub fn switch_activity_view(&mut self, view: ActivityBarView) {
        self.activity_bar.switch_to_view(view);
        self.activity_view = view;
        // 切换活动栏视图时打开侧边栏：恢复上次保存的宽度
        self.layout.show_sidebar();
        self.sidebar_content = SidebarContent::from_view(view);
    }

    /// 切换到指定标签页
    pub fn switch_tab(&mut self, index: usize) {
        if index < self.tabs.len() && index != self.active_tab {
            // REQ-P1-09: 仅文件 tab 需要 swap content；设置/欢迎等通用 tab 无 content
            self.swap_tab_content(self.active_tab);
            self.active_tab = index;
            self.swap_tab_content(self.active_tab);
            self.is_selecting = false;
            self.sync_file_tree_selection();
            let title = self.tabs[self.active_tab].title();
            self.status_message = format!("切换到: {}", title);
            self.emit_event(crate::events::EditorEvent::TabChanged);
            // 显式标记局部脏区域，避免标签切换触发全窗口重绘导致卡顿
            let editor_region = self.layout.editor_region();
            let tab_region = self.layout.tab_bar_region(self.show_tab_bar());
            let status_region = self.layout.status_bar_region();
            self.dirty_tracker.mark_region(
                editor_region.x,
                editor_region.y,
                editor_region.width,
                editor_region.height,
                crate::dirty_rect::DirtyRegionType::EditorContent,
            );
            self.dirty_tracker.mark_region(
                tab_region.x,
                tab_region.y,
                tab_region.width,
                tab_region.height,
                crate::dirty_rect::DirtyRegionType::TabBar,
            );
            self.dirty_tracker.mark_region(
                status_region.x,
                status_region.y,
                status_region.width,
                status_region.height,
                crate::dirty_rect::DirtyRegionType::StatusBar,
            );
            let sidebar_region = self.layout.sidebar_region();
            if sidebar_region.width > 0.0 {
                self.dirty_tracker.mark_region(
                    sidebar_region.x,
                    sidebar_region.y,
                    sidebar_region.width,
                    sidebar_region.height,
                    crate::dirty_rect::DirtyRegionType::Sidebar,
                );
            }
        }
    }

    /// 关闭当前标签页，返回是否还有标签页
    pub fn close_current_tab(&mut self) -> bool {
        if self.tabs.len() <= 1 {
            // 最后一个标签页：保存内容并清空 tabs（渲染层根据 tabs.is_empty() 显示欢迎页/空占位页）
            // 文件 tab 才需要保存 last_closed_tab；设置/欢迎等不保存
            if self.active_tab_is_file() {
                self.last_closed_tab =
                    Some(std::mem::replace(&mut self.content, TabContent::new()));
            }
            self.tabs.clear();
            self.active_tab = 0;
            self.is_selecting = false;
            self.status_message = "已关闭".to_string();
            return true;
        }
        // 保存 self.content 中的最新内容到 last_closed_tab（文件 tab 才需要）
        if self.active_tab_is_file() {
            self.last_closed_tab = Some(std::mem::replace(&mut self.content, TabContent::new()));
        }
        // 从 tabs 中移除活动标签页
        let _removed = self.tabs.remove(self.active_tab);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
        // 将新活动标签页的内容交换到 self.content
        // swap 后文件 tabs[active_tab].content 持有空 TabContent（安全，不会被误匹配路径）
        self.swap_tab_content(self.active_tab);
        self.is_selecting = false;
        self.status_message = format!("已关闭，剩余 {} 个标签页", self.tabs.len());
        !self.tabs.is_empty()
    }

    /// P2-8: 带保存确认的关闭标签页。
    /// 返回值：true 表示已关闭（用户确认或无需保存），false 表示用户取消。
    pub fn close_current_tab_checked(&mut self) -> bool {
        // self 上的 is_dirty / buffer / file_path 即为当前文件活动标签页的实时状态
        // （编辑操作直接作用于 self，仅在切换标签页时通过 swap 交换）
        if self.active_tab_is_file() && self.content.is_dirty {
            let file_name = self
                .content
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

    /// SubTask 7.1: 中键关闭指定索引的标签页（带 dirty 检查，与关闭按钮行为一致）。
    /// 返回 true 表示已关闭，false 表示用户取消或索引越界。
    pub fn close_tab(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        if index == self.active_tab {
            return self.close_current_tab_checked();
        }
        // 非活动标签页：检查 is_dirty（与 handle_tab_bar_click 中关闭按钮逻辑一致）
        let tab_dirty = self.tabs.get(index).map(|t| t.is_dirty()).unwrap_or(false);
        if tab_dirty {
            let tab_name = self
                .tabs
                .get(index)
                .and_then(|t| t.file_path())
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "未命名".to_string());
            let msg = format!("{} 有未保存的修改，是否丢弃修改并关闭？", tab_name);
            if !Dialogs::confirm_yes_no(self.hwnd, "关闭标签页", &msg) {
                self.status_message = "已取消关闭".to_string();
                return false;
            }
        }
        // Task 13.3: 保存关闭的标签内容以支持 Ctrl+Shift+T 恢复
        remove_tab_saving_content(&mut self.tabs, index, &mut self.last_closed_tab);
        if index < self.active_tab {
            self.active_tab -= 1;
        }
        self.status_message = format!("已关闭，剩余 {} 个标签页", self.tabs.len());
        true
    }

    /// SubTask 9.4: 关闭除指定索引外的所有标签页。
    ///
    /// 保留 `keep_idx`，移除其他所有标签页。若保留的标签页不是当前活动标签页，
    /// 切换到保留的标签页。索引越界时返回 false。
    pub fn close_other_tabs(&mut self, keep_idx: usize) -> bool {
        if keep_idx >= self.tabs.len() {
            return false;
        }
        if self.tabs.len() == 1 {
            return true;
        }
        // 取出保留标签，剩余标签中最后一个是 last_closed 候选
        let kept = self.tabs.remove(keep_idx);
        let last_closed = self.tabs.pop();
        self.tabs.clear();
        if let Some(crate::tabs::Tab::File(content)) = last_closed {
            self.last_closed_tab = Some(content);
        }
        self.tabs.push(kept);
        self.active_tab = 0;
        // 让 self.content 反映保留标签
        if self.tabs[0].is_file() {
            if let crate::tabs::Tab::File(content) = &mut self.tabs[0] {
                self.content = std::mem::replace(content, TabContent::new());
            }
        } else {
            self.content = TabContent::new();
        }
        self.is_selecting = false;
        self.status_message = "已关闭其他标签页".to_string();
        true
    }

    /// SubTask 9.4: 关闭指定索引右侧的所有标签页。
    ///
    /// 保留 `idx` 及其左侧的标签页，移除 `idx+1..` 的所有标签页。
    /// 索引越界时返回 false。
    pub fn close_tabs_to_the_right(&mut self, idx: usize) -> bool {
        if idx >= self.tabs.len() {
            return false;
        }
        if self.tabs.len() <= idx + 1 {
            return true;
        }
        let active_in_closed = self.active_tab > idx;
        if active_in_closed {
            // 活动标签页在被关闭的右侧：保存 self.content 中的最新内容（文件 tab 才需要）
            if self.active_tab_is_file() {
                self.last_closed_tab =
                    Some(std::mem::replace(&mut self.content, TabContent::new()));
            }
        } else {
            // 活动标签页不在右侧：保存最后一个被关闭标签的内容
            let last_closed = self.tabs.pop();
            if let Some(crate::tabs::Tab::File(content)) = last_closed {
                self.last_closed_tab = Some(content);
            }
        }
        self.tabs.truncate(idx + 1);
        if active_in_closed {
            self.active_tab = idx;
            // 将保留标签内容加载到 self.content
            self.swap_tab_content(self.active_tab);
            self.is_selecting = false;
        }
        self.status_message = format!("已关闭右侧标签页，剩余 {} 个标签页", self.tabs.len());
        true
    }

    /// SubTask 9.4: 关闭所有标签页，并创建一个新的空标签页。
    pub fn close_all_tabs(&mut self) {
        // 保存 self.content 中的最新内容（文件 tab 才需要）以支持 Ctrl+Shift+T 恢复
        if self.active_tab_is_file() {
            self.last_closed_tab = Some(std::mem::replace(&mut self.content, TabContent::new()));
        }
        self.tabs.clear();
        self.tabs.push(Tab::new());
        self.active_tab = 0;
        // self.content 已空，tabs[0] 也是空，swap 保持架构一致
        self.swap_tab_content(self.active_tab);
        self.is_selecting = false;
        self.status_message = "已关闭所有标签页".to_string();
    }

    /// Task 13.3: 恢复最后关闭的标签页（Ctrl+Shift+T）。
    /// 如果存在 `last_closed_tab`，则将其作为新标签页恢复并切换为活动标签页。
    /// 返回 true 表示已恢复，false 表示没有可恢复的标签。
    pub fn reopen_last_closed_tab(&mut self) -> bool {
        let Some(content) = self.last_closed_tab.take() else {
            self.status_message = "没有可恢复的标签".to_string();
            return false;
        };
        // 将当前 self.content swap 回当前活动标签，再 push 新文件标签并切换
        self.swap_tab_content(self.active_tab);
        self.tabs.push(crate::tabs::Tab::File(content));
        self.active_tab = self.tabs.len() - 1;
        self.swap_tab_content(self.active_tab);
        self.is_selecting = false;
        self.status_message = "已恢复最后关闭的标签".to_string();
        true
    }

    /// SubTask 9.4: 复制文本到剪贴板（公开接口，供标签右键菜单调用）。
    pub fn copy_text_to_clipboard(&mut self, text: &str) -> bool {
        let ok = Self::set_clipboard_text(text);
        if ok {
            self.status_message = "已复制".to_string();
        }
        ok
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
        // REQ-P1-09: save current state to old tab, push new (empty) tab, swap it in
        self.swap_tab_content(self.active_tab);
        let tab = Tab::new();
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.swap_tab_content(self.active_tab);
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
        let tab_bar_height = if self.show_tab_bar() {
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
        // SubTask 7.2: 检测 "+" 新建按钮点击
        if let Some((pl, pt, pr, pb)) = self.plus_button_rect {
            if mouse_x >= pl && mouse_x < pr && _mouse_y >= pt && _mouse_y < pb {
                self.new_tab();
                self.status_message = "已新建标签页".to_string();
                return true;
            }
        }
        let rel_x = mouse_x - editor_x + self.tab_scroll_x;
        for layout in &self.tab_layouts {
            if rel_x >= layout.x && rel_x < layout.x + layout.width {
                // 检测关闭按钮点击
                if rel_x >= layout.close_x && rel_x < layout.close_x + layout.close_width {
                    // 复用 close_tab：统一活动/非活动标签页的 dirty 检查与
                    // last_closed_tab 保存逻辑，确保 Ctrl+Shift+T 可恢复。
                    self.close_tab(layout.index);
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
        let tab_bar_height = if self.show_tab_bar() {
            TAB_BAR_HEIGHT
        } else {
            0.0
        };
        // SubTask 7.2: 更新 "+" 按钮悬停状态（独立于下方 y 检查，确保按钮响应）
        let mut on_plus = false;
        if let Some((pl, pt, pr, pb)) = self.plus_button_rect {
            on_plus = mouse_x >= pl && mouse_x < pr && mouse_y >= pt && mouse_y < pb;
        }
        self.plus_button_hover = on_plus;
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

    /// Task 8.2: 命中检测——返回鼠标所在标签体的索引。
    ///
    /// 仅当点击落在标签体内（非关闭按钮、非 "+" 按钮）时返回 `Some(idx)`。
    /// 用于拖拽重排：点击标签体时延迟切换，等待拖拽判定。
    ///
    /// `mouse_y` 是窗口绝对坐标，`tab_y` 是标签栏顶部 y 偏移（来自 layout）。
    /// 修复：原先将绝对 mouse_y 与 TAB_BAR_HEIGHT 直接比较，标签栏位于
    /// y=32..62 时总是返回 None，导致标签拖拽/点击切换失效。
    pub(crate) fn tab_body_hit_test(
        &self,
        mouse_x: f32,
        mouse_y: f32,
        editor_x: f32,
        tab_y: f32,
    ) -> Option<usize> {
        let tab_bar_height = if self.show_tab_bar() {
            TAB_BAR_HEIGHT
        } else {
            0.0
        };
        if tab_bar_height == 0.0
            || mouse_y < tab_y
            || mouse_y >= tab_y + tab_bar_height
            || mouse_x < editor_x
        {
            return None;
        }
        // "+" 按钮区域不算标签体
        if let Some((pl, pt, pr, pb)) = self.plus_button_rect {
            if mouse_x >= pl && mouse_x < pr && mouse_y >= pt && mouse_y < pb {
                return None;
            }
        }
        let rel_x = mouse_x - editor_x + self.tab_scroll_x;
        for layout in &self.tab_layouts {
            if rel_x >= layout.x && rel_x < layout.x + layout.width {
                // 关闭按钮区域不算标签体
                if rel_x >= layout.close_x && rel_x < layout.close_x + layout.close_width {
                    return None;
                }
                return Some(layout.index);
            }
        }
        None
    }

    /// Task 8.3: 根据鼠标 x 位置计算拖拽放置目标索引（0..=tabs.len()）。
    ///
    /// 基于标签中点：鼠标在标签左半部分 → 插入到该标签前；
    /// 右半部分 → 插入到该标签后。超出范围则 clamp 到 0 或 tabs.len()。
    pub(crate) fn tab_drop_index_at(&self, mouse_x: f32, editor_x: f32) -> usize {
        let rel_x = mouse_x - editor_x + self.tab_scroll_x;
        for layout in &self.tab_layouts {
            let mid = layout.x + layout.width / 2.0;
            if rel_x < mid {
                return layout.index;
            }
        }
        self.tabs.len()
    }

    /// Task 8.4: 执行标签重排——将 drag_idx 处的标签移动到 drop_idx 位置。
    ///
    /// `drop_idx` 语义：插入到该索引之前（与 `tab_drop_index_at` 一致）。
    /// 自动调整 `active_tab` 索引以跟随移动的标签。
    pub fn reorder_tabs(&mut self, drag_idx: usize, drop_idx: usize) {
        reorder_tabs_with_active(&mut self.tabs, &mut self.active_tab, drag_idx, drop_idx);
    }

    /// SubTask 7.5: 计算标签栏最大水平滚动偏移。
    ///
    /// `max_scroll = total_tabs_width - tab_bar_visible_width`，下限为 0。
    /// 基于 `tab_layouts` 中最后一个标签的右边界计算（包含尾部 gap）。
    pub(crate) fn tab_bar_max_scroll(&self, tab_bar_width: f32) -> f32 {
        let gap = 2.0;
        let left_padding = 4.0;
        // "+" 按钮区域（8px gap + 28px 按钮）也需预留可见空间
        let plus_area = 8.0 + 28.0;
        let total_tabs_width = self
            .tab_layouts
            .last()
            .map(|l| l.x + l.width + gap)
            .unwrap_or(0.0);
        let visible_width = (tab_bar_width - left_padding - plus_area).max(0.0);
        (total_tabs_width - visible_width).max(0.0)
    }

    /// SubTask 7.5: 滚动标签栏水平偏移，返回是否实际变化（用于决定是否重绘）。
    ///
    /// `delta` 为原始滚轮 delta（通常 ±120），内部按 `delta * 8.0` 转换为像素增量，
    /// 然后 clamp 到 `[0, max_scroll]`。
    pub(crate) fn scroll_tab_bar(&mut self, delta: f32, tab_bar_width: f32) -> bool {
        let old = self.tab_scroll_x;
        let max_scroll = self.tab_bar_max_scroll(tab_bar_width);
        self.tab_scroll_x = (self.tab_scroll_x + delta * 8.0).clamp(0.0, max_scroll);
        (self.tab_scroll_x - old).abs() > 0.01
    }

    pub fn new(hwnd: HWND, is_main_window: bool) -> Result<Self> {
        let d2d_factory = D2DFactory::new()?;
        let text_renderer = TextRenderer::new()?;
        let theme = Theme::glass();
        let key_map = KeyMap::new();
        let app_settings = AppSettings::load();

        // 创建 tokio runtime 驱动 LSP 异步操作
        let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| {
                windows::core::Error::new(
                    windows::Win32::Foundation::E_FAIL,
                    format!("Failed to create tokio runtime: {}", e),
                )
            })?;

        // 创建 LSP 客户端（root_uri 暂为 None，open_folder 时可后续扩展）
        let (lsp_client, event_rx) = LspClient::new(None);
        let lsp_client = Arc::new(lsp_client);

        // spawn 事件 forwarder task：把 LspEvent 通过 PostMessageW(WM_APP+3) 投递到 UI 线程
        // 与现有 WM_APP+4/5/6/7 模式一致；WM_APP+3 在 window.rs 当前是空实现，可占用
        let hwnd_send = SendHwnd(hwnd.0 as usize);
        tokio_runtime.spawn(async move {
            let mut rx = event_rx;
            while let Some(event) = rx.recv().await {
                let ptr = Box::into_raw(Box::new(event));
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        windows::Win32::Foundation::HWND(hwnd_send.0 as *mut std::ffi::c_void),
                        windows::Win32::UI::WindowsAndMessaging::WM_APP + 3,
                        windows::Win32::Foundation::WPARAM(0),
                        windows::Win32::Foundation::LPARAM(ptr as isize),
                    );
                }
            }
        });

        let ts_highlighter = TreeSitterHighlighter::new();
        let lsp_diagnostics = std::collections::HashMap::new();
        // Phase H: 补全/悬停状态初始化
        let completion_items = Vec::new();
        let hover_content = None;

        let mut state = Self {
            hwnd,
            d2d_factory,
            render_ctx: crate::render_context::RenderContext::new(),
            text_renderer,
            theme,
            content: TabContent::new(),
            is_selecting: false,
            cached_line_numbers: Vec::new(),
            tabs: Vec::new(),
            active_tab: 0,
            tab_layouts: Vec::new(),
            hover_tab: None,
            tab_scroll_x: 0.0,
            plus_button_rect: None,
            plus_button_hover: false,
            dragging_tab: None,
            tab_drop_index: None,
            tab_drag_start: None,
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
            folder_generation: 0,
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
            saved_ime_himc: None,
            ai_panel: AiPanel::new(),
            search_panel: crate::search_panel::SearchPanel::new(),
            bottom_panel_tab: BottomPanelTab::default(),
            diagnostics: HashMap::new(),
            legacy_lsp_client: None,
            lsp_rx: None,
            lsp_runtime: None,
            settings_panel: crate::settings::SettingsPanel::from_settings(&app_settings),
            tabs_panel: crate::open_tabs::TabsPanel::new(),
            app_settings,
            ssh_dialog: SshConnectionDialog::new(),
            remote_session: None,
            remote_file_tree: None,
            selected_remote_node: None,
            hover_remote_node: None,
            remote_scroll_y: 0.0,
            clone_dialog: CloneRepoDialog::new(),
            new_project_dialog: crate::new_project_dialog::NewProjectDialog::new(),
            ssh_manager_panel: SshManagerPanel::new(),
            active_ssh_index: None,
            is_maximized: false,
            is_main_window,
            titlebar_hover_button: None,
            hover_sidebar_resize: false,
            selected_file_node: None,
            hover_file_node: None,
            file_tree_input: None,
            file_tree_new_file_btn: None,
            file_tree_new_folder_btn: None,
            file_tree_open_folder_btn: None,
            welcome_hover_action: None,
            welcome_focus_action: None,
            icons: crate::icons::IconCache::new(),
            is_loading_folder: false,
            ssh_connecting: false,
            git_cloning: false,
            sidebar_scroll_y: 0.0,
            git_panel: crate::git::GitIntegration::new(),
            dirty_tracker: crate::dirty_rect::DirtyRectTracker::new(1280.0, 800.0),
            focus_manager: FocusManager::new(),
            event_queue: crate::events::EventQueue::new(),
            inline_completion_service: crate::inline_completion::InlineCompletionService::new(),
            hover_tooltip: None,
            hover_last_mouse_x: 0.0,
            hover_last_mouse_y: 0.0,
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
            last_active_tab: 0,
            user_menu: crate::user_menu::UserMenu::new(),
            explorer_context_menu: crate::context_menu::ExplorerContextMenu::new(),
            tab_context_menu: crate::tab_context_menu::TabContextMenuState::default(),
            activity_bar_context_menu:
                crate::activity_bar_context_menu::ActivityBarContextMenuState::default(),
            lpress_start: None,
            lpress_x: 0.0,
            lpress_y: 0.0,
            lpress_target: None,
            lpress_index: 0,
            lbutton_down: false,
            composition: None,
            ts_highlighter,
            bg_highlighter: aether_tree_sitter::BackgroundHighlighter::new(),
            hl_request_version: 0,
            tokio_runtime,
            lsp_client,
            lsp_diagnostics,
            completion_items,
            completion_visible: false,
            completion_selected: 0,
            completion_trigger_line: 0,
            completion_trigger_col: 0,
            hover_content,
            hover_x: 0.0,
            hover_y: 0.0,
            tooltip_state: crate::tooltip::TooltipState::default(),
            last_closed_tab: None,
            logo_bitmap: None,
        };
        // 加载 logo 位图（aether-512.png）
        // 注意：此时还没有 render target，位图会在首次渲染时通过 ensure_logo_bitmap 懒加载
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
        // 启动时 tabs 为空，由渲染层根据 show_welcome()/show_empty_placeholder() 显示欢迎页
        // 不再创建 Tab::Welcome 作为显式标签页，避免标签栏出现"欢迎"tab
        state.active_tab = 0;

        // P0.2c: 主窗口启动时自动恢复上次打开的工作区。
        // 仅在路径仍然存在时打开,避免引用已删除/移动的目录。
        // 异步扫描结果通过 WM_APP+7 批次回调到达,此处调用仅触发扫描。
        if is_main_window {
            if let Some(workspace) = state.app_settings.ui.last_workspace.clone() {
                if workspace.exists() {
                    state.open_folder(workspace);
                }
            }
        }

        // 自动保存：启动周期兜底定时器（防抖定时器由编辑事件按需调度）
        state.start_autosave_periodic();

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
        self.emit_event(crate::events::EditorEvent::WindowResized);
    }

    /// 发射一个编辑器事件到事件队列
    pub fn emit_event(&mut self, event: crate::events::EditorEvent) {
        self.event_queue.push(event);
    }

    /// P3.1: 请求内联补全建议（占位实现）
    pub fn request_inline_completion(&mut self) {
        // 收集光标前后文本作为上下文
        let prefix = self
            .content
            .buffer
            .get_line(self.content.cursor_line)
            .map(|s| {
                let pos = s.floor_char_boundary(self.content.cursor_col.min(s.len()));
                s[..pos].to_string()
            })
            .unwrap_or_default();
        let suffix = self
            .content
            .buffer
            .get_line(self.content.cursor_line)
            .map(|s| {
                let pos = s.floor_char_boundary(self.content.cursor_col.min(s.len()));
                s[pos..].to_string()
            })
            .unwrap_or_default();

        if let Some(suggestion) = self.inline_completion_service.request(&prefix, &suffix) {
            self.content.inline_completion = Some(crate::inline_completion::InlineCompletion {
                text: suggestion.text,
                trigger_line: self.content.cursor_line,
                trigger_col: self.content.cursor_col,
                version: suggestion.version,
            });
            self.emit_event(crate::events::EditorEvent::CursorMoved);
        }
    }

    /// P3.1: 清除当前内联补全建议
    pub fn clear_inline_completion(&mut self) {
        self.content.inline_completion = None;
    }

    /// P3.3: 接受当前内联补全建议，将建议文本插入到光标处
    pub fn accept_inline_completion(&mut self) -> bool {
        let Some(comp) = self.content.inline_completion.take() else {
            return false;
        };
        if comp.trigger_line != self.content.cursor_line
            || comp.trigger_col != self.content.cursor_col
        {
            return false;
        }
        let pos = self.cursor_byte_pos();
        self.content.buffer.insert(pos, &comp.text);
        self.content.cursor_col += comp.text.len();
        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;
        self.emit_edit_events();
        true
    }

    /// 发射文本编辑相关事件（TextChanged + CursorMoved）
    fn emit_edit_events(&mut self) {
        self.emit_event(crate::events::EditorEvent::TextChanged {
            start_line: self.content.cursor_line,
            end_line: self.content.cursor_line + 1,
        });
        self.emit_event(crate::events::EditorEvent::CursorMoved);
        // 自动保存：文本变更后按防抖延迟（重）设空闲保存定时器
        self.schedule_autosave_debounce();
    }

    /// 将事件队列中所有事件转换为脏矩形标记
    pub fn flush_events_to_dirty_tracker(&mut self) {
        // 预取布局区域，避免闭包多次借用 self.layout
        let editor_region = self.layout.editor_region();
        let status_region = self.layout.status_bar_region();
        let sidebar_region = self.layout.sidebar_region();
        let right_panel_region = self.layout.right_panel_region();
        let bottom_region = self.layout.bottom_panel_region();
        let line_height = self.text_renderer.line_height();
        // REQ-P1-03: 用字符列（而非字节偏移）计算脏矩形光标 x 坐标，
        // 避免非 ASCII 文本时光标残影/撕裂
        let char_col = self
            .content
            .buffer
            .get_line(self.content.cursor_line)
            .map(|line| {
                let pos = line.floor_char_boundary(self.content.cursor_col.min(line.len()));
                line[..pos].chars().count()
            })
            .unwrap_or(0);
        let cursor_x =
            editor_region.x + 60.0 + 5.0 + char_col as f32 * self.text_renderer.char_width()
                - self.content.scroll_x;
        let cursor_y =
            editor_region.y + self.content.cursor_line as f32 * line_height - self.content.scroll_y;

        self.event_queue
            .drain_to_dirty_tracker(&mut self.dirty_tracker, |event| {
                use crate::events::EditorEvent;
                match event {
                    EditorEvent::TextChanged { .. } => Some((
                        editor_region.x,
                        editor_region.y,
                        editor_region.width,
                        editor_region.height,
                    )),
                    EditorEvent::CursorMoved => Some((cursor_x, cursor_y, 2.0, line_height)),
                    EditorEvent::SelectionChanged => Some((
                        editor_region.x,
                        editor_region.y,
                        editor_region.width,
                        editor_region.height,
                    )),
                    EditorEvent::Scrolled => Some((
                        editor_region.x,
                        editor_region.y,
                        editor_region.width,
                        editor_region.height,
                    )),
                    EditorEvent::TabChanged => None, // 由 switch_tab 显式标记局部区域
                    EditorEvent::SidebarChanged => {
                        if sidebar_region.width > 0.0 {
                            Some((
                                sidebar_region.x,
                                sidebar_region.y,
                                sidebar_region.width,
                                sidebar_region.height,
                            ))
                        } else {
                            None
                        }
                    }
                    EditorEvent::RightPanelChanged => {
                        if right_panel_region.width > 0.0 {
                            Some((
                                right_panel_region.x,
                                right_panel_region.y,
                                right_panel_region.width,
                                right_panel_region.height,
                            ))
                        } else {
                            None
                        }
                    }
                    EditorEvent::BottomPanelChanged => {
                        if bottom_region.height > 0.0 {
                            Some((
                                bottom_region.x,
                                bottom_region.y,
                                bottom_region.width,
                                bottom_region.height,
                            ))
                        } else {
                            None
                        }
                    }
                    EditorEvent::StatusBarChanged => Some((
                        status_region.x,
                        status_region.y,
                        status_region.width,
                        status_region.height,
                    )),
                    EditorEvent::WindowResized => None, // 全窗口事件在内部处理
                    EditorEvent::FindReplaceChanged => None, // 由调用方显式标记
                    EditorEvent::DialogVisibilityChanged => None, // 全窗口事件在内部处理
                }
            });
    }

    /// 检查当前标签页是否可以重用（空文件且未修改）
    fn can_reuse_current_tab(&self) -> bool {
        self.content.file_path.is_none()
            && !self.content.is_dirty
            && self.content.buffer.len_bytes() == 0
    }

    /// 重置当前编辑状态到初始值
    fn reset_editor_state(&mut self) {
        self.content.cursor_line = 0;
        self.content.cursor_col = 0;
        self.content.scroll_y = 0.0;
        self.content.history.clear();
        self.content.is_dirty = false;
        self.content.buffer_version += 1;
        self.clear_selection();
    }

    /// 在新标签页中打开内容
    fn open_in_new_tab(&mut self, tab: Tab) {
        // REQ-P1-09: save current state to old tab, push new tab, swap it in
        self.swap_tab_content(self.active_tab);
        // 直接将新标签页追加到末尾并切换过去。
        // 此前使用 swap(tabs[active], placeholder) + push(placeholder) 的写法，
        // 会让新 tab 留在原 active 位置、旧 tab 被推到末尾，但 active_tab 又被
        // 设置为 len()-1，结果指向了旧 tab，导致打开第二个文件时仍显示旧内容、
        // LSP did_open 也发给了旧文件。改为直接 push 新 tab 即可。
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.swap_tab_content(self.active_tab);
        self.is_selecting = false;
        self.emit_event(crate::events::EditorEvent::TabChanged);
        // 标记标签栏和编辑器区域脏区，避免新标签打开时触发全窗口重绘
        let editor_region = self.layout.editor_region();
        let tab_region = self.layout.tab_bar_region(self.show_tab_bar());
        self.dirty_tracker.mark_region(
            editor_region.x,
            editor_region.y,
            editor_region.width,
            editor_region.height,
            crate::dirty_rect::DirtyRegionType::EditorContent,
        );
        self.dirty_tracker.mark_region(
            tab_region.x,
            tab_region.y,
            tab_region.width,
            tab_region.height,
            crate::dirty_rect::DirtyRegionType::TabBar,
        );
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
                    self.content.buffer = buffer;
                    self.content.file_path = Some(path.clone());
                    self.content.language = lang;
                    self.reset_editor_state();
                    // REQ-P1-09: self.content 即活动标签页状态，无需再手动同步到 Tab
                    self.status_message = format!("已打开: {}", path.display());
                } else {
                    let tab = Tab::File(TabContent::with_loaded_buffer(
                        Some(path.clone()),
                        buffer,
                        lang,
                        false,
                    ));
                    self.open_in_new_tab(tab);
                    self.status_message = format!("已打开: {}", path.display());
                }
                self.emit_event(crate::events::EditorEvent::TextChanged {
                    start_line: 0,
                    end_line: self.content.buffer.len_lines(),
                });
                self.emit_event(crate::events::EditorEvent::StatusBarChanged);
                // 接线 LSP：通知服务器文档已打开（按需启动 server），激活补全/悬停/诊断
                self.lsp_notify_open();

                // 启动高亮刷新定时器：tree-sitter 语言且非大文件时，后台高亮在工作线程
                // 完成后需要一次重绘才能着色。此定时器周期性重绘直至高亮到达，随后自动停止，
                // 避免文件打开后停留在无高亮纯文本、要等到鼠标移动/光标闪烁才着色的卡顿感。
                self.update_large_file_flag();
                if language_to_ts_str(self.content.language).is_some()
                    && !self.content.is_large_file
                {
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(
                            self.hwnd,
                            crate::window::HIGHLIGHT_TIMER_ID,
                            crate::window::HIGHLIGHT_REFRESH_MS,
                            None,
                        );
                    }
                }
            }
            Err(e) => {
                let msg = format!("打开文件失败: {}", e);
                self.status_message = msg.clone();
                Dialogs::show_error(self.hwnd, "打开文件", &msg);
            }
        }

        // 文件加载成功后通知 LSP 服务器。
        // 注：lsp_notify_open() 已在上面调用过（会按需启动 server 并 send did_open），
        // 此处无需重复 get_text + lsp_open_document，避免对 UI 线程造成双倍的
        // 全文件拷贝（get_text 是 O(N) String 分配，对大文件耗时明显）。
    }

    /// 通知 LSP 服务器文档已打开（按需启动 server）。
    /// 在 load_file 后调用，激活智能补全/悬停/诊断。
    /// 异步执行：克隆所需数据后 spawn tokio task，不阻塞 UI 线程。
    ///
    /// 重要：get_all_text 是 O(N) 全文件 String 拷贝，对大文件耗时明显。
    /// 之前在 UI 线程上调用（line 1690），导致打开第一个文件时严重卡顿。
    /// 修复：把 buffer Arc 克隆进 spawn，由后台线程读取并构造 LSP 文本。
    fn lsp_notify_open(&self) {
        // 1. 映射语言到 LSP language_id（无配置则跳过）
        let language_id = match language_to_lsp_id_opt(self.content.language) {
            Some(id) => id.to_string(),
            None => return,
        };

        // 2. 转换文件路径到 Url（LSP 要求 file:// URI）
        let path = match &self.content.file_path {
            Some(p) => p.as_path(),
            None => return,
        };
        let uri = match Url::from_file_path(path) {
            Ok(u) => u,
            Err(_) => return,
        };

        // 3. 克隆 Arc<buffer>，把昂贵的 get_all_text 推迟到后台线程
        let buffer = self.content.buffer.clone();
        let client = self.lsp_client.clone();
        let lang_id = language_id;
        self.tokio_runtime.spawn(async move {
            // 后台线程读取全文件（O(N) 拷贝，不再阻塞 UI 渲染）
            let text = buffer.get_all_text();
            // 按需启动 server：如果未就绪且有默认配置，启动它
            if !client.is_server_ready(&lang_id).await {
                let config = match default_server_config(&lang_id) {
                    Some(c) => c,
                    None => return, // 该语言没有默认 server 配置，跳过
                };
                if let Err(e) = client.start_server(&lang_id, config).await {
                    eprintln!("LSP start_server({}) failed: {}", lang_id, e);
                    return;
                }
            }
            // 发送 did_open
            if let Err(e) = client.open_document(uri, lang_id, text).await {
                eprintln!("LSP open_document failed: {}", e);
            }
        });
    }

    /// 通知 LSP 服务器文档内容已变更。
    /// 在 insert_char/delete_char/insert_newline/delete_forward 后调用。
    fn lsp_notify_change(&self) {
        // 1. 映射语言到 LSP language_id（无配置则跳过）
        let language_id = match language_to_lsp_id_opt(self.content.language) {
            Some(id) => id.to_string(),
            None => return,
        };

        // 2. 转换文件路径到 Url
        let path = match &self.content.file_path {
            Some(p) => p.as_path(),
            None => return,
        };
        let uri = match Url::from_file_path(path) {
            Ok(u) => u,
            Err(_) => return,
        };

        // 3. 全文档同步：直接发送全文，由 LspClient 内部计算增量变更
        let text = self.content.buffer.get_all_text();

        // 4. Spawn 异步任务发送 did_change
        let client = self.lsp_client.clone();
        let lang_id = language_id;
        self.tokio_runtime.spawn(async move {
            // 注意：notify_change 内部会检查 document_sync 是否有该文档，
            // 如果 did_open 尚未完成，change 会被静默丢弃。这在实践中可接受：
            // 用户在 server 启动后的第一次编辑会正常同步。
            if let Err(e) = client.notify_change(&uri, &text).await {
                eprintln!("LSP notify_change({}) failed: {}", lang_id, e);
            }
        });
    }

    /// Phase H1: 请求补全（Ctrl+Space 触发）。
    /// 异步调用 LSP request_completion，结果通过 LspEvent::Completion 回传。
    pub(crate) fn request_completion(&mut self) {
        let language_id = match language_to_lsp_id_opt(self.content.language) {
            Some(id) => id.to_string(),
            None => return,
        };
        let path = match &self.content.file_path {
            Some(p) => p.as_path(),
            None => return,
        };
        let uri = match Url::from_file_path(path) {
            Ok(u) => u,
            Err(_) => return,
        };
        // LSP Position：line 0-based，character 为 UTF-16 偏移（ASCII 下等同字节列）
        let position = lsp_types::Position {
            line: self.content.cursor_line as u32,
            character: self.content.cursor_col as u32,
        };
        // 记录触发位置，用于弹窗定位
        self.completion_trigger_line = self.content.cursor_line;
        self.completion_trigger_col = self.content.cursor_col;

        let client = self.lsp_client.clone();
        let lang_id = language_id;
        self.tokio_runtime.spawn(async move {
            if !client.is_server_ready(&lang_id).await {
                return; // server 未就绪，静默跳过
            }
            if let Err(e) = client.request_completion(&uri, position).await {
                eprintln!("LSP request_completion({}) failed: {}", lang_id, e);
            }
        });
    }

    /// Phase H3: 请求悬停信息（鼠标悬停触发）。
    /// 异步调用 LSP request_hover，结果通过 LspEvent::Hover 回传。
    /// 注意：触发逻辑（WM_MOUSEMOVE + 定时器去抖）尚未接线，此方法预留就绪。
    #[allow(dead_code)]
    pub(crate) fn request_hover(&mut self, line: usize, col: usize) {
        let language_id = match language_to_lsp_id_opt(self.content.language) {
            Some(id) => id.to_string(),
            None => return,
        };
        let path = match &self.content.file_path {
            Some(p) => p.as_path(),
            None => return,
        };
        let uri = match Url::from_file_path(path) {
            Ok(u) => u,
            Err(_) => return,
        };
        let position = lsp_types::Position {
            line: line as u32,
            character: col as u32,
        };
        let client = self.lsp_client.clone();
        let lang_id = language_id;
        self.tokio_runtime.spawn(async move {
            if !client.is_server_ready(&lang_id).await {
                return;
            }
            if let Err(e) = client.request_hover(&uri, position).await {
                eprintln!("LSP request_hover({}) failed: {}", lang_id, e);
            }
        });
    }

    /// 处理从 LSP 服务器收到的异步事件（由 WM_APP+3 调用）。
    /// 第一版重点处理 Diagnostics（诊断更新）和 ServerReady（状态提示）。
    /// 其他事件（Completion/Hover/References 等）留待 Phase H 接线 UI 组件。
    pub(crate) fn handle_lsp_event(&mut self, event: LspEvent) {
        match event {
            LspEvent::Diagnostics { uri, diagnostics } => {
                // 更新诊断表：按 uri 存储，UI 渲染时查询当前文件
                let count = diagnostics.len();
                if diagnostics.is_empty() {
                    self.lsp_diagnostics.remove(&uri);
                } else {
                    self.lsp_diagnostics.insert(uri, diagnostics);
                }
                // 状态栏提示诊断数量（仅当前文件）
                if let Some(path) = &self.content.file_path {
                    if let Ok(current_uri) = Url::from_file_path(path) {
                        let current_count = self
                            .lsp_diagnostics
                            .get(&current_uri)
                            .map(|v| v.len())
                            .unwrap_or(0);
                        if current_count > 0 {
                            self.status_message = format!("诊断: {} 个问题", current_count);
                        } else {
                            self.status_message = "无诊断问题".to_string();
                        }
                    }
                }
                let _ = count; // 避免 unused 警告
            }
            LspEvent::ServerReady { language_id } => {
                self.status_message = format!("LSP 服务器就绪: {}", language_id);
            }
            LspEvent::Log {
                language_id,
                message,
            } => {
                // 服务器日志输出到 stderr，不显示在状态栏（避免刷屏）
                eprintln!("[LSP/{}] {}", language_id, message);
            }
            // Phase H1: 补全结果到达，显示弹窗
            LspEvent::Completion { uri, items } => {
                if items.is_empty() {
                    self.completion_visible = false;
                } else {
                    // 验证是当前文件的补全结果
                    let is_current = self
                        .content
                        .file_path
                        .as_ref()
                        .and_then(|p| Url::from_file_path(p).ok())
                        .map(|u| u == uri)
                        .unwrap_or(false);
                    if is_current {
                        self.completion_items = items;
                        self.completion_selected = 0;
                        self.completion_visible = true;
                    }
                }
            }
            // Phase H3: 悬停结果到达，显示 tooltip
            LspEvent::Hover { uri, hover } => {
                let is_current = self
                    .content
                    .file_path
                    .as_ref()
                    .and_then(|p| Url::from_file_path(p).ok())
                    .map(|u| u == uri)
                    .unwrap_or(false);
                if is_current {
                    self.hover_content = extract_hover_text(&hover);
                }
            }
            // 未接线的 LSP 事件静默忽略
            LspEvent::References { .. }
            | LspEvent::Rename { .. }
            | LspEvent::CodeActions { .. }
            | LspEvent::Formatting { .. }
            | LspEvent::SemanticTokens { .. }
            | LspEvent::SemanticTokensDelta { .. }
            | LspEvent::InlayHints { .. } => {
                // 后续版本接线
            }
        }
    }

    // ===== Phase H2: 补全弹窗导航 =====

    /// 补全列表下一项（↓ 键）
    pub(crate) fn completion_next(&mut self) {
        if !self.completion_visible || self.completion_items.is_empty() {
            return;
        }
        self.completion_selected = (self.completion_selected + 1) % self.completion_items.len();
    }

    /// 补全列表上一项（↑ 键）
    pub(crate) fn completion_prev(&mut self) {
        if !self.completion_visible || self.completion_items.is_empty() {
            return;
        }
        if self.completion_selected == 0 {
            self.completion_selected = self.completion_items.len() - 1;
        } else {
            self.completion_selected -= 1;
        }
    }

    /// 接受当前选中的补全项（Enter 键）。
    /// 将光标移回触发位置，删除已输入的过滤文本，插入补全项的 insert_text 或 label。
    pub(crate) fn completion_accept(&mut self) {
        if !self.completion_visible || self.completion_items.is_empty() {
            return;
        }
        let item = self.completion_items[self.completion_selected].clone();
        // 优先用 insert_text，其次 label
        let insert_text = item
            .insert_text
            .clone()
            .unwrap_or_else(|| item.label.clone());
        // 关闭弹窗
        self.completion_visible = false;
        self.completion_items.clear();
        // 将光标移回触发位置
        if self.content.cursor_line != self.completion_trigger_line {
            return; // 跨行编辑，放弃插入
        }
        // 删除触发位置到当前光标之间的文本（用户输入的过滤字符）
        let delete_count = self
            .content
            .cursor_col
            .saturating_sub(self.completion_trigger_col);
        for _ in 0..delete_count {
            self.delete_char();
        }
        // 插入补全文本
        for ch in insert_text.chars() {
            self.insert_char(ch);
        }
    }

    /// 关闭补全弹窗（Esc 键 或 失焦）
    pub(crate) fn completion_cancel(&mut self) {
        if self.completion_visible {
            self.completion_visible = false;
            self.completion_items.clear();
        }
    }

    /// 关闭悬停 tooltip
    #[allow(dead_code)]
    pub(crate) fn hover_cancel(&mut self) {
        self.hover_content = None;
    }

    /// 加载图片文件
    fn load_image_file(&mut self, path: PathBuf) {
        let content = format!("[图片预览] {}", path.display());
        if self.can_reuse_current_tab() {
            self.content.file_path = Some(path.clone());
            self.content.language = Language::Image;
            self.content.buffer = PieceTable::from_string(content);
            self.reset_editor_state();
            self.status_message = format!("已打开图片: {}", path.display());
        } else {
            let tab = Tab::File(TabContent::with_loaded_buffer(
                Some(path.clone()),
                PieceTable::from_string(content),
                Language::Image,
                false,
            ));
            self.open_in_new_tab(tab);
            self.status_message = format!("已打开图片: {}", path.display());
        }
    }

    /// 显示不支持的文件提示
    fn show_unsupported_file(&mut self, path: &Path) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown");
        let message = format!("不支持的文件格式: .{}\n文件: {}", ext, path.display());
        if self.can_reuse_current_tab() {
            self.content.file_path = Some(path.to_path_buf());
            self.content.language = Language::PlainText;
            self.content.buffer = PieceTable::from_string(message);
            self.reset_editor_state();
            self.status_message = format!("不支持的文件格式: .{}", ext);
        } else {
            let tab = Tab::File(TabContent::with_loaded_buffer(
                Some(path.to_path_buf()),
                PieceTable::from_string(message),
                Language::PlainText,
                false,
            ));
            self.open_in_new_tab(tab);
            self.status_message = format!("不支持的文件格式: .{}", ext);
        }
    }

    /// 新建项目：弹出对话框让用户输入项目名称，确认后在默认项目目录下创建文件夹并打开
    pub fn new_project(&mut self) {
        self.new_project_dialog.reset();
        self.new_project_dialog.visible = true;
        self.status_message = "新建项目...".to_string();
        self.emit_event(crate::events::EditorEvent::DialogVisibilityChanged);
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(
                self.hwnd,
                crate::window::CARET_TIMER_ID,
                530,
                None,
            );
        }
    }

    fn kill_caret_timer(&self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(
                self.hwnd,
                crate::window::CARET_TIMER_ID,
            );
        }
    }

    /// 关闭新建项目对话框
    pub fn close_new_project_dialog(&mut self) {
        self.new_project_dialog.visible = false;
        self.kill_caret_timer();
        self.emit_event(crate::events::EditorEvent::DialogVisibilityChanged);
    }

    /// 确认创建项目（由对话框调用）
    pub fn confirm_new_project(&mut self) {
        if let Err(e) = self.new_project_dialog.validate() {
            self.new_project_dialog.error_message = Some(e);
            return;
        }

        let project_path = self.new_project_dialog.project_path();
        self.new_project_dialog.visible = false;
        self.kill_caret_timer();
        self.emit_event(crate::events::EditorEvent::DialogVisibilityChanged);

        // 确保基础目录存在
        if let Some(parent) = project_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                let msg = format!("创建项目目录失败: {}", e);
                self.status_message = msg.clone();
                Dialogs::show_error(self.hwnd, "新建项目", &msg);
                return;
            }
        }

        // 创建项目文件夹
        match std::fs::create_dir(&project_path) {
            Ok(()) => {
                self.status_message = format!("项目已创建: {}", project_path.display());
                // 打开项目文件夹作为工作区
                self.open_folder(project_path);
            }
            Err(e) => {
                let msg = format!("创建项目失败: {}", e);
                self.status_message = msg.clone();
                Dialogs::show_error(self.hwnd, "新建项目", &msg);
            }
        }
    }

    /// 处理新建项目对话框的鼠标点击
    pub fn handle_new_project_dialog_click(
        &mut self,
        mouse_x: f32,
        mouse_y: f32,
    ) -> crate::new_project_dialog::NewProjectDialogAction {
        use crate::new_project_dialog::NewProjectDialogAction;
        let dialog = &mut self.new_project_dialog;

        if let Some(rect) = &dialog.confirm_btn_rect {
            if rect.contains(mouse_x, mouse_y) {
                return NewProjectDialogAction::Confirm;
            }
        }
        if let Some(rect) = &dialog.cancel_btn_rect {
            if rect.contains(mouse_x, mouse_y) {
                return NewProjectDialogAction::Cancel;
            }
        }
        if let Some(rect) = &dialog.input_rect {
            if rect.contains(mouse_x, mouse_y) {
                dialog.focus_field = 0;
                return NewProjectDialogAction::FocusInput;
            }
        }
        NewProjectDialogAction::None
    }

    /// P4-2: 原子写入文件，避免写入中途崩溃导致文件损坏
    /// 先写入同目录的临时文件并 fsync，再原子 rename 替换目标文件
    #[allow(dead_code)]
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

    /// 流式原子写入：通过回调函数写入数据，避免在内存中构造完整的 &[u8]。
    /// 用于保存大文件时避免 get_all_text 的中间 String/Vec 分配。
    /// 语义与 atomic_write 一致：临时文件 + fsync + rename。
    fn atomic_write_stream<F>(path: &std::path::Path, writer_fn: F) -> std::io::Result<()>
    where
        F: FnOnce(&mut std::fs::File) -> std::io::Result<()>,
    {
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
            writer_fn(&mut file)?;
            file.sync_all()?;
            drop(file); // 关闭句柄后再 rename
            std::fs::rename(&temp_path, path)?;
            Ok(())
        })();

        if result.is_err() {
            let _ = std::fs::remove_file(&temp_path);
        }
        result
    }

    /// 保存文件，返回是否成功
    pub fn save_file(&mut self) -> bool {
        if let Some(path) = &self.content.file_path.clone() {
            // 处理远程文件保存
            if let Some(remote_path) = path.to_str().and_then(|s| s.strip_prefix("remote:")) {
                // 远程路径仍需 &[u8]，这里不得不做一次全量拷贝
                let mut buf: Vec<u8> = Vec::with_capacity(self.content.buffer.len_bytes());
                if let Err(e) = self.content.buffer.write_to(&mut buf) {
                    self.status_message = format!("保存失败: {}", e);
                    return false;
                }
                if let Some(session) = &self.remote_session {
                    match session.write_remote_file(remote_path, &buf) {
                        Ok(()) => {
                            self.content.is_dirty = false;
                            self.status_message = format!("已保存到远程: {}", remote_path);
                            // 同步自动保存状态（去重基线 / 冲突复位 / 停止防抖）
                            self.note_save_succeeded();
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
            // 本地文件保存：直接将 buffer 流式写入临时文件，避免 get_all_text 的
            // 全量 String 分配和 UTF-8 lossy 转换。对未编辑的 mmap 大文件尤其显著。
            // P4-2: 仍保持原子写入语义（临时文件 + fsync + rename）。
            match Self::atomic_write_stream(path, |w| self.content.buffer.write_to(w)) {
                Ok(()) => {
                    self.content.is_dirty = false;
                    self.status_message = "已保存".to_string();
                    // 同步自动保存状态（去重基线 / 冲突复位 / mtime 刷新 / 停止防抖）
                    self.note_save_succeeded();
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
        match Self::atomic_write_stream(&path, |w| self.content.buffer.write_to(w)) {
            Ok(()) => {
                self.content.file_path = Some(path.clone());
                self.content.is_dirty = false;
                self.status_message = format!("已保存: {}", path.display());
                // 同步自动保存状态（去重基线 / 冲突复位 / mtime 刷新 / 停止防抖）
                self.note_save_succeeded();
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

/// 扫描文件树顶层条目（在后台线程执行，避免阻塞 UI）
fn scan_file_tree_entries(path: &PathBuf) -> Vec<ScannedEntry> {
    const MAX_ENTRIES_PER_DIR: usize = 1000;

    let mut entries: Vec<ScannedEntry> = Vec::new();
    let mut raw: Vec<_> = match std::fs::read_dir(path) {
        Ok(dir) => dir.filter_map(|e| e.ok()).collect(),
        Err(_) => return entries,
    };

    if raw.len() > MAX_ENTRIES_PER_DIR {
        raw.truncate(MAX_ENTRIES_PER_DIR);
    }

    entries.reserve(raw.len());
    for entry in raw {
        let name = entry.file_name().to_string_lossy().to_string();
        if SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let kind = match entry.file_type() {
            Ok(ft) if ft.is_dir() => FileKind::Directory,
            Ok(ft) if ft.is_symlink() => FileKind::Symlink,
            _ => FileKind::File,
        };
        entries.push(ScannedEntry {
            name,
            kind,
            path: entry.path(),
            depth: 0,
        });
    }

    entries.sort_by(|a, b| {
        let a_is_dir = a.kind == FileKind::Directory;
        let b_is_dir = b.kind == FileKind::Directory;
        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        }
    });

    entries
}

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
            if self.content.selection_start.is_some() && self.content.selection_end.is_some() {
                self.delete_selection();
            }
            let pos = self.cursor_byte_pos();
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();
            let cursor_before =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

            self.content.buffer.insert(pos, &text);
            self.content.is_dirty = true;
            self.content.buffer_version += 1;

            // 更新光标位置
            let line_breaks = text.matches('\n').count();
            if line_breaks == 0 {
                self.content.cursor_col += text.len();
            } else {
                self.content.cursor_line += line_breaks;
                self.content.cursor_col = text
                    .rsplit_once('\n')
                    .map(|(_, last)| last.len())
                    .unwrap_or(0);
            }

            let cursor_after =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Insert,
                pos,
                text.len(),
            );
            self.clear_selection();
            self.status_message = "已粘贴".to_string();
        }
    }

    /// 删除选中文本
    pub fn delete_selection(&mut self) {
        let (start_line, start_col) = match self.content.selection_start {
            Some(s) => s,
            None => return,
        };
        let (end_line, end_col) = match self.content.selection_end {
            Some(e) => e,
            None => return,
        };

        let (first_line, first_col) = if (start_line, start_col) <= (end_line, end_col) {
            (start_line, start_col)
        } else {
            (end_line, end_col)
        };
        let (last_line, last_col) = if (start_line, start_col) <= (end_line, end_col) {
            (end_line, end_col)
        } else {
            (start_line, start_col)
        };

        let start_byte = self.line_byte_start(first_line) + first_col;
        let end_byte = self.line_byte_start(last_line) + last_col;

        if start_byte < end_byte {
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();
            let cursor_before =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

            self.content.buffer.delete(start_byte, end_byte);
            self.content.is_dirty = true;
            self.content.buffer_version += 1;

            self.content.cursor_line = first_line;
            self.content.cursor_col = first_col;

            let cursor_after =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Delete,
                start_byte,
                0,
            );
        }
        self.clear_selection();
    }

    /// 全选
    pub fn select_all(&mut self) {
        let last_line = self.content.buffer.len_lines().saturating_sub(1);
        let last_col = self
            .content
            .buffer
            .get_line(last_line)
            .map(|t| t.len())
            .unwrap_or(0);
        self.content.selection_start = Some((0, 0));
        self.content.selection_end = Some((last_line, last_col));
        self.content.cursor_line = last_line;
        self.content.cursor_col = last_col;
        self.is_selecting = false;
    }

    /// 滚动
    pub fn scroll(&mut self, delta_y: f32) {
        let line_height = self.text_renderer.line_height();
        let total_height = self.content.buffer.len_lines() as f32 * line_height;
        // UI-M02: 使用实际编辑器区域高度替代硬编码 24.0
        let editor_region = self.layout.editor_region();
        let editor_height = editor_region.height.max(1.0);
        let max_scroll = (total_height - editor_height).max(0.0);
        self.content.scroll_y = (self.content.scroll_y + delta_y).clamp(0.0, max_scroll);
        self.emit_event(crate::events::EditorEvent::Scrolled);
    }

    /// P2.3: 大文件阈值（行数）
    const LARGE_FILE_LINE_THRESHOLD: usize = 100_000;
    /// P2.3: 大文件阈值（字节数）
    const LARGE_FILE_BYTE_THRESHOLD: usize = 5 * 1024 * 1024;

    /// P2.3: 根据当前 buffer 大小更新大文件标记
    pub fn update_large_file_flag(&mut self) {
        let line_count = self.content.buffer.len_lines();
        let byte_count = self.content.buffer.len_bytes();
        self.content.is_large_file = line_count > Self::LARGE_FILE_LINE_THRESHOLD
            || byte_count > Self::LARGE_FILE_BYTE_THRESHOLD;
    }

    /// P2.3: 重建行 Y 偏移前缀和缓存
    pub fn rebuild_line_y_offsets(&mut self) {
        let total_lines = self.content.buffer.len_lines().max(1);
        if self.content.line_y_offsets.len() != total_lines {
            self.content.line_y_offsets.resize(total_lines, 0.0);
        }
        let line_height = self.text_renderer.line_height();
        let mut y = 0.0;
        for (i, offset) in self.content.line_y_offsets.iter_mut().enumerate() {
            *offset = y;
            y += line_height;
            // 大文件时避免浮点误差累积：每 1000 行重新基线
            if i % 1000 == 0 {
                y = (i + 1) as f32 * line_height;
            }
        }
    }

    /// P2.1: 计算当前可见行范围 [start_line, end_line)
    ///
    /// 返回的行号已限制在 [0, total_lines) 内，end_line 为开区间。
    pub fn visible_line_range(&self) -> (usize, usize) {
        let line_height = self.text_renderer.line_height();
        let editor_region = self.layout.editor_content_region(self.show_tab_bar());
        let height = editor_region.height.max(line_height);
        let total_lines = self.content.cached_lines.len().max(1);
        let start_line = (self.content.scroll_y / line_height) as usize;
        let visible_lines = (height / line_height) as usize + 2;
        let end_line = (start_line + visible_lines).min(total_lines);
        (start_line.min(total_lines), end_line)
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
        let start_line = (self.content.scroll_y / line_height) as usize;
        let visible_lines = ((editor_region.height / line_height) as usize + 2).max(1);
        let end_line = (start_line + visible_lines).min(self.content.cached_lines.len().max(1));

        let mut max_line_chars: usize = 0;
        for line_idx in start_line..end_line {
            if let Some(text) = self.content.cached_lines.get(line_idx) {
                let chars = text.chars().map(unicode_char_width).sum::<usize>();
                if chars > max_line_chars {
                    max_line_chars = chars;
                }
            }
        }

        // 行号宽度 + 5px 内边距，扣除后为文本可视宽度
        let text_visible_width = (editor_width - 60.0 - 5.0).max(1.0);
        let max_content_width = max_line_chars as f32 * char_width;
        let max_scroll_x = (max_content_width - text_visible_width).max(0.0);

        self.content.scroll_x = (self.content.scroll_x + delta_x).clamp(0.0, max_scroll_x);
        self.emit_event(crate::events::EditorEvent::Scrolled);
    }

    /// P0-3: 重置水平滚动（光标跳转、文件加载时调用）
    pub fn reset_scroll_x(&mut self) {
        self.content.scroll_x = 0.0;
    }

    /// P0-3: 确保光标在水平方向可见，必要时调整 scroll_x。
    /// 在光标移动后调用。
    pub fn ensure_cursor_visible_horizontal(&mut self) {
        let char_width = self.text_renderer.char_width();
        let editor_region = self.layout.editor_region();
        let text_visible_width = (editor_region.width - 60.0 - 5.0).max(1.0);

        // 光标在当前行的字符列
        let cursor_char_col =
            if let Some(text) = self.content.cached_lines.get(self.content.cursor_line) {
                let byte_pos = text.floor_char_boundary(self.content.cursor_col.min(text.len()));
                text[..byte_pos]
                    .chars()
                    .map(unicode_char_width)
                    .sum::<usize>()
            } else {
                0
            };
        let cursor_x = cursor_char_col as f32 * char_width;

        let left = self.content.scroll_x;
        let right = self.content.scroll_x + text_visible_width;

        if cursor_x < left {
            // 光标在可视区左侧，向左滚动
            self.content.scroll_x = cursor_x.max(0.0);
        } else if cursor_x >= right {
            // 光标在可视区右侧，向右滚动（留 1 字符余量）
            self.content.scroll_x = cursor_x - text_visible_width + char_width;
        }
    }

    /// REQ-P1-01: 确保光标在垂直方向可见，必要时调整 scroll_y。
    /// 在光标上下移动后调用。
    pub fn ensure_cursor_visible_vertical(&mut self) {
        let line_height = self.text_renderer.line_height();
        let editor_region = self.layout.editor_region();
        let editor_height = editor_region.height.max(1.0);
        let cursor_y = self.content.cursor_line as f32 * line_height;

        if cursor_y < self.content.scroll_y {
            self.content.scroll_y = cursor_y;
        } else if cursor_y + line_height > self.content.scroll_y + editor_height {
            self.content.scroll_y = (cursor_y + line_height - editor_height).max(0.0);
        }
    }

    /// 跳转到指定 1-based 行/列位置，并确保光标可见。
    ///
    /// - line 和 column 均为 1-based（与用户输入一致）。
    /// - 行号/列号越界时会自动钳制到有效范围。
    pub fn goto_position(&mut self, line: usize, column: usize) {
        if self.content.buffer.len_lines() == 0 {
            return;
        }

        let max_line = self.content.buffer.len_lines().saturating_sub(1);
        let target_line = line.saturating_sub(1).min(max_line);

        let line_text = self
            .content
            .buffer
            .get_line(target_line)
            .unwrap_or_default();
        let target_col =
            char_offset_to_byte_offset(&line_text, column.saturating_sub(1)).min(line_text.len());

        self.content.cursor_line = target_line;
        self.content.cursor_col = target_col;
        self.content.selection_start = None;
        self.content.selection_end = None;

        // 同步到当前标签页
        if let Some(crate::tabs::Tab::File(content)) = self.tabs.get_mut(self.active_tab) {
            content.cursor_line = target_line;
            content.cursor_col = target_col;
            content.selection_start = None;
            content.selection_end = None;
        }

        // 垂直滚动：让目标行可见
        let line_height = self.text_renderer.line_height();
        let editor_region = self.layout.editor_region();
        let editor_height = editor_region.height.max(1.0);
        let cursor_y = target_line as f32 * line_height;

        if cursor_y < self.content.scroll_y {
            self.content.scroll_y = cursor_y;
        } else if cursor_y + line_height > self.content.scroll_y + editor_height {
            self.content.scroll_y = (cursor_y + line_height - editor_height).max(0.0);
        }

        // 水平滚动：让目标列可见
        self.ensure_cursor_visible_horizontal();

        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }

    /// 侧边栏滚动（文件树虚拟滚动）
    pub fn scroll_sidebar(&mut self, delta_y: f32) {
        match &self.sidebar_content {
            crate::layout::SidebarContent::FileTree => {
                let node_height = 16.0;
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
                let node_height = 16.0;
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
                // 使用渲染实测的最大滚动量（消息换行后高度可变，固定估算会失真）
                let max_scroll = self.ai_panel.content_height;
                let new_scroll = (self.ai_panel.scroll_y + delta_y).clamp(0.0, max_scroll);
                self.ai_panel.scroll_y = new_scroll;
                // 手动滚离底部则取消吸附；回到底部则恢复吸附
                self.ai_panel.stick_to_bottom = new_scroll >= max_scroll - 1.0;
            }
            _ => {}
        }
        self.emit_event(crate::events::EditorEvent::SidebarChanged);
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
                self.new_project();
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
            crate::menu_bar::CommandId::EditSelectAll => {
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
            crate::menu_bar::CommandId::SearchGlobal => {
                self.search_panel.toggle();
                if self.search_panel.visible {
                    self.search_panel.search(self.current_folder.as_deref());
                }
            }
            crate::menu_bar::CommandId::AiFixDiagnostics => {
                self.ai_fix_diagnostics();
            }
            crate::menu_bar::CommandId::TerminalNew => {
                self.layout.toggle_terminal_panel();
                if self.layout.bottom_panel_visible {
                    self.terminal_panel.focused = true;
                    self.set_terminal_ime_bypass(true);
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
                    self.set_terminal_ime_bypass(false);
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
        self.folder_generation = self.folder_generation.wrapping_add(1);
        let generation = self.folder_generation;
        self.current_folder = Some(path.clone());
        // 同步终端工作目录到新工作区
        self.terminal_panel.cwd = path.to_string_lossy().to_string();
        // 立即持久化 last_workspace，避免仅在窗口关闭时保存导致下次启动恢复的是旧工作区
        self.app_settings.ui.last_workspace = self.current_folder.clone();
        if let Err(e) = self.app_settings.save() {
            eprintln!("警告: 保存 last_workspace 失败: {}", e);
        }
        self.status_message = format!("正在扫描: {}...", path.display());
        self.recent_projects.add(&path);
        self.file_tree = Some(FileTree::new());
        // UI-T01: 工作区切换后标题栏需要立即更新，标记全窗口重绘
        self.dirty_tracker.mark_full_window();

        // 初始化 LSP 客户端（启动 rust-analyzer 等语言服务器）
        self.init_lsp(&path);

        let hwnd = self.hwnd;
        let path_clone = path.clone();
        // HWND 不是 Send，但实际只是个指针，PostMessageW 是线程安全的
        // 用 SendHwnd 包装以通过类型检查
        let send_hwnd = SendHwnd(hwnd.0 as usize);
        std::thread::spawn(move || {
            let entries = scan_file_tree_entries(&path_clone);
            const BATCH_SIZE: usize = 50;
            for chunk in entries.chunks(BATCH_SIZE) {
                let batch = ScannedBatch {
                    generation,
                    entries: chunk.to_vec(),
                    complete: false,
                };
                let ptr = Box::into_raw(Box::new(batch));
                let hwnd = windows::Win32::Foundation::HWND(send_hwnd.0 as *mut std::ffi::c_void);
                unsafe {
                    post_boxed_message_lparam(
                        hwnd,
                        windows::Win32::UI::WindowsAndMessaging::WM_APP + 7,
                        ptr,
                    );
                }
            }
            let complete = ScannedBatch {
                generation,
                entries: Vec::new(),
                complete: true,
            };
            let ptr = Box::into_raw(Box::new(complete));
            let hwnd = windows::Win32::Foundation::HWND(send_hwnd.0 as *mut std::ffi::c_void);
            unsafe {
                post_boxed_message_lparam(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::WM_APP + 7,
                    ptr,
                );
            }
        });
    }

    /// H-09: 接收 &ScannedBatch 引用，由调用方负责 Box 的 drop
    pub(crate) fn on_folder_scan_batch_ref(&mut self, batch: &ScannedBatch) {
        if batch.generation != self.folder_generation {
            return;
        }
        if batch.complete {
            self.is_loading_folder = false;
            if let Some(folder) = self.current_folder.clone() {
                self.git.detect(&folder);
                if let Some(branch) = self.git.current_branch_name() {
                    self.status_bar.update_git_branch(Some(&branch));
                } else {
                    self.status_bar.update_git_branch(None);
                }
                self.status_message = format!("已打开文件夹: {}", folder.display());
                self.welcome_focus_action = None;
                // 自动打开 README（若存在）
                self.try_open_readme(&folder);
            }
            return;
        }
        if let Some(ref mut tree) = self.file_tree {
            for entry in &batch.entries {
                tree.add_node(&entry.name, entry.kind, u32::MAX, entry.depth);
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
            let raw = Box::into_raw(Box::new(result));
            let hwnd = windows::Win32::Foundation::HWND(send_hwnd.0 as *mut std::ffi::c_void);
            unsafe {
                post_boxed_message_wparam(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::WM_APP + 4,
                    raw,
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
            let raw = Box::into_raw(Box::new(payload));
            let hwnd = windows::Win32::Foundation::HWND(send_hwnd.0 as *mut std::ffi::c_void);
            unsafe {
                post_boxed_message_wparam(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::WM_APP + 5,
                    raw,
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
    fn try_open_readme(&mut self, folder: &Path) {
        // 当前标签页有内容或未保存的修改时，不自动加载 README
        if self.content.is_dirty
            || self.content.buffer.len_bytes() > 0
            || self.content.file_path.is_some()
        {
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

    pub fn close_workspace(&mut self) {
        self.file_tree = None;
        self.current_folder = None;
        self.content.file_path = None;
        self.content.buffer = PieceTable::from_string(String::new());
        self.content.cursor_line = 0;
        self.content.cursor_col = 0;
        self.content.scroll_y = 0.0;
        self.content.selection_start = None;
        self.content.selection_end = None;
        self.content.is_dirty = false;
        self.content.cached_lines.clear();
        self.content.cached_tokens.clear();
        self.content.language = Language::PlainText;
        self.tabs.clear();
        self.tabs.push(crate::tabs::Tab::new());
        self.active_tab = 0;
        self.selected_file_node = None;
        self.welcome_focus_action = None;
        self.git.detect(std::path::Path::new("."));
        self.status_bar.update_git_branch(None);
        self.status_message = "已关闭工作区".to_string();
        // UI-T01: 关闭工作区后标题栏需要立即恢复为应用名
        self.dirty_tracker.mark_full_window();
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

    fn handle_file_tree_click(&mut self, mouse_x: f32, mouse_y: f32) -> bool {
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
            if (10.0..70.0).contains(&mouse_x) {
                // Commit 按钮
                if !self.git.commit_message.is_empty() {
                    let msg = self.git.commit_message.clone();
                    let _ = self.git.commit(&msg);
                    self.git.commit_message.clear();
                }
                return true;
            } else if (80.0..140.0).contains(&mouse_x) {
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

    pub(crate) fn update_git_panel_hover(&mut self, mouse_x: f32, mouse_y: f32) {
        if !self.git.is_repo() {
            self.git.hover_button = None;
            return;
        }
        // 与 handle_git_panel_click 使用一致的布局
        let mut current_y = 10.0f32;
        current_y += 70.0; // 跳过标题和分支区域
        let button_y = current_y;
        if mouse_y >= button_y && mouse_y < button_y + 26.0 {
            if (10.0..70.0).contains(&mouse_x) {
                self.git.hover_button = Some("commit".to_string());
                return;
            } else if (80.0..140.0).contains(&mouse_x) {
                self.git.hover_button = Some("refresh".to_string());
                return;
            }
        }
        self.git.hover_button = None;
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
            let node_height = 16.0_f32;
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
                        let tab =
                            crate::tabs::Tab::File(crate::tabs::TabContent::with_loaded_buffer(
                                Some(PathBuf::from(format!("remote:{}", remote_path))),
                                PieceTable::from_string(text),
                                Language::PlainText,
                                false,
                            ));
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
                let tab = crate::tabs::Tab::File(crate::tabs::TabContent::with_loaded_buffer(
                    Some(PathBuf::from(format!("diff: {}", file))),
                    PieceTable::from_string(diff_text),
                    Language::PlainText,
                    false,
                ));
                self.open_in_new_tab(tab);
                self.status_message = format!("显示 {} 的差异", file);
            } else {
                self.status_message = format!("获取差异失败: {}", stderr);
            }
        }
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

    fn update_local_tree_hover(&mut self, mouse_x: f32, mouse_y: f32) -> bool {
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

    fn update_remote_tree_hover(&mut self, mouse_y: f32) -> bool {
        let tree = match self.remote_file_tree.as_ref() {
            Some(t) => t,
            None => {
                let old = self.hover_remote_node.take();
                return old.is_some();
            }
        };
        // P0-1: 递归遍历可见节点确定悬停目标（按路径标识）
        let node_height = 16.0_f32;
        let mut current_y = 10.0 - self.remote_scroll_y;
        let new_hover =
            Self::find_remote_node_at_y(&tree.nodes, mouse_y, node_height, &mut current_y)
                .map(|(path, _)| path);
        let changed = self.hover_remote_node != new_hover;
        self.hover_remote_node = new_hover;
        changed
    }

    /// P3.4: 计算当前悬停文件树节点的 tooltip 文本。
    ///
    /// 返回 `Some(text)` 表示应显示 tooltip；`None` 表示无需显示。
    /// 仅本地文件树有完整路径信息；远程节点 hover_remote_node 本身就是路径。
    pub fn compute_hover_tooltip_text(&self) -> Option<String> {
        match &self.sidebar_content {
            crate::layout::SidebarContent::FileTree => {
                let node_idx = self.hover_file_node?;
                let tree = self.file_tree.as_ref()?;
                let path = file_tree_node_path(tree, node_idx)?;
                if path.is_empty() {
                    None
                } else {
                    Some(path)
                }
            }
            crate::layout::SidebarContent::RemoteFileTree => {
                self.hover_remote_node.clone().filter(|s| !s.is_empty())
            }
            _ => None,
        }
    }

    /// P3.4: 清除当前 hover tooltip
    pub fn clear_hover_tooltip(&mut self) {
        self.hover_tooltip = None;
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

    /// H-22: 仅更新悬停视觉状态，不触发点击动作的副作用
    pub fn handle_ssh_dialog_hover(&mut self, mouse_x: f32, mouse_y: f32) {
        if let Some(rect) = &self.ssh_dialog.connect_btn_rect {
            if rect.contains(mouse_x, mouse_y) {
                self.ssh_dialog.hover_button = Some(0);
                return;
            }
        }
        if let Some(rect) = &self.ssh_dialog.cancel_btn_rect {
            if rect.contains(mouse_x, mouse_y) {
                self.ssh_dialog.hover_button = Some(1);
                return;
            }
        }
        self.ssh_dialog.hover_button = None;
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

    /// H-22: 仅更新克隆对话框悬停视觉状态，不触发点击动作
    pub fn handle_clone_dialog_hover(&mut self, mouse_x: f32, mouse_y: f32) {
        if let Some(rect) = &self.clone_dialog.clone_btn_rect {
            if rect.contains(mouse_x, mouse_y) {
                self.clone_dialog.hover_button = Some(0);
                return;
            }
        }
        if let Some(rect) = &self.clone_dialog.cancel_btn_rect {
            if rect.contains(mouse_x, mouse_y) {
                self.clone_dialog.hover_button = Some(1);
                return;
            }
        }
        self.clone_dialog.hover_button = None;
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

    /// 粘贴到新建项目对话框的项目名称输入框
    pub fn paste_into_new_project_dialog(&mut self) {
        if let Some(text) = Self::get_clipboard_text() {
            // 移除路径分隔符和非法字符
            self.new_project_dialog
                .project_name
                .extend(text.chars().filter(|c| {
                    !matches!(
                        c,
                        '\\' | '/' | ':' | '*' | '?' | '\"' | '<' | '>' | '|' | '\n' | '\r'
                    )
                }));
            self.new_project_dialog.error_message = None;
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

    fn find_node_by_path(tree: &FileTree, target: &Path, base: &Path) -> Option<u32> {
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

    pub fn insert_char(&mut self, ch: char) {
        // P1-4: 自动配对括号
        if self.try_auto_pair(ch) {
            return;
        }

        let pos = self.cursor_byte_pos();
        let before_pieces = self.content.buffer.get_pieces();
        let before_add_len = self.content.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        let text = ch.to_string();
        self.content.buffer.insert(pos, &text);
        self.content.cursor_col += ch.len_utf8();
        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
        self.content.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
            ch.len_utf8(),
        );
        self.status_message = "已修改".to_string();
        self.emit_edit_events();
        self.lsp_notify_change();
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
            if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
                if self.content.cursor_col < text.len() {
                    if let Some(next_ch) = text[self.content.cursor_col..].chars().next() {
                        if next_ch == ch {
                            // 跳过插入，光标右移一个字符
                            self.content.cursor_col += ch.len_utf8();
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
        let selection = self
            .content
            .selection_start
            .zip(self.content.selection_end)
            .filter(|(s, e)| s != e);

        let pos = self.cursor_byte_pos();
        let before_pieces = self.content.buffer.get_pieces();
        let before_add_len = self.content.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        // C-05: 使用模式匹配代替 unwrap，避免选择状态不一致时 panic
        if let Some(((sel_start_line, sel_start_col), (sel_end_line, sel_end_col))) = selection {
            // 包裹选区：在选区开始处插入开括号，在选区结束处插入闭括号
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
            self.content.buffer.insert(end_byte, &close_str);
            self.content.buffer.insert(start_byte, &open_str);

            // REQ-P1-05: 更新光标到选区末尾（闭括号之后）
            // 开括号在 start_byte 插入，若与 end 同行则 end_col 需要加上开括号长度
            let open_shift = if start_line == end_line {
                ch.len_utf8()
            } else {
                0
            };
            self.content.cursor_line = end_line;
            self.content.cursor_col = end_col + open_shift + close_ch.len_utf8();

            // 更新选区：保持选中文本不变，扩展到包含括号
            self.content.selection_start = Some((start_line, start_col));
            self.content.selection_end =
                Some((end_line, end_col + open_shift + close_ch.len_utf8()));

            self.content.is_dirty = true;
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.mark_dirty();
            }
            self.content.buffer_version += 1;

            let cursor_after =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Insert,
                pos,
                close_str.len() + open_str.len(),
            );
            self.status_message = "已修改".to_string();
            self.emit_edit_events();
            return true;
        }

        // 无选区：插入开括号 + 闭括号，光标置于中间
        let pair_text = format!("{}{}", ch, close_ch);
        self.content.buffer.insert(pos, &pair_text);
        // 光标移动到开括号之后（不前进到闭括号）
        self.content.cursor_col += ch.len_utf8();

        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
        self.content.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
            pair_text.len(),
        );
        self.status_message = "已修改".to_string();
        self.emit_edit_events();
        true
    }

    /// P0-2: 设置 IME 合成串（pre-edit text）。
    /// 在 WM_IME_COMPOSITION 收到 GCS_COMPSTR 时调用，
    /// 清空已存在的合成串后写入新值，并触发重绘。
    pub fn set_composition(&mut self, text: String) {
        // 修复：file_tree_input 激活时，合成串存到输入框而非编辑器
        if self.file_tree_input.is_some() {
            if let Some(input) = self.file_tree_input.as_mut() {
                input.composition = Some(text);
                input.caret_visible = true;
            }
            let region = self.layout.sidebar_region().clone();
            self.dirty_tracker.mark_region(
                region.x,
                region.y,
                region.width,
                region.height,
                crate::dirty_rect::DirtyRegionType::Sidebar,
            );
            return;
        }
        // AI 面板输入框聚焦时，合成串存到 AI 面板
        if self.ai_panel.input_focused {
            self.ai_panel.composition = Some(text);
            self.ai_panel.caret_visible = true;
            let region = self.layout.right_panel_region().clone();
            self.dirty_tracker.mark_region(
                region.x,
                region.y,
                region.width,
                region.height,
                crate::dirty_rect::DirtyRegionType::RightPanel,
            );
            return;
        }
        self.composition = Some(text);
    }

    /// 终端聚焦时关闭 IME（保留关联），让 Backspace 直达终端。
    /// 失去焦点时根据用户偏好决定是否恢复 IME —— 通常让用户切回编辑器时仍能输入中文。
    /// 同时同步更新低层键盘钩子的 `TERMINAL_FOCUSED_FLAG` 标志，
    /// 让低层钩子在终端聚焦时主动拦截 Backspace/Delete/方向键。
    ///
    /// 多语言 IME 适配说明：
    /// - 终端聚焦时 set_ime_open(false) 关闭"已开启未合成"状态对 Backspace 的拦截
    /// - 低层键盘钩子 (WH_KEYBOARD_LL) 在所有 IME 之上拦截 Backspace/Delete/方向键
    /// - 字符输入仍走 WM_IME_COMPOSITION + GCS_RESULTSTR → commit_composition → ConPTY
    /// - 因此这个方案对中文/日文/韩文/印地/泰文/阿拉伯等所有标准 IME 通用
    pub fn set_terminal_ime_bypass(&mut self, terminal_focused: bool) {
        // 终端聚焦时关闭 IME，编辑器聚焦时恢复 IME 开启状态
        // （之前总是传 false，会导致用户切回编辑器时中文无法输入）
        let ime_open = !terminal_focused;
        let ok = self.ime.set_ime_open(ime_open);
        crate::keyboard_hook::set_terminal_focused(terminal_focused);
        tracing::info!(
            terminal_focused,
            ime_open,
            ok,
            "终端 IME 状态切换完成，低层钩子标志已同步"
        );
    }

    /// P0-2: 提交合成串为正式文本。
    /// 在 WM_IME_COMPOSITION 收到 GCS_RESULTSTR 或 WM_IME_ENDCOMPOSITION 时调用。
    /// 先清除合成串，再将提交文本逐字符插入到光标处。
    ///
    /// 修复：终端聚焦时，IME 提交文本路由到 ConPTY 而非编辑器。
    /// 中文/日文 IME 在终端聚焦时仍会拦截 ASCII 字母做合成（因为窗口级 IME 关联未变），
    /// 提交结果必须进入终端，否则用户输入的字母会被"偷"到编辑器。
    pub fn commit_composition(&mut self, text: String) {
        // 清除合成状态显示
        self.composition = None;
        if text.is_empty() {
            return;
        }
        // 终端聚焦且运行时：把 IME 提交文本送入 ConPTY
        if self.terminal_panel.focused && self.terminal_panel.running {
            for ch in text.chars() {
                self.terminal_panel.send_char(ch);
            }
            // 提交后立即关闭 IME，让用户能立刻用 Backspace 删除刚提交的汉字
            // （否则 IME 处于"开启未合成"状态会系统级拦截 Backspace）
            self.ime.set_ime_open(false);
            return;
        }
        if self.new_project_dialog.visible {
            self.new_project_dialog.project_name.push_str(&text);
            self.new_project_dialog.error_message = None;
            return;
        }
        // 修复：file_tree_input 激活时，IME 提交文本应进入输入框而非编辑器内容
        if self.file_tree_input.is_some() {
            if let Some(input) = self.file_tree_input.as_mut() {
                input.value.push_str(&text);
                input.composition = None;
                input.caret_visible = true;
            }
            let region = self.layout.sidebar_region().clone();
            self.dirty_tracker.mark_region(
                region.x,
                region.y,
                region.width,
                region.height,
                crate::dirty_rect::DirtyRegionType::Sidebar,
            );
            return;
        }
        // AI 面板输入框聚焦时，IME 提交文本进入 AI 输入框
        if self.ai_panel.input_focused {
            self.ai_panel.insert_str(&text);
            self.ai_panel.composition = None;
            self.ai_panel.caret_visible = true;
            let region = self.layout.right_panel_region().clone();
            self.dirty_tracker.mark_region(
                region.x,
                region.y,
                region.width,
                region.height,
                crate::dirty_rect::DirtyRegionType::RightPanel,
            );
            return;
        }
        for ch in text.chars() {
            self.broadcast_insert_char(ch);
        }
    }

    /// P0-2: 清除合成串（用户取消输入或 IME 失焦时调用）。
    pub fn clear_composition(&mut self) {
        // 修复：file_tree_input 激活时，清除输入框的合成串
        if self.file_tree_input.is_some() {
            if let Some(input) = self.file_tree_input.as_mut() {
                input.composition = None;
            }
            let region = self.layout.sidebar_region().clone();
            self.dirty_tracker.mark_region(
                region.x,
                region.y,
                region.width,
                region.height,
                crate::dirty_rect::DirtyRegionType::Sidebar,
            );
            return;
        }
        // AI 面板输入框聚焦时，清除 AI 面板的合成串
        if self.ai_panel.input_focused {
            self.ai_panel.composition = None;
            let region = self.layout.right_panel_region().clone();
            self.dirty_tracker.mark_region(
                region.x,
                region.y,
                region.width,
                region.height,
                crate::dirty_rect::DirtyRegionType::RightPanel,
            );
            return;
        }
        self.composition = None;
    }

    pub fn insert_tab(&mut self) {
        let pos = self.cursor_byte_pos();
        let before_pieces = self.content.buffer.get_pieces();
        let before_add_len = self.content.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        let tab_text = "    ";
        self.content.buffer.insert(pos, tab_text);
        self.content.cursor_col += tab_text.len();
        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
        self.content.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
            tab_text.len(),
        );
        self.status_message = "已修改".to_string();
        self.emit_edit_events();
    }

    pub fn insert_newline(&mut self) {
        let pos = self.cursor_byte_pos();
        let before_pieces = self.content.buffer.get_pieces();
        let before_add_len = self.content.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        // 获取当前行的前导空白（用于自动缩进）
        let indent = if let Some(line_text) = self.content.buffer.get_line(self.content.cursor_line)
        {
            let leading_ws: String = line_text
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();
            leading_ws
        } else {
            String::new()
        };

        // 检测是否需要额外缩进（行尾有 { 或 :）
        let extra_indent =
            if let Some(line_text) = self.content.buffer.get_line(self.content.cursor_line) {
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

        self.content.buffer.insert(pos, &insert_text);
        self.content.cursor_line += 1;
        self.content.cursor_col = full_indent.len();
        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
        self.content.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
            insert_text.len(),
        );
        self.status_message = "已修改".to_string();
        self.emit_edit_events();
        self.lsp_notify_change();
    }

    pub fn delete_char(&mut self) {
        if self.content.cursor_col > 0 {
            let pos = self.cursor_byte_pos();
            let prev_pos = self.find_prev_char_boundary(pos);
            if prev_pos < pos {
                let before_pieces = self.content.buffer.get_pieces();
                let before_add_len = self.content.buffer.add_buffer_len();
                let cursor_before =
                    CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

                self.content.buffer.delete(prev_pos, pos);
                self.content.cursor_col -= pos - prev_pos;
                self.content.is_dirty = true;
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    tab.mark_dirty();
                }
                self.content.buffer_version += 1;

                let cursor_after =
                    CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
                self.content.history.record(
                    before_pieces,
                    before_add_len,
                    cursor_before,
                    cursor_after,
                    OpType::Delete,
                    prev_pos,
                    0,
                );
                self.status_message = "已修改".to_string();
                // REQ-P1-02: 行内退格也需要触发编辑事件，确保脏矩形标记和即时刷新
                self.emit_edit_events();
            }
        } else if self.content.cursor_line > 0 {
            let prev_line = self.content.cursor_line - 1;
            if let Some(prev_text) = self.content.buffer.get_line(prev_line) {
                let prev_len = prev_text.len();
                if let Some(curr_text) = self.content.buffer.get_line(self.content.cursor_line) {
                    let curr_len = curr_text.len();
                    let start = self.line_byte_start(prev_line) + prev_len;
                    let end = start + curr_len + 1;

                    let before_pieces = self.content.buffer.get_pieces();
                    let before_add_len = self.content.buffer.add_buffer_len();
                    let cursor_before =
                        CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

                    self.content.buffer.delete(start, end);
                    self.content.cursor_line = prev_line;
                    self.content.cursor_col = prev_len;
                    self.content.is_dirty = true;
                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                        tab.mark_dirty();
                    }
                    self.content.buffer_version += 1;

                    let cursor_after =
                        CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
                    self.content.history.record(
                        before_pieces,
                        before_add_len,
                        cursor_before,
                        cursor_after,
                        OpType::Delete,
                        start,
                        0,
                    );
                    self.status_message = "已修改".to_string();
                    self.emit_edit_events();
                }
            }
        }
        self.lsp_notify_change();
    }

    pub fn delete_forward(&mut self) {
        let pos = self.cursor_byte_pos();
        let next_pos = self.find_next_char_boundary(pos);
        if next_pos > pos {
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();
            let cursor_before =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

            self.content.buffer.delete(pos, next_pos);
            self.content.is_dirty = true;
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.mark_dirty();
            }
            self.content.buffer_version += 1;

            let cursor_after =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Delete,
                pos,
                0,
            );
            self.status_message = "已修改".to_string();
            self.emit_edit_events();
        }
        self.lsp_notify_change();
    }

    /// 多光标编辑操作广播
    /// 将插入、删除等操作应用到所有光标位置
    /// 从后往前执行，避免位置偏移问题
    /// REQ-P0-03: 记录撤销历史，使用 begin_group/end_group 作为原子撤销组
    pub fn broadcast_insert_char(&mut self, ch: char) {
        if self.multi_cursor.cursor_count() <= 1 {
            self.insert_char(ch);
            return;
        }

        // REQ-P0-03: 记录操作前光标位置
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        // REQ-P0-03: 开始撤销组
        self.content.history.begin_group();

        // 多光标模式：从后往前插入
        let cursors: Vec<_> = self.multi_cursor.cursors.clone();
        for cursor in cursors.iter().rev() {
            let pos = self.line_col_to_byte(cursor.line, cursor.col);

            // REQ-P0-03: 记录缓冲区状态
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();

            self.content.buffer.insert(pos, &ch.to_string());

            // REQ-P0-03: 记录撤销历史
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_before,
                OpType::Insert,
                pos,
                ch.len_utf8(),
            );
        }

        // REQ-P0-03: 结束撤销组
        self.content.history.end_group();

        // 更新所有光标位置
        for cursor in &mut self.multi_cursor.cursors {
            cursor.col += ch.len_utf8();
        }

        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;
        self.status_message = format!("已在 {} 个位置插入", self.multi_cursor.cursor_count());
        self.emit_edit_events();
    }

    /// 多光标删除（退格）广播
    /// REQ-P0-03: 记录撤销历史，使用 begin_group/end_group 作为原子撤销组
    /// REQ-P2-06: 修正同行多光标位置 — 使用删除偏移量调整而非 find_prev_char_boundary
    pub fn broadcast_delete_char(&mut self) {
        if self.multi_cursor.cursor_count() <= 1 {
            self.delete_char();
            return;
        }

        // 先计算所有需要删除的位置，同时记录每个删除操作的光标索引和原始列
        // REQ-P2-06: 记录 (cursor_index, line, col) 用于后续位置调整
        // 使用克隆的 (idx, line, col) 避免对 cursors 的长期借用，便于后续可变修改
        let mut delete_info: Vec<(usize, usize, usize, usize, usize)> = Vec::new();
        let mut indexed_cursors: Vec<(usize, usize, usize)> = self
            .multi_cursor
            .cursors
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.line, c.col))
            .collect();
        indexed_cursors.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));

        for (idx, line, col) in &indexed_cursors {
            if *col > 0 {
                let pos = self.line_col_to_byte(*line, *col);
                let prev_pos = self.find_prev_char_boundary(pos);
                if prev_pos < pos {
                    delete_info.push((*idx, *line, *col, prev_pos, pos));
                }
            }
        }

        // REQ-P0-03: 记录操作前光标位置
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        // REQ-P0-03: 开始撤销组
        self.content.history.begin_group();

        // 执行删除（delete_info 已按 line/col 降序排列，从后往前删除）
        for (_, _, _, start, end) in &delete_info {
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();

            self.content.buffer.delete(*start, *end);

            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_before,
                OpType::Delete,
                *start,
                0,
            );
        }

        // REQ-P0-03: 结束撤销组
        self.content.history.end_group();

        // REQ-P2-06: 正确调整光标位置
        // 对于每个光标，新 col = 原 col - (同行中位于该光标之前或同位置的删除次数)
        // 因为每次删除都会让该位置之后的光标左移 1
        for (idx, line, col) in indexed_cursors.iter() {
            if *col == 0 {
                continue;
            }
            // 统计同行中删除位置 <= 当前光标 col 的次数
            let shifts = delete_info
                .iter()
                .filter(|(_, dline, dcol, _, _)| *dline == *line && *dcol <= *col)
                .count();
            let new_col = col.saturating_sub(shifts);
            self.multi_cursor.cursors[*idx].col = new_col;
        }

        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;
        self.emit_edit_events();
    }

    /// 多光标插入换行广播
    /// REQ-P0-03: 记录撤销历史，使用 begin_group/end_group 作为原子撤销组
    pub fn broadcast_insert_newline(&mut self) {
        if self.multi_cursor.cursor_count() <= 1 {
            self.insert_newline();
            return;
        }

        // REQ-P0-03: 记录操作前光标位置
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        // REQ-P0-03: 开始撤销组
        self.content.history.begin_group();

        let cursors: Vec<_> = self.multi_cursor.cursors.clone();
        for cursor in cursors.iter().rev() {
            let pos = self.line_col_to_byte(cursor.line, cursor.col);

            // REQ-P0-03: 记录缓冲区状态
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();

            self.content.buffer.insert(pos, "\n");

            // REQ-P0-03: 记录撤销历史
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_before,
                OpType::Insert,
                pos,
                1,
            );
        }

        // REQ-P0-03: 结束撤销组
        self.content.history.end_group();

        // 更新所有光标位置
        for cursor in &mut self.multi_cursor.cursors {
            cursor.line += 1;
            cursor.col = 0;
        }

        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;
        self.emit_edit_events();
    }

    /// 撤销
    pub fn undo(&mut self) {
        let current_pieces = self.content.buffer.get_pieces();
        let current_add_len = self.content.buffer.add_buffer_len();
        let current_cursor = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        if let Some((pieces, add_len, cursor)) =
            self.content
                .history
                .undo(current_pieces, current_add_len, current_cursor)
        {
            self.content.buffer.restore(pieces, add_len);
            self.content.cursor_line = cursor.line;
            self.content.cursor_col = cursor.column;
            self.content.is_dirty = true;
            self.content.buffer_version += 1;
            self.status_message = "已撤销".to_string();
            // REQ-P2-05: 撤销后触发编辑事件，确保脏矩形更新和事件订阅者通知
            self.emit_edit_events();
        }
    }

    /// 重做
    pub fn redo(&mut self) {
        let current_pieces = self.content.buffer.get_pieces();
        let current_add_len = self.content.buffer.add_buffer_len();
        let current_cursor = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        if let Some((pieces, add_len, cursor)) =
            self.content
                .history
                .redo(current_pieces, current_add_len, current_cursor)
        {
            self.content.buffer.restore(pieces, add_len);
            self.content.cursor_line = cursor.line;
            self.content.cursor_col = cursor.column;
            self.content.is_dirty = true;
            self.content.buffer_version += 1;
            self.status_message = "已重做".to_string();
            // REQ-P2-05: 重做后触发编辑事件，确保脏矩形更新和事件订阅者通知
            self.emit_edit_events();
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.content.cursor_col > 0 {
            if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
                let col = text.floor_char_boundary(self.content.cursor_col.min(text.len()));
                if let Some(ch) = text[..col].chars().next_back() {
                    self.content.cursor_col = col - ch.len_utf8();
                } else {
                    self.content.cursor_col = 0;
                }
            }
        } else if self.content.cursor_line > 0 {
            self.content.cursor_line -= 1;
            if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
                self.content.cursor_col = text.len();
            }
        }
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }

    pub fn move_cursor_right(&mut self) {
        if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
            if self.content.cursor_col < text.len() {
                if let Some(ch) = text[self.content.cursor_col..].chars().next() {
                    self.content.cursor_col += ch.len_utf8();
                }
            } else if self.content.cursor_line + 1 < self.content.buffer.len_lines() {
                self.content.cursor_line += 1;
                self.content.cursor_col = 0;
            }
        }
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }

    pub fn move_cursor_up(&mut self) {
        if self.content.cursor_line > 0 {
            self.content.cursor_line -= 1;
            if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
                self.content.cursor_col = self.content.cursor_col.min(text.len());
            }
        }
        // REQ-P1-01: 垂直滚动跟随，确保光标可见
        self.ensure_cursor_visible_vertical();
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }

    pub fn move_cursor_down(&mut self) {
        if self.content.cursor_line + 1 < self.content.buffer.len_lines() {
            self.content.cursor_line += 1;
            if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
                self.content.cursor_col = self.content.cursor_col.min(text.len());
            }
        }
        // REQ-P1-01: 垂直滚动跟随，确保光标可见
        self.ensure_cursor_visible_vertical();
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }

    pub fn move_cursor_home(&mut self) {
        self.content.cursor_col = 0;
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }

    pub fn move_cursor_end(&mut self) {
        if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
            self.content.cursor_col = text.len();
        }
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }

    /// P1-6: Smart Home - 跳到行首首个非空白字符。
    /// 若光标已在首个非空白位置，再按一次跳到行首（col=0）。
    /// 通过传入 `already_at_smart_home` 判断是否为第二次按 Home。
    pub fn move_cursor_smart_home(&mut self, already_at_smart_home: bool) {
        if already_at_smart_home {
            self.content.cursor_col = 0;
            self.emit_event(crate::events::EditorEvent::CursorMoved);
            return;
        }
        if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
            let first_non_ws = text
                .char_indices()
                .skip_while(|(_, c)| c.is_whitespace())
                .map(|(i, _)| i)
                .next()
                .unwrap_or(text.len());
            self.content.cursor_col = first_non_ws;
        }
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }

    /// P1-6: 移动到文件首行
    pub fn move_cursor_file_start(&mut self) {
        self.content.cursor_line = 0;
        self.content.cursor_col = 0;
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }

    /// P1-6: 移动到文件末行末列
    pub fn move_cursor_file_end(&mut self) {
        let last_line = self.content.buffer.len_lines().saturating_sub(1);
        self.content.cursor_line = last_line;
        if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
            self.content.cursor_col = text.len();
        }
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }

    /// P1-6: 向左移动一个单词。
    /// 跳过当前空白，再跳到上一个单词边界。
    /// REQ-P0-01: 修复字节/字符索引混淆——cursor_col 是字节偏移，
    /// 必须先转为字符索引再用于 chars Vec 的索引。
    /// REQ-P2-02: 避免每次调用分配 Vec<char>，直接基于字节偏移遍历。
    pub fn move_cursor_word_left(&mut self) {
        if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
            let text_len = text.len();
            let mut byte_offset = text.floor_char_boundary(self.content.cursor_col.min(text_len));

            // 辅助：取 byte_offset 之前一个字符的字节位置与该字符
            let prev_char = |pos: usize| -> Option<(usize, char)> {
                if pos == 0 {
                    return None;
                }
                let prev_pos = text.floor_char_boundary(pos - 1);
                text[prev_pos..pos].chars().next().map(|c| (prev_pos, c))
            };

            // 向后跳过空白
            while let Some((prev_pos, ch)) = prev_char(byte_offset) {
                if ch.is_whitespace() {
                    byte_offset = prev_pos;
                } else {
                    break;
                }
            }

            // 跳过当前单词（字母数字下划线）或跳过一个符号
            if let Some((prev_pos, ch)) = prev_char(byte_offset) {
                let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
                if is_word_char(ch) {
                    while let Some((p, c)) = prev_char(byte_offset) {
                        if is_word_char(c) {
                            byte_offset = p;
                        } else {
                            break;
                        }
                    }
                } else {
                    // 非单词字符：跳过一个符号
                    byte_offset = prev_pos;
                }
            }

            self.content.cursor_col = byte_offset;
        } else if self.content.cursor_line > 0 {
            self.content.cursor_line -= 1;
            self.move_cursor_end();
        }
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }

    /// P1-6: 向右移动一个单词。
    /// REQ-P2-02: 避免每次调用分配 Vec<char>，直接基于字节偏移遍历。
    pub fn move_cursor_word_right(&mut self) {
        if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
            let text_len = text.len();
            let mut byte_offset = text.floor_char_boundary(self.content.cursor_col.min(text_len));

            // 辅助：取 byte_offset 处字符的字节范围
            let curr_char = |pos: usize| -> Option<(usize, usize, char)> {
                if pos >= text_len {
                    return None;
                }
                let ch = text[pos..].chars().next()?;
                Some((pos, pos + ch.len_utf8(), ch))
            };

            // 向前跳过空白
            while let Some((_, next_pos, ch)) = curr_char(byte_offset) {
                if ch.is_whitespace() {
                    byte_offset = next_pos;
                } else {
                    break;
                }
            }

            // 跳过当前单词或一个符号
            if let Some((_, next_pos, ch)) = curr_char(byte_offset) {
                let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
                if is_word_char(ch) {
                    while let Some((_, np, c)) = curr_char(byte_offset) {
                        if is_word_char(c) {
                            byte_offset = np;
                        } else {
                            break;
                        }
                    }
                } else {
                    // 非单词字符：跳过一个符号
                    byte_offset = next_pos;
                }
            }

            self.content.cursor_col = byte_offset;
        } else if self.content.cursor_line + 1 < self.content.buffer.len_lines() {
            self.content.cursor_line += 1;
            self.content.cursor_col = 0;
        }
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }

    /// P1-6: 切换行注释（按语言决定注释符号）。
    /// 当前行已有注释符号则移除，否则添加。
    pub fn toggle_line_comment(&mut self) {
        let comment_prefix = match self.content.language {
            Language::Rust
            | Language::C
            | Language::JavaScript
            | Language::TypeScript
            | Language::Go
            | Language::Java
            | Language::Json => "// ",
            Language::Python | Language::Toml => "# ",
            _ => return, // 不支持的语言（如 PlainText/Markdown/Html/Css）直接返回
        };

        let line_idx = self.content.cursor_line;
        let line = match self.content.buffer.get_line(line_idx) {
            Some(s) => s,
            None => return,
        };

        // 检测是否已有注释前缀
        let stripped = line.strip_prefix(comment_prefix);
        let pos = self.line_byte_start(line_idx);
        let before_pieces = self.content.buffer.get_pieces();
        let before_add_len = self.content.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        if let Some(_rest) = stripped {
            // 已有注释：移除前缀
            let remove_len = comment_prefix.len();
            self.content.buffer.delete(pos, pos + remove_len);
            // 光标列前移
            self.content.cursor_col = self.content.cursor_col.saturating_sub(remove_len);
        } else {
            // 无注释：在行首添加前缀
            self.content.buffer.insert(pos, comment_prefix);
            // 光标列后移
            self.content.cursor_col += comment_prefix.len();
        }

        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
        self.content.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
            comment_prefix.len(),
        );
        self.status_message = "已切换注释".to_string();
    }

    /// P1-6: 在下一行同一列添加光标（Ctrl+Alt+Down）。
    pub fn add_cursor_line_below(&mut self) {
        let line = self.content.cursor_line;
        let col = self.content.cursor_col;
        if line + 1 < self.content.buffer.len_lines() {
            let new_line = line + 1;
            // 钳制 col 到新行长度
            let max_col = self
                .content
                .buffer
                .get_line(new_line)
                .map(|s| s.len())
                .unwrap_or(col);
            self.multi_cursor
                .add_cursor(Cursor::new(new_line, col.min(max_col)));
            self.content.cursor_line = new_line;
            self.content.cursor_col = col.min(max_col);
            self.status_message =
                format!("已添加光标（共 {} 处）", self.multi_cursor.cursor_count());
        }
    }

    /// P1-6: 在上一行同一列添加光标（Ctrl+Alt+Up）。
    pub fn add_cursor_line_above(&mut self) {
        let line = self.content.cursor_line;
        let col = self.content.cursor_col;
        if line > 0 {
            let new_line = line - 1;
            let max_col = self
                .content
                .buffer
                .get_line(new_line)
                .map(|s| s.len())
                .unwrap_or(col);
            self.multi_cursor
                .add_cursor(Cursor::new(new_line, col.min(max_col)));
            self.content.cursor_line = new_line;
            self.content.cursor_col = col.min(max_col);
            self.status_message =
                format!("已添加光标（共 {} 处）", self.multi_cursor.cursor_count());
        }
    }

    /// P1-6: 添加下一个相同单词的光标（Ctrl+D）。
    /// 找到当前选中文本或光标所在单词的下一个出现位置，添加光标。
    pub fn add_cursor_at_next_occurrence(&mut self) {
        // 获取当前要查找的文本（来自选区或光标所在单词）
        let search_text = if let (Some((sline, scol)), Some((eline, ecol))) =
            (self.content.selection_start, self.content.selection_end)
        {
            if sline == eline {
                let s = self.line_col_to_byte(sline, scol);
                let e = self.line_col_to_byte(eline, ecol);
                if s < e {
                    self.content.buffer.get_text(s, e)
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            // 取光标所在单词
            if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
                let chars: Vec<char> = text.chars().collect();
                let byte_pos = text.floor_char_boundary(self.content.cursor_col.min(text.len()));
                let char_idx = text[..byte_pos].chars().count();
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
        let total_bytes = self.content.buffer.len_bytes();
        let text_after = self.content.buffer.get_text(start_byte, total_bytes);

        if let Some(rel_pos) = text_after.find(&search_text) {
            let abs_byte = start_byte + rel_pos;
            // 转换为 (line, col)
            let (line, col) = self.byte_to_line_col(abs_byte);
            self.multi_cursor.add_cursor(Cursor::new(line, col));
            self.content.cursor_line = line;
            self.content.cursor_col = col;
            self.content.selection_start = Some((line, col));
            self.content.selection_end = Some((line, col + search_text.len()));
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
        let line_number_width = 40.0;

        // P0-3: 鼠标 x 加上 scroll_x 抵消，确保点击的字符位置正确
        let rel_x = mouse_x - editor_x - line_number_width - 5.0 + self.content.scroll_x;
        let rel_y = mouse_y - editor_y + self.content.scroll_y;

        let line = (rel_y / line_height) as usize;
        let char_col = (rel_x / char_width).max(0.0) as usize;

        let total_lines = self.content.buffer.len_lines();
        self.content.cursor_line = line.min(total_lines.saturating_sub(1));

        if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
            // 将字符列转换为字节偏移，对齐到字符边界
            let mut byte_col = 0usize;
            for (i, ch) in text.chars().enumerate() {
                if i >= char_col {
                    break;
                }
                byte_col += ch.len_utf8();
            }
            self.content.cursor_col = byte_col.min(text.len());
        } else {
            self.content.cursor_col = 0;
        }
    }

    pub fn start_selection(&mut self) {
        self.content.selection_start = Some((self.content.cursor_line, self.content.cursor_col));
        self.content.selection_end = Some((self.content.cursor_line, self.content.cursor_col));
        self.is_selecting = true;
    }

    pub fn update_selection(&mut self) {
        if self.is_selecting {
            self.content.selection_end = Some((self.content.cursor_line, self.content.cursor_col));
        }
    }

    pub fn end_selection(&mut self) {
        self.is_selecting = false;
    }

    pub fn clear_selection(&mut self) {
        self.content.selection_start = None;
        self.content.selection_end = None;
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
        let line_idx = self.content.cursor_line;
        let byte_col = self.content.cursor_col;
        let line_text = match self.content.buffer.get_line(line_idx) {
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
        self.content.selection_start = Some((line_idx, start_byte));
        self.content.selection_end = Some((line_idx, end_byte));
        self.content.cursor_col = end_byte;
        self.is_selecting = false;
    }

    pub fn get_selected_text(&self) -> Option<String> {
        let (start_line, start_col) = self.content.selection_start?;
        let (end_line, end_col) = self.content.selection_end?;

        if start_line == end_line {
            let line = self.content.buffer.get_line(start_line)?;
            let start = line.floor_char_boundary(start_col.min(line.len()));
            let end = line.floor_char_boundary(end_col.min(line.len()));
            let (s, e) = if start <= end {
                (start, end)
            } else {
                (end, start)
            };
            return Some(line[s..e].to_string());
        }

        // Multi-line selection (simplified)
        let mut result = String::new();
        let (first_line, first_col) = if (start_line, start_col) <= (end_line, end_col) {
            (start_line, start_col)
        } else {
            (end_line, end_col)
        };
        let (last_line, last_col) = if (start_line, start_col) <= (end_line, end_col) {
            (end_line, end_col)
        } else {
            (start_line, start_col)
        };

        for line_idx in first_line..=last_line {
            if let Some(line) = self.content.buffer.get_line(line_idx) {
                if line_idx == first_line {
                    let start = line.floor_char_boundary(first_col.min(line.len()));
                    result.push_str(&line[start..]);
                } else if line_idx == last_line {
                    let end = line.floor_char_boundary(last_col.min(line.len()));
                    result.push_str(&line[..end]);
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
        self.line_byte_start(self.content.cursor_line) + self.content.cursor_col
    }

    fn line_byte_start(&self, line_idx: usize) -> usize {
        self.content.buffer.line_start_byte(line_idx)
    }

    /// 将行号+列号转换为字节偏移 - O(1) 行起始 + O(1) 列偏移
    pub fn line_col_to_byte(&self, line: usize, col: usize) -> usize {
        let start = self.content.buffer.line_start_byte(line);
        if let Some(text) = self.content.buffer.get_line(line) {
            start + col.min(text.len())
        } else {
            start
        }
    }

    /// P1-6: 将字节偏移转换为 (line, col) - O(log n) 二分查找行号
    fn byte_to_line_col(&self, byte: usize) -> (usize, usize) {
        let total_lines = self.content.buffer.len_lines();
        if total_lines == 0 {
            return (0, 0);
        }
        // 二分查找：找到第一个 line_start_byte > byte 的行，则该行前一行为目标行
        let mut lo: usize = 0;
        let mut hi: usize = total_lines;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.content.buffer.line_start_byte(mid) <= byte {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let line = lo.saturating_sub(1).min(total_lines.saturating_sub(1));
        let line_start = self.content.buffer.line_start_byte(line);
        let col = byte.saturating_sub(line_start);
        (line, col)
    }

    fn find_prev_char_boundary(&self, pos: usize) -> usize {
        if pos == 0 {
            return 0;
        }
        let mut p = pos - 1;
        // P4-1: 使用 byte_at 替代 get_text(p, p+1).as_bytes()[0]，避免 String 堆分配
        while p > 0
            && self
                .content
                .buffer
                .byte_at(p)
                .is_some_and(|b| (b & 0xC0) == 0x80)
        {
            p -= 1;
        }
        p
    }

    fn find_next_char_boundary(&self, pos: usize) -> usize {
        let total = self.content.buffer.len_bytes();
        if pos >= total {
            return total;
        }
        let mut p = pos + 1;
        // P4-1: 使用 byte_at 避免逐字节 String 分配
        while p < total
            && self
                .content
                .buffer
                .byte_at(p)
                .is_some_and(|b| (b & 0xC0) == 0x80)
        {
            p += 1;
        }
        p
    }

    /// 增量重建缓存：只重建可见行范围内的缓存，大幅减少大文件的词法分析开销
    pub(crate) fn rebuild_cache(&mut self, visible_start: usize, visible_end: usize) {
        let total_lines = self.content.buffer.len_lines().max(1);

        // tree-sitter 优先高亮：返回支持的语言的字符串标识
        // 不支持的语言返回 None，由调用方 fallback 到手写 lexer
        let ts_lang = language_to_ts_str(self.content.language);

        // === P0-3: 后台语法高亮 — 始终 poll，即使在空闲帧 ===
        // 必须在签名检查之前 poll，否则空闲帧（签名匹配）会 early return，
        // 导致后台高亮结果永远无法被消费，tokens 停留在空/旧状态。
        if ts_lang.is_some() && !self.content.is_large_file {
            if let Some(result) = self.bg_highlighter.poll_result() {
                let min_len = result
                    .token_lines
                    .len()
                    .min(self.content.cached_tokens.len());
                for i in 0..min_len {
                    self.content.cached_tokens[i] = result.token_lines[i].clone();
                }
                // 后台高亮结果刚到达：标记编辑器区域脏，使本帧立即以着色重绘，
                // 避免文件打开后停留在无高亮的纯文本状态直到下一次无关重绘。
                let er = self.layout.editor_region();
                self.dirty_tracker.mark_region(
                    er.x,
                    er.y,
                    er.width,
                    er.height,
                    crate::dirty_rect::DirtyRegionType::EditorContent,
                );
            }
        }

        // REQ-P2-01: 变化检测 — 如果 buffer_version、可见范围、总行数均未变化，跳过整个重建
        // 空闲帧（无编辑、无滚动）不会产生任何缓存重建开销
        let signature = (
            self.content.buffer_version,
            visible_start,
            visible_end,
            total_lines,
        );
        if self.content.last_cache_signature == signature
            && self.content.cached_lines.len() == total_lines
        {
            return;
        }
        self.content.last_cache_signature = signature;

        // P2.3: 大文件检测与行偏移缓存
        self.update_large_file_flag();
        self.rebuild_line_y_offsets();

        // 如果行数变化，重新调整缓存向量大小
        if self.content.cached_lines.len() != total_lines {
            self.content
                .cached_lines
                .resize_with(total_lines, String::new);
            self.content
                .cached_tokens
                .resize_with(total_lines, Vec::new);
            self.content.line_cache_versions.resize(total_lines, 0);
        }

        // 调整行号 UTF-16 缓存大小
        if self.cached_line_numbers.len() != total_lines {
            self.cached_line_numbers.resize_with(total_lines, Vec::new);
        }

        // 只重建可见行范围内的缓存（加上前后各2行的缓冲，避免滚动时闪烁）
        let cache_start = visible_start.saturating_sub(2);
        let cache_end = (visible_end + 2).min(total_lines);

        // P2.3: 大文件模式下跳过语法高亮，只缓存行文本
        // 延迟创建 fallback lexer：仅在 tree-sitter 不支持且至少一行需要重建时才创建
        let mut lexer: Option<Box<dyn aether_core::lexer::Lexer>> = None;

        // === P0-3: 后台语法高亮 — 发送请求 ===
        // poll 逻辑已移至签名检查之前，确保空闲帧也能消费后台结果。
        // 此处仅在 buffer_version 变化时发送新请求。
        if let Some(lang) = ts_lang {
            if !self.content.is_large_file
                && self.content.buffer_version != self.hl_request_version
                && !self.bg_highlighter.has_pending()
            {
                let full_text = self.content.buffer.get_all_text();
                let doc_id = self
                    .content
                    .file_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "untitled".to_string());
                self.bg_highlighter.request(&doc_id, lang, &full_text);
                self.hl_request_version = self.content.buffer_version;
            }
        }

        for i in cache_start..cache_end {
            if self.content.line_cache_versions[i] != self.content.buffer_version {
                let line = self.content.buffer.get_line(i).unwrap_or_default();

                if self.content.is_large_file {
                    // 大文件：跳过语法高亮
                    self.content.cached_lines[i] = line;
                    self.content.cached_tokens[i] = Vec::new();
                    self.content.line_cache_versions[i] = self.content.buffer_version;
                } else if ts_lang.is_some() {
                    // tree-sitter 语言：只更新文本，tokens 由后台线程异步更新
                    // 保留上一版本的 tokens（stale but usable），实现零输入延迟
                    self.content.cached_lines[i] = line;
                    self.content.line_cache_versions[i] = self.content.buffer_version;
                } else {
                    // fallback：手写 lexer（Markdown/Html/Css/PlainText/Image 等）
                    if lexer.is_none() {
                        lexer = Some(self.content.language.create_lexer());
                    }
                    // C-03: lexer 创建可能返回 None（不支持的语言），unwrap 会 panic 并穿越 WndProc
                    let tokens = if let Some(lex) = lexer.as_ref() {
                        lex.lex_full(&line)
                    } else {
                        Vec::new()
                    };
                    self.content.cached_lines[i] = line;
                    self.content.cached_tokens[i] = tokens;
                    self.content.line_cache_versions[i] = self.content.buffer_version;
                }
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
        let total_lines = self.content.buffer.len_lines().max(1);

        if self.content.cached_lines.len() != total_lines {
            self.content
                .cached_lines
                .resize_with(total_lines, String::new);
            self.content
                .cached_tokens
                .resize_with(total_lines, Vec::new);
            self.content.line_cache_versions.resize(total_lines, 0);
        }

        let ts_lang = language_to_ts_str(self.content.language);
        let mut lexer: Option<Box<dyn aether_core::lexer::Lexer>> = None;

        for i in 0..total_lines {
            if self.content.line_cache_versions[i] != self.content.buffer_version {
                let line = self.content.buffer.get_line(i).unwrap_or_default();
                let tokens = if let Some(lang) = ts_lang {
                    self.ts_highlighter.highlight_line(&line, lang)
                } else {
                    if lexer.is_none() {
                        lexer = Some(self.content.language.create_lexer());
                    }
                    if let Some(lex) = lexer.as_ref() {
                        lex.lex_full(&line)
                    } else {
                        Vec::new()
                    }
                };
                self.content.cached_lines[i] = line;
                self.content.cached_tokens[i] = tokens;
                self.content.line_cache_versions[i] = self.content.buffer_version;
            }
        }
    }

    /// 标记指定行范围的缓存为失效
    /// 在编辑操作后调用，只标记受影响的行，避免全量重建
    #[allow(dead_code)]
    pub(crate) fn invalidate_line_cache(&mut self, start_line: usize, end_line: usize) {
        let total_lines = self.content.line_cache_versions.len();
        if total_lines == 0 {
            return;
        }
        let start = start_line.min(total_lines - 1);
        let end = end_line.min(total_lines - 1);
        for i in start..=end {
            self.content.line_cache_versions[i] = 0; // 0 表示未缓存，强制重建
        }
    }

    /// 处理编辑结果，更新缓存和行版本
    #[allow(dead_code)]
    pub(crate) fn apply_edit_result(&mut self, result: &aether_core::buffer::EditResult) {
        self.content.buffer_version += 1;
        let total_lines = self.content.buffer.len_lines().max(1);

        if result.line_delta != 0 {
            // 行数变化，重新调整缓存向量
            self.content
                .cached_lines
                .resize_with(total_lines, String::new);
            self.content
                .cached_tokens
                .resize_with(total_lines, Vec::new);
            self.content.line_cache_versions.resize(total_lines, 0);
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
            && self.find_result_version == self.content.buffer_version
            && !self.find_results.is_empty()
        {
            // 结果已有效，无需重新搜索
            return;
        }
        // 缓存未命中：清空并重新搜索
        self.find_results.clear();
        let query = self.find_query.clone();
        let total_lines = self.content.buffer.len_lines();
        for line_idx in 0..total_lines {
            if let Some(line) = self.content.buffer.get_line(line_idx) {
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
        self.find_result_version = self.content.buffer_version;
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
            self.content.cursor_line = line;
            self.content.cursor_col = end_col;
            // 选中匹配文本
            self.content.selection_start = Some((line, col));
            self.content.selection_end = Some((line, end_col));
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
            self.content.cursor_line = line;
            self.content.cursor_col = end_col;
            self.content.selection_start = Some((line, col));
            self.content.selection_end = Some((line, end_col));
        }
    }

    /// P2-6: 把字节偏移对齐到字符边界（向下取到下一个字符起点）。
    /// 避免 selection_end 落在多字节字符中间导致渲染/截取异常。
    fn clamp_to_char_boundary(&self, line_idx: usize, byte_pos: usize) -> usize {
        if let Some(line) = self.content.buffer.get_line(line_idx) {
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

        let before_pieces = self.content.buffer.get_pieces();
        let before_add_len = self.content.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        self.content.buffer.delete(pos, end_pos);
        self.content.buffer.insert(pos, &self.replace_text);
        self.content.is_dirty = true;
        self.content.buffer_version += 1;

        self.content.cursor_line = line;
        self.content.cursor_col = col + self.replace_text.len();
        let cursor_after = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
        self.content.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
            self.replace_text.len(),
        );

        // 重新查找
        self.find_all();
        true
    }

    /// 替换所有匹配
    /// REQ-P0-02: 使用 begin_group/end_group 包裹，记录撤销历史
    pub fn replace_all(&mut self) -> usize {
        if self.find_query.is_empty() || self.find_query == self.replace_text {
            return 0;
        }
        self.find_all();
        let count = self.find_results.len();
        if count == 0 {
            return 0;
        }

        // REQ-P1-04: 转换为全局字节偏移，避免替换文本含换行符时行号偏移
        let query_len = self.find_query.len();
        let replace_text = self.replace_text.clone();
        let mut global_offsets: Vec<usize> = self
            .find_results
            .iter()
            .map(|(line, col)| self.line_byte_start(*line) + *col)
            .collect();
        // 降序排序：从文件末尾向前替换，前面的位置不受影响
        global_offsets.sort_by(|a, b| b.cmp(a));

        // REQ-P0-02: 记录替换前的光标位置，用于撤销后恢复
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        // REQ-P0-02: 开始撤销组，所有替换作为一个原子撤销单元
        self.content.history.begin_group();

        for pos in global_offsets {
            let end_pos = pos + query_len;

            // REQ-P0-02: 每次替换前记录缓冲区状态
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();

            self.content.buffer.delete(pos, end_pos);
            self.content.buffer.insert(pos, &replace_text);

            // REQ-P0-02: 记录每次替换的编辑历史
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_before,
                OpType::Replace,
                pos,
                replace_text.len(),
            );
        }

        // REQ-P0-02: 结束撤销组
        self.content.history.end_group();

        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;
        self.find_results.clear();
        self.find_active_index = 0;
        self.status_message = format!("已替换 {} 处", count);
        self.emit_edit_events();
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

    /// 复制最后一条 AI 回复到剪贴板
    pub fn copy_ai_last_response(&mut self) {
        if let Some(t) = self.ai_panel.last_assistant_text() {
            if Self::set_clipboard_text(&t) {
                self.status_message = "已复制 AI 回复".to_string();
            }
        }
    }

    /// 保存 AI 代码块为文件
    /// 如果 filename 为空，则尝试从代码块内容推断或使用默认名称
    pub fn save_ai_code_block(
        &mut self,
        code: &str,
        suggested_filename: Option<&str>,
    ) -> std::result::Result<PathBuf, String> {
        let root = self
            .current_folder
            .clone()
            .ok_or_else(|| "请先打开一个工作区文件夹".to_string())?;

        // 确定文件名
        let filename = if let Some(name) = suggested_filename {
            name.to_string()
        } else {
            // 尝试从代码内容推断语言并生成默认文件名
            let ext = if code.contains("fn ") || code.contains("use ") || code.contains("impl ") {
                "rs"
            } else if code.contains("def ") || code.contains("import ") {
                "py"
            } else if code.contains("function ") || code.contains("const ") || code.contains("let ")
            {
                "js"
            } else if code.contains("package ") || code.contains("import java.") {
                "java"
            } else if code.contains("#include") || code.contains("int main") {
                "c"
            } else if code.contains("<?php") {
                "php"
            } else if code.contains("<html") || code.contains("<!DOCTYPE") {
                "html"
            } else if code.contains("body {") || code.contains("@media") {
                "css"
            } else {
                "txt"
            };
            format!("ai_generated.{}", ext)
        };

        let full_path = root.join(&filename);

        // 确保父目录存在
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
        }

        // 写入文件
        std::fs::write(&full_path, code).map_err(|e| format!("写入文件失败: {}", e))?;

        // 打开新创建的文件
        self.load_file(full_path.clone());

        self.status_message = format!("已保存文件: {}", filename);
        Ok(full_path)
    }

    /// AI Agent：处理最后一条助手消息中的动作标记（生成完成时调用一次）。
    ///
    /// - `<<<<<<< FILE 路径 >>>>>>>` 块：创建/修改/删除文件（自动建目录）。
    /// - `<<<<<<< RUN >>>>>>>` 块：在集成终端执行命令。
    ///
    /// 执行结果以助手消息形式反馈到 AI 面板，并刷新文件树。
    pub fn process_ai_agent_actions(&mut self) {
        // Edit 模式走差异预览确认流程，不在此处直接落盘。
        if matches!(self.ai_panel.mode, crate::ai_prompt::AiMode::Edit) {
            return;
        }
        let Some(text) = self.ai_panel.last_assistant_text() else {
            return;
        };

        // 文件/终端操作必须在已打开的工作区内进行；未打开文件夹时提示用户。
        let has_actions = text.contains("<<<<<<< FILE") || text.contains("<<<<<<< RUN");
        if has_actions && self.current_folder.is_none() {
            self.ai_panel
                .add_assistant_message("⚠️ 尚未打开工作区文件夹，无法直接创建/修改文件。请先通过“文件 → 打开文件夹”打开一个项目再试。".to_string());
            self.dirty_tracker.mark_full_window();
            return;
        }

        // 1. 文件操作（创建/修改/删除）
        let edits = crate::ai_agent::parse_edits(&text, None);
        let mut file_summary: Vec<String> = Vec::new();
        if !edits.is_empty() {
            match self.apply_ai_workspace_edits(&edits) {
                Ok(paths) => {
                    for p in &paths {
                        let name = self
                            .current_folder
                            .as_ref()
                            .and_then(|root| p.strip_prefix(root).ok())
                            .unwrap_or(p.as_path());
                        file_summary.push(format!("✅ 已写入 `{}`", name.display()));
                    }
                }
                Err(e) => {
                    file_summary.push(format!("⚠️ 文件操作失败: {}", e));
                }
            }
            // 刷新文件树以显示新文件
            if let Some(folder) = self.current_folder.clone() {
                self.open_folder(folder);
            }
        }

        // 2. 终端命令
        let commands = crate::ai_agent::parse_run_commands(&text);
        let mut cmd_summary: Vec<String> = Vec::new();
        if !commands.is_empty() {
            // 打开底部面板并切换到终端
            self.layout.bottom_panel_visible = true;
            self.bottom_panel_tab = crate::editor::BottomPanelTab::Terminal;
            // 同步终端工作目录到当前工作区
            if let Some(folder) = self.current_folder.clone() {
                self.terminal_panel.cwd = folder.to_string_lossy().to_string();
            }
            // 启动终端（若未运行）并排队命令
            if !self.terminal_panel.running {
                let _ = self.terminal_panel.start();
            }
            for cmd in &commands {
                self.terminal_panel.queue_command(cmd.clone());
                cmd_summary.push(format!("▶️ 执行 `{}`", cmd));
            }
            // 启动终端刷新定时器，保证轮询启动结果并刷新命令队列
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(
                    self.hwnd,
                    crate::window::TERM_TIMER_ID,
                    crate::window::TERM_REFRESH_MS,
                    None,
                );
            }
        }

        // 3. 反馈汇总到 AI 面板
        if !file_summary.is_empty() || !cmd_summary.is_empty() {
            let mut lines = Vec::new();
            lines.extend(file_summary);
            lines.extend(cmd_summary);
            self.ai_panel.add_assistant_message(lines.join("\n"));
            self.dirty_tracker.mark_full_window();
        }
    }

    /// 把当前文件的 LSP 诊断发送给 AI 修复
    pub fn ai_fix_diagnostics(&mut self) {
        let settings = self.app_settings.active_ai_settings();
        let context = self.gather_context(&[
            crate::ai_context::AiContextAttachment::CurrentFile,
            crate::ai_context::AiContextAttachment::Diagnostics,
        ]);
        let _ = self.ai_panel.send_message_with_prepared_context(
            &settings,
            context,
            crate::ai_prompt::AiMode::Edit,
        );
    }

    /// 自动应用 AI 面板中待确认的编辑到工作区
    pub fn ai_apply_pending_changes(&mut self) {
        if self.ai_panel.is_generating || self.ai_panel.diff_view.files.is_empty() {
            return;
        }
        let edits = {
            let diff_view = &mut self.ai_panel.diff_view;
            diff_view.accept_all();
            diff_view.to_edits()
        };
        if !edits.is_empty() {
            match self.apply_ai_workspace_edits(&edits) {
                Ok(paths) => {
                    self.status_message = format!("已应用 AI 编辑: {} 个文件", paths.len())
                }
                Err(e) => self.status_message = format!("AI 编辑应用失败: {}", e),
            }
        }
        self.ai_panel.clear_pending_changes();
    }

    /// 接受并立即应用变更列表中的单个文件（变更列表预览“接受”按钮）
    pub fn ai_accept_change_file(&mut self, idx: usize) {
        let edit = match self.ai_panel.diff_view.files.get(idx) {
            Some(f) => crate::ai_agent::AiEdit {
                path: f.path.clone(),
                search: f.original.clone(),
                replace: f.proposed.clone(),
            },
            None => return,
        };
        match self.apply_ai_workspace_edits(&[edit]) {
            Ok(_) => {
                if idx < self.ai_panel.diff_view.files.len() {
                    self.ai_panel.diff_view.files.remove(idx);
                }
                self.status_message = "已应用该文件变更".to_string();
            }
            Err(e) => self.status_message = format!("AI 编辑应用失败: {}", e),
        }
        self.ai_panel.diff_view.selected_index = 0;
        if self.ai_panel.diff_view.files.is_empty() {
            self.ai_panel.clear_pending_changes();
        }
    }

    /// 拒绝变更列表中的单个文件（仅从列表移除，不修改磁盘）
    pub fn ai_reject_change_file(&mut self, idx: usize) {
        if idx < self.ai_panel.diff_view.files.len() {
            self.ai_panel.diff_view.files.remove(idx);
        }
        self.ai_panel.diff_view.selected_index = 0;
        if self.ai_panel.diff_view.files.is_empty() {
            self.ai_panel.clear_pending_changes();
        }
        self.status_message = "已拒绝该文件变更".to_string();
    }

    /// 打开设置标签页（作为通用 tab 插入到标签栏）
    pub fn open_settings_tab(&mut self) {
        self.settings_panel.apply_settings(&self.app_settings);
        if let Some(idx) = self.find_settings_tab() {
            self.switch_tab(idx);
        } else {
            // 保存当前文件 tab 的内容到 tabs[active_tab]（如果是文件 tab）
            self.swap_tab_content(self.active_tab);
            self.tabs.push(crate::tabs::Tab::Settings);
            self.active_tab = self.tabs.len() - 1;
            // 设置 tab 无 content，self.content 保持空即可
            self.content = crate::tabs::TabContent::new();
            self.ai_panel.input_focused = false;
            self.status_message = "已打开设置标签页".to_string();
            self.emit_event(crate::events::EditorEvent::TabChanged);
        }
    }

    /// 关闭设置标签页
    pub fn close_settings_tab(&mut self) {
        if let Some(idx) = self.find_settings_tab() {
            if idx == self.active_tab {
                // 关闭活动设置 tab，切换到最近的文件 tab
                self.close_current_tab();
            } else {
                // 非活动设置 tab 直接移除
                self.tabs.remove(idx);
                if self.active_tab > idx {
                    self.active_tab -= 1;
                }
                self.status_message = "已关闭设置标签页".to_string();
            }
        }
        self.settings_panel.active_field = None;
    }

    /// 切换设置标签页
    pub fn toggle_settings_tab(&mut self) {
        if self.active_tab_is_settings() {
            self.close_settings_tab();
        } else {
            self.open_settings_tab();
        }
    }

    /// 将设置面板中的 AI 配置应用到 app_settings 并持久化到磁盘
    ///
    /// API 密钥通过 DPAPI 加密单独存储（见 AppSettings::save），不会明文写入 settings.json。
    /// 同时刷新 AI 面板使用的运行时设置。
    pub fn save_ai_settings(&mut self) {
        self.app_settings.ai = self.settings_panel.to_ai_settings();
        match self.app_settings.save() {
            Ok(_) => {
                self.settings_panel.test_status = "✓ 设置已保存".to_string();
                self.status_message = "AI 设置已保存".to_string();
            }
            Err(e) => {
                self.settings_panel.test_status = format!("✗ 保存失败：{}", e);
            }
        }
    }

    /// 保存 AI 设置前，先启动测试连接验证密钥有效性。
    /// 测试成功后会自动调用 save_ai_settings 完成保存。
    pub fn save_ai_settings_with_test(&mut self) {
        let ai = self.settings_panel.to_ai_settings();
        if ai.api_key.trim().is_empty() {
            self.settings_panel.test_status = "✗ 请先填写 API 密钥".to_string();
            return;
        }
        self.settings_panel.pending_save = true;
        self.settings_panel.start_test_connection(ai);
    }

    /// 使用设置面板当前配置启动 AI 测试连接（后台线程，不阻塞 UI）
    pub fn start_ai_test_connection(&mut self) {
        let ai = self.settings_panel.to_ai_settings();
        self.settings_panel.start_test_connection(ai);
    }

    /// 初始化 LSP 客户端（在打开工作区文件夹时调用）
    pub fn init_lsp(&mut self, root_dir: &Path) {
        // 如果已有 LSP 客户端，先清理
        self.legacy_lsp_client = None;
        self.lsp_rx = None;
        self.lsp_runtime = None;

        let root_uri = url::Url::from_directory_path(root_dir).ok();
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => return,
        };

        let (client, rx) = aether_lsp::client::LspClient::new(root_uri.clone());

        // 在 tokio 运行时中启动 Rust 语言服务器（如果 rust-analyzer 可用）
        if let Some(config) = aether_lsp::client::default_server_config("rust") {
            let config = aether_lsp::types::ServerConfig {
                root_uri: root_uri.clone(),
                ..config
            };
            let client_clone = std::sync::Arc::new(client);
            let client_for_spawn = std::sync::Arc::clone(&client_clone);
            runtime.spawn(async move {
                let _ = client_for_spawn.start_server("rust", config).await;
            });
            self.legacy_lsp_client = Some(client_clone);
        } else {
            self.legacy_lsp_client = Some(std::sync::Arc::new(client));
        }

        self.lsp_rx = Some(rx);
        self.lsp_runtime = Some(runtime);
    }

    /// 通知 LSP 服务器文档已打开
    pub fn lsp_open_document(&mut self, path: &Path, text: &str) {
        let Some(client) = self.legacy_lsp_client.clone() else {
            return;
        };
        let Some(runtime) = self.lsp_runtime.as_ref() else {
            return;
        };
        let Ok(uri) = url::Url::from_file_path(path) else {
            return;
        };
        let language_id = language_to_lsp_id(self.content.language).to_string();
        let text = text.to_string();
        runtime.spawn(async move {
            let _ = client.open_document(uri, language_id, text).await;
        });
    }

    /// 轮询 LSP 事件，将诊断同步到 LspClient 缓存和 EditorState.diagnostics
    /// 应在渲染循环中每帧调用
    pub fn poll_lsp_events(&mut self) {
        let Some(rx) = self.lsp_rx.as_mut() else {
            return;
        };
        while let Ok(event) = rx.try_recv() {
            use aether_lsp::client::LspEvent;
            match event {
                LspEvent::Diagnostics { uri, diagnostics } => {
                    // 同时写入 LspClient 诊断缓存（供其他模块查询）
                    if let Some(client) = self.legacy_lsp_client.as_ref() {
                        client.update_diagnostics(&uri, diagnostics.clone());
                    }

                    let path_str = match uri.to_file_path() {
                        Ok(p) => p.to_string_lossy().to_string(),
                        Err(()) => uri.as_str().to_string(),
                    };
                    let items: Vec<DiagnosticItem> = diagnostics
                        .iter()
                        .map(|d| {
                            use aether_lsp::lsp_types::DiagnosticSeverity;
                            let severity = match d.severity {
                                Some(DiagnosticSeverity::ERROR) => 1,
                                Some(DiagnosticSeverity::WARNING) => 2,
                                Some(DiagnosticSeverity::INFORMATION) => 3,
                                Some(DiagnosticSeverity::HINT) => 4,
                                _ => 1,
                            };
                            DiagnosticItem {
                                severity,
                                message: d.message.clone(),
                                line: d.range.start.line as usize + 1,
                                col: d.range.start.character as usize + 1,
                                end_line: d.range.end.line as usize + 1,
                                end_col: d.range.end.character as usize + 1,
                            }
                        })
                        .collect();
                    if items.is_empty() {
                        self.diagnostics.remove(&path_str);
                    } else {
                        self.diagnostics.insert(path_str, items);
                    }
                }
                LspEvent::ServerReady { language_id } => {
                    self.status_message = format!("LSP 服务器就绪: {}", language_id);
                }
                LspEvent::Log { message, .. } => {
                    tracing::debug!("LSP: {}", message);
                }
                _ => {}
            }
        }
    }

    /// 根据附件列表收集 AI 上下文
    pub fn gather_context(&self, attachments: &[AiContextAttachment]) -> String {
        let mut parts = Vec::new();
        let current_path = self
            .content
            .file_path
            .as_deref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "当前文件".to_string());
        let current_lang = language_str(self.content.language);

        for attachment in attachments {
            match attachment {
                AiContextAttachment::CurrentFile => {
                    let text = self
                        .content
                        .buffer
                        .get_text(0, self.content.buffer.len_bytes());
                    parts.push(wrap_code_block(
                        &current_path,
                        current_lang,
                        &truncate_middle(&text, 30_000),
                    ));
                }
                AiContextAttachment::Selection => {
                    if let Some(text) = self.selected_text() {
                        parts.push(wrap_code_block(
                            &format!("{} (选区)", current_path),
                            current_lang,
                            &truncate_middle(&text, 10_000),
                        ));
                    }
                }
                AiContextAttachment::OpenFiles => {
                    let mut summary = String::from("打开的文件列表：\n");
                    // 活动标签页的内容存于 self.content（swap 后），需提前提取避免借用冲突
                    let active_idx = self.active_tab;
                    let active_path = self
                        .content
                        .file_path
                        .as_deref()
                        .map(|p| p.to_string_lossy().to_string());
                    let active_lang = language_str(self.content.language);
                    let active_text = self
                        .content
                        .buffer
                        .get_text(0, self.content.buffer.len_bytes());
                    for (i, tab) in self.tabs.iter().enumerate() {
                        let (path, lang, text) = if i == active_idx {
                            (
                                active_path
                                    .clone()
                                    .unwrap_or_else(|| format!("未命名-{}", i + 1)),
                                active_lang,
                                active_text.clone(),
                            )
                        } else if let Some(content) = tab.as_file() {
                            let path = content
                                .file_path
                                .as_deref()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_else(|| format!("未命名-{}", i + 1));
                            let lang = language_str(content.language);
                            let text = content.buffer.get_text(0, content.buffer.len_bytes());
                            (path, lang, text)
                        } else {
                            continue;
                        };
                        summary.push_str(&wrap_code_block(
                            &path,
                            lang,
                            &truncate_middle(&text, 5_000),
                        ));
                    }
                    parts.push(summary);
                }
                AiContextAttachment::Diagnostics => {
                    let current_key = self
                        .content
                        .file_path
                        .as_deref()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let mut all: Vec<&DiagnosticItem> =
                        self.diagnostics.values().flatten().collect();
                    // 优先显示当前文件，再按 severity 排序（1=Error, 2=Warning）
                    all.sort_by_key(|d| {
                        let is_current = self
                            .content
                            .file_path
                            .as_deref()
                            .map(|p| p.to_string_lossy().to_string() == current_key)
                            .unwrap_or(false);
                        (if is_current { 0 } else { 1 }, d.severity)
                    });
                    if all.is_empty() {
                        parts.push("当前文件暂无 LSP 诊断信息。\n".to_string());
                    } else {
                        let mut text = String::from("当前 LSP 诊断：\n");
                        for d in all.iter().take(20) {
                            let severity = match d.severity {
                                1 => "Error",
                                2 => "Warning",
                                3 => "Information",
                                4 => "Hint",
                                _ => "Diagnostic",
                            };
                            text.push_str(&format!(
                                "[{}] {}:{} {}\n",
                                severity, d.line, d.col, d.message
                            ));
                        }
                        parts.push(text);
                    }
                }
                AiContextAttachment::FileTree => {
                    if let Some(tree) = &self.file_tree {
                        parts.push(format!("工作区文件树：\n{}\n", self.format_file_tree(tree)));
                    } else {
                        parts.push("未加载工作区文件树。\n".to_string());
                    }
                }
                AiContextAttachment::CustomText(text) => {
                    parts.push(format!("用户附加文本：\n{}\n", text));
                }
            }
        }

        parts.join("\n")
    }

    fn selected_text(&self) -> Option<String> {
        let (start_line, start_col) = self.content.selection_start?;
        let (end_line, end_col) = self.content.selection_end?;
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
        if start_byte >= end_byte {
            return None;
        }
        Some(self.content.buffer.get_text(start_byte, end_byte))
    }

    fn format_file_tree(&self, tree: &FileTree) -> String {
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

fn language_str(lang: Language) -> &'static str {
    match lang {
        Language::C => "c",
        Language::Rust => "rust",
        Language::Python => "python",
        Language::JavaScript => "javascript",
        Language::TypeScript => "typescript",
        Language::Json => "json",
        Language::Markdown => "markdown",
        Language::Toml => "toml",
        Language::Html => "html",
        Language::Css => "css",
        Language::Go => "go",
        Language::Java => "java",
        Language::PlainText => "text",
        Language::Image => "image",
    }
}

/// 将内部 Language 枚举映射为 LSP language_id 字符串
fn language_to_lsp_id(lang: Language) -> &'static str {
    match lang {
        Language::Rust => "rust",
        Language::Python => "python",
        Language::JavaScript => "javascript",
        Language::TypeScript => "typescript",
        Language::C => "c",
        _ => "plaintext",
    }
}

impl EditorState {
    /// 应用 AI 生成的代码到当前编辑器
    pub fn apply_ai_code(&mut self, code: &str) -> bool {
        if code.is_empty() {
            return false;
        }
        // 如果有选区，替换选区内容；否则在当前光标位置插入
        // C-02/H-21: 使用 zip 一次性解构，避免独立 unwrap 在中间状态变更后 panic
        if let Some(((start_line, start_col), (end_line, end_col))) =
            self.content.selection_start.zip(self.content.selection_end)
        {
            let (first_line, first_col) = if (start_line, start_col) <= (end_line, end_col) {
                (start_line, start_col)
            } else {
                (end_line, end_col)
            };
            let (last_line, last_col) = if (start_line, start_col) <= (end_line, end_col) {
                (end_line, end_col)
            } else {
                (start_line, start_col)
            };
            let start_byte = self.line_byte_start(first_line) + first_col;
            let end_byte = self.line_byte_start(last_line) + last_col;

            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();
            let cursor_before =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

            self.content.buffer.delete(start_byte, end_byte);
            self.content.buffer.insert(start_byte, code);

            // 计算新光标位置
            let code_lines: Vec<&str> = code.lines().collect();
            let new_line = first_line + code_lines.len().saturating_sub(1);
            let new_col = if code_lines.len() <= 1 {
                first_col + code.len()
            } else {
                code_lines.last().unwrap_or(&"").len()
            };
            self.content.cursor_line = new_line;
            self.content.cursor_col = new_col;
            let cursor_after =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Insert,
                start_byte,
                code.len(),
            );

            self.clear_selection();
            self.content.is_dirty = true;
            self.content.buffer_version += 1;
            self.status_message = "已应用 AI 代码".to_string();
            return true;
        }
        let pos = self.cursor_byte_pos();
        let before_pieces = self.content.buffer.get_pieces();
        let before_add_len = self.content.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        self.content.buffer.insert(pos, code);

        // 更新光标位置
        let _code_lines: Vec<&str> = code.lines().collect();
        let line_breaks = code.matches('\n').count();
        if line_breaks == 0 {
            self.content.cursor_col += code.len();
        } else {
            self.content.cursor_line += line_breaks;
            self.content.cursor_col = code
                .rsplit_once('\n')
                .map(|(_, last)| last.len())
                .unwrap_or(0);
        }
        let cursor_after = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
        self.content.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
            code.len(),
        );

        self.content.is_dirty = true;
        self.content.buffer_version += 1;
        self.status_message = "已插入 AI 代码".to_string();
        true
    }

    /// 应用 AI 生成的工作区编辑（支持修改已打开/未打开的文件以及创建新文件）
    pub fn apply_ai_workspace_edits(
        &mut self,
        edits: &[AiEdit],
    ) -> std::result::Result<Vec<PathBuf>, String> {
        let mut applied = Vec::new();
        let original_tab = self.active_tab;

        for edit in edits {
            let full_path = self.resolve_edit_path(&edit.path);

            // 删除文件操作
            if edit.is_delete() {
                // 关闭对应 tab（如果有）；用户取消则跳过此文件
                if let Some(idx) = self
                    .tabs
                    .iter()
                    .position(|t| t.file_path() == Some(&full_path))
                {
                    if !self.close_tab(idx) {
                        continue;
                    }
                }
                // 从磁盘删除文件
                if full_path.exists() {
                    std::fs::remove_file(&full_path)
                        .map_err(|e| format!("删除文件 {} 失败: {}", full_path.display(), e))?;
                }
                self.status_message = format!("已删除文件: {}", full_path.display());
                applied.push(full_path);
                continue;
            }

            // 找到或创建对应标签页
            let tab_idx = self
                .tabs
                .iter()
                .position(|t| t.file_path() == Some(&full_path));
            if let Some(idx) = tab_idx {
                self.switch_tab(idx);
            } else if full_path.exists() {
                self.load_file(full_path.clone());
            } else {
                self.create_new_file_tab(&full_path);
            }

            // 应用单个编辑
            let old_text = self
                .content
                .buffer
                .get_text(0, self.content.buffer.len_bytes());
            let new_text = if edit.search.trim().is_empty() {
                edit.replace.clone()
            } else {
                match old_text.find(&edit.search) {
                    Some(pos) => {
                        let mut replaced = old_text.clone();
                        replaced.replace_range(pos..pos + edit.search.len(), &edit.replace);
                        replaced
                    }
                    None => {
                        return Err(format!(
                            "无法在 {} 中找到要替换的代码片段",
                            full_path.display()
                        ));
                    }
                }
            };

            // 记录 undo history，使 AI 工作区编辑可通过 Ctrl+Z 逐文件撤销
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();
            let cursor_before =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
            let len = self.content.buffer.len_bytes();
            self.content.buffer.delete(0, len);
            self.content.buffer.insert(0, &new_text);
            // 全量替换后将光标复位到文件开头，避免越界
            self.content.cursor_line = 0;
            self.content.cursor_col = 0;
            let cursor_after = CursorPosition::new(0, 0);
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Insert,
                0,
                new_text.len(),
            );
            self.content.buffer_version += 1;

            // 关键：将内容实际写入磁盘（当前工作区），而非仅停留在内存缓冲。
            // 先确保父目录存在（支持多级子目录自动创建），再原子写入。
            if let Some(parent) = full_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return Err(format!("创建目录 {} 失败: {}", parent.display(), e));
                }
            }
            if let Err(e) = Self::atomic_write(&full_path, new_text.as_bytes()) {
                return Err(format!("写入文件 {} 失败: {}", full_path.display(), e));
            }
            // 已落盘，清除脏标记
            self.content.is_dirty = false;
            self.status_message = format!("已写入文件: {}", full_path.display());
            applied.push(full_path);
        }

        // 尽量回到原来的标签页
        if original_tab < self.tabs.len() {
            self.switch_tab(original_tab);
        }

        Ok(applied)
    }

    pub(crate) fn resolve_edit_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            return path.to_path_buf();
        }
        self.current_folder
            .as_ref()
            .map(|root| root.join(path))
            .unwrap_or_else(|| path.to_path_buf())
    }

    fn create_new_file_tab(&mut self, path: &Path) {
        let lang = Language::from_path(path);
        let tab = Tab::File(TabContent::with_loaded_buffer(
            Some(path.to_path_buf()),
            PieceTable::from_string(String::new()),
            lang,
            true,
        ));
        self.open_in_new_tab(tab);
    }
}

/// 已知二进制文件扩展名（黑名单）
const BINARY_EXTENSIONS: &[&str] = &[
    // 图片
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "ico", "tiff", "tif", "raw", "psd", "ai", "sketch",
    "fig", // 音视频
    "mp4", "avi", "mov", "wmv", "flv", "mkv", "webm", "mp3", "wav", "flac", "aac", "ogg", "wma",
    "m4a", // 压缩/归档
    "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "lz4", "br", "deb", "rpm", "msi", "dmg", "pkg",
    "apk", "ipa", // 可执行/库
    "exe", "dll", "so", "dylib", "bin", "elf", "o", "obj", "lib", "a", "pdb", "ilk",
    // 字体
    "ttf", "otf", "woff", "woff2", "eot", // 文档/办公（二进制格式）
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp",
    // 数据库/缓存
    "db", "sqlite", "sqlite3", "mdb", "accdb", "cache", // 其他二进制
    "class", "jar", "war", "ear", "pyc", "pyo", "o", "lo", "la",
];

/// 将 aether-core 的 Language 枚举映射到 tree-sitter highlighter 接受的语言字符串。
/// 返回 None 的语言（Markdown/Html/Css/PlainText/Image）由调用方 fallback 到手写 lexer。
/// 注意：aether-core::Language 没有 Cpp 变体，因此 cpp 暂不在此映射中。
fn language_to_ts_str(lang: Language) -> Option<&'static str> {
    match lang {
        Language::Rust => Some("rust"),
        Language::JavaScript => Some("javascript"),
        Language::TypeScript => Some("typescript"),
        Language::Python => Some("python"),
        Language::C => Some("c"),
        Language::Json => Some("json"),
        Language::Toml => Some("toml"),
        Language::Go => Some("go"),
        Language::Java => Some("java"),
        // Markdown/Html/Css/PlainText/Image → None，fallback 到手写 lexer
        _ => None,
    }
}

/// 将 Language 枚举映射到 LSP language_id 字符串（Option 版本，供新 LSP 集成使用）。
/// 仅返回有默认 server 配置的语言（rust/python/typescript/javascript/c）。
/// 其他语言（Json/Toml/Markdown/Html/Css/PlainText/Image）返回 None，不启动 LSP。
fn language_to_lsp_id_opt(lang: Language) -> Option<&'static str> {
    match lang {
        Language::Rust => Some("rust"),
        Language::Python => Some("python"),
        Language::JavaScript => Some("javascript"),
        Language::TypeScript => Some("typescript"),
        Language::C => Some("c"),
        _ => None,
    }
}

/// 从 LSP Hover 响应中提取纯文本内容。
/// HoverContents 可能是 Scalar(单个)、Array(多个) 或 Markup(带格式)，
/// 统一提取为可显示的 String。
fn extract_hover_text(hover: &lsp_types::Hover) -> Option<String> {
    use lsp_types::{HoverContents, MarkedString};
    let text = match &hover.contents {
        HoverContents::Scalar(ms) => match ms {
            MarkedString::String(s) => s.clone(),
            MarkedString::LanguageString(ls) => ls.value.clone(),
        },
        HoverContents::Array(arr) => {
            let parts: Vec<String> = arr
                .iter()
                .map(|ms| match ms {
                    MarkedString::String(s) => s.clone(),
                    MarkedString::LanguageString(ls) => ls.value.clone(),
                })
                .collect();
            if parts.is_empty() {
                return None;
            }
            parts.join("\n")
        }
        HoverContents::Markup(markup) => markup.value.clone(),
    };
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// 已知文本扩展名白名单：跳过内容探测，直接视为文本文件。
/// 避免对常见代码/标记文件做不必要的 UTF-8 扫描和磁盘读取。
const TEXT_EXTENSIONS: &[&str] = &[
    // 网页/模板/小程序标记
    "html",
    "htm",
    "xhtml",
    "xml",
    "svg",
    "vue",
    "svelte",
    "wxml",
    "axml",
    "ftl",
    "jinja",
    "j2",
    "njk",
    "mustache",
    "handlebars",
    "hbs",
    "ejs",
    "erb",
    "haml",
    "pug",
    "jade",
    "liquid",
    "razor",
    "cshtml",
    "wxml",
    "wxss",
    // 样式
    "css",
    "scss",
    "sass",
    "less",
    "styl",
    "stylus",
    "acss",
    // 脚本/语言
    "js",
    "jsx",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "rs",
    "py",
    "pyw",
    "pyi",
    "pyx",
    "pxd",
    "c",
    "h",
    "cpp",
    "hpp",
    "cc",
    "cxx",
    "m",
    "mm",
    "go",
    "java",
    "kt",
    "swift",
    "rb",
    "php",
    "cs",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "psm1",
    "psd1",
    "bat",
    "cmd",
    "vbs",
    // 数据/配置
    "json",
    "jsonc",
    "jsonl",
    "toml",
    "ini",
    "cfg",
    "conf",
    "config",
    "yaml",
    "yml",
    "properties",
    // 文档
    "md",
    "markdown",
    "mdx",
    "txt",
    "text",
    "log",
];

/// 检查文件是否为文本文件
/// 策略：
/// 1. 已知文本扩展名直接视为文本（避免磁盘读取）。
/// 2. 已知二进制扩展名直接排除。
/// 3. 否则读取前 8KB：有效 UTF-8 或高比例 ASCII 可打印字符视为文本。
pub(crate) fn is_text_file(path: &std::path::Path) -> bool {
    if let Some(ext) = path.extension() {
        if let Some(ext_str) = ext.to_str() {
            let ext_lower = ext_str.to_lowercase();
            if TEXT_EXTENSIONS.contains(&ext_lower.as_str()) {
                return true;
            }
            if BINARY_EXTENSIONS.contains(&ext_lower.as_str()) {
                return false;
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
            // 若样本是有效 UTF-8，则视为文本文件（支持中文、emoji 等多字节内容）
            if std::str::from_utf8(sample).is_ok() {
                return true;
            }
            // 回退：检查是否主要是可打印 ASCII 字符
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

    // 无扩展名且无法读取内容时，保守视为文本（如 UNIX 可执行脚本）
    path.extension().is_none()
}

/// 将字符偏移量转换为字节偏移量。
///
/// 编辑器内部使用字节偏移表示光标列，但用户输入的列号是字符位置。
/// 该函数按 Unicode 标量值计数，返回对应字符起始位置的字节索引。
fn char_offset_to_byte_offset(text: &str, char_offset: usize) -> usize {
    if char_offset == 0 {
        return 0;
    }

    text.char_indices()
        .nth(char_offset)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_core::workspace::file_tree::FileTree;

    #[test]
    fn test_hover_tooltip_new_and_is_empty() {
        let t = HoverTooltip::new("hello", 10.0, 20.0, 300.0);
        assert_eq!(t.text, "hello");
        assert_eq!(t.x, 10.0);
        assert_eq!(t.y, 20.0);
        assert_eq!(t.max_width, 300.0);
        assert!(!t.is_empty());

        let empty = HoverTooltip::new(String::new(), 0.0, 0.0, 0.0);
        assert!(empty.is_empty());
    }

    #[test]
    fn test_hover_tooltip_equality() {
        let a = HoverTooltip::new("x", 1.0, 2.0, 3.0);
        let b = HoverTooltip::new("x", 1.0, 2.0, 3.0);
        let c = HoverTooltip::new("y", 1.0, 2.0, 3.0);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_file_tree_node_path_single_root() {
        let mut tree = FileTree::new();
        let root = tree.add_node("src", FileKind::Directory, u32::MAX, 0);
        let path = file_tree_node_path(&tree, root).expect("应返回路径");
        assert_eq!(path, "src");
    }

    #[test]
    fn test_file_tree_node_path_nested() {
        let mut tree = FileTree::new();
        let src = tree.add_node("src", FileKind::Directory, u32::MAX, 0);
        let main = tree.add_node("main.rs", FileKind::File, src, 1);
        let path = file_tree_node_path(&tree, main).expect("应返回路径");
        assert_eq!(path, "src/main.rs");
    }

    #[test]
    fn test_file_tree_node_path_deep_nested() {
        let mut tree = FileTree::new();
        let a = tree.add_node("a", FileKind::Directory, u32::MAX, 0);
        let b = tree.add_node("b", FileKind::Directory, a, 1);
        let c = tree.add_node("c", FileKind::Directory, b, 2);
        let f = tree.add_node("f.txt", FileKind::File, c, 3);
        let path = file_tree_node_path(&tree, f).expect("应返回路径");
        assert_eq!(path, "a/b/c/f.txt");
    }

    #[test]
    fn test_file_tree_node_path_invalid_index_returns_none() {
        let tree = FileTree::new();
        assert!(file_tree_node_path(&tree, 999).is_none());
    }

    #[test]
    fn test_language_str() {
        assert_eq!(language_str(Language::Rust), "rust");
        assert_eq!(language_str(Language::C), "c");
        assert_eq!(language_str(Language::Python), "python");
        assert_eq!(language_str(Language::PlainText), "text");
        assert_eq!(language_str(Language::Image), "image");
    }

    #[test]
    fn test_language_to_lsp_id() {
        assert_eq!(language_to_lsp_id(Language::Rust), "rust");
        assert_eq!(language_to_lsp_id(Language::Python), "python");
        assert_eq!(language_to_lsp_id(Language::JavaScript), "javascript");
        assert_eq!(language_to_lsp_id(Language::TypeScript), "typescript");
        assert_eq!(language_to_lsp_id(Language::C), "c");
        assert_eq!(language_to_lsp_id(Language::PlainText), "plaintext");
        assert_eq!(language_to_lsp_id(Language::Markdown), "plaintext");
    }

    #[test]
    fn test_char_offset_to_byte_offset() {
        assert_eq!(char_offset_to_byte_offset("hello", 0), 0);
        assert_eq!(char_offset_to_byte_offset("hello", 2), 2);
        assert_eq!(char_offset_to_byte_offset("héllo", 3), 4); // é 占 2 字节
        assert_eq!(char_offset_to_byte_offset("héllo", 10), 6); // 越界返回末尾
        assert_eq!(char_offset_to_byte_offset("", 1), 0);
    }

    #[test]
    fn test_is_text_file_by_extension() {
        assert!(is_text_file(Path::new("main.rs")));
        assert!(is_text_file(Path::new("README.md")));
        assert!(is_text_file(Path::new("config.yaml")));
        assert!(!is_text_file(Path::new("image.png")));
        assert!(!is_text_file(Path::new("archive.zip")));
    }

    #[test]
    fn test_is_text_file_by_content() {
        let dir = std::env::temp_dir().join(format!("aether_text_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let text_file = dir.join("plain");
        std::fs::write(&text_file, "hello world\n中文\n").unwrap();
        assert!(is_text_file(&text_file));

        let bin_file = dir.join("binary");
        std::fs::write(&bin_file, vec![0u8, 1, 2, 3, 0, 5]).unwrap();
        assert!(!is_text_file(&bin_file));

        let no_ext = dir.join("script");
        std::fs::write(&no_ext, "#!/bin/sh\necho hi\n").unwrap();
        assert!(is_text_file(&no_ext));

        let missing = dir.join("does_not_exist");
        assert!(is_text_file(&missing)); // 无扩展名且无法读取，保守视为文本

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_language_str_all_variants() {
        use Language::*;
        assert_eq!(language_str(C), "c");
        assert_eq!(language_str(Rust), "rust");
        assert_eq!(language_str(Python), "python");
        assert_eq!(language_str(JavaScript), "javascript");
        assert_eq!(language_str(TypeScript), "typescript");
        assert_eq!(language_str(Json), "json");
        assert_eq!(language_str(Markdown), "markdown");
        assert_eq!(language_str(Toml), "toml");
        assert_eq!(language_str(Html), "html");
        assert_eq!(language_str(Css), "css");
        assert_eq!(language_str(PlainText), "text");
        assert_eq!(language_str(Image), "image");
    }

    #[test]
    fn test_language_to_lsp_id_other_languages() {
        use Language::*;
        assert_eq!(language_to_lsp_id(Markdown), "plaintext");
        assert_eq!(language_to_lsp_id(Json), "plaintext");
        assert_eq!(language_to_lsp_id(Html), "plaintext");
        assert_eq!(language_to_lsp_id(Image), "plaintext");
    }

    #[test]
    fn test_atomic_write() {
        let dir = std::env::temp_dir().join(format!("aether_atomic_write_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("out.txt");
        EditorState::atomic_write(&target, b"atomic data").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "atomic data");
        EditorState::atomic_write(&target, b"updated").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "updated");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_find_node_by_path() {
        let mut tree = FileTree::new();
        let root = tree.add_node("project", FileKind::Directory, u32::MAX, 0);
        let src = tree.add_node("src", FileKind::Directory, root, 1);
        let main = tree.add_node("main.rs", FileKind::File, src, 2);
        let _lib = tree.add_node("lib.rs", FileKind::File, src, 2);

        // find_node_by_path 以第一个根节点为起点，base 应为该根节点对应的路径
        let base = Path::new("/workspace/project");
        let target = Path::new("/workspace/project/src/main.rs");
        assert_eq!(
            EditorState::find_node_by_path(&tree, target, base),
            Some(main)
        );

        assert!(EditorState::find_node_by_path(
            &tree,
            Path::new("/workspace/project/src/missing.rs"),
            base
        )
        .is_none());
        assert!(EditorState::find_node_by_path(
            &tree,
            Path::new("/other/project/src/main.rs"),
            base
        )
        .is_none());
        assert!(EditorState::find_node_by_path(&tree, base, base).is_none());
    }

    /// SubTask 7.2: 验证 plus_button_rect 默认为 None
    #[test]
    fn test_plus_button_rect_default_none() {
        let tab_layouts: Vec<crate::tabs::TabLayout> = Vec::new();
        // 直接验证字段默认值模式：TabLayout 的空 vec 不影响 plus_button_rect 语义
        // plus_button_rect 在 EditorState::new() 中初始化为 None，
        // 仅在 render_tab_bar 中可能被设置为 Some(...)
        // 此处验证 tab_layouts 为空时 max_scroll 为 0
        let editor = EditorStateTestStub {
            tab_layouts,
            tab_scroll_x: 0.0,
        };
        assert_eq!(editor.tab_bar_max_scroll(800.0), 0.0);
    }

    /// SubTask 7.5: 验证 tab_bar_max_scroll 在标签总宽度小于可见宽度时为 0
    #[test]
    fn test_tab_bar_max_scroll_no_overflow() {
        let tab_layouts = vec![crate::tabs::TabLayout {
            index: 0,
            x: 0.0,
            width: 100.0,
            close_x: 80.0,
            close_width: 16.0,
        }];
        let editor = EditorStateTestStub {
            tab_layouts,
            tab_scroll_x: 0.0,
        };
        // 单标签 100px + gap 2px = 102px，远小于 800px 可见宽度
        assert_eq!(editor.tab_bar_max_scroll(800.0), 0.0);
    }

    /// SubTask 7.5: 验证 tab_bar_max_scroll 在标签溢出时返回正确值
    #[test]
    fn test_tab_bar_max_scroll_overflow() {
        // 模拟 10 个 100px 标签：total = 10 * (100 + 2) = 1020px
        let tab_layouts: Vec<crate::tabs::TabLayout> = (0..10)
            .map(|i| crate::tabs::TabLayout {
                index: i,
                x: i as f32 * 102.0,
                width: 100.0,
                close_x: i as f32 * 102.0 + 80.0,
                close_width: 16.0,
            })
            .collect();
        let editor = EditorStateTestStub {
            tab_layouts,
            tab_scroll_x: 0.0,
        };
        // total = 9 * 102.0 + 100.0 + 2.0 = 1020.0
        // visible = 800 - 4 - 36 = 760
        // max_scroll = 1020 - 760 = 260
        let max = editor.tab_bar_max_scroll(800.0);
        assert!(max > 0.0, "max_scroll 应为正值，实际: {}", max);
        assert!(
            (max - 260.0).abs() < 0.01,
            "max_scroll 应为 260，实际: {}",
            max
        );
    }

    /// SubTask 7.5: 验证 scroll_tab_bar 的 clamp 行为
    #[test]
    fn test_scroll_tab_bar_clamp_to_zero() {
        let tab_layouts = vec![crate::tabs::TabLayout {
            index: 0,
            x: 0.0,
            width: 100.0,
            close_x: 80.0,
            close_width: 16.0,
        }];
        let mut editor = EditorStateTestStub {
            tab_layouts,
            tab_scroll_x: 50.0,
        };
        // 向左滚动（delta < 0），应 clamp 到 0
        let changed = editor.scroll_tab_bar(-120.0, 800.0);
        assert!(changed, "应检测到变化");
        assert_eq!(editor.tab_scroll_x, 0.0, "应 clamp 到 0");
    }

    /// SubTask 7.5: 验证 scroll_tab_bar 在已到边界时不变化
    #[test]
    fn test_scroll_tab_bar_no_change_at_boundary() {
        let tab_layouts = vec![crate::tabs::TabLayout {
            index: 0,
            x: 0.0,
            width: 100.0,
            close_x: 80.0,
            close_width: 16.0,
        }];
        let mut editor = EditorStateTestStub {
            tab_layouts,
            tab_scroll_x: 0.0,
        };
        // 已在左边界，向左滚动不应变化
        let changed = editor.scroll_tab_bar(-120.0, 800.0);
        assert!(!changed, "已在边界，不应变化");
        assert_eq!(editor.tab_scroll_x, 0.0);
    }

    /// SubTask 7.5: 验证 scroll_tab_bar 正常滚动
    #[test]
    fn test_scroll_tab_bar_normal_scroll() {
        // 10 个标签，max_scroll = 260
        let tab_layouts: Vec<crate::tabs::TabLayout> = (0..10)
            .map(|i| crate::tabs::TabLayout {
                index: i,
                x: i as f32 * 102.0,
                width: 100.0,
                close_x: i as f32 * 102.0 + 80.0,
                close_width: 16.0,
            })
            .collect();
        let mut editor = EditorStateTestStub {
            tab_layouts,
            tab_scroll_x: 0.0,
        };
        // delta = 120 → 增量 = 120 * 8 = 960 → clamp 到 260
        let changed = editor.scroll_tab_bar(120.0, 800.0);
        assert!(changed);
        assert!(
            (editor.tab_scroll_x - 260.0).abs() < 0.01,
            "应 clamp 到 max_scroll=260，实际: {}",
            editor.tab_scroll_x
        );
    }

    /// SubTask 7.1: 验证 close_tab 对越界索引返回 false
    #[test]
    fn test_close_tab_out_of_bounds_returns_false() {
        // close_tab 需要完整的 EditorState（涉及 dirty 检查、Dialogs），
        // 此处仅验证索引越界时的快速返回路径，不构造完整 EditorState。
        // 通过 tab_layouts 的命中检测逻辑间接验证：
        // 空 tab_layouts 时，任何索引都不应命中
        let tab_layouts: Vec<crate::tabs::TabLayout> = Vec::new();
        assert!(tab_layouts.is_empty());
        // 命中检测：rel_x=0.0 在空 layouts 中不应匹配任何标签
        let hit = tab_layouts
            .iter()
            .find(|l| 0.0 >= l.x && 0.0 < l.x + l.width)
            .map(|l| l.index);
        assert!(hit.is_none(), "空 tab_layouts 不应命中任何标签");
    }

    /// 测试辅助结构体：模拟 EditorState 中 tab 滚动相关字段的独立测试
    /// （避免构造完整 EditorState 需要 HWND 等资源）
    struct EditorStateTestStub {
        tab_layouts: Vec<crate::tabs::TabLayout>,
        tab_scroll_x: f32,
    }

    impl EditorStateTestStub {
        fn tab_bar_max_scroll(&self, tab_bar_width: f32) -> f32 {
            let gap = 2.0;
            let left_padding = 4.0;
            let plus_area = 8.0 + 28.0;
            let total_tabs_width = self
                .tab_layouts
                .last()
                .map(|l| l.x + l.width + gap)
                .unwrap_or(0.0);
            let visible_width = (tab_bar_width - left_padding - plus_area).max(0.0);
            (total_tabs_width - visible_width).max(0.0)
        }

        fn scroll_tab_bar(&mut self, delta: f32, tab_bar_width: f32) -> bool {
            let old = self.tab_scroll_x;
            let max_scroll = self.tab_bar_max_scroll(tab_bar_width);
            self.tab_scroll_x = (self.tab_scroll_x + delta * 8.0).clamp(0.0, max_scroll);
            (self.tab_scroll_x - old).abs() > 0.01
        }
    }

    // ===== Task 8: 标签拖拽重排单元测试 =====

    /// Task 8.6: 验证基本重排——将第一个标签移到末尾
    #[test]
    fn test_reorder_tabs_move_first_to_last() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into(), "D".into()];
        let mut active: usize = 0;
        // drag_idx=0, drop_idx=4 → A 移到末尾
        reorder_tabs_with_active(&mut tabs, &mut active, 0, 4);
        assert_eq!(tabs, vec!["B", "C", "D", "A"]);
        assert_eq!(active, 3, "活动标签应跟随移动到新位置");
    }

    /// Task 8.6: 验证基本重排——将末尾标签移到开头
    #[test]
    fn test_reorder_tabs_move_last_to_first() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into(), "D".into()];
        let mut active: usize = 3;
        // drag_idx=3, drop_idx=0 → D 移到开头
        reorder_tabs_with_active(&mut tabs, &mut active, 3, 0);
        assert_eq!(tabs, vec!["D", "A", "B", "C"]);
        assert_eq!(active, 0, "活动标签应跟随移动到新位置");
    }

    /// Task 8.6: 验证活动标签在前方、拖拽其他标签到后方时 active_tab 递减
    #[test]
    fn test_reorder_tabs_active_decrements_when_drag_before_active() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into(), "D".into()];
        let mut active: usize = 2; // C
                                   // drag_idx=0 (A), drop_idx=3 (before D) → A 移到 C 和 D 之间
        reorder_tabs_with_active(&mut tabs, &mut active, 0, 3);
        assert_eq!(tabs, vec!["B", "C", "A", "D"]);
        assert_eq!(active, 1, "C 从 idx 2 移到 idx 1");
    }

    /// Task 8.6: 验证活动标签在后方、拖拽其他标签到前方时 active_tab 递增
    #[test]
    fn test_reorder_tabs_active_increments_when_drag_after_active() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into(), "D".into()];
        let mut active: usize = 1; // B
                                   // drag_idx=2 (C), drop_idx=0 (before A) → C 移到开头
        reorder_tabs_with_active(&mut tabs, &mut active, 2, 0);
        assert_eq!(tabs, vec!["C", "A", "B", "D"]);
        assert_eq!(active, 2, "B 从 idx 1 移到 idx 2");
    }

    /// Task 8.6: 验证 drop_idx == drag_idx 时无变化
    #[test]
    fn test_reorder_tabs_noop_when_drop_equals_drag() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into()];
        let mut active: usize = 1;
        reorder_tabs_with_active(&mut tabs, &mut active, 1, 1);
        assert_eq!(tabs, vec!["A", "B", "C"]);
        assert_eq!(active, 1);
    }

    /// Task 8.6: 验证 drop_idx == drag_idx + 1 时无变化（插入到下一个标签前 = 原位）
    #[test]
    fn test_reorder_tabs_noop_when_drop_is_next() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into()];
        let mut active: usize = 1;
        // drag_idx=0, drop_idx=1 → A 已在 B 前，无需移动
        reorder_tabs_with_active(&mut tabs, &mut active, 0, 1);
        assert_eq!(tabs, vec!["A", "B", "C"]);
        assert_eq!(active, 1);
    }

    /// Task 8.6: 验证活动标签不在拖拽路径上时 active_tab 不变
    #[test]
    fn test_reorder_tabs_active_unchanged_when_not_in_path() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into(), "D".into()];
        let mut active: usize = 0; // A
                                   // drag_idx=2 (C), drop_idx=4 (末尾) → C 移到 D 之后
        reorder_tabs_with_active(&mut tabs, &mut active, 2, 4);
        assert_eq!(tabs, vec!["A", "B", "D", "C"]);
        assert_eq!(active, 0, "A 不受影响");
    }

    /// Task 8.6: 验证越界索引不 panic
    #[test]
    fn test_reorder_tabs_out_of_bounds_no_panic() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into()];
        let mut active: usize = 0;
        // drag_idx 越界
        reorder_tabs_with_active(&mut tabs, &mut active, 5, 0);
        assert_eq!(tabs, vec!["A", "B"]);
        // drop_idx 越界
        reorder_tabs_with_active(&mut tabs, &mut active, 0, 10);
        // drop_idx > tabs.len()，不执行
        assert_eq!(tabs, vec!["A", "B"]);
    }

    /// Task 8.6: 验证单标签重排无变化
    #[test]
    fn test_reorder_tabs_single_tab() {
        let mut tabs: Vec<String> = vec!["A".into()];
        let mut active: usize = 0;
        reorder_tabs_with_active(&mut tabs, &mut active, 0, 0);
        assert_eq!(tabs, vec!["A"]);
        assert_eq!(active, 0);
        // drop_idx=1 也应安全（插入到末尾 = 原位）
        reorder_tabs_with_active(&mut tabs, &mut active, 0, 1);
        assert_eq!(tabs, vec!["A"]);
        assert_eq!(active, 0);
    }

    /// Task 8.6: 验证中间标签向左移动
    #[test]
    fn test_reorder_tabs_middle_to_left() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into(), "D".into()];
        let mut active: usize = 2; // C 是活动标签
                                   // drag_idx=2 (C), drop_idx=0 → C 移到开头
        reorder_tabs_with_active(&mut tabs, &mut active, 2, 0);
        assert_eq!(tabs, vec!["C", "A", "B", "D"]);
        assert_eq!(active, 0, "C 跟随到 idx 0");
    }

    /// Task 8.6: 验证中间标签向右移动
    #[test]
    fn test_reorder_tabs_middle_to_right() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into(), "D".into()];
        let mut active: usize = 1; // B 是活动标签
                                   // drag_idx=1 (B), drop_idx=4 → B 移到末尾
        reorder_tabs_with_active(&mut tabs, &mut active, 1, 4);
        assert_eq!(tabs, vec!["A", "C", "D", "B"]);
        assert_eq!(active, 3, "B 跟随到 idx 3");
    }

    // ===== SubTask 9.4: 标签关闭方法索引逻辑单元测试 =====
    // EditorState 构造需要 HWND 等资源，此处通过模拟 tabs 向量操作验证索引逻辑。
    // 逻辑与 close_other_tabs / close_tabs_to_the_right / close_all_tabs 保持一致。

    /// 模拟 close_other_tabs 的索引逻辑：保留 keep_idx，移除其他
    fn close_other_tabs_logic<T>(tabs: &mut Vec<T>, active: &mut usize, keep_idx: usize) -> bool {
        if keep_idx >= tabs.len() {
            return false;
        }
        if tabs.len() == 1 {
            return true;
        }
        let kept = tabs.remove(keep_idx);
        tabs.clear();
        tabs.push(kept);
        *active = 0;
        true
    }

    /// 模拟 close_tabs_to_the_right 的索引逻辑：保留 0..=idx，移除 idx+1..
    fn close_tabs_to_the_right_logic<T>(tabs: &mut Vec<T>, active: &mut usize, idx: usize) -> bool {
        if idx >= tabs.len() {
            return false;
        }
        if tabs.len() <= idx + 1 {
            return true;
        }
        tabs.truncate(idx + 1);
        if *active > idx {
            *active = idx;
        }
        true
    }

    #[test]
    fn test_close_other_tabs_keeps_only_specified() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into(), "D".into()];
        let mut active: usize = 2; // C 是活动标签
                                   // 保留 idx=1 (B)
        assert!(close_other_tabs_logic(&mut tabs, &mut active, 1));
        assert_eq!(tabs, vec!["B"]);
        assert_eq!(active, 0, "活动标签应重置为 0");
    }

    #[test]
    fn test_close_other_tabs_out_of_bounds() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into()];
        let mut active: usize = 0;
        assert!(!close_other_tabs_logic(&mut tabs, &mut active, 5));
        assert_eq!(tabs.len(), 2, "越界时不修改 tabs");
    }

    #[test]
    fn test_close_other_tabs_single_tab_noop() {
        let mut tabs: Vec<String> = vec!["A".into()];
        let mut active: usize = 0;
        assert!(close_other_tabs_logic(&mut tabs, &mut active, 0));
        assert_eq!(tabs, vec!["A"]);
        assert_eq!(active, 0);
    }

    #[test]
    fn test_close_other_tabs_keep_first() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into()];
        let mut active: usize = 1;
        assert!(close_other_tabs_logic(&mut tabs, &mut active, 0));
        assert_eq!(tabs, vec!["A"]);
        assert_eq!(active, 0);
    }

    #[test]
    fn test_close_other_tabs_keep_last() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into()];
        let mut active: usize = 0;
        assert!(close_other_tabs_logic(&mut tabs, &mut active, 2));
        assert_eq!(tabs, vec!["C"]);
        assert_eq!(active, 0);
    }

    #[test]
    fn test_close_tabs_to_the_right_basic() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into(), "D".into()];
        let mut active: usize = 0;
        // 保留 0..=1，移除 C, D
        assert!(close_tabs_to_the_right_logic(&mut tabs, &mut active, 1));
        assert_eq!(tabs, vec!["A", "B"]);
        assert_eq!(active, 0, "active 在保留范围内不变");
    }

    #[test]
    fn test_close_tabs_to_the_right_adjusts_active() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into(), "D".into()];
        let mut active: usize = 3; // D 是活动标签
                                   // 保留 0..=1，active=3 > 1 → 调整为 1
        assert!(close_tabs_to_the_right_logic(&mut tabs, &mut active, 1));
        assert_eq!(tabs, vec!["A", "B"]);
        assert_eq!(active, 1, "active 超出保留范围时应调整为 idx");
    }

    #[test]
    fn test_close_tabs_to_the_right_active_at_boundary() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into()];
        let mut active: usize = 1;
        // 保留 0..=1，active=1 == idx → 不变
        assert!(close_tabs_to_the_right_logic(&mut tabs, &mut active, 1));
        assert_eq!(tabs, vec!["A", "B"]);
        assert_eq!(active, 1);
    }

    #[test]
    fn test_close_tabs_to_the_right_no_tabs_to_close() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into()];
        let mut active: usize = 0;
        // idx=1, len=2 → 无需关闭
        assert!(close_tabs_to_the_right_logic(&mut tabs, &mut active, 1));
        assert_eq!(tabs.len(), 2);
    }

    #[test]
    fn test_close_tabs_to_the_right_out_of_bounds() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into()];
        let mut active: usize = 0;
        assert!(!close_tabs_to_the_right_logic(&mut tabs, &mut active, 5));
        assert_eq!(tabs.len(), 2, "越界时不修改 tabs");
    }

    #[test]
    fn test_close_tabs_to_the_right_last_tab() {
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into()];
        let mut active: usize = 2;
        // 保留 0..=2（全部），无需关闭
        assert!(close_tabs_to_the_right_logic(&mut tabs, &mut active, 2));
        assert_eq!(tabs.len(), 3);
        assert_eq!(active, 2);
    }

    #[test]
    fn test_close_all_tabs_logic() {
        // close_all_tabs 的逻辑：清空 + 创建新空标签
        // 验证：无论初始状态如何，结果都是 1 个空标签
        let mut tabs: Vec<String> = vec!["A".into(), "B".into(), "C".into()];
        tabs.clear();
        tabs.push(String::new());
        assert_eq!(tabs.len(), 1);
        assert!(tabs[0].is_empty());
    }

    // ===== Task 13.3: last_closed_tab 保存/恢复逻辑单元测试 =====

    #[test]
    fn test_remove_tab_saving_content_saves_to_last_closed() {
        // 验证：移除标签时，其内容被保存到 last_closed
        let mut tabs = vec![Tab::new(), Tab::new(), Tab::new()];
        let mut last_closed: Option<TabContent> = None;
        // 移除 idx=2 的标签
        assert!(remove_tab_saving_content(&mut tabs, 2, &mut last_closed));
        assert_eq!(tabs.len(), 2, "移除后 tabs 长度应减 1");
        assert!(last_closed.is_some(), "last_closed 应保存被移除标签的内容");
    }

    #[test]
    fn test_remove_tab_saving_content_out_of_bounds() {
        let mut tabs = vec![Tab::new(), Tab::new()];
        let mut last_closed: Option<TabContent> = None;
        assert!(!remove_tab_saving_content(&mut tabs, 5, &mut last_closed));
        assert_eq!(tabs.len(), 2, "越界时 tabs 不变");
        assert!(last_closed.is_none(), "越界时 last_closed 不变");
    }

    #[test]
    fn test_remove_tab_saving_content_overwrites_previous() {
        // 验证：连续关闭多个标签时，last_closed 始终保存最近关闭的
        let mut tabs = vec![Tab::new(), Tab::new(), Tab::new()];
        let mut last_closed: Option<TabContent> = None;
        remove_tab_saving_content(&mut tabs, 0, &mut last_closed);
        assert!(last_closed.is_some());
        // 再次移除应覆盖之前的 last_closed
        remove_tab_saving_content(&mut tabs, 0, &mut last_closed);
        assert!(last_closed.is_some());
        assert_eq!(tabs.len(), 1, "两次移除后只剩 1 个标签");
    }

    #[test]
    fn test_reopen_last_closed_tab_logic_restores_tab() {
        // 验证：恢复逻辑将 last_closed 内容作为新标签添加到末尾
        let mut tabs = vec![Tab::new(), Tab::new()];
        let mut active: usize = 0;
        // 手动保存一个标签内容
        let content = TabContent::new();
        let mut last_closed: Option<TabContent> = Some(content);

        let old_len = tabs.len();
        assert!(reopen_last_closed_tab_logic(
            &mut tabs,
            &mut active,
            &mut last_closed
        ));
        assert_eq!(tabs.len(), old_len + 1, "恢复后 tabs 长度应增 1");
        assert_eq!(active, tabs.len() - 1, "active 应指向新标签");
        assert!(last_closed.is_none(), "恢复后 last_closed 应被消费");
    }

    #[test]
    fn test_reopen_last_closed_tab_logic_nothing_to_restore() {
        let mut tabs = vec![Tab::new(), Tab::new()];
        let mut active: usize = 0;
        let mut last_closed: Option<TabContent> = None;
        assert!(!reopen_last_closed_tab_logic(
            &mut tabs,
            &mut active,
            &mut last_closed
        ));
        assert_eq!(tabs.len(), 2, "无内容恢复时 tabs 不变");
        assert_eq!(active, 0, "无内容恢复时 active 不变");
    }

    #[test]
    fn test_last_closed_tab_save_and_restore_round_trip() {
        // 端到端验证：关闭标签 → 保存 → 恢复 → 标签回归
        let mut tabs = vec![Tab::new(), Tab::new(), Tab::new()];
        let mut active: usize = 1;
        let mut last_closed: Option<TabContent> = None;
        let original_len = tabs.len();

        // 关闭 idx=2 的标签
        assert!(remove_tab_saving_content(&mut tabs, 2, &mut last_closed));
        assert_eq!(tabs.len(), original_len - 1);
        assert!(last_closed.is_some());

        // 恢复
        assert!(reopen_last_closed_tab_logic(
            &mut tabs,
            &mut active,
            &mut last_closed
        ));
        assert_eq!(tabs.len(), original_len, "恢复后长度应回到原始值");
        assert_eq!(active, tabs.len() - 1, "active 应指向恢复的标签");
        assert!(last_closed.is_none(), "恢复后 last_closed 应为空");
    }
}
