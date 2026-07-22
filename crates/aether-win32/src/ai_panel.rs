use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use aether_ai::{AiClient, AiStreamEvent, ChatMessage};
use aether_shared::settings::AiSettings;

use crate::ai_context::{truncate_middle, AiContextAttachment};
use crate::ai_prompt::{build_chat_prompt, AiMode};
use crate::editor::EditorState;

/// 脱敏错误消息，避免泄漏 API 密钥等敏感信息
/// SEC-C04: 用于 test_connection 路径等所有 UI 错误展示
/// AI-M04: 扩展覆盖 x-api-key、URL 参数、响应体中的密钥
/// H-02: 循环移除所有 Bearer/x-api-key/authorization 出现，而非仅首个
///
/// 注意：当前代码路径已改用 `AiError::safe_display()`，此函数保留供
/// 需要对原始字符串（如日志）做脱敏的场景使用。
#[allow(dead_code)]
pub fn sanitize_error(err: &str) -> String {
    let mut result = err.to_string();

    // H-02: 循环移除所有 "Bearer xxx" 出现（之前仅处理首个，多 Token 时第二个泄露）
    while let Some(pos) = result.find("Bearer ") {
        let start = pos + 7;
        let end = result[start..]
            .find(|c: char| c.is_whitespace() || c == '\n' || c == '\r')
            .map(|p| start + p)
            .unwrap_or(result.len());
        if end > start {
            result.replace_range(start..end, "[REDACTED]");
        } else {
            break;
        }
    }
    // H-02: 循环移除所有 x-api-key 头（支持冒号和等号分隔，大小写不敏感）
    let lower = result.to_lowercase();
    let mut search_from = 0;
    while let Some(rel_pos) = lower[search_from..].find("x-api-key") {
        let pos = search_from + rel_pos;
        // 跳过 "x-api-key" 本身（9 字符）
        let mut value_start = pos + 9;
        // 跳过分隔符（: 或 =）和可选空格
        let rest = &result[value_start..];
        let trimmed_start = rest
            .find(|c: char| c != ':' && c != '=' && c != ' ' && c != '\t')
            .map(|p| value_start + p)
            .unwrap_or(value_start);
        value_start = trimmed_start;
        let end = result[value_start..]
            .find(|c: char| ['\n', '\r'].contains(&c))
            .map(|p| value_start + p)
            .unwrap_or(result.len());
        if end > value_start {
            result.replace_range(value_start..end, "[REDACTED]");
        }
        search_from = pos + 9;
        if search_from >= result.len() {
            break;
        }
    }

    // H-02: 循环移除所有 authorization 头（大小写不敏感）
    let lower = result.to_lowercase();
    let mut search_from = 0;
    while let Some(rel_pos) = lower[search_from..].find("authorization") {
        let pos = search_from + rel_pos;
        let mut value_start = pos + 13; // "authorization" = 13 字符
        let rest = &result[value_start..];
        let trimmed_start = rest
            .find(|c: char| ![':', '=', ' ', '\t'].contains(&c))
            .map(|p| value_start + p)
            .unwrap_or(value_start);
        value_start = trimmed_start;
        let end = result[value_start..]
            .find(|c: char| ['\n', '\r'].contains(&c))
            .map(|p| value_start + p)
            .unwrap_or(result.len());
        if end > value_start {
            result.replace_range(value_start..end, "[REDACTED]");
        }
        search_from = pos + 13;
        if search_from >= result.len() {
            break;
        }
    }

    // 限制长度（H-02: 在 UTF-8 字符边界截断，避免半截 Token 可见）
    if result.len() > 500 {
        let safe_len = result.floor_char_boundary(500);
        result.truncate(safe_len);
        result.push_str("...");
    }
    result
}

/// AI 助手消息
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AiMessage {
    pub role: AiRole,
    pub content: String,
    /// "深度思考"内容（DeepSeek reasoner 的 reasoning_content）；None 表示无思考。
    /// 与 content 分离存储，UI 上作为独立的"思考过程"分类展示。
    pub reasoning: Option<String>,
    /// 思考块是否折叠（默认展开，生成完成后自动折叠；用户可点击标题切换）
    #[serde(default)]
    pub reasoning_collapsed: bool,
}

impl AiMessage {
    pub fn new(role: AiRole, content: String) -> Self {
        Self {
            role,
            content,
            reasoning: None,
            reasoning_collapsed: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiRole {
    User,
    Assistant,
    System,
}

/// 流式响应的共享状态
#[derive(Clone, Debug, Default)]
pub struct AiStreamState {
    /// 已累积但尚未被 UI 取走的 token（最终回答）
    pub partial: String,
    /// 已累积但尚未被 UI 取走的"深度思考"内容（DeepSeek reasoning_content 等）
    pub reasoning: String,
    /// 流是否已结束
    pub done: bool,
    /// 流式过程中发生的错误
    pub error: Option<String>,
}

/// AI 助手欢迎语（新对话初始系统消息）
pub const AI_WELCOME: &str = "你好！我是 AI 助手，可以帮助你解释代码、重构、修复问题、生成测试等。你可以直接输入问题，或选中代码后使用快捷操作。";

/// 当前 Unix 秒级时间戳（对话创建/更新时间）
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 生成对话 ID（时间戳毫秒 + 计数，保证唯一）
pub fn gen_conversation_id() -> String {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("conv-{}-{}", ms, n)
}

/// 单个 AI 对话会话（多标签页 + 历史记录的基本单元）。
///
/// 活动会话的实时状态保存在 `AiPanel` 的扁平字段中（沿用旧逻辑，避免大面积改动）；
/// 非活动会话以本结构存放于 `AiPanel::conversations`，可在后台并发流式生成。
#[derive(Clone, Debug)]
pub struct AiConversation {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub messages: Vec<AiMessage>,
    pub input: String,
    pub caret_pos: usize,
    pub composition: Option<String>,
    pub is_generating: bool,
    pub scroll_y: f32,
    pub content_height: f32,
    pub stick_to_bottom: bool,
    pub mode: AiMode,
    pub attachments: Vec<AiContextAttachment>,
    pub stream_state: Arc<Mutex<AiStreamState>>,
    pub should_stop: Arc<AtomicBool>,
    /// 本轮注入过的 playbook 条目 ID（用于反馈归因）
    pub used_bullet_ids: Vec<String>,
}

impl AiConversation {
    pub fn new(id: String, title: String) -> Self {
        let now = now_secs();
        Self {
            id,
            title,
            created_at: now,
            updated_at: now,
            messages: vec![AiMessage::new(AiRole::System, AI_WELCOME.to_string())],
            input: String::new(),
            caret_pos: 0,
            composition: None,
            is_generating: false,
            scroll_y: 0.0,
            content_height: 0.0,
            stick_to_bottom: true,
            mode: AiMode::Agent,
            attachments: Vec::new(),
            stream_state: Arc::new(Mutex::new(AiStreamState::default())),
            should_stop: Arc::new(AtomicBool::new(false)),
            used_bullet_ids: Vec::new(),
        }
    }

    fn add_assistant_message(&mut self, content: String) {
        self.messages
            .push(AiMessage::new(AiRole::Assistant, content));
        self.stick_to_bottom = true;
        self.updated_at = now_secs();
    }

    /// 最后一条助手消息文本
    pub fn last_assistant_text(&self) -> Option<String> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == AiRole::Assistant)
            .map(|m| m.content.clone())
    }

    /// 首条用户消息（用于自动生成标题）
    pub fn first_user_text(&self) -> Option<String> {
        self.messages
            .iter()
            .find(|m| m.role == AiRole::User)
            .map(|m| m.content.clone())
    }

    /// 后台（非活动）会话的流式轮询：把新 token 追加到消息，返回本帧是否刚完成。
    /// 与 `AiPanel::check_background_result` 逻辑一致，但作用于本会话，支持并发。
    pub fn drain_background(&mut self) -> bool {
        if !self.is_generating {
            return false;
        }
        let delta = if let Ok(mut s) = self.stream_state.lock() {
            let partial = std::mem::take(&mut s.partial);
            let reasoning = std::mem::take(&mut s.reasoning);
            let done = s.done;
            let error = s.error.take();
            if done {
                s.done = false;
            }
            Some((partial, reasoning, done, error))
        } else {
            None
        };
        let mut just_completed = false;
        if let Some((partial, reasoning, done, error)) = delta {
            // 深度思考通常先于回答到达：确保有一条助手消息承载 reasoning
            if !reasoning.is_empty() {
                if !matches!(self.messages.last(), Some(m) if m.role == AiRole::Assistant) {
                    self.messages
                        .push(AiMessage::new(AiRole::Assistant, String::new()));
                }
                if let Some(last) = self.messages.last_mut() {
                    last.reasoning
                        .get_or_insert_with(String::new)
                        .push_str(&reasoning);
                }
                self.stick_to_bottom = true;
                self.updated_at = now_secs();
            }
            if !partial.is_empty() {
                self.stick_to_bottom = true;
                if !matches!(self.messages.last(), Some(m) if m.role == AiRole::Assistant) {
                    self.messages
                        .push(AiMessage::new(AiRole::Assistant, String::new()));
                }
                if let Some(last) = self.messages.last_mut() {
                    last.content.push_str(&partial);
                }
                self.updated_at = now_secs();
            }
            if let Some(err) = error {
                self.add_assistant_message(err);
                self.is_generating = false;
                return false;
            }
            if done {
                self.is_generating = false;
                // 生成完成：自动折叠思考块，保持界面整洁
                if let Some(last) = self.messages.last_mut() {
                    if last.role == AiRole::Assistant && last.reasoning.is_some() {
                        last.reasoning_collapsed = true;
                    }
                }
                just_completed = true;
            }
        }
        just_completed
    }
}

/// 历史记录轻量元数据（懒加载：列表只用元数据，点击时才读完整会话）
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    pub updated_at: u64,
    pub message_count: usize,
    pub preview: String,
    /// 会话模式（"Ask" / "Agent"；旧数据可能为空串）
    #[serde(default)]
    pub mode: String,
}

/// 历史记录时间筛选
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HistoryTimeFilter {
    /// 全部
    All,
    /// 最近 24 小时
    Today,
    /// 最近 7 天
    Week,
    /// 最近 30 天
    Month,
}

impl HistoryTimeFilter {
    pub const ALL: [HistoryTimeFilter; 4] = [
        HistoryTimeFilter::All,
        HistoryTimeFilter::Today,
        HistoryTimeFilter::Week,
        HistoryTimeFilter::Month,
    ];

    pub fn label(self) -> &'static str {
        match self {
            HistoryTimeFilter::All => "全部",
            HistoryTimeFilter::Today => "今天",
            HistoryTimeFilter::Week => "本周",
            HistoryTimeFilter::Month => "本月",
        }
    }

    /// 时间下限（Unix 秒）；None 表示不限
    pub fn cutoff(self, now: u64) -> Option<u64> {
        match self {
            HistoryTimeFilter::All => None,
            HistoryTimeFilter::Today => Some(now.saturating_sub(24 * 3600)),
            HistoryTimeFilter::Week => Some(now.saturating_sub(7 * 24 * 3600)),
            HistoryTimeFilter::Month => Some(now.saturating_sub(30 * 24 * 3600)),
        }
    }
}

/// 历史列表可选的类型筛选项（None = 全部）
pub const HISTORY_TYPE_FILTERS: [Option<&str>; 3] = [None, Some("Ask"), Some("Agent")];

/// 历史列表每页条数
pub const HISTORY_PAGE_SIZE: usize = 6;

/// 相对时间显示（历史列表用）
pub fn relative_time(updated_at: u64, now: u64) -> String {
    let d = now.saturating_sub(updated_at);
    if d < 60 {
        "刚刚".to_string()
    } else if d < 3600 {
        format!("{} 分钟前", d / 60)
    } else if d < 86400 {
        format!("{} 小时前", d / 3600)
    } else if d < 7 * 86400 {
        format!("{} 天前", d / 86400)
    } else if d < 30 * 86400 {
        format!("{} 周前", d / (7 * 86400))
    } else {
        format!("{} 个月前", d / (30 * 86400))
    }
}

/// AI 助手面板状态
#[derive(Debug)]
pub struct AiPanel {
    /// 是否可见
    pub visible: bool,
    /// 聊天历史
    pub messages: Vec<AiMessage>,
    /// 当前输入
    pub input: String,
    /// 是否正在生成回复
    pub is_generating: bool,
    /// 滚动偏移
    pub scroll_y: f32,
    /// Apply 按钮悬停状态
    pub hover_apply_button: bool,
    /// AI-H01: 后台线程流式状态，UI 渲染时轮询此字段
    pub stream_state: Arc<Mutex<AiStreamState>>,
    /// C-10: 输入框是否聚焦。仅当聚焦时才拦截键盘输入，避免面板可见即劫持编辑器
    pub input_focused: bool,
    /// 当前 AI 模式（Ask / Agent）
    pub mode: AiMode,
    /// 底部工具栏"当前模型"下拉是否展开（在对话框内切换当前使用的模型）
    pub model_menu_open: bool,
    /// 已附加的上下文项
    pub attachments: Vec<AiContextAttachment>,
    /// 模式切换按钮命中区域 (mode, x, y, w, h)
    pub mode_button_regions: Vec<(AiMode, f32, f32, f32, f32)>,
    /// 附件 chip 命中区域 (index, x, y, w, h)
    pub attachment_chip_regions: Vec<(usize, f32, f32, f32, f32)>,
    /// 悬停的附件 chip 索引
    pub hover_attachment: Option<usize>,
    /// 上一帧渲染的消息内容总高度（用于滚动条与自动滚底）
    pub content_height: f32,
    /// 代码块保存按钮区域 (msg_index, seg_index, x, y, w, h, suggested_filename)
    pub code_save_regions: Vec<(usize, usize, f32, f32, f32, f32, String)>,
    /// 是否吸附底部：新消息/流式到达时自动滚动到底部
    pub stick_to_bottom: bool,
    /// 输入框光标位置（字符索引，0 = 开头）
    pub caret_pos: usize,
    /// 输入框光标可见状态（闪烁，由 CARET_TIMER 切换）
    pub caret_visible: bool,
    /// IME 合成串（中文输入法预编辑文本），渲染时显示在 input 之后
    pub composition: Option<String>,
    /// 停止生成标志：后台流式线程在下一次循环检查时退出
    pub should_stop: Arc<AtomicBool>,
    /// 全部对话会话（多标签页）。活动会话的实时数据在上面的扁平字段中；
    /// conversations[active] 作为槽位，其 id/title/时间戳为权威值，切换时回写消息等数据。
    pub conversations: Vec<AiConversation>,
    /// 当前活动会话下标
    pub active: usize,
    /// 对话标签命中区 (conv_index, x, y, w, h)
    pub tab_regions: Vec<(usize, f32, f32, f32, f32)>,
    /// 标签关闭按钮命中区 (conv_index, x, y, w, h)
    pub tab_close_regions: Vec<(usize, f32, f32, f32, f32)>,
    /// "新建对话"按钮命中区
    pub new_tab_region: Option<(f32, f32, f32, f32)>,
    /// "历史记录"按钮命中区
    pub history_button_region: Option<(f32, f32, f32, f32)>,
    /// 悬停的标签下标
    pub hover_tab: Option<usize>,
    /// 是否展开历史记录列表
    pub history_open: bool,
    /// 历史记录条目命中区 (history_index, x, y, w, h)
    pub history_item_regions: Vec<(usize, f32, f32, f32, f32)>,
    /// 历史索引（懒加载：仅元数据，点击时才读取完整会话）
    pub history: Vec<ConversationMeta>,
    /// 思考块折叠切换命中区 (msg_index, x, y, w, h)（作用于活动会话 messages 索引）
    pub reasoning_toggle_regions: Vec<(usize, f32, f32, f32, f32)>,
    /// 热数据持久化存储（三阶段架构：热/温）
    pub hot_data_store: Option<crate::ai_hot_data::HotDataStore>,
    /// 温数据持久化存储（MemoryStore：SQLite + sqlite-vec）
    pub warm_data_store: Option<crate::ai_warm_data::WarmDataStore>,
    /// 历史列表：仅显示当前工作区的会话
    pub history_workspace_only: bool,
    /// Playbook 管理面板是否展开
    pub playbook_open: bool,
    /// Playbook 面板条目缓存（展开时从 SQLite 加载）
    pub playbook_items: Vec<crate::memory_store::PlaybookBullet>,
    /// Playbook 标题栏按钮命中区 (x, y, w, h)
    pub playbook_button_region: Option<(f32, f32, f32, f32)>,
    /// Playbook 条目删除按钮命中区 (item_index, x, y, w, h)
    pub playbook_delete_regions: Vec<(usize, f32, f32, f32, f32)>,
    /// 历史面板「仅当前工作区」开关命中区 (x, y, w, h)
    pub history_ws_toggle_region: Option<(f32, f32, f32, f32)>,
    /// 历史列表当前页码（0 起）
    pub history_page: usize,
    /// 历史时间筛选
    pub history_time_filter: HistoryTimeFilter,
    /// 历史类型筛选（None=全部，否则匹配 mode 字符串，如 "Ask"/"Agent"）
    pub history_type_filter: Option<String>,
    /// 历史详情视图：当前查看的会话 id（Some 时历史面板显示详情而非列表）
    pub history_detail_id: Option<String>,
    /// 历史详情缓存的完整会话（懒加载）
    pub history_detail_conv: Option<AiConversation>,
    /// 历史条目删除按钮命中区 (history_index, x, y, w, h)
    pub history_delete_regions: Vec<(usize, f32, f32, f32, f32)>,
    /// 历史分页「上一页」命中区
    pub history_page_prev_region: Option<(f32, f32, f32, f32)>,
    /// 历史分页「下一页」命中区
    pub history_page_next_region: Option<(f32, f32, f32, f32)>,
    /// 历史时间筛选按钮命中区 (HistoryTimeFilter::ALL 下标, x, y, w, h)
    pub history_time_filter_regions: Vec<(usize, f32, f32, f32, f32)>,
    /// 历史类型筛选按钮命中区 (HISTORY_TYPE_FILTERS 下标, x, y, w, h)
    pub history_type_filter_regions: Vec<(usize, f32, f32, f32, f32)>,
    /// 「清空全部」按钮命中区
    pub history_clear_all_region: Option<(f32, f32, f32, f32)>,
    /// 详情视图「返回」按钮命中区
    pub history_detail_back_region: Option<(f32, f32, f32, f32)>,
    /// 详情视图「恢复此对话」按钮命中区
    pub history_detail_restore_region: Option<(f32, f32, f32, f32)>,
    /// Agent 自动续跑轮次计数（用户手动发消息时重置；防止工具回环无限迭代）
    pub agent_iter_count: u32,
}

impl AiPanel {
    pub fn new() -> Self {
        let mut panel = Self {
            visible: false,
            messages: vec![AiMessage::new(AiRole::System, AI_WELCOME.to_string())],
            input: String::new(),
            is_generating: false,
            scroll_y: 0.0,
            hover_apply_button: false,
            stream_state: Arc::new(Mutex::new(AiStreamState::default())),
            input_focused: false,
            mode: AiMode::Agent,
            model_menu_open: false,
            attachments: Vec::new(),
            mode_button_regions: Vec::new(),
            attachment_chip_regions: Vec::new(),
            hover_attachment: None,
            content_height: 0.0,
            code_save_regions: Vec::new(),
            stick_to_bottom: true,
            caret_pos: 0,
            caret_visible: false,
            composition: None,
            should_stop: Arc::new(AtomicBool::new(false)),
            conversations: vec![AiConversation::new(
                gen_conversation_id(),
                "新对话".to_string(),
            )],
            active: 0,
            tab_regions: Vec::new(),
            tab_close_regions: Vec::new(),
            new_tab_region: None,
            history_button_region: None,
            hover_tab: None,
            history_open: false,
            history_item_regions: Vec::new(),
            history: Vec::new(),
            reasoning_toggle_regions: Vec::new(),
            hot_data_store: Self::init_hot_data_store(),
            warm_data_store: Self::init_warm_data_store(),
            history_workspace_only: false,
            playbook_open: false,
            playbook_items: Vec::new(),
            playbook_button_region: None,
            playbook_delete_regions: Vec::new(),
            history_ws_toggle_region: None,
            history_page: 0,
            history_time_filter: HistoryTimeFilter::All,
            history_type_filter: None,
            history_detail_id: None,
            history_detail_conv: None,
            history_delete_regions: Vec::new(),
            history_page_prev_region: None,
            history_page_next_region: None,
            history_time_filter_regions: Vec::new(),
            history_type_filter_regions: Vec::new(),
            history_clear_all_region: None,
            history_detail_back_region: None,
            history_detail_restore_region: None,
            agent_iter_count: 0,
        };
        panel.restore_latest_conversation();
        panel
    }

    /// 初始化热数据存储
    fn init_hot_data_store() -> Option<crate::ai_hot_data::HotDataStore> {
        let base_dir = dirs::config_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Aether")
            .join("conversations");
        match crate::ai_hot_data::HotDataStore::new(base_dir) {
            Ok(store) => Some(store),
            Err(e) => {
                eprintln!("[AiPanel] 热数据存储初始化失败: {}", e);
                None
            }
        }
    }

    /// 初始化温数据存储
    fn init_warm_data_store() -> Option<crate::ai_warm_data::WarmDataStore> {
        let base_dir = dirs::config_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Aether")
            .join("conversations");
        match crate::ai_warm_data::WarmDataStore::new(base_dir) {
            Ok(store) => Some(store),
            Err(e) => {
                eprintln!("[AiPanel] 温数据存储初始化失败: {}", e);
                None
            }
        }
    }

    /// 同步当前状态到热数据存储
    fn sync_hot_data(&mut self) {
        // 先 snapshot 到槽位，确保热数据看到的是完整状态
        self.snapshot_active_into_slot();
        if let Some(store) = self.hot_data_store.take() {
            let panel_clone = self.clone_for_sync();
            let mut new_store = store;
            new_store.sync_from_panel(panel_clone);
            self.hot_data_store = Some(new_store);
        }
    }

    /// 为热数据同步克隆必要字段（避免 Clone 整个 AiPanel）
    fn clone_for_sync(&self) -> crate::ai_panel::AiPanel {
        crate::ai_panel::AiPanel {
            visible: self.visible,
            messages: self.messages.clone(),
            input: self.input.clone(),
            is_generating: self.is_generating,
            scroll_y: self.scroll_y,
            hover_apply_button: self.hover_apply_button,
            stream_state: Arc::clone(&self.stream_state),
            input_focused: self.input_focused,
            mode: self.mode,
            model_menu_open: self.model_menu_open,
            attachments: self.attachments.clone(),
            mode_button_regions: self.mode_button_regions.clone(),
            attachment_chip_regions: self.attachment_chip_regions.clone(),
            hover_attachment: self.hover_attachment,
            content_height: self.content_height,
            code_save_regions: self.code_save_regions.clone(),
            stick_to_bottom: self.stick_to_bottom,
            caret_pos: self.caret_pos,
            caret_visible: self.caret_visible,
            composition: self.composition.clone(),
            should_stop: Arc::clone(&self.should_stop),
            conversations: self.conversations.clone(),
            active: self.active,
            tab_regions: self.tab_regions.clone(),
            tab_close_regions: self.tab_close_regions.clone(),
            new_tab_region: self.new_tab_region,
            history_button_region: self.history_button_region,
            hover_tab: self.hover_tab,
            history_open: self.history_open,
            history_item_regions: self.history_item_regions.clone(),
            history: self.history.clone(),
            reasoning_toggle_regions: self.reasoning_toggle_regions.clone(),
            hot_data_store: None,
            warm_data_store: None,
            history_workspace_only: self.history_workspace_only,
            playbook_open: self.playbook_open,
            playbook_items: Vec::new(),
            playbook_button_region: None,
            playbook_delete_regions: Vec::new(),
            history_ws_toggle_region: None,
            history_page: self.history_page,
            history_time_filter: self.history_time_filter,
            history_type_filter: self.history_type_filter.clone(),
            history_detail_id: None,
            history_detail_conv: None,
            history_delete_regions: Vec::new(),
            history_page_prev_region: None,
            history_page_next_region: None,
            history_time_filter_regions: Vec::new(),
            history_type_filter_regions: Vec::new(),
            history_clear_all_region: None,
            history_detail_back_region: None,
            history_detail_restore_region: None,
            agent_iter_count: 0,
        }
    }

    /// 触发温数据归档（空闲时调用）
    pub fn trigger_warm_archive(&mut self) {
        // 1. 先收割归档结果：后台线程异步完成，结果在后续调用中才就绪
        let results = self
            .warm_data_store
            .as_ref()
            .map(|s| s.poll_results())
            .unwrap_or_default();
        for result in results {
            match result {
                crate::ai_warm_data::ArchiveResult::Success { conv_id } => {
                    if let Some(hot_store) = self.hot_data_store.as_mut() {
                        hot_store.clear_dirty(&conv_id);
                    }
                    if let Some(warm_store) = self.warm_data_store.as_ref() {
                        warm_store.request_remove_hot_log(conv_id);
                    }
                }
                crate::ai_warm_data::ArchiveResult::Failed { conv_id, error } => {
                    eprintln!("[AiPanel] 归档失败 {}: {}", conv_id, error);
                }
            }
        }

        // 2. 空闲且有脏会话时发起新一轮归档
        if let Some(hot_store) = self.hot_data_store.as_mut() {
            if hot_store.should_warm_archive() {
                let dirty_sessions: Vec<crate::ai_panel::AiConversation> = hot_store
                    .dirty_sessions()
                    .iter()
                    .map(|c| (*c).clone())
                    .collect();
                if let Some(warm_store) = self.warm_data_store.as_ref() {
                    warm_store.request_archive_all(dirty_sessions, true);
                }
            }
        }
    }

    /// 应用退出前调用：同步归档所有有效会话并关闭归档线程。
    /// 与空闲归档的区别：不限于脏会话（覆盖聊完不足 30 秒就退出的场景），
    /// 且 shutdown() 会等待后台线程把队列写完，保证数据真正落盘。
    /// 跳过 LLM 反思，避免退出被网络请求阻塞。
    pub fn archive_all_on_exit(&mut self) {
        self.snapshot_active_into_slot();
        let sessions: Vec<AiConversation> = self
            .conversations
            .iter()
            .filter(|c| c.messages.len() > 1 && c.messages.iter().any(|m| m.role == AiRole::User))
            .cloned()
            .collect();
        if let Some(warm_store) = self.warm_data_store.as_mut() {
            if !sessions.is_empty() {
                warm_store.request_archive_all(sessions, false);
            }
            warm_store.shutdown();
        }
    }

    /// 启动时恢复最近一次的会话（若数据库中有归档），否则保持新建的"新对话"
    fn restore_latest_conversation(&mut self) {
        let Some(store) = self.warm_data_store.as_ref() else {
            return;
        };
        let Ok(meta_list) = store.load_history_meta() else {
            return;
        };
        let Some(latest) = meta_list.first() else {
            return;
        };
        if let Ok(conv) = store.load_conversation(&latest.id) {
            self.conversations[0] = conv;
            self.load_slot_into_active(0);
        }
    }

    // ===== 多会话（标签页 / 并发 / 历史）=====

    /// 活动会话槽位下标越界保护后的引用
    pub fn active_conversation(&self) -> Option<&AiConversation> {
        self.conversations.get(self.active)
    }

    /// 标签标题（活动会话取槽位标题，槽位标题在 sync_active_title 中维护）
    pub fn conv_title(&self, i: usize) -> &str {
        self.conversations
            .get(i)
            .map(|c| c.title.as_str())
            .unwrap_or("")
    }

    /// 某会话是否正在生成（活动会话读扁平字段，其余读槽位）
    pub fn conv_is_generating(&self, i: usize) -> bool {
        if i == self.active {
            self.is_generating
        } else {
            self.conversations
                .get(i)
                .map(|c| c.is_generating)
                .unwrap_or(false)
        }
    }

    /// 将活动会话的实时（扁平）状态回写到 conversations[active] 槽位。
    /// 切换/关闭/保存前调用，保证槽位数据最新。
    pub fn snapshot_active_into_slot(&mut self) {
        if self.active >= self.conversations.len() {
            return;
        }
        let slot = &mut self.conversations[self.active];
        slot.messages = self.messages.clone();
        slot.input = self.input.clone();
        slot.caret_pos = self.caret_pos;
        slot.composition = self.composition.clone();
        slot.is_generating = self.is_generating;
        slot.scroll_y = self.scroll_y;
        slot.content_height = self.content_height;
        slot.stick_to_bottom = self.stick_to_bottom;
        slot.mode = self.mode;
        slot.attachments = self.attachments.clone();
        slot.stream_state = Arc::clone(&self.stream_state);
        slot.should_stop = Arc::clone(&self.should_stop);
        slot.updated_at = now_secs();
    }

    /// 把某槽位会话加载为活动会话的实时（扁平）状态。
    fn load_slot_into_active(&mut self, idx: usize) {
        if idx >= self.conversations.len() {
            return;
        }
        let slot = self.conversations[idx].clone();
        self.messages = slot.messages;
        self.input = slot.input;
        self.caret_pos = slot.caret_pos;
        self.composition = slot.composition;
        self.is_generating = slot.is_generating;
        self.scroll_y = slot.scroll_y;
        self.content_height = slot.content_height;
        self.stick_to_bottom = slot.stick_to_bottom;
        self.mode = slot.mode;
        self.attachments = slot.attachments;
        self.stream_state = slot.stream_state;
        self.should_stop = slot.should_stop;
        self.active = idx;
    }

    /// 切换到指定会话标签
    pub fn switch_to(&mut self, idx: usize) {
        if idx == self.active || idx >= self.conversations.len() {
            return;
        }
        self.snapshot_active_into_slot();
        self.load_slot_into_active(idx);
        self.model_menu_open = false;
        self.history_open = false;
    }

    /// 新建一个空对话并激活
    pub fn new_conversation(&mut self) {
        self.snapshot_active_into_slot();
        let conv = AiConversation::new(gen_conversation_id(), "新对话".to_string());
        self.conversations.push(conv);
        let idx = self.conversations.len() - 1;
        self.load_slot_into_active(idx);
        self.input_focused = true;
        self.model_menu_open = false;
        self.history_open = false;
    }

    /// 关闭指定会话标签（正在生成的后台线程会被请求停止）
    /// 关闭前将会话归档到历史记录（内存中，Phase 2 再持久化到磁盘）。
    pub fn close_conversation(&mut self, idx: usize) {
        if idx >= self.conversations.len() {
            return;
        }
        self.conversations[idx]
            .should_stop
            .store(true, Ordering::SeqCst);
        // 归档到历史（仅非空对话）
        let conv = &self.conversations[idx];
        let msg_count = conv.messages.len();
        let has_user_msg = conv.messages.iter().any(|m| m.role == AiRole::User);
        if has_user_msg && msg_count > 1 {
            let preview = conv
                .messages
                .iter()
                .rev()
                .find(|m| m.role == AiRole::Assistant)
                .map(|m| {
                    let s = m.content.trim();
                    if s.len() > 60 {
                        format!("{}…", &s[..s.floor_char_boundary(60)])
                    } else {
                        s.to_string()
                    }
                })
                .unwrap_or_default();
            let meta = ConversationMeta {
                id: conv.id.clone(),
                title: conv.title.clone(),
                updated_at: conv.updated_at,
                message_count: msg_count,
                preview,
                mode: format!("{:?}", conv.mode),
            };
            // 去重：同 id 替换旧记录
            if let Some(pos) = self.history.iter().position(|h| h.id == meta.id) {
                self.history.remove(pos);
            }
            self.history.insert(0, meta);
            // 限制内存历史条数（避免无限增长）
            const MAX_HISTORY: usize = 50;
            if self.history.len() > MAX_HISTORY {
                self.history.truncate(MAX_HISTORY);
            }
            // 持久化：异步归档进 SQLite（温数据层，含向量索引）
            if let Some(warm_store) = self.warm_data_store.as_ref() {
                warm_store.request_archive(conv.id.clone(), conv.clone());
            }
        }
        if idx == self.active {
            self.conversations.remove(idx);
            if self.conversations.is_empty() {
                self.conversations.push(AiConversation::new(
                    gen_conversation_id(),
                    "新对话".to_string(),
                ));
                self.load_slot_into_active(0);
            } else {
                let new_active = idx.min(self.conversations.len() - 1);
                self.load_slot_into_active(new_active);
            }
        } else {
            self.conversations.remove(idx);
            if idx < self.active {
                self.active -= 1;
            }
        }
        self.model_menu_open = false;
        self.history_open = false;
    }

    /// 从历史记录中恢复指定会话为新的活动标签页
    pub fn restore_from_history(&mut self, hist_idx: usize) {
        if hist_idx >= self.history.len() {
            return;
        }
        let (id, title, updated_at) = {
            let meta = &self.history[hist_idx];
            (meta.id.clone(), meta.title.clone(), meta.updated_at)
        };
        // 若该会话仍在 conversations 中（未真正关闭），直接切换
        if let Some(pos) = self.conversations.iter().position(|c| c.id == id) {
            self.switch_to(pos);
            self.history_open = false;
            return;
        }
        // 否则尝试从 SQLite 加载完整会话，失败则创建占位会话
        self.snapshot_active_into_slot();
        let conv = self
            .warm_data_store
            .as_ref()
            .and_then(|store| store.load_conversation(&id).ok())
            .unwrap_or_else(|| {
                let mut c = AiConversation::new(id, title);
                c.updated_at = updated_at;
                c
            });
        self.conversations.push(conv);
        let new_idx = self.conversations.len() - 1;
        self.load_slot_into_active(new_idx);
        self.history_open = false;
    }

    /// 用首条用户消息自动生成活动会话标题（仍为默认标题时）
    pub fn sync_active_title(&mut self) {
        if self.active >= self.conversations.len() {
            return;
        }
        if self.conversations[self.active].title == "新对话" {
            if let Some(u) = self
                .messages
                .iter()
                .find(|m| m.role == AiRole::User)
                .map(|m| m.content.clone())
            {
                let t: String = u.trim().chars().take(18).collect();
                if !t.is_empty() {
                    self.conversations[self.active].title = t;
                }
            }
        }
    }

    /// 并发轮询所有会话：活动会话走扁平逻辑，其余走后台 drain。
    /// 返回本帧"刚完成"的会话下标列表，供调用方逐个处理 Agent 动作。
    pub fn poll_all_background(&mut self) -> Vec<usize> {
        let mut completed = Vec::new();
        if self.check_background_result() {
            completed.push(self.active);
        }
        let active = self.active;
        for i in 0..self.conversations.len() {
            if i == active {
                continue;
            }
            if self.conversations[i].drain_background() {
                completed.push(i);
            }
        }
        completed
    }

    /// 是否存在任一会话正在生成（用于维持定时重绘）
    pub fn any_generating(&self) -> bool {
        self.is_generating
            || self
                .conversations
                .iter()
                .enumerate()
                .any(|(i, c)| i != self.active && c.is_generating)
    }

    /// 指定会话的模式（活动会话读扁平，其余读槽位）
    pub fn mode_of(&self, conv_idx: usize) -> AiMode {
        if conv_idx == self.active {
            self.mode
        } else {
            self.conversations
                .get(conv_idx)
                .map(|c| c.mode)
                .unwrap_or(self.mode)
        }
    }

    /// 指定会话的最后一条助手消息文本
    pub fn last_assistant_text_of(&self, conv_idx: usize) -> Option<String> {
        if conv_idx == self.active {
            self.last_assistant_text()
        } else {
            self.conversations
                .get(conv_idx)
                .and_then(|c| c.last_assistant_text())
        }
    }

    /// 向指定会话追加一条助手消息（用于会话作用域的 Agent 动作反馈）
    pub fn add_assistant_message_to(&mut self, conv_idx: usize, content: String) {
        if conv_idx == self.active {
            self.add_assistant_message(content);
        } else if let Some(c) = self.conversations.get_mut(conv_idx) {
            c.messages.push(AiMessage::new(AiRole::Assistant, content));
            c.stick_to_bottom = true;
            c.updated_at = now_secs();
        }
    }

    /// 添加用户消息
    pub fn add_user_message(&mut self, content: String) {
        self.messages.push(AiMessage::new(AiRole::User, content));
        self.stick_to_bottom = true;
        self.sync_hot_data();
    }

    /// 添加助手消息
    pub fn add_assistant_message(&mut self, content: String) {
        self.messages
            .push(AiMessage::new(AiRole::Assistant, content));
        self.stick_to_bottom = true;
        self.sync_hot_data();
    }

    /// 发送消息（AI-H01: 非阻塞 — HTTP 调用在后台线程执行，结果通过 stream_state 流式返回）
    pub fn send_message(&mut self, settings: &AiSettings) -> Result<String, String> {
        self.agent_iter_count = 0;
        self.send_message_internal(settings, self.input.clone(), AiMode::Ask, None)
    }

    /// 发送消息，并附带当前编辑器的上下文
    pub fn send_message_with_context(
        &mut self,
        settings: &AiSettings,
        editor: &EditorState,
        mode: AiMode,
    ) -> Result<String, String> {
        self.agent_iter_count = 0;
        let context = editor.gather_context(&self.attachments);
        self.send_message_internal(settings, self.input.clone(), mode, Some(context))
    }

    /// 发送消息，使用已经准备好的上下文字符串
    pub fn send_message_with_prepared_context(
        &mut self,
        settings: &AiSettings,
        context: String,
        mode: AiMode,
    ) -> Result<String, String> {
        self.agent_iter_count = 0;
        self.send_message_internal(settings, self.input.clone(), mode, Some(context))
    }

    /// Agent 工具结果回喂：把终端命令输出作为上下文再次发起请求，驱动
    /// 「推理 → 执行 → 结果回喂 → 继续推理」循环。受最大轮次限制防止无限回环。
    pub fn continue_agent_with_tool_result(
        &mut self,
        settings: &AiSettings,
        feedback: String,
        mode: AiMode,
    ) -> Result<String, String> {
        const MAX_AGENT_ITERATIONS: u32 = 5;
        if self.agent_iter_count >= MAX_AGENT_ITERATIONS {
            return Err(format!("已达最大自动执行轮次（{}）", MAX_AGENT_ITERATIONS));
        }
        self.agent_iter_count += 1;
        self.send_message_internal(settings, feedback, mode, None)
    }

    fn send_message_internal(
        &mut self,
        settings: &AiSettings,
        user_input: String,
        mode: AiMode,
        context: Option<String>,
    ) -> Result<String, String> {
        if user_input.is_empty() {
            return Err("输入为空".to_string());
        }

        // H-17: 限制并发线程数 — 正在生成时拒绝新请求，防止无限制 spawn 线程
        if self.is_generating {
            return Err("正在等待上一次回复，请稍后再试".to_string());
        }

        // 限制输入长度（M-03）
        const MAX_INPUT_LEN: usize = 10000;
        let user_input = if user_input.len() > MAX_INPUT_LEN {
            let safe_len = user_input.floor_char_boundary(MAX_INPUT_LEN);
            user_input[..safe_len].to_string()
        } else {
            user_input
        };

        self.add_user_message(user_input.clone());
        self.input.clear();
        self.caret_pos = 0;
        self.is_generating = true;
        self.should_stop.store(false, Ordering::SeqCst);
        // 重置流式状态
        if let Ok(mut s) = self.stream_state.lock() {
            *s = AiStreamState::default();
        }

        // 限制消息历史长度（M-05: 滑动窗口，保留最近 40 条非系统消息 + 系统消息）。
        // 显示历史的上界；实际发送给模型的历史再按 token 预算二次窗口切片，见
        // history_to_chat_messages，兼顾上下文连续性与性能。
        const MAX_HISTORY: usize = 40;
        if self.messages.len() > MAX_HISTORY + 1 {
            let system_msgs: Vec<AiMessage> = self
                .messages
                .iter()
                .filter(|m| m.role == AiRole::System)
                .cloned()
                .collect();
            let non_system: Vec<AiMessage> = self
                .messages
                .iter()
                .filter(|m| m.role != AiRole::System)
                .cloned()
                .collect();
            let recent_start = non_system.len().saturating_sub(MAX_HISTORY);
            let recent: Vec<AiMessage> = non_system.into_iter().skip(recent_start).collect();
            self.messages = system_msgs;
            self.messages.extend(recent);
        }

        let settings = settings.clone();
        let context = context.unwrap_or_default();
        // 系统前缀（system/Agent 能力/模式/上下文）+ 经窗口切片的会话历史（含本轮输入），
        // 保证同一轮对话上下文连续；历史来自本会话的 self.messages，天然与其它标签页隔离。
        let mut messages = build_chat_prompt(&settings, &context, mode);
        // ACE playbook：注入已沉淀的经验策略，并记录条目 ID 供反馈归因
        let mut used_bullet_ids: Vec<String> = Vec::new();
        if let Some(warm) = self.warm_data_store.as_ref() {
            if let Ok(hits) = warm.search_playbook(&user_input, 5) {
                if !hits.is_empty() {
                    used_bullet_ids = hits.iter().map(|(b, _)| b.id.clone()).collect();
                    messages.push(ChatMessage {
                        role: "system".to_string(),
                        content: crate::reflector::format_bullets(&hits),
                    });
                }
            }
        }
        // 记录到活动会话槽位（接受/拒绝编辑时回填 helpful/harmful）
        if !used_bullet_ids.is_empty() {
            if let Some(slot) = self.conversations.get_mut(self.active) {
                slot.used_bullet_ids = used_bullet_ids;
            }
        }
        messages.extend(Self::history_to_chat_messages(&self.messages));
        let stream_state = Arc::clone(&self.stream_state);
        let should_stop = Arc::clone(&self.should_stop);

        std::thread::spawn(move || {
            let client = AiClient::new(&settings);
            match client.chat_completion_stream(&messages) {
                Ok(rx) => {
                    while let Ok(event) = rx.recv() {
                        if should_stop.load(Ordering::SeqCst) {
                            if let Ok(mut s) = stream_state.lock() {
                                s.done = true;
                            }
                            break;
                        }
                        match event {
                            AiStreamEvent::Token(token) => {
                                if let Ok(mut s) = stream_state.lock() {
                                    s.partial.push_str(&token);
                                }
                            }
                            AiStreamEvent::Reasoning(r) => {
                                if let Ok(mut s) = stream_state.lock() {
                                    s.reasoning.push_str(&r);
                                }
                            }
                            AiStreamEvent::Done => {
                                if let Ok(mut s) = stream_state.lock() {
                                    s.done = true;
                                }
                                break;
                            }
                            AiStreamEvent::Error(err) => {
                                if let Ok(mut s) = stream_state.lock() {
                                    s.error = Some(format!("请求失败: {}", sanitize_error(&err)));
                                    s.done = true;
                                }
                                break;
                            }
                        }
                    }
                }
                // H-21: 使用 safe_display() 替代 sanitize_error(&Display)，
                // 不包含已截断但仍可能有敏感信息的 API 响应体
                Err(e) => {
                    if let Ok(mut s) = stream_state.lock() {
                        s.error = Some(format!("请求失败: {}", e.safe_display()));
                        s.done = true;
                    }
                }
            }
        });

        Ok("请求已提交".to_string())
    }

    /// 估算文本 token 数（保守上界：按字符数计，CJK≈1 token/字，英文会高估但更安全）
    fn estimate_tokens(s: &str) -> usize {
        s.chars().count()
    }

    /// 将本会话消息转换为发送给模型的历史，应用"窗口切片"：
    /// - 跳过用于展示的 System 欢迎语（真正的 system 由 build_chat_prompt 注入）；
    /// - 从最近往前累加，受最大消息数与 token 预算双重限制，避免上下文过长影响性能；
    /// - 始终至少包含最后一条（当前用户输入）。
    ///
    /// 历史取自各会话自身的 messages，因此不同标签页/对话轮次天然隔离、互不串扰。
    fn history_to_chat_messages(messages: &[AiMessage]) -> Vec<ChatMessage> {
        const MAX_MSGS: usize = 30;
        const MAX_TOKENS: usize = 6000;
        let eligible: Vec<&AiMessage> = messages
            .iter()
            .filter(|m| m.role != AiRole::System)
            .collect();
        let mut selected: Vec<&AiMessage> = Vec::new();
        let mut tokens = 0usize;
        for m in eligible.iter().rev() {
            let t = Self::estimate_tokens(&m.content);
            if !selected.is_empty() && (selected.len() >= MAX_MSGS || tokens + t > MAX_TOKENS) {
                break;
            }
            tokens += t;
            selected.push(m);
        }
        selected.reverse();
        selected
            .into_iter()
            .map(|m| match m.role {
                AiRole::User => ChatMessage::user(m.content.clone()),
                _ => ChatMessage {
                    role: "assistant".to_string(),
                    content: m.content.clone(),
                },
            })
            .collect()
    }

    /// 输入字符（在光标位置插入）
    pub fn input_char(&mut self, ch: char) {
        if self.caret_pos > self.input.len() {
            self.caret_pos = self.input.len();
        }
        self.input.insert(self.caret_pos, ch);
        self.caret_pos += ch.len_utf8();
    }

    /// 在光标位置插入字符串（用于 IME 提交等一次性多字符输入）
    pub fn insert_str(&mut self, s: &str) {
        if self.caret_pos > self.input.len() {
            self.caret_pos = self.input.len();
        }
        self.input.insert_str(self.caret_pos, s);
        self.caret_pos += s.len();
    }

    /// 退格（删除光标前一个字符）
    pub fn backspace(&mut self) {
        if self.caret_pos > 0 {
            let prev_pos = self.prev_char_boundary();
            self.input.drain(prev_pos..self.caret_pos);
            self.caret_pos = prev_pos;
        }
    }

    /// 删除（删除光标后一个字符）
    pub fn delete(&mut self) {
        if self.caret_pos < self.input.len() {
            let next_pos = self.next_char_boundary();
            self.input.drain(self.caret_pos..next_pos);
        }
    }

    /// 光标左移
    pub fn move_caret_left(&mut self) {
        if self.caret_pos > 0 {
            self.caret_pos = self.prev_char_boundary();
        }
    }

    /// 光标右移
    pub fn move_caret_right(&mut self) {
        if self.caret_pos < self.input.len() {
            self.caret_pos = self.next_char_boundary();
        }
    }

    /// 光标移到行首
    pub fn move_caret_home(&mut self) {
        self.caret_pos = 0;
    }

    /// 光标移到行尾
    pub fn move_caret_end(&mut self) {
        self.caret_pos = self.input.len();
    }

    /// 获取前一个字符边界（UTF-8）
    fn prev_char_boundary(&self) -> usize {
        let mut pos = self.caret_pos;
        while pos > 0 {
            pos -= 1;
            if self.input.is_char_boundary(pos) {
                return pos;
            }
        }
        0
    }

    /// 获取后一个字符边界（UTF-8）
    fn next_char_boundary(&self) -> usize {
        let mut pos = self.caret_pos + 1;
        while pos < self.input.len() {
            if self.input.is_char_boundary(pos) {
                return pos;
            }
            pos += 1;
        }
        self.input.len()
    }

    /// 清除输入
    pub fn clear_input(&mut self) {
        self.input.clear();
        self.caret_pos = 0;
    }

    /// 停止当前生成（后台线程在下一次循环检查时退出）
    pub fn stop_generation(&mut self) {
        self.should_stop.store(true, Ordering::SeqCst);
        self.is_generating = false;
        if let Ok(mut s) = self.stream_state.lock() {
            s.done = true;
        }
    }

    /// 重新生成：移除末尾助手消息，用最近一条用户消息重新发送
    pub fn regenerate(&mut self, settings: &AiSettings) {
        if self.is_generating {
            return;
        }
        while matches!(self.messages.last(), Some(m) if m.role == AiRole::Assistant) {
            self.messages.pop();
        }
        let last_user = self
            .messages
            .iter()
            .rev()
            .find(|m| m.role == AiRole::User)
            .map(|m| m.content.clone());
        if let Some(input) = last_user {
            if matches!(self.messages.last(), Some(m) if m.role == AiRole::User) {
                self.messages.pop();
            }
            self.input = input;
            let _ = self.send_message(settings);
        }
    }

    /// 清除所有对话
    pub fn clear_history(&mut self) {
        self.messages.clear();
        self.messages.push(AiMessage::new(
            AiRole::System,
            "你好！我是 AI 助手，可以帮助你解释代码、重构、修复问题、生成测试等。".to_string(),
        ));
        if let Ok(mut s) = self.stream_state.lock() {
            *s = AiStreamState::default();
        }
        self.is_generating = false;
    }

    /// AI-H01: 轮询后台线程结果，应在渲染循环中调用
    ///
    /// 返回 `true` 表示本帧生成刚刚完成（done 边沿），调用方应在此时处理 Agent 动作
    /// （创建/修改文件、执行终端命令）。
    pub fn check_background_result(&mut self) -> bool {
        if !self.is_generating {
            return false;
        }
        let delta = {
            if let Ok(mut s) = self.stream_state.lock() {
                let partial = std::mem::take(&mut s.partial);
                let reasoning = std::mem::take(&mut s.reasoning);
                let done = s.done;
                let error = s.error.take();
                if done {
                    s.done = false;
                }
                Some((partial, reasoning, done, error))
            } else {
                None
            }
        };
        let mut just_completed = false;
        if let Some((partial, reasoning, done, error)) = delta {
            // 深度思考（DeepSeek reasoning_content）先于回答到达：单独承载于助手消息的 reasoning
            if !reasoning.is_empty() {
                if !matches!(self.messages.last(), Some(m) if m.role == AiRole::Assistant) {
                    self.messages
                        .push(AiMessage::new(AiRole::Assistant, String::new()));
                }
                if let Some(last) = self.messages.last_mut() {
                    last.reasoning
                        .get_or_insert_with(String::new)
                        .push_str(&reasoning);
                }
                self.stick_to_bottom = true;
            }
            if !partial.is_empty() {
                self.stick_to_bottom = true;
                if !matches!(self.messages.last(), Some(m) if m.role == AiRole::Assistant) {
                    self.messages
                        .push(AiMessage::new(AiRole::Assistant, String::new()));
                }
                if let Some(last) = self.messages.last_mut() {
                    last.content.push_str(&partial);
                }
            }
            if let Some(err) = error {
                self.add_assistant_message(err);
                self.is_generating = false;
                return false;
            }
            if done {
                self.is_generating = false;
                // 生成完成：自动折叠思考块，保持界面整洁
                if let Some(last) = self.messages.last_mut() {
                    if last.role == AiRole::Assistant && last.reasoning.is_some() {
                        last.reasoning_collapsed = true;
                    }
                }
                just_completed = true;
                // 同步热数据（生成完成，消息已最终确定）
                self.sync_hot_data();
            }
        }
        just_completed
    }

    /// 从最后一条助手消息中提取代码块
    pub fn extract_last_code_block(&self) -> Option<String> {
        for msg in self.messages.iter().rev() {
            if msg.role == AiRole::Assistant {
                return Self::extract_code_blocks(&msg.content);
            }
        }
        None
    }

    /// 提取所有代码块（```...``` 之间的内容）
    fn extract_code_blocks(text: &str) -> Option<String> {
        let mut result = String::new();
        let mut in_code = false;
        let mut code_content = String::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                if in_code {
                    if !code_content.is_empty() {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str(&code_content);
                    }
                    code_content.clear();
                    in_code = false;
                } else {
                    in_code = true;
                }
            } else if in_code {
                if !code_content.is_empty() {
                    code_content.push('\n');
                }
                code_content.push_str(line);
            }
        }

        // AI-L01: 未闭合代码围栏时，将累积内容也加入结果
        if in_code && !code_content.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&code_content);
        }

        if !result.is_empty() {
            Some(result)
        } else {
            None
        }
    }

    /// 从代码围栏行提取建议的文件名
    /// 例如 ```python:main.py 或 ```rust src/main.rs
    pub fn extract_filename_from_fence(line: &str) -> Option<String> {
        let trimmed = line.trim();
        if !trimmed.starts_with("```") {
            return None;
        }
        let after_fence = trimmed.strip_prefix("```")?.trim();
        // 检查是否包含冒号或空格分隔的文件名
        if let Some(colon_pos) = after_fence.find(':') {
            let filename = after_fence[colon_pos + 1..].trim();
            if !filename.is_empty() && !filename.contains(' ') {
                return Some(filename.to_string());
            }
        }
        // 检查格式：语言 文件名（如 "python main.py"）
        let parts: Vec<&str> = after_fence.split_whitespace().collect();
        if parts.len() >= 2 {
            // 第二部分看起来像文件名（包含 . 或 /）
            let candidate = parts[1];
            if candidate.contains('.') || candidate.contains('/') || candidate.contains("\\") {
                return Some(candidate.to_string());
            }
        }
        None
    }

    /// 获取最后一条助手消息的纯文本（去掉代码块标记）
    pub fn last_assistant_text(&self) -> Option<String> {
        for msg in self.messages.iter().rev() {
            if msg.role == AiRole::Assistant {
                return Some(msg.content.clone());
            }
        }
        None
    }

    /// 切换附件：已存在则移除，否则添加
    pub fn toggle_attachment(&mut self, attachment: AiContextAttachment) {
        let pos = self
            .attachments
            .iter()
            .position(|a| match (a, &attachment) {
                (AiContextAttachment::CurrentFile, AiContextAttachment::CurrentFile) => true,
                (AiContextAttachment::Selection, AiContextAttachment::Selection) => true,
                (AiContextAttachment::OpenFiles, AiContextAttachment::OpenFiles) => true,
                (AiContextAttachment::Diagnostics, AiContextAttachment::Diagnostics) => true,
                (AiContextAttachment::FileTree, AiContextAttachment::FileTree) => true,
                (AiContextAttachment::CustomText(x), AiContextAttachment::CustomText(y)) => x == y,
                _ => false,
            });
        if let Some(idx) = pos {
            self.attachments.remove(idx);
        } else {
            self.attachments.push(attachment);
        }
    }

    /// 清除所有上下文附件
    pub fn clear_attachments(&mut self) {
        self.attachments.clear();
    }

    /// 可通过工具栏切换的 5 种上下文附件（不含 CustomText）
    pub fn toggleable_attachments() -> [AiContextAttachment; 5] {
        [
            AiContextAttachment::CurrentFile,
            AiContextAttachment::Selection,
            AiContextAttachment::OpenFiles,
            AiContextAttachment::Diagnostics,
            AiContextAttachment::FileTree,
        ]
    }

    /// 判断某类附件是否已附加（按变体判断，忽略 CustomText 内部内容）
    pub fn has_attachment(&self, att: &AiContextAttachment) -> bool {
        self.attachments
            .iter()
            .any(|a| std::mem::discriminant(a) == std::mem::discriminant(att))
    }

    /// 当前已附加的上下文文本摘要（用于 UI 展示）
    pub fn attachment_summary(&self) -> String {
        if self.attachments.is_empty() {
            return String::new();
        }
        let labels: Vec<String> = self.attachments.iter().map(|a| a.short_label()).collect();
        format!("上下文: {}", labels.join(" "))
    }

    /// 限制并格式化自定义文本附件
    pub fn prepare_custom_text(text: &str) -> AiContextAttachment {
        AiContextAttachment::CustomText(truncate_middle(text, 2000))
    }

    /// 命中测试：模式切换按钮
    pub fn hit_test_mode_button(&self, px: f32, py: f32) -> Option<AiMode> {
        for (mode, x, y, w, h) in &self.mode_button_regions {
            if px >= *x && px <= *x + *w && py >= *y && py <= *y + *h {
                return Some(*mode);
            }
        }
        None
    }

    /// 命中测试：附件 chip（返回索引）
    pub fn hit_test_attachment(&self, px: f32, py: f32) -> Option<usize> {
        for (idx, x, y, w, h) in &self.attachment_chip_regions {
            if px >= *x && px <= *x + *w && py >= *y && py <= *y + *h {
                return Some(*idx);
            }
        }
        None
    }

    /// 清除所有命中区域（每帧渲染前调用）
    pub fn clear_hit_regions(&mut self) {
        self.mode_button_regions.clear();
        self.attachment_chip_regions.clear();
        self.code_save_regions.clear();
        self.tab_regions.clear();
        self.tab_close_regions.clear();
        self.new_tab_region = None;
        self.history_button_region = None;
        self.history_item_regions.clear();
        self.reasoning_toggle_regions.clear();
        self.playbook_button_region = None;
        self.playbook_delete_regions.clear();
        self.history_ws_toggle_region = None;
        self.history_delete_regions.clear();
        self.history_page_prev_region = None;
        self.history_page_next_region = None;
        self.history_time_filter_regions.clear();
        self.history_type_filter_regions.clear();
        self.history_clear_all_region = None;
        self.history_detail_back_region = None;
        self.history_detail_restore_region = None;
    }

    // ===== Playbook 管理面板 =====

    /// 切换 Playbook 管理面板展开/收起（展开时从 SQLite 加载条目）
    pub fn toggle_playbook_panel(&mut self) {
        self.playbook_open = !self.playbook_open;
        if self.playbook_open {
            self.reload_playbook();
        }
    }

    /// 重新加载 Playbook 条目缓存
    pub fn reload_playbook(&mut self) {
        if let Some(warm) = self.warm_data_store.as_ref() {
            self.playbook_items = warm.list_playbook(None).unwrap_or_default();
        }
    }

    /// 删除指定下标的 Playbook 条目（调用方需先弹确认）
    pub fn delete_playbook_item(&mut self, idx: usize) -> Result<(), String> {
        let id = self
            .playbook_items
            .get(idx)
            .map(|b| b.id.clone())
            .ok_or_else(|| "条目不存在".to_string())?;
        if let Some(warm) = self.warm_data_store.as_ref() {
            warm.delete_bullet(&id)?;
        }
        self.reload_playbook();
        Ok(())
    }

    /// 切换历史列表的工作区过滤
    pub fn toggle_history_workspace_only(&mut self) {
        self.history_workspace_only = !self.history_workspace_only;
        self.history_page = 0;
    }

    // ===== 历史记录：筛选 / 分页 / 详情 / 清除 =====

    /// 应用时间 + 类型筛选后的历史下标（对应 self.history 的原始下标）
    pub fn filtered_history_indices(&self) -> Vec<usize> {
        let cutoff = self.history_time_filter.cutoff(now_secs());
        self.history
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                let time_ok = cutoff.map(|c| m.updated_at >= c).unwrap_or(true);
                let type_ok = self
                    .history_type_filter
                    .as_ref()
                    .map(|t| m.mode.eq_ignore_ascii_case(t))
                    .unwrap_or(true);
                time_ok && type_ok
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// 筛选后的总页数（至少 1 页）
    pub fn history_page_count(&self) -> usize {
        let n = self.filtered_history_indices().len();
        (n + HISTORY_PAGE_SIZE - 1) / HISTORY_PAGE_SIZE.max(1)
    }

    /// 把当前页码收敛到合法范围（筛选/删除/刷新后调用）
    pub fn clamp_history_page(&mut self) {
        let pc = self.history_page_count().max(1);
        if self.history_page >= pc {
            self.history_page = pc - 1;
        }
    }

    /// 当前页显示的历史下标
    pub fn history_page_indices(&self) -> Vec<usize> {
        let start = self.history_page * HISTORY_PAGE_SIZE;
        self.filtered_history_indices()
            .into_iter()
            .skip(start)
            .take(HISTORY_PAGE_SIZE)
            .collect()
    }

    /// 设置时间筛选（回到第一页）
    pub fn set_history_time_filter(&mut self, f: HistoryTimeFilter) {
        self.history_time_filter = f;
        self.history_page = 0;
    }

    /// 设置类型筛选（回到第一页）
    pub fn set_history_type_filter(&mut self, f: Option<String>) {
        self.history_type_filter = f;
        self.history_page = 0;
    }

    /// 下一页（到末页为止）
    pub fn history_next_page(&mut self) {
        if self.history_page + 1 < self.history_page_count() {
            self.history_page += 1;
        }
    }

    /// 上一页
    pub fn history_prev_page(&mut self) {
        self.history_page = self.history_page.saturating_sub(1);
    }

    /// 删除一条历史记录（内存索引 + SQLite 级联删除）
    pub fn delete_history_item(&mut self, hist_idx: usize) -> Result<(), String> {
        let meta = self
            .history
            .get(hist_idx)
            .cloned()
            .ok_or_else(|| "历史记录不存在".to_string())?;
        if let Some(warm) = self.warm_data_store.as_ref() {
            warm.delete_conversation(&meta.id)?;
        }
        self.history.remove(hist_idx);
        if self.history_detail_id.as_deref() == Some(meta.id.as_str()) {
            self.close_history_detail();
        }
        self.clamp_history_page();
        Ok(())
    }

    /// 清空全部历史记录；返回删除条数
    pub fn clear_all_history(&mut self) -> Result<usize, String> {
        let n = if let Some(warm) = self.warm_data_store.as_ref() {
            warm.clear_all_conversations()?
        } else {
            self.history.len()
        };
        self.history.clear();
        self.history_page = 0;
        self.close_history_detail();
        Ok(n)
    }

    /// 打开历史详情视图（懒加载完整会话消息）
    pub fn open_history_detail(&mut self, hist_idx: usize) {
        let Some(meta) = self.history.get(hist_idx) else {
            return;
        };
        let id = meta.id.clone();
        self.history_detail_conv = self
            .warm_data_store
            .as_ref()
            .and_then(|s| s.load_conversation(&id).ok());
        self.history_detail_id = Some(id);
    }

    /// 关闭历史详情视图，返回列表
    pub fn close_history_detail(&mut self) {
        self.history_detail_id = None;
        self.history_detail_conv = None;
    }

    /// 从详情视图恢复该会话为活动标签页
    pub fn restore_history_detail(&mut self) {
        if let Some(id) = self.history_detail_id.clone() {
            self.close_history_detail();
            if let Some(idx) = self.history.iter().position(|m| m.id == id) {
                self.restore_from_history(idx);
            }
        }
    }
}

/// 解析段落内的轻量 Markdown：标题(`#`/`##`/`###`)、无序列表(`-`/`*`/`+`)、粗体(`**`)。
///
/// 返回 `(清洗后的 UTF-16 文本, 粗体范围, 标题范围[start,len,字号])`，
/// 范围以 UTF-16 code unit 为单位，直接供 `IDWriteTextLayout` 的 range 样式使用。
#[allow(clippy::type_complexity)]
pub fn parse_markdown_segment(text: &str) -> (Vec<u16>, Vec<(u32, u32)>, Vec<(u32, u32, f32)>) {
    let mut clean: Vec<u16> = Vec::new();
    let mut bolds: Vec<(u32, u32)> = Vec::new();
    let mut headings: Vec<(u32, u32, f32)> = Vec::new();

    for (li, line) in text.lines().enumerate() {
        if li > 0 {
            clean.push(b'\n' as u16);
        }
        let line_start = clean.len() as u32;

        // 行首标题标记
        let trimmed = line.trim_start();
        let (mut content, heading_size): (&str, Option<f32>) =
            if let Some(rest) = trimmed.strip_prefix("### ") {
                (rest, Some(13.5))
            } else if let Some(rest) = trimmed.strip_prefix("## ") {
                (rest, Some(15.0))
            } else if let Some(rest) = trimmed.strip_prefix("# ") {
                (rest, Some(17.0))
            } else {
                (line, None)
            };

        // 行首无序列表标记（非标题时），替换为圆点
        if heading_size.is_none() {
            let t = content.trim_start();
            if let Some(rest) = t
                .strip_prefix("- ")
                .or_else(|| t.strip_prefix("* "))
                .or_else(|| t.strip_prefix("+ "))
            {
                clean.push(0x2022); // •
                clean.push(b' ' as u16);
                content = rest;
            }
        }

        // 行内粗体 **text**
        let chars: Vec<char> = content.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
                if let Some(end) = find_double_star(&chars, i + 2) {
                    let b_start = clean.len() as u32;
                    for &c in &chars[i + 2..end] {
                        push_utf16(&mut clean, c);
                    }
                    let b_len = clean.len() as u32 - b_start;
                    if b_len > 0 {
                        bolds.push((b_start, b_len));
                    }
                    i = end + 2;
                    continue;
                }
            }
            push_utf16(&mut clean, chars[i]);
            i += 1;
        }

        if let Some(size) = heading_size {
            let line_len = clean.len() as u32 - line_start;
            if line_len > 0 {
                headings.push((line_start, line_len, size));
            }
        }
    }

    (clean, bolds, headings)
}

fn push_utf16(buf: &mut Vec<u16>, c: char) {
    let mut tmp = [0u16; 2];
    for u in c.encode_utf16(&mut tmp) {
        buf.push(*u);
    }
}

fn find_double_star(chars: &[char], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < chars.len() {
        if chars[i] == '*' && chars[i + 1] == '*' {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: AiRole, content: &str) -> AiMessage {
        AiMessage::new(role, content.to_string())
    }

    #[test]
    fn history_keeps_order_and_maps_roles() {
        let history = vec![
            msg(AiRole::System, "欢迎语（应被跳过）"),
            msg(AiRole::User, "你好"),
            msg(AiRole::Assistant, "你好！我是助手"),
            msg(AiRole::User, "我刚刚问了什么"),
        ];
        let out = AiPanel::history_to_chat_messages(&history);
        // System 欢迎语被跳过，其余按序映射
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].role, "user");
        assert_eq!(out[0].content, "你好");
        assert_eq!(out[1].role, "assistant");
        assert_eq!(out[2].role, "user");
        assert_eq!(out[2].content, "我刚刚问了什么");
    }

    #[test]
    fn history_window_always_includes_last_even_if_huge() {
        // 单条超预算也必须包含（保证当前输入不被丢弃）
        let big = "字".repeat(20_000);
        let history = vec![msg(AiRole::User, &big)];
        let out = AiPanel::history_to_chat_messages(&history);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].role, "user");
    }

    #[test]
    fn history_window_drops_oldest_when_over_budget() {
        // 构造多条大消息，超出 token 预算时应丢弃较早的，保留最近的
        let mut history = Vec::new();
        for i in 0..10 {
            history.push(msg(AiRole::User, &"x".repeat(1000)));
            history.push(msg(AiRole::Assistant, &format!("回复{}", i)));
        }
        let out = AiPanel::history_to_chat_messages(&history);
        // 至少保留最近若干条，且不超过消息数上限
        assert!(!out.is_empty());
        assert!(out.len() <= 30);
        // 最后一条应为最近的助手回复（保留最近）
        assert_eq!(out.last().unwrap().content, "回复9");
    }

    #[test]
    fn empty_history_yields_empty() {
        let out = AiPanel::history_to_chat_messages(&[]);
        assert!(out.is_empty());
    }

    // ===== 历史记录列表：筛选 / 分页 / 详情 / 清除 =====

    fn test_panel() -> AiPanel {
        let mut p = AiPanel::new();
        // 测试不触碰真实持久化层
        p.warm_data_store = None;
        p.hot_data_store = None;
        p
    }

    fn meta(id: &str, updated_at: u64, mode: &str) -> ConversationMeta {
        ConversationMeta {
            id: id.to_string(),
            title: format!("会话{}", id),
            updated_at,
            message_count: 3,
            preview: String::new(),
            mode: mode.to_string(),
        }
    }

    #[test]
    fn history_time_filter_keeps_recent_only() {
        let now = now_secs();
        let mut p = test_panel();
        p.history = vec![
            meta("a", now, "Ask"),
            meta("b", now.saturating_sub(3 * 86400), "Agent"),
            meta("c", now.saturating_sub(40 * 86400), "Ask"),
        ];
        p.set_history_time_filter(HistoryTimeFilter::Today);
        assert_eq!(p.filtered_history_indices(), vec![0]);
        p.set_history_time_filter(HistoryTimeFilter::Week);
        assert_eq!(p.filtered_history_indices(), vec![0, 1]);
        p.set_history_time_filter(HistoryTimeFilter::All);
        assert_eq!(p.filtered_history_indices(), vec![0, 1, 2]);
    }

    #[test]
    fn history_type_filter_matches_mode_case_insensitive() {
        let now = now_secs();
        let mut p = test_panel();
        p.history = vec![meta("a", now, "Ask"), meta("b", now, "Agent")];
        p.set_history_type_filter(Some("ask".to_string()));
        assert_eq!(p.filtered_history_indices(), vec![0]);
        p.set_history_type_filter(Some("Agent".to_string()));
        assert_eq!(p.filtered_history_indices(), vec![1]);
        p.set_history_type_filter(None);
        assert_eq!(p.filtered_history_indices(), vec![0, 1]);
    }

    #[test]
    fn history_filter_resets_page() {
        let now = now_secs();
        let mut p = test_panel();
        p.history = (0..13)
            .map(|i| meta(&format!("c{}", i), now, "Ask"))
            .collect();
        p.history_page = 2;
        p.set_history_time_filter(HistoryTimeFilter::Week);
        assert_eq!(p.history_page, 0);
        p.history_page = 1;
        p.set_history_type_filter(Some("Agent".to_string()));
        assert_eq!(p.history_page, 0);
    }

    #[test]
    fn history_pagination_pages_and_bounds() {
        let now = now_secs();
        let mut p = test_panel();
        p.history = (0..13)
            .map(|i| meta(&format!("c{}", i), now, "Ask"))
            .collect();
        assert_eq!(p.history_page_count(), 3); // 6 + 6 + 1
        assert_eq!(p.history_page_indices().len(), HISTORY_PAGE_SIZE);
        p.history_next_page();
        assert_eq!(p.history_page, 1);
        p.history_next_page();
        assert_eq!(p.history_page, 2);
        assert_eq!(p.history_page_indices().len(), 1);
        // 末页后继续 next 不变
        p.history_next_page();
        assert_eq!(p.history_page, 2);
        p.history_prev_page();
        assert_eq!(p.history_page, 1);
        // 首页 prev 保持 0
        p.history_prev_page();
        p.history_prev_page();
        assert_eq!(p.history_page, 0);
        // 筛选后页数收缩时 clamp 收敛页码
        p.history_page = 2;
        p.clamp_history_page();
        assert_eq!(p.history_page, 2);
        p.set_history_time_filter(HistoryTimeFilter::Today);
        p.clamp_history_page();
        assert_eq!(p.history_page, 0);
    }

    #[test]
    fn delete_history_item_removes_entry_and_closes_detail() {
        let now = now_secs();
        let mut p = test_panel();
        p.history = vec![meta("a", now, "Ask"), meta("b", now, "Agent")];
        p.open_history_detail(0);
        assert_eq!(p.history_detail_id.as_deref(), Some("a"));
        p.delete_history_item(0).unwrap();
        assert_eq!(p.history.len(), 1);
        assert_eq!(p.history[0].id, "b");
        // 详情指向被删会话时自动关闭
        assert!(p.history_detail_id.is_none());
        // 越界删除报错
        assert!(p.delete_history_item(5).is_err());
    }

    #[test]
    fn clear_all_history_empties_list_and_resets_page() {
        let now = now_secs();
        let mut p = test_panel();
        p.history = (0..8)
            .map(|i| meta(&format!("c{}", i), now, "Ask"))
            .collect();
        p.history_page = 1;
        p.open_history_detail(0);
        let n = p.clear_all_history().unwrap();
        assert_eq!(n, 8);
        assert!(p.history.is_empty());
        assert_eq!(p.history_page, 0);
        assert!(p.history_detail_id.is_none());
    }

    #[test]
    fn history_detail_open_and_restore() {
        let now = now_secs();
        let mut p = test_panel();
        p.history = vec![meta("a", now, "Ask")];
        // 无 warm store：详情打开但消息为空（会话仍在 conversations 中可直接恢复）
        p.open_history_detail(0);
        assert_eq!(p.history_detail_id.as_deref(), Some("a"));
        assert!(p.history_detail_conv.is_none());
        p.close_history_detail();
        assert!(p.history_detail_id.is_none());
        // 越界打开为 no-op
        p.open_history_detail(9);
        assert!(p.history_detail_id.is_none());
    }

    #[test]
    fn relative_time_buckets() {
        let now = 1_000_000_000u64;
        assert_eq!(relative_time(now, now), "刚刚");
        assert_eq!(relative_time(now - 120, now), "2 分钟前");
        assert_eq!(relative_time(now - 7200, now), "2 小时前");
        assert_eq!(relative_time(now - 2 * 86400, now), "2 天前");
        assert_eq!(relative_time(now - 14 * 86400, now), "2 周前");
        assert_eq!(relative_time(now - 60 * 86400, now), "2 个月前");
        // 未来时间戳不回绕
        assert_eq!(relative_time(now + 100, now), "刚刚");
    }
}
