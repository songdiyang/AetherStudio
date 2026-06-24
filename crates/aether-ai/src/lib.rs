use aether_shared::settings::AiSettings;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiProvider {
    OpenAi,
    Claude,
    Kimi,
    Azure,
    Custom,
}

impl AiProvider {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "openai" | "gpt" | "gpt-4" | "gpt-3.5-turbo" => Self::OpenAi,
            "claude" | "anthropic" => Self::Claude,
            "kimi" | "moonshot" => Self::Kimi,
            "azure" | "azure_openai" | "azure-openai" => Self::Azure,
            _ => Self::Custom,
        }
    }

    pub fn default_base_url(&self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Claude => "https://api.anthropic.com/v1",
            Self::Kimi => "https://api.moonshot.cn/v1",
            Self::Azure => "",
            Self::Custom => "",
        }
    }

    pub fn default_model(&self) -> &'static str {
        match self {
            Self::OpenAi => "gpt-4",
            Self::Claude => "claude-3-sonnet-20240229",
            Self::Kimi => "moonshot-v1-8k",
            Self::Azure => "gpt-4",
            Self::Custom => "",
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Claude => "claude",
            Self::Kimi => "kimi",
            Self::Azure => "azure",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug)]
pub enum AiError {
    Http(String),
    Parse(String),
    Config(String),
    Api { code: u16, message: String },
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiError::Http(e) => write!(f, "HTTP error: {}", e),
            AiError::Parse(e) => write!(f, "Parse error: {}", e),
            AiError::Config(e) => write!(f, "Config error: {}", e),
            AiError::Api { code, message } => write!(f, "API error {}: {}", code, message),
        }
    }
}

impl std::error::Error for AiError {}

#[derive(Clone, Debug)]
pub struct AiConfig {
    pub provider: AiProvider,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
}

impl AiConfig {
    pub fn from_settings(settings: &AiSettings) -> Self {
        let provider = AiProvider::from_str(&settings.provider);
        let base_url = settings.base_url.clone().or_else(|| {
            let default = provider.default_base_url();
            if default.is_empty() {
                None
            } else {
                Some(default.to_string())
            }
        });
        let model = if settings.model.is_empty() {
            provider.default_model().to_string()
        } else {
            settings.model.clone()
        };
        Self {
            provider,
            api_key: settings.api_key.clone(),
            base_url,
            model,
        }
    }
}

pub struct AiClient {
    config: AiConfig,
    http: ureq::Agent,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

impl AiClient {
    pub fn new(config: &AiSettings) -> Self {
        let config = AiConfig::from_settings(config);
        let http = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(30))
            .build();
        Self { config, http }
    }

    pub fn test_connection(&self) -> Result<String, AiError> {
        self.complete("Hello, this is a test. Please reply with a simple greeting.")
    }

    pub fn complete(&self, prompt: &str) -> Result<String, AiError> {
        match self.config.provider {
            AiProvider::OpenAi | AiProvider::Kimi | AiProvider::Azure | AiProvider::Custom => {
                self.complete_openai_compatible(prompt)
            }
            AiProvider::Claude => self.complete_claude(prompt),
        }
    }

    pub fn chat_completion(&self, messages: &[ChatMessage]) -> Result<String, AiError> {
        match self.config.provider {
            AiProvider::OpenAi | AiProvider::Kimi | AiProvider::Azure | AiProvider::Custom => {
                self.chat_openai_compatible(messages)
            }
            AiProvider::Claude => self.chat_claude(messages),
        }
    }

    fn complete_openai_compatible(&self, prompt: &str) -> Result<String, AiError> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1");
        let url = format!("{}/chat/completions", base_url);

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": 100,
        });

        let response = self
            .http
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.config.api_key))
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| AiError::Http(e.to_string()))?;

        let status = response.status();
        if status != 200 {
            let text = response.into_string().unwrap_or_default();
            return Err(AiError::Api {
                code: status,
                message: text,
            });
        }

        let json: serde_json::Value = response
            .into_json()
            .map_err(|e| AiError::Parse(e.to_string()))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(content)
    }

    fn complete_claude(&self, prompt: &str) -> Result<String, AiError> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com/v1");
        let url = format!("{}/messages", base_url);

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": 100,
        });

        let response = self
            .http
            .post(&url)
            .set("x-api-key", &self.config.api_key)
            .set("anthropic-version", "2023-06-01")
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| AiError::Http(e.to_string()))?;

        let status = response.status();
        if status != 200 {
            let text = response.into_string().unwrap_or_default();
            return Err(AiError::Api {
                code: status,
                message: text,
            });
        }

        let json: serde_json::Value = response
            .into_json()
            .map_err(|e| AiError::Parse(e.to_string()))?;

        let content = json["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(content)
    }

    fn chat_openai_compatible(&self, messages: &[ChatMessage]) -> Result<String, AiError> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1");
        let url = format!("{}/chat/completions", base_url);

        let msgs: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                })
            })
            .collect();

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": msgs,
            "max_tokens": 2048,
        });

        let response = self
            .http
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.config.api_key))
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| AiError::Http(e.to_string()))?;

        let status = response.status();
        if status != 200 {
            let text = response.into_string().unwrap_or_default();
            return Err(AiError::Api {
                code: status,
                message: text,
            });
        }

        let json: serde_json::Value = response
            .into_json()
            .map_err(|e| AiError::Parse(e.to_string()))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(content)
    }

    fn chat_claude(&self, messages: &[ChatMessage]) -> Result<String, AiError> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com/v1");
        let url = format!("{}/messages", base_url);

        let msgs: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                })
            })
            .collect();

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": msgs,
            "max_tokens": 2048,
        });

        let response = self
            .http
            .post(&url)
            .set("x-api-key", &self.config.api_key)
            .set("anthropic-version", "2023-06-01")
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| AiError::Http(e.to_string()))?;

        let status = response.status();
        if status != 200 {
            let text = response.into_string().unwrap_or_default();
            return Err(AiError::Api {
                code: status,
                message: text,
            });
        }

        let json: serde_json::Value = response
            .into_json()
            .map_err(|e| AiError::Parse(e.to_string()))?;

        let content = json["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(content)
    }
}
