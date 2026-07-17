use aether_shared::settings::{AiModelProfile, AiSettings, AppSettings};
use std::sync::{Arc, Mutex};

/// 设置面板字段标识
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsField {
    Provider,
    ApiKey,
    BaseUrl,
    Model,
    Temperature,
    MaxTokens,
    SystemPrompt,
}

/// 设置面板按钮标识
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsButton {
    Save,
    TestConnection,
}

/// 设置标签页类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    /// 通用：主题、字体等
    General,
    /// AI 接口：provider / key / url / model / temperature / max_tokens / system_prompt / 测试连接
    Ai,
    /// 外观：侧边栏、密度等
    Appearance,
    /// 远程：SSH 主机等
    Remote,
    /// 账户
    Account,
    /// 模型管理
    Models,
}

impl SettingsTab {
    pub fn label(&self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::Ai => "AI",
            Self::Appearance => "外观",
            Self::Remote => "远程",
            Self::Account => "账户",
            Self::Models => "模型",
        }
    }

    pub const ALL: [SettingsTab; 6] = [
        SettingsTab::General,
        SettingsTab::Ai,
        SettingsTab::Appearance,
        SettingsTab::Remote,
        SettingsTab::Account,
        SettingsTab::Models,
    ];
}

/// 服务商模板按钮
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderTemplateButton {
    DeepSeek,
    Kimi,
    Custom,
}

/// 设置下拉类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsDropdownKind {
    Provider,
    Model,
}

/// 模型按钮类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelButton {
    Add,
    Edit,
    Delete,
    ToggleEnabled,
}

/// 测试连接轮询结果
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestPollResult {
    /// 测试成功，且有待保存的设置
    SuccessWithPendingSave,
    /// 测试成功，无待保存操作
    Success,
    /// 测试失败，且有待保存的设置
    FailedWithPendingSave,
    /// 测试失败，无待保存操作
    Failed,
    /// 测试尚未完成
    Pending,
}

/// 模型配置项
#[derive(Clone, Debug)]
pub struct ModelConfig {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub provider: String,
    pub description: String,
    pub enabled: bool,
    // 多模型：完整配置字段
    pub api_key: String,
    pub base_url: String,
    pub temperature: String,
    pub max_tokens: String,
    pub system_prompt: String,
}

impl ModelConfig {
    /// 转换为持久化用的 AiModelProfile
    pub fn to_profile(&self) -> AiModelProfile {
        AiModelProfile {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            provider: self.provider.clone(),
            api_key: self.api_key.clone(),
            base_url: if self.base_url.is_empty() {
                None
            } else {
                Some(self.base_url.clone())
            },
            model: self.name.clone(),
            temperature: self.temperature.trim().parse().ok(),
            max_tokens: self.max_tokens.trim().parse().ok(),
            system_prompt: if self.system_prompt.is_empty() {
                None
            } else {
                Some(self.system_prompt.clone())
            },
            enabled: self.enabled,
        }
    }

    /// 从持久化的 AiModelProfile 构造
    pub fn from_profile(p: &AiModelProfile) -> Self {
        Self {
            id: p.id.clone(),
            name: p.model.clone(),
            display_name: if p.display_name.is_empty() {
                p.model.clone()
            } else {
                p.display_name.clone()
            },
            provider: p.provider.clone(),
            description: String::new(),
            enabled: p.enabled,
            api_key: p.api_key.clone(),
            base_url: p.base_url.clone().unwrap_or_default(),
            temperature: p
                .temperature
                .map(|t| t.to_string())
                .unwrap_or_else(|| "0.7".to_string()),
            max_tokens: p
                .max_tokens
                .map(|m| m.to_string())
                .unwrap_or_else(|| "2048".to_string()),
            system_prompt: p.system_prompt.clone().unwrap_or_default(),
        }
    }
}

/// AI 设置面板状态
#[derive(Clone, Debug)]
pub struct SettingsPanel {
    pub provider: String,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub temperature: String,
    pub max_tokens: String,
    pub system_prompt: String,
    pub active_field: Option<SettingsField>,
    pub hover_button: Option<SettingsButton>,
    pub test_status: String,
    pub is_testing: bool,
    /// 后台测试连接结果（Some(Ok)=成功，Some(Err)=失败，None=未完成）
    pub test_result: Arc<Mutex<Option<Result<String, String>>>>,
    /// 测试通过后是否自动保存设置
    pub pending_save: bool,
    // Cached layout for hit testing
    pub field_regions: Vec<(SettingsField, f32, f32, f32, f32)>,
    pub button_regions: Vec<(SettingsButton, f32, f32, f32, f32)>,
    /// 标签页：当前激活
    pub active_tab: SettingsTab,
    /// 标签页：鼠标悬停
    pub hover_tab: Option<SettingsTab>,
    /// 标签页命中区域缓存 (tab, x, y, w, h)
    pub tab_regions: Vec<(SettingsTab, f32, f32, f32, f32)>,
    // 导航栏
    pub nav_width: f32,
    pub hover_nav_resize: bool,
    pub nav_resizing: bool,
    // 模型管理
    pub models: Vec<ModelConfig>,
    pub selected_model_id: Option<String>,
    pub hover_model_id: Option<String>,
    pub active_model_id: Option<String>,
    pub hover_model_button: Option<ModelButton>,
    /// 悬停按钮对应的模型ID（用于区分不同模型项上的按钮）
    pub hover_model_button_id: Option<String>,
    // 模型按钮/项命中区域
    model_button_regions: Vec<(ModelButton, f32, f32, f32, f32)>,
    model_item_regions: Vec<(String, f32, f32, f32, f32)>,
    // 下拉框
    pub open_dropdown: Option<SettingsDropdownKind>,
    pub dropdown_trigger_regions: Vec<(SettingsDropdownKind, f32, f32, f32, f32)>,
    pub dropdown_item_regions: Vec<(SettingsDropdownKind, usize, f32, f32, f32, f32)>,
    pub hover_dropdown: Option<SettingsDropdownKind>,
    pub hover_dropdown_index: Option<usize>,
    // 滚动：scroll_offset 为当前偏移，content_height 为最大可滚动距离（总内容高 - 可视高）
    pub scroll_offset: f32,
    pub content_height: f32,
    // API 密钥显隐
    pub show_api_key: bool,
    pub hover_api_key_toggle: bool,
    pub api_key_toggle_region: Option<(f32, f32, f32, f32)>,
    // 温度滑块轨道命中区
    pub temp_slider_region: Option<(f32, f32, f32, f32)>,
    /// 温度滑块是否处于拖拽中
    pub temp_slider_dragging: bool,
    /// 打开设置面板时的 AI 配置快照，用于"未保存更改"检测
    pub baseline_ai: Option<AiSettings>,
}

impl SettingsPanel {
    pub fn new() -> Self {
        Self {
            provider: "deepseek".to_string(),
            api_key: String::new(),
            base_url: "https://api.deepseek.com/v1".to_string(),
            model: "deepseek-v4-pro".to_string(),
            temperature: "0.7".to_string(),
            max_tokens: "2048".to_string(),
            system_prompt: String::new(),
            active_field: None,
            hover_button: None,
            test_status: String::new(),
            is_testing: false,
            pending_save: false,
            test_result: Arc::new(Mutex::new(None)),
            field_regions: Vec::new(),
            button_regions: Vec::new(),
            active_tab: SettingsTab::Ai,
            hover_tab: None,
            tab_regions: Vec::new(),
            nav_width: 160.0,
            hover_nav_resize: false,
            nav_resizing: false,
            models: Vec::new(),
            selected_model_id: None,
            hover_model_id: None,
            active_model_id: None,
            hover_model_button: None,
            hover_model_button_id: None,
            model_button_regions: Vec::new(),
            model_item_regions: Vec::new(),
            open_dropdown: None,
            dropdown_trigger_regions: Vec::new(),
            dropdown_item_regions: Vec::new(),
            hover_dropdown: None,
            hover_dropdown_index: None,
            scroll_offset: 0.0,
            content_height: 0.0,
            show_api_key: false,
            hover_api_key_toggle: false,
            api_key_toggle_region: None,
            temp_slider_region: None,
            temp_slider_dragging: false,
            baseline_ai: None,
        }
    }

    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            provider: settings.ai.provider.clone(),
            api_key: settings.ai.api_key.clone(),
            base_url: settings.ai.base_url.clone().unwrap_or_default(),
            model: settings.ai.model.clone(),
            temperature: settings
                .ai
                .temperature
                .map(|t| t.to_string())
                .unwrap_or_else(|| "0.7".to_string()),
            max_tokens: settings
                .ai
                .max_tokens
                .map(|m| m.to_string())
                .unwrap_or_else(|| "2048".to_string()),
            system_prompt: settings.ai.system_prompt.clone().unwrap_or_default(),
            active_field: None,
            hover_button: None,
            test_status: String::new(),
            is_testing: false,
            pending_save: false,
            test_result: Arc::new(Mutex::new(None)),
            field_regions: Vec::new(),
            button_regions: Vec::new(),
            active_tab: SettingsTab::Ai,
            hover_tab: None,
            tab_regions: Vec::new(),
            nav_width: 160.0,
            hover_nav_resize: false,
            nav_resizing: false,
            models: Vec::new(),
            selected_model_id: None,
            hover_model_id: None,
            active_model_id: None,
            hover_model_button: None,
            hover_model_button_id: None,
            model_button_regions: Vec::new(),
            model_item_regions: Vec::new(),
            open_dropdown: None,
            dropdown_trigger_regions: Vec::new(),
            dropdown_item_regions: Vec::new(),
            hover_dropdown: None,
            hover_dropdown_index: None,
            scroll_offset: 0.0,
            content_height: 0.0,
            show_api_key: false,
            hover_api_key_toggle: false,
            api_key_toggle_region: None,
            temp_slider_region: None,
            temp_slider_dragging: false,
            baseline_ai: None,
        }
    }

    pub fn to_ai_settings(&self) -> AiSettings {
        AiSettings {
            provider: self.provider.clone(),
            api_key: self.api_key.clone(),
            base_url: if self.base_url.is_empty() {
                None
            } else {
                Some(self.base_url.clone())
            },
            model: self.model.clone(),
            temperature: self.temperature.parse().ok(),
            max_tokens: self.max_tokens.parse().ok(),
            system_prompt: if self.system_prompt.is_empty() {
                None
            } else {
                Some(self.system_prompt.clone())
            },
        }
    }

    pub fn apply_settings(&mut self, settings: &AppSettings) {
        // 加载模型列表（多模型架构）
        self.models = settings
            .ai_models
            .iter()
            .map(ModelConfig::from_profile)
            .collect();
        self.active_model_id = settings
            .active_model_id
            .clone()
            .filter(|id| self.models.iter().any(|m| &m.id == id))
            .or_else(|| self.models.first().map(|m| m.id.clone()));
        // 加载激活模型（或回退旧单一配置）到 AI 页字段
        self.load_active_model_fields(&settings.ai);
        // 记录打开时的快照，作为未保存更改检测的基准
        self.baseline_ai = Some(self.to_ai_settings());
    }

    /// 把激活模型的配置加载到 AI 页编辑字段；无模型时回退到传入的 fallback 配置
    pub fn load_active_model_fields(&mut self, fallback_ai: &AiSettings) {
        let found = self
            .active_model_id
            .as_ref()
            .and_then(|id| self.models.iter().find(|m| &m.id == id))
            .cloned();
        if let Some(m) = found {
            self.provider = m.provider;
            self.api_key = m.api_key;
            self.base_url = m.base_url;
            self.model = m.name;
            self.temperature = m.temperature;
            self.max_tokens = m.max_tokens;
            self.system_prompt = m.system_prompt;
        } else {
            self.provider = fallback_ai.provider.clone();
            self.api_key = fallback_ai.api_key.clone();
            self.base_url = fallback_ai.base_url.clone().unwrap_or_default();
            self.model = fallback_ai.model.clone();
            self.temperature = fallback_ai
                .temperature
                .map(|t| t.to_string())
                .unwrap_or_else(|| "0.7".to_string());
            self.max_tokens = fallback_ai
                .max_tokens
                .map(|m| m.to_string())
                .unwrap_or_else(|| "2048".to_string());
            self.system_prompt = fallback_ai.system_prompt.clone().unwrap_or_default();
        }
    }

    /// 把当前 AI 页编辑字段写回激活模型档案（无激活模型时用当前字段自动创建）
    pub fn store_fields_to_active_model(&mut self) {
        let has_active = self
            .active_model_id
            .as_ref()
            .map(|id| self.models.iter().any(|m| &m.id == id))
            .unwrap_or(false);
        if !has_active {
            // 完全空配置则不创建
            if self.provider.is_empty() && self.api_key.is_empty() && self.model.is_empty() {
                return;
            }
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let new_id = format!("model-{}", stamp);
            self.models.push(ModelConfig {
                id: new_id.clone(),
                name: String::new(),
                display_name: String::new(),
                provider: "deepseek".to_string(),
                description: String::new(),
                enabled: true,
                api_key: String::new(),
                base_url: String::new(),
                temperature: "0.7".to_string(),
                max_tokens: "2048".to_string(),
                system_prompt: String::new(),
            });
            self.active_model_id = Some(new_id);
        }
        let provider = self.provider.clone();
        let api_key = self.api_key.clone();
        let base_url = self.base_url.clone();
        let model = self.model.clone();
        let temperature = self.temperature.clone();
        let max_tokens = self.max_tokens.clone();
        let system_prompt = self.system_prompt.clone();
        if let Some(id) = self.active_model_id.clone() {
            if let Some(m) = self.models.iter_mut().find(|m| m.id == id) {
                m.provider = provider;
                m.api_key = api_key;
                m.base_url = base_url;
                m.name = model.clone();
                m.temperature = temperature;
                m.max_tokens = max_tokens;
                m.system_prompt = system_prompt;
                if m.display_name.is_empty() && !model.is_empty() {
                    m.display_name = model;
                }
            }
        }
    }

    /// 切换激活模型：先保存当前编辑，再加载目标模型字段
    pub fn set_active_model(&mut self, id: &str, fallback_ai: &AiSettings) {
        self.store_fields_to_active_model();
        self.active_model_id = Some(id.to_string());
        self.load_active_model_fields(fallback_ai);
    }

    /// 新建一个模型档案并设为激活，返回其 id
    pub fn create_new_model(&mut self) -> String {
        self.store_fields_to_active_model();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let id = format!("model-{}", stamp);
        self.models.push(ModelConfig {
            id: id.clone(),
            name: String::new(),
            display_name: "新模型".to_string(),
            provider: "deepseek".to_string(),
            description: String::new(),
            enabled: true,
            api_key: String::new(),
            base_url: "https://api.deepseek.com/v1".to_string(),
            temperature: "0.7".to_string(),
            max_tokens: "2048".to_string(),
            system_prompt: String::new(),
        });
        self.active_model_id = Some(id.clone());
        self.provider = "deepseek".to_string();
        self.api_key = String::new();
        self.base_url = "https://api.deepseek.com/v1".to_string();
        self.model = String::new();
        self.temperature = "0.7".to_string();
        self.max_tokens = "2048".to_string();
        self.system_prompt = String::new();
        id
    }

    /// 把模型列表与激活选择同步回 AppSettings（供持久化）
    pub fn sync_to_app_settings(&self, app: &mut AppSettings) {
        app.ai_models = self.models.iter().map(|m| m.to_profile()).collect();
        app.active_model_id = self.active_model_id.clone();
    }

    /// 当前激活/编辑模型的展示名（用于 AI 页指示）
    pub fn active_model_display(&self) -> String {
        if let Some(id) = &self.active_model_id {
            if let Some(m) = self.models.iter().find(|m| &m.id == id) {
                let name = if m.display_name.is_empty() {
                    &m.name
                } else {
                    &m.display_name
                };
                if !name.is_empty() {
                    return name.clone();
                }
            }
        }
        "新模型".to_string()
    }

    pub fn clear_regions(&mut self) {
        self.field_regions.clear();
        self.button_regions.clear();
        self.tab_regions.clear();
        self.model_button_regions.clear();
        self.model_item_regions.clear();
        self.dropdown_trigger_regions.clear();
        self.dropdown_item_regions.clear();
    }

    pub fn add_field_region(&mut self, field: SettingsField, x: f32, y: f32, w: f32, h: f32) {
        self.field_regions.push((field, x, y, w, h));
    }

    pub fn add_button_region(&mut self, button: SettingsButton, x: f32, y: f32, w: f32, h: f32) {
        self.button_regions.push((button, x, y, w, h));
    }

    pub fn add_tab_region(&mut self, tab: SettingsTab, x: f32, y: f32, w: f32, h: f32) {
        self.tab_regions.push((tab, x, y, w, h));
    }

    /// 命中检测：标签页
    pub fn hit_test_tab(&self, x: f32, y: f32) -> Option<SettingsTab> {
        for (tab, tx, ty, tw, th) in &self.tab_regions {
            if x >= *tx && x < tx + tw && y >= *ty && y < ty + th {
                return Some(*tab);
            }
        }
        None
    }

    pub fn hit_test_field(&self, x: f32, y: f32) -> Option<SettingsField> {
        for (field, fx, fy, fw, fh) in &self.field_regions {
            if x >= *fx && x < fx + fw && y >= *fy && y < fy + fh {
                return Some(*field);
            }
        }
        None
    }

    pub fn hit_test_button(&self, x: f32, y: f32) -> Option<SettingsButton> {
        for (button, bx, by, bw, bh) in &self.button_regions {
            if x >= *bx && x < bx + bw && y >= *by && y < by + bh {
                return Some(*button);
            }
        }
        None
    }

    pub fn input_char(&mut self, ch: char) {
        if let Some(field) = self.active_field {
            match field {
                SettingsField::Provider => self.provider.push(ch),
                SettingsField::ApiKey => self.api_key.push(ch),
                SettingsField::BaseUrl => self.base_url.push(ch),
                SettingsField::Model => self.model.push(ch),
                SettingsField::Temperature => self.temperature.push(ch),
                SettingsField::MaxTokens => self.max_tokens.push(ch),
                SettingsField::SystemPrompt => self.system_prompt.push(ch),
                _ => {}
            }
        }
    }

    /// 粘贴文本到当前活动字段
    pub fn paste_text(&mut self, text: &str) {
        if let Some(field) = self.active_field {
            match field {
                SettingsField::Provider => self.provider.push_str(text),
                SettingsField::ApiKey => self.api_key.push_str(text),
                SettingsField::BaseUrl => self.base_url.push_str(text),
                SettingsField::Model => self.model.push_str(text),
                SettingsField::Temperature => self.temperature.push_str(text),
                SettingsField::MaxTokens => self.max_tokens.push_str(text),
                SettingsField::SystemPrompt => self.system_prompt.push_str(text),
                _ => {}
            }
        }
    }

    /// 退格
    pub fn backspace(&mut self) {
        if let Some(field) = self.active_field {
            match field {
                SettingsField::Provider => {
                    self.provider.pop();
                }
                SettingsField::ApiKey => {
                    self.api_key.pop();
                }
                SettingsField::BaseUrl => {
                    self.base_url.pop();
                }
                SettingsField::Model => {
                    self.model.pop();
                }
                SettingsField::Temperature => {
                    self.temperature.pop();
                }
                SettingsField::MaxTokens => {
                    self.max_tokens.pop();
                }
                SettingsField::SystemPrompt => {
                    self.system_prompt.pop();
                }
                _ => {}
            }
        }
    }

    /// UI-M05: Delete 键清除活动字段（区别于 Backspace 删除末尾字符）
    pub fn delete_forward(&mut self) {
        if let Some(field) = self.active_field {
            match field {
                SettingsField::Provider => self.provider.clear(),
                SettingsField::ApiKey => self.api_key.clear(),
                SettingsField::BaseUrl => self.base_url.clear(),
                SettingsField::Model => self.model.clear(),
                SettingsField::Temperature => self.temperature.clear(),
                SettingsField::MaxTokens => self.max_tokens.clear(),
                SettingsField::SystemPrompt => self.system_prompt.clear(),
                _ => {}
            }
        }
    }

    /// 当前可用键盘聚焦的字段序列。
    /// 下拉框（Provider / Model）由鼠标操作，不参与 Tab 循环；
    /// BaseUrl 仅在自定义服务商模式下可编辑。
    fn focusable_fields(&self) -> Vec<SettingsField> {
        let mut fields = vec![SettingsField::ApiKey];
        if self.provider == "custom" {
            fields.push(SettingsField::BaseUrl);
        }
        // 温度已改为滑块交互，不参与键盘 Tab 循环
        fields.push(SettingsField::MaxTokens);
        fields.push(SettingsField::SystemPrompt);
        fields
    }

    pub fn next_field(&mut self) {
        let fields = self.focusable_fields();
        self.active_field = match self.active_field {
            None => fields.first().copied(),
            Some(cur) => match fields.iter().position(|f| *f == cur) {
                Some(i) if i + 1 < fields.len() => Some(fields[i + 1]),
                _ => None,
            },
        };
    }

    pub fn prev_field(&mut self) {
        let fields = self.focusable_fields();
        self.active_field = match self.active_field {
            None => fields.last().copied(),
            Some(cur) => match fields.iter().position(|f| *f == cur) {
                Some(i) if i > 0 => Some(fields[i - 1]),
                _ => None,
            },
        };
    }

    /// Mask API key for display (show last 4 chars, rest as dots)
    pub fn masked_api_key(&self) -> String {
        let chars: Vec<char> = self.api_key.chars().collect();
        if chars.len() <= 4 {
            "•".repeat(chars.len())
        } else {
            let dots = "•".repeat(chars.len().saturating_sub(4));
            let last_four: String = chars.iter().rev().take(4).rev().collect();
            format!("{}{}", dots, last_four)
        }
    }

    /// 显示用的 API 密钥文本：显隐开关打开时明文，否则掩码
    pub fn display_api_key(&self) -> String {
        if self.show_api_key {
            self.api_key.clone()
        } else {
            self.masked_api_key()
        }
    }

    /// 切换 API 密钥显隐
    pub fn toggle_api_key_visibility(&mut self) {
        self.show_api_key = !self.show_api_key;
    }

    /// 温度是否合法（0.0-2.0）
    pub fn temperature_valid(&self) -> bool {
        matches!(self.temperature.trim().parse::<f32>(), Ok(v) if (0.0..=2.0).contains(&v))
    }

    /// Max Tokens 是否合法（1..=1_000_000 的正整数）
    pub fn max_tokens_valid(&self) -> bool {
        matches!(self.max_tokens.trim().parse::<u32>(), Ok(v) if (1..=1_000_000).contains(&v))
    }

    /// 滚动内容（delta>0 向下），并夹紧到有效范围
    pub fn scroll_by(&mut self, delta: f32) {
        self.scroll_offset = (self.scroll_offset + delta).clamp(0.0, self.content_height.max(0.0));
    }

    /// 夹紧滚动偏移（内容高度变化后调用）
    pub fn clamp_scroll(&mut self) {
        self.scroll_offset = self.scroll_offset.clamp(0.0, self.content_height.max(0.0));
    }

    /// 命中：API 密钥显隐按钮
    pub fn hit_test_api_key_toggle(&self, x: f32, y: f32) -> bool {
        if let Some((rx, ry, rw, rh)) = self.api_key_toggle_region {
            x >= rx && x < rx + rw && y >= ry && y < ry + rh
        } else {
            false
        }
    }

    /// 命中：温度滑块轨道，返回点击位置对应的温度（0.0-2.0，步进 0.1）
    pub fn hit_test_temp_slider(&self, x: f32, y: f32) -> Option<f32> {
        if let Some((rx, ry, rw, rh)) = self.temp_slider_region {
            if x >= rx - 4.0 && x <= rx + rw + 4.0 && y >= ry - 8.0 && y <= ry + rh + 8.0 {
                let ratio = ((x - rx) / rw).clamp(0.0, 1.0);
                let val = (ratio * 2.0 * 10.0).round() / 10.0;
                return Some(val);
            }
        }
        None
    }

    /// 拖拽中根据鼠标 x 更新温度（忽略 y，超出轨道两端自动夹紧）。返回是否有变化。
    pub fn set_temperature_from_slider_x(&mut self, x: f32) -> bool {
        if let Some((rx, _ry, rw, _rh)) = self.temp_slider_region {
            if rw > 0.0 {
                let ratio = ((x - rx) / rw).clamp(0.0, 1.0);
                let val = (ratio * 2.0 * 10.0).round() / 10.0;
                let new_str = format!("{:.1}", val);
                if self.temperature != new_str {
                    self.temperature = new_str;
                    return true;
                }
            }
        }
        false
    }

    /// 当前 AI 配置相对打开时的快照是否有未保存更改
    pub fn is_dirty(&self) -> bool {
        match &self.baseline_ai {
            Some(baseline) => self.to_ai_settings() != *baseline,
            None => false,
        }
    }

    /// 标记当前配置为已保存（更新基准快照）
    pub fn mark_saved(&mut self) {
        self.baseline_ai = Some(self.to_ai_settings());
    }

    // 模型管理方法

    pub fn provider_display_label(&self) -> String {
        match self.provider.as_str() {
            "kimi" => "Kimi".to_string(),
            "deepseek" => "DeepSeek".to_string(),
            _ => self.provider.clone(),
        }
    }

    pub fn provider_dropdown_options() -> Vec<(&'static str, &'static str)> {
        vec![
            ("deepseek", "DeepSeek"),
            ("kimi", "Kimi"),
            ("custom", "自定义"),
        ]
    }

    pub fn model_dropdown_options(&self) -> Vec<(String, String)> {
        // 返回当前服务商的模型列表
        match self.provider.as_str() {
            "kimi" => vec![("kimi-code".to_string(), "kimi-code".to_string())],
            "deepseek" => vec![("deepseek-v4-pro".to_string(), "deepseek-v4-pro".to_string())],
            _ => vec![(self.model.clone(), self.model.clone())],
        }
    }

    pub fn poll_test_result(&mut self) -> TestPollResult {
        if !self.is_testing {
            return TestPollResult::Pending;
        }
        let outcome = self
            .test_result
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        match outcome {
            Some(Ok(reply)) => {
                let snippet: String = reply.chars().take(60).collect();
                self.test_status = format!("✓ 连接成功：{}", snippet.trim());
                self.is_testing = false;
                if self.pending_save {
                    self.pending_save = false;
                    TestPollResult::SuccessWithPendingSave
                } else {
                    TestPollResult::Success
                }
            }
            Some(Err(e)) => {
                self.test_status = format!("✗ 连接失败：{}", e);
                self.is_testing = false;
                if self.pending_save {
                    self.pending_save = false;
                    TestPollResult::FailedWithPendingSave
                } else {
                    TestPollResult::Failed
                }
            }
            None => TestPollResult::Pending,
        }
    }

    /// 启动后台测试连接（非阻塞，HTTP 请求在后台线程执行）
    pub fn start_test_connection(&mut self, ai: AiSettings) {
        if self.is_testing {
            return;
        }
        if ai.api_key.trim().is_empty() {
            self.test_status = "✗ 请先填写 API 密钥".to_string();
            return;
        }
        self.is_testing = true;
        self.test_status = "正在测试连接…".to_string();
        if let Ok(mut slot) = self.test_result.lock() {
            *slot = None;
        }
        let result = Arc::clone(&self.test_result);
        std::thread::spawn(move || {
            let client = aether_ai::AiClient::new(&ai);
            let r = client.test_connection_safe();
            if let Ok(mut slot) = result.lock() {
                *slot = Some(r);
            }
        });
    }

    /// 命中检测：下拉触发区
    pub fn hit_test_dropdown_trigger(&self, x: f32, y: f32) -> Option<SettingsDropdownKind> {
        for (kind, tx, ty, tw, th) in &self.dropdown_trigger_regions {
            if x >= *tx && x < tx + tw && y >= *ty && y < ty + th {
                return Some(*kind);
            }
        }
        None
    }

    /// 命中检测：下拉选项（返回类型与索引）
    pub fn hit_test_dropdown_item(&self, x: f32, y: f32) -> Option<(SettingsDropdownKind, usize)> {
        for (kind, idx, ix, iy, iw, ih) in &self.dropdown_item_regions {
            if x >= *ix && x < ix + iw && y >= *iy && y < iy + ih {
                return Some((*kind, *idx));
            }
        }
        None
    }

    /// 按下拉索引选择服务商，并将模型重置为该服务商的第一个预置模型
    pub fn select_provider_by_index(&mut self, idx: usize) {
        let opts = Self::provider_dropdown_options();
        if let Some((id, _)) = opts.get(idx) {
            self.provider = id.to_string();
            // 根据选择的厂商自动设置 base_url
            match *id {
                "deepseek" => {
                    self.base_url = "https://api.deepseek.com/v1".to_string();
                }
                "kimi" => {
                    self.base_url = "https://api.moonshot.cn/v1".to_string();
                }
                _ => {
                    self.base_url = String::new();
                }
            }
            if let Some((mid, _)) = self.model_dropdown_options().first() {
                self.model = mid.clone();
            }
        }
    }

    /// 按下拉索引选择模型
    pub fn select_model_by_index(&mut self, idx: usize) {
        let opts = self.model_dropdown_options();
        if let Some((mid, _)) = opts.get(idx) {
            self.model = mid.clone();
        }
    }

    pub fn add_model_button_region(&mut self, button: ModelButton, x: f32, y: f32, w: f32, h: f32) {
        self.model_button_regions.push((button, x, y, w, h));
    }

    pub fn add_model_item_region(&mut self, id: String, x: f32, y: f32, w: f32, h: f32) {
        self.model_item_regions.push((id, x, y, w, h));
    }

    /// 命中检测：模型项（返回模型ID）
    pub fn hit_test_model_item(&self, x: f32, y: f32) -> Option<String> {
        for (id, ix, iy, iw, ih) in &self.model_item_regions {
            if x >= *ix && x < ix + iw && y >= *iy && y < iy + ih {
                return Some(id.clone());
            }
        }
        None
    }

    /// 命中检测：模型按钮（返回按钮类型和模型ID）
    pub fn hit_test_model_button(&self, x: f32, y: f32) -> Option<(ModelButton, String)> {
        for (button, bx, by, bw, bh) in &self.model_button_regions {
            if x >= *bx && x < bx + bw && y >= *by && y < by + bh {
                // 找到对应的模型项ID
                if let Some(model_id) = self.hit_test_model_item(x, y) {
                    return Some((*button, model_id));
                }
                // 如果是添加按钮，没有对应的模型项
                if *button == ModelButton::Add {
                    return Some((*button, String::new()));
                }
            }
        }
        None
    }

    /// 切换模型启用状态
    pub fn toggle_model_enabled(&mut self, model_id: &str) {
        if let Some(model) = self.models.iter_mut().find(|m| m.id == model_id) {
            model.enabled = !model.enabled;
        }
    }

    /// 删除模型
    pub fn delete_model(&mut self, model_id: &str) {
        self.models.retain(|m| m.id != model_id);
        if self.active_model_id.as_deref() == Some(model_id) {
            self.active_model_id = None;
        }
        if self.selected_model_id.as_deref() == Some(model_id) {
            self.selected_model_id = None;
        }
    }

    pub fn current_provider_button(&self) -> Option<ProviderTemplateButton> {
        match self.provider.as_str() {
            "deepseek" => Some(ProviderTemplateButton::DeepSeek),
            "kimi" => Some(ProviderTemplateButton::Kimi),
            _ => None,
        }
    }
}
