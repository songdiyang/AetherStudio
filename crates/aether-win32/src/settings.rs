use aether_shared::settings::{AiSettings, AppSettings};

/// Settings panel field identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsField {
    Provider,
    ApiKey,
    BaseUrl,
    Model,
}

/// Settings panel button identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsButton {
    Save,
    TestConnection,
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
    }

    pub fn add_field_region(&mut self, field: SettingsField, x: f32, y: f32, w: f32, h: f32) {
        self.field_regions.push((field, x, y, w, h));
    }

    pub fn add_button_region(&mut self, button: SettingsButton, x: f32, y: f32, w: f32, h: f32) {
        self.button_regions.push((button, x, y, w, h));
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
        if self.api_key.len() <= 4 {
            "•".repeat(self.api_key.len())
        } else {
            let dots = "•".repeat(self.api_key.len().saturating_sub(4));
            format!(
                "{}{}",
                dots,
                &self.api_key[self.api_key.len().saturating_sub(4)..]
            )
        }
    }
}
