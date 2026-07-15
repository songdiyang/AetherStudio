use aether_shared::settings::{AiSettings, AppSettings};

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
    /// 添加模型对话框字段
    ContextInput,
    ContextOutput,
    ToolCallRounds,
    /// 展示名称
    DisplayName,
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

    pub const ALL: [SettingsTab; 4] = [
        SettingsTab::General,
        SettingsTab::Ai,
        SettingsTab::Appearance,
        SettingsTab::Remote,
    ];
}

/// 服务商模板按钮
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderTemplateButton {
    DeepSeek,
    Kimi,
    Claude,
    CustomOpenAi,
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
    Activate,
    Delete,
}

/// 添加模型对话框按钮
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddModelDialogButton {
    Close,
    AddModel,
}

/// 添加模型对话框标签页
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddModelDialogTab {
    Provider,
    Custom,
}

impl AddModelDialogTab {
    pub const ALL: [AddModelDialogTab; 2] =
        [AddModelDialogTab::Provider, AddModelDialogTab::Custom];

    pub fn label(&self) -> &'static str {
        match self {
            AddModelDialogTab::Provider => "服务商",
            AddModelDialogTab::Custom => "自定义",
        }
    }
}

/// 添加模型下拉类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddModelDropdownKind {
    Provider,
    Model,
}

/// 模型配置项
#[derive(Clone, Debug)]
pub struct ModelConfig {
    pub id: String,
    pub name: String,
    pub provider: String,
}

/// 添加模型对话框状态
#[derive(Clone, Debug)]
pub struct AddModelDialog {
    pub visible: bool,
    pub active_tab: AddModelDialogTab,
    pub hover_tab: Option<AddModelDialogTab>,
    pub hover_button: Option<AddModelDialogButton>,
    pub close_region: Option<(f32, f32, f32, f32)>,
    pub open_dropdown: Option<AddModelDropdownKind>,
    pub hover_dropdown: Option<AddModelDropdownKind>,
    pub hover_dropdown_index: Option<usize>,
    pub selected_provider_button: Option<ProviderTemplateButton>,
    pub selected_model_id: String,
    pub display_name: String,
    pub context_input: String,
    pub context_output: String,
    pub tool_call_rounds: String,
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub field_regions: Vec<(SettingsField, f32, f32, f32, f32)>,
    pub button_regions: Vec<(AddModelDialogButton, f32, f32, f32, f32)>,
    pub dropdown_trigger_regions: Vec<(AddModelDropdownKind, f32, f32, f32, f32)>,
    pub dropdown_item_regions: Vec<(AddModelDropdownKind, usize, f32, f32, f32, f32)>,
    pub provider_template_regions: Vec<(ProviderTemplateButton, f32, f32, f32, f32)>,
    pub advanced_toggle_region: Option<(f32, f32, f32, f32)>,
    pub advanced_expanded: bool,
    pub tab_regions: Vec<(AddModelDialogTab, f32, f32, f32, f32)>,
    pub active_field: Option<SettingsField>,
}

impl AddModelDialog {
    pub fn new() -> Self {
        Self {
            visible: false,
            active_tab: AddModelDialogTab::Provider,
            hover_tab: None,
            hover_button: None,
            close_region: None,
            open_dropdown: None,
            hover_dropdown: None,
            hover_dropdown_index: None,
            selected_provider_button: None,
            selected_model_id: String::new(),
            display_name: String::new(),
            context_input: String::new(),
            context_output: String::new(),
            tool_call_rounds: "3".to_string(),
            provider: String::new(),
            base_url: String::new(),
            model: String::new(),
            field_regions: Vec::new(),
            button_regions: Vec::new(),
            dropdown_trigger_regions: Vec::new(),
            dropdown_item_regions: Vec::new(),
            provider_template_regions: Vec::new(),
            advanced_toggle_region: None,
            advanced_expanded: false,
            tab_regions: Vec::new(),
            active_field: None,
        }
    }

    pub fn provider_label(&self) -> String {
        self.selected_provider_button
            .map(|b| match b {
                ProviderTemplateButton::DeepSeek => "DeepSeek".to_string(),
                ProviderTemplateButton::Kimi => "Kimi".to_string(),
                ProviderTemplateButton::Claude => "Claude".to_string(),
                ProviderTemplateButton::CustomOpenAi => "OpenAI".to_string(),
            })
            .unwrap_or_else(|| "请选择".to_string())
    }

    pub fn masked_api_key(&self) -> String {
        "••••".to_string()
    }

    pub fn model_options(&self) -> Vec<String> {
        vec![]
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
    // 添加模型对话框
    pub add_model_dialog: AddModelDialog,
    // 模型按钮/项命中区域
    model_button_regions: Vec<(ModelButton, f32, f32, f32, f32)>,
    model_item_regions: Vec<(String, f32, f32, f32, f32)>,
    // 下拉框
    pub open_dropdown: Option<SettingsDropdownKind>,
    pub dropdown_trigger_regions: Vec<(SettingsDropdownKind, f32, f32, f32, f32)>,
    pub dropdown_item_regions: Vec<(SettingsDropdownKind, usize, f32, f32, f32, f32)>,
    pub hover_dropdown: Option<SettingsDropdownKind>,
    pub hover_dropdown_index: Option<usize>,
}

impl SettingsPanel {
    pub fn new() -> Self {
        Self {
            provider: "openai".to_string(),
            api_key: String::new(),
            base_url: String::new(),
            model: "gpt-4".to_string(),
            temperature: "0.7".to_string(),
            max_tokens: "2048".to_string(),
            system_prompt: String::new(),
            active_field: None,
            hover_button: None,
            test_status: String::new(),
            is_testing: false,
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
            add_model_dialog: AddModelDialog::new(),
            model_button_regions: Vec::new(),
            model_item_regions: Vec::new(),
            open_dropdown: None,
            dropdown_trigger_regions: Vec::new(),
            dropdown_item_regions: Vec::new(),
            hover_dropdown: None,
            hover_dropdown_index: None,
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
            add_model_dialog: AddModelDialog::new(),
            model_button_regions: Vec::new(),
            model_item_regions: Vec::new(),
            open_dropdown: None,
            dropdown_trigger_regions: Vec::new(),
            dropdown_item_regions: Vec::new(),
            hover_dropdown: None,
            hover_dropdown_index: None,
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
        self.provider = settings.ai.provider.clone();
        self.api_key = settings.ai.api_key.clone();
        self.base_url = settings.ai.base_url.clone().unwrap_or_default();
        self.model = settings.ai.model.clone();
        self.temperature = settings
            .ai
            .temperature
            .map(|t| t.to_string())
            .unwrap_or_else(|| "0.7".to_string());
        self.max_tokens = settings
            .ai
            .max_tokens
            .map(|m| m.to_string())
            .unwrap_or_else(|| "2048".to_string());
        self.system_prompt = settings.ai.system_prompt.clone().unwrap_or_default();
    }

    pub fn clear_regions(&mut self) {
        self.field_regions.clear();
        self.button_regions.clear();
        self.tab_regions.clear();
        self.model_button_regions.clear();
        self.model_item_regions.clear();
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

    pub fn next_field(&mut self) {
        let next = match self.active_field {
            None => Some(SettingsField::Provider),
            Some(SettingsField::Provider) => Some(SettingsField::ApiKey),
            Some(SettingsField::ApiKey) => Some(SettingsField::BaseUrl),
            Some(SettingsField::BaseUrl) => Some(SettingsField::Model),
            Some(SettingsField::Model) => Some(SettingsField::Temperature),
            Some(SettingsField::Temperature) => Some(SettingsField::MaxTokens),
            Some(SettingsField::MaxTokens) => Some(SettingsField::SystemPrompt),
            Some(SettingsField::SystemPrompt) => None,
            _ => None,
        };
        self.active_field = next;
    }

    pub fn prev_field(&mut self) {
        let prev = match self.active_field {
            None => Some(SettingsField::SystemPrompt),
            Some(SettingsField::SystemPrompt) => Some(SettingsField::MaxTokens),
            Some(SettingsField::MaxTokens) => Some(SettingsField::Temperature),
            Some(SettingsField::Temperature) => Some(SettingsField::Model),
            Some(SettingsField::Model) => Some(SettingsField::BaseUrl),
            Some(SettingsField::BaseUrl) => Some(SettingsField::ApiKey),
            Some(SettingsField::ApiKey) => Some(SettingsField::Provider),
            Some(SettingsField::Provider) => None,
            _ => None,
        };
        self.active_field = prev;
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

    // 模型管理方法

    pub fn provider_display_label(&self) -> String {
        match self.provider.as_str() {
            "openai" => "OpenAI".to_string(),
            "kimi" => "Kimi".to_string(),
            "deepseek" => "DeepSeek".to_string(),
            "claude" => "Claude".to_string(),
            _ => self.provider.clone(),
        }
    }

    pub fn provider_dropdown_options() -> Vec<(&'static str, &'static str)> {
        vec![
            ("openai", "OpenAI"),
            ("kimi", "Kimi"),
            ("deepseek", "DeepSeek"),
            ("claude", "Claude"),
            ("custom", "自定义"),
        ]
    }

    pub fn model_dropdown_options(&self) -> Vec<(String, String)> {
        // 返回当前服务商的模型列表
        match self.provider.as_str() {
            "openai" => vec![
                ("gpt-4".to_string(), "GPT-4".to_string()),
                ("gpt-4-turbo".to_string(), "GPT-4 Turbo".to_string()),
                ("gpt-3.5-turbo".to_string(), "GPT-3.5 Turbo".to_string()),
            ],
            "kimi" => vec![
                ("moonshot-v1-8k".to_string(), "Moonshot 8K".to_string()),
                ("moonshot-v1-32k".to_string(), "Moonshot 32K".to_string()),
                ("moonshot-v1-128k".to_string(), "Moonshot 128K".to_string()),
            ],
            "deepseek" => vec![
                ("deepseek-chat".to_string(), "DeepSeek Chat".to_string()),
                ("deepseek-coder".to_string(), "DeepSeek Coder".to_string()),
            ],
            "claude" => vec![
                ("claude-3-opus".to_string(), "Claude 3 Opus".to_string()),
                ("claude-3-sonnet".to_string(), "Claude 3 Sonnet".to_string()),
                ("claude-3-haiku".to_string(), "Claude 3 Haiku".to_string()),
            ],
            _ => vec![(self.model.clone(), self.model.clone())],
        }
    }

    pub fn poll_test_result(&mut self) -> bool {
        // 模拟测试连接结果轮询
        if self.is_testing {
            self.is_testing = false;
            self.test_status = "连接成功".to_string();
            true
        } else {
            false
        }
    }

    pub fn add_model_button_region(&mut self, button: ModelButton, x: f32, y: f32, w: f32, h: f32) {
        self.model_button_regions.push((button, x, y, w, h));
    }

    pub fn add_model_item_region(&mut self, id: String, x: f32, y: f32, w: f32, h: f32) {
        self.model_item_regions.push((id, x, y, w, h));
    }

    pub fn dropdown_items(&self, kind: AddModelDropdownKind) -> Vec<String> {
        match kind {
            AddModelDropdownKind::Provider => Self::provider_dropdown_options()
                .into_iter()
                .map(|(_, name)| name.to_string())
                .collect(),
            AddModelDropdownKind::Model => self
                .model_dropdown_options()
                .into_iter()
                .map(|(_, name)| name)
                .collect(),
        }
    }

    pub fn current_provider_button(&self) -> Option<ProviderTemplateButton> {
        match self.provider.as_str() {
            "deepseek" => Some(ProviderTemplateButton::DeepSeek),
            "kimi" => Some(ProviderTemplateButton::Kimi),
            "claude" => Some(ProviderTemplateButton::Claude),
            "openai" => Some(ProviderTemplateButton::CustomOpenAi),
            _ => None,
        }
    }
}
