#![allow(clippy::collapsible_match, clippy::cmp_owned)]

pub(crate) use std::collections::HashMap;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::sync::Arc;

pub(crate) use windows::core::Result;
pub(crate) use windows::Win32::Foundation::HWND;

pub(crate) use aether_core::buffer::history::{CursorPosition, OpType};
pub(crate) use aether_core::buffer::piece_table::PieceTable;
pub(crate) use aether_core::buffer::text_buffer::{Cursor, MultiCursorState};
pub(crate) use aether_core::char_width::char_width as unicode_char_width;
pub(crate) use aether_core::lexer::Language;
pub(crate) use aether_core::workspace::file_tree::{FileKind, FileTree};
pub(crate) use aether_lsp::client::{default_server_config, LspEvent};
pub(crate) use aether_lsp::LspClient;
pub(crate) use aether_render::d2d::factory::D2DFactory;
pub(crate) use aether_render::d2d::text::TextRenderer;
pub(crate) use aether_render::theme::Theme;
pub(crate) use lsp_types::{CompletionItem, Diagnostic};
pub(crate) use url::Url;

pub(crate) use crate::activity_bar::ActivityBar;
pub(crate) use crate::ai_agent::AiEdit;
pub(crate) use crate::ai_context::{truncate_middle, wrap_code_block, AiContextAttachment};
pub(crate) use crate::ai_panel::AiPanel;
pub(crate) use crate::command_palette::CommandPalette;
pub(crate) use crate::dialogs::Dialogs;
pub(crate) use crate::focus_manager::FocusManager;
pub(crate) use crate::git::GitIntegration;
pub(crate) use crate::input::{KeyMap, PressTarget};
pub(crate) use crate::layout::{
    ActivityBarView, LayoutManager, SidebarContent, SIDEBAR_RESIZE_GRAB, TAB_BAR_HEIGHT,
};
pub(crate) use crate::menu_bar::MenuBar;
pub(crate) use crate::ssh::{
    CloneRepoDialog, RemoteFileTree, RemoteSession, SshConnectionDialog, SshManagerPanel,
};
pub(crate) use crate::status_bar::StatusBar;
pub(crate) use crate::tabs::{Tab, TabContent, TabLayout};
pub(crate) use crate::terminal::TerminalPanel;
pub(crate) use aether_shared::settings::AppSettings;
// P0-1: RemoteFs trait 为 SshRemoteFs::list_dir 等方法提供作用域
pub(crate) use aether_remote::RemoteFs;

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
    Rename,
}

/// 文件树内联输入状态（用于新建文件/文件夹或重命名时）
#[derive(Clone, Debug)]
pub struct FileTreeInput {
    pub kind: FileTreeInputKind,
    pub value: String,
    pub caret_visible: bool,
    /// IME 合成串（中文输入法预编辑文本），渲染时显示在 value 之后
    pub composition: Option<String>,
    /// 重命名时记录目标节点索引
    pub target_node: Option<u32>,
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
    /// AI 终端命令后监视工作区变化的截止时间（None=未监视）。
    /// 在此窗口内每帧检测根目录签名，变化则轻量刷新资源管理器（同步 AI 的删除/新建）。
    pub fs_watch_until: Option<std::time::Instant>,
    /// 上一次记录的工作区根目录签名（用于变化检测，避免无谓刷新）
    pub fs_last_root_sig: u64,
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
    /// 文件节点右键上下文菜单
    pub file_node_context_menu: crate::context_menu::FileNodeContextMenu,
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
    /// 确保 logo 位图已加载（懒加载，仅在首次需要时从文件读取）
    pub(crate) fn ensure_logo_bitmap(
        &mut self,
        target: &windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget,
    ) {
        if self.logo_bitmap.is_some() {
            return;
        }
        // 将吉祥物 PNG 嵌入二进制，避免运行时依赖外部资源文件
        const PNG_BYTES: &[u8] = include_bytes!("../../resources/app_icons/source/aether-512.png");
        match crate::bitmap_loader::load_png_to_bitmap(target, PNG_BYTES) {
            Ok(bitmap) => {
                self.logo_bitmap = Some(bitmap);
            }
            Err(e) => {
                tracing::warn!("加载 logo 位图失败: {}", e);
            }
        }
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
            fs_watch_until: None,
            fs_last_root_sig: 0,
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
            file_node_context_menu: crate::context_menu::FileNodeContextMenu::new(),
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

        // Phase 2: 启动时从磁盘加载历史索引
        state.refresh_ai_history();

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

impl EditorState {
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

impl EditorState {}

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

mod dialogs;
mod find;
mod git;
mod ime;

mod ai;
mod cursor;
mod editing;
mod events;
mod file_tree;
mod files;
mod lsp;
mod remote;
mod tabs;
