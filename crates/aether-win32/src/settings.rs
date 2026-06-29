use aether_shared::settings::{AiSettings, AppSettings};

/// 设置面板字段标识
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsField {
    Provider,
    ApiKey,
    BaseUrl,
    Model,
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
    /// AI 接口：provider / key / url / model / 测试连接
    Ai,
    /// 外观：侧边栏、密度等
    Appearance,
    /// 远程：SSH 主机等
    Remote,
}

impl SettingsTab {
    pub fn label(&self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::Ai => "AI",
            Self::Appearance => "外观",
            Self::Remote => "远程",
        }
    }

    pub const ALL: [SettingsTab; 4] = [
        SettingsTab::General,
        SettingsTab::Ai,
        SettingsTab::Appearance,
        SettingsTab::Remote,
    ];
}

/// AI 设置面板状态
#[derive(Clone, Debug)]
pub struct SettingsPanel {
    pub provider: String,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
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
}

impl SettingsPanel {
    pub fn new() -> Self {
        Self {
            provider: "openai".to_string(),
            api_key: String::new(),
            base_url: String::new(),
            model: "gpt-4".to_string(),
            active_field: None,
            hover_button: None,
            test_status: String::new(),
            is_testing: false,
            field_regions: Vec::new(),
            button_regions: Vec::new(),
            active_tab: SettingsTab::Ai,
            hover_tab: None,
            tab_regions: Vec::new(),
        }
    }

    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            provider: settings.ai.provider.clone(),
            api_key: settings.ai.api_key.clone(),
            base_url: settings.ai.base_url.clone().unwrap_or_default(),
            model: settings.ai.model.clone(),
            active_field: None,
            hover_button: None,
            test_status: String::new(),
            is_testing: false,
            field_regions: Vec::new(),
            button_regions: Vec::new(),
            active_tab: SettingsTab::Ai,
            hover_tab: None,
            tab_regions: Vec::new(),
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
        }
    }

    pub fn apply_settings(&mut self, settings: &AppSettings) {
        self.provider = settings.ai.provider.clone();
        self.api_key = settings.ai.api_key.clone();
        self.base_url = settings.ai.base_url.clone().unwrap_or_default();
        self.model = settings.ai.model.clone();
    }

    pub fn clear_regions(&mut self) {
        self.field_regions.clear();
        self.button_regions.clear();
        self.tab_regions.clear();
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
            }
        }
    }

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
            }
        }
    }

    pub fn next_field(&mut self) {
        let next = match self.active_field {
            None => Some(SettingsField::Provider),
            Some(SettingsField::Provider) => Some(SettingsField::ApiKey),
            Some(SettingsField::ApiKey) => Some(SettingsField::BaseUrl),
            Some(SettingsField::BaseUrl) => Some(SettingsField::Model),
            Some(SettingsField::Model) => None,
        };
        self.active_field = next;
    }

    pub fn prev_field(&mut self) {
        let prev = match self.active_field {
            None => Some(SettingsField::Model),
            Some(SettingsField::Model) => Some(SettingsField::BaseUrl),
            Some(SettingsField::BaseUrl) => Some(SettingsField::ApiKey),
            Some(SettingsField::ApiKey) => Some(SettingsField::Provider),
            Some(SettingsField::Provider) => None,
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
}
