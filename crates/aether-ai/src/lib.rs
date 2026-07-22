use aether_shared::settings::AiSettings;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc;
use url::Url;

// H-01: SSRF DNS 重绑定限制说明
//
// 当前实现对 DNS 解析返回的所有 IP 做私有地址校验（resolve_and_lock），
// 能阻断「域名始终解析到内网 IP」的静态攻击。
//
// 但由于 ureq + rustls 不支持在保持 TLS 主机名校验的前提下固定连接 IP，
// DNS 重绑定攻击（验证时返回公网 IP，连接时返回 169.254.169.254）仍有残余风险。
// 彻底修复需要自定义 TLS connector + IP pinning，属于架构级改造，暂不实施。
// 此处保留 DNS 校验作为纵深防御层，并移除从未使用的 verified_ips 死代码。

#[derive(Clone, PartialEq, Eq)]
pub enum AiProvider {
    OpenAi,
    Claude,
    Kimi,
    Azure,
    Custom,
}

impl std::fmt::Debug for AiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenAi => write!(f, "OpenAi"),
            Self::Claude => write!(f, "Claude"),
            Self::Kimi => write!(f, "Kimi"),
            Self::Azure => write!(f, "Azure"),
            Self::Custom => write!(f, "Custom"),
        }
    }
}

impl AiProvider {
    #[allow(clippy::should_implement_trait)]
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
    /// H-21: message 已截断至 200 字符，但仍可能含敏感信息，
    /// 展示给用户时应使用 `safe_display()` 而非 `Display`。
    Api {
        code: u16,
        message: String,
    },
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

impl AiError {
    /// H-18 / H-21: 返回对用户安全的错误描述，不包含原始 API 响应体。
    ///
    /// `Display` 实现包含完整（已截断）的 API 响应体，可能含 API Key 等敏感信息，
    /// 仅供日志使用。展示给用户时应调用此方法，仅返回 HTTP 状态码和通用描述。
    pub fn safe_display(&self) -> String {
        match self {
            AiError::Http(_) => "网络请求失败，请检查网络连接".to_string(),
            AiError::Parse(_) => "API 响应解析失败".to_string(),
            AiError::Config(e) => e.clone(),
            AiError::Api { code, .. } => {
                let desc = match *code {
                    401 => "API Key 无效或已过期",
                    403 => "API Key 权限不足",
                    404 => "请求的资源不存在（请检查 Base URL 和模型名）",
                    429 => "请求频率超限，请稍后重试",
                    500..=599 => "API 服务器内部错误，请稍后重试",
                    _ => "API 请求失败",
                };
                format!("HTTP {}: {}", code, desc)
            }
        }
    }
}

#[derive(Clone)]
pub struct AiConfig {
    pub provider: AiProvider,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub system_prompt: Option<String>,
}

impl std::fmt::Debug for AiConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiConfig")
            .field("provider", &self.provider)
            .field("api_key", &"[REDACTED]")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .field(
                "system_prompt",
                &self.system_prompt.as_deref().map(|_| "[PRESENT]"),
            )
            .finish()
    }
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
            temperature: settings.temperature,
            max_tokens: settings.max_tokens,
            system_prompt: settings.system_prompt.clone(),
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

/// AI 流式响应事件
#[derive(Clone, Debug)]
pub enum AiStreamEvent {
    /// 一个新的文本 token（最终回答内容）
    Token(String),
    /// 一个新的"深度思考"token（如 DeepSeek reasoner 的 reasoning_content）
    Reasoning(String),
    /// 流结束
    Done,
    /// 流式过程中出现错误
    Error(String),
}

/// 已解析并校验的公网端点
#[derive(Debug, PartialEq, Eq)]
struct ResolvedEndpoint {
    host: String,
    port: u16,
}

/// 将消息列表拆分为 Claude 格式：system 消息合并为顶层 system 文本，
/// 其余消息（user/assistant）原样保留。
///
/// Anthropic /messages 接口的 messages 数组只允许 user/assistant 角色，
/// system 内容必须放在请求的顶层 system 字段，否则接口返回 400。
fn split_claude_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<serde_json::Value>) {
    let mut system_parts: Vec<&str> = Vec::new();
    let mut msgs: Vec<serde_json::Value> = Vec::new();
    for m in messages {
        if m.role == "system" {
            system_parts.push(&m.content);
        } else {
            msgs.push(serde_json::json!({
                "role": m.role,
                "content": m.content,
            }));
        }
    }
    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };
    (system, msgs)
}

impl AiClient {
    pub fn new(config: &AiSettings) -> Self {
        let config = AiConfig::from_settings(config);
        // SEC-C02: 禁用自动重定向，防止 SSRF 通过 302 跳转到内网地址
        let http = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(30))
            .redirects(0)
            .build();
        Self { config, http }
    }

    pub fn test_connection(&self) -> Result<String, AiError> {
        self.complete("Hello, this is a test. Please reply with a simple greeting.")
    }

    /// H-18: test_connection 的安全版本，错误信息经过脱敏处理，
    /// 可直接用于 UI 展示。调用方无需再单独 sanitize。
    pub fn test_connection_safe(&self) -> Result<String, String> {
        self.test_connection().map_err(|e| e.safe_display())
    }

    fn validate_https(url: &str) -> Result<(), AiError> {
        if !url.starts_with("https://") {
            return Err(AiError::Config(format!(
                "API base URL 必须使用 HTTPS: {}",
                url
            )));
        }
        Ok(())
    }

    /// 校验 URL 不属于私有/保留 IP 范围（SSRF 防护）
    /// SEC-C05: 使用 url::Url 进行严格 URL 解析，防止 userinfo/IPv6 绕过
    /// SEC-C03: 在发起 HTTP 请求前进行二次 DNS 校验（TOCTOU 防护）
    /// AI-H04: 扩展云元数据黑名单
    fn validate_not_private_ip(url_str: &str) -> Result<(), AiError> {
        // SEC-C05: 使用 url::Url 进行严格的 URL 解析
        let parsed =
            Url::parse(url_str).map_err(|e| AiError::Config(format!("无效的 URL 格式: {}", e)))?;

        let host_str = parsed
            .host_str()
            .ok_or_else(|| AiError::Config("URL 缺少主机名".to_string()))?;

        let port = parsed.port().unwrap_or(443);

        // 检查是否为 IP 地址（包括 IPv6）
        if let Ok(ip) = host_str.parse::<std::net::IpAddr>() {
            Self::check_ip_private(ip, host_str)?;
        }

        // SEC-C03: DNS TOCTOU 防护 — 对主机名做一次 DNS 解析并校验所有 IP
        // 后续在发起 HTTP 请求前还会做二次校验
        if let Ok(addrs) = std::net::ToSocketAddrs::to_socket_addrs(&(host_str, port)) {
            for addr in addrs {
                let ip = addr.ip();
                Self::check_ip_private(ip, host_str)?;
            }
        }

        // AI-H04: 阻止常见云元数据端点（扩展黑名单）
        let blocked_hosts_lower = host_str.to_lowercase();
        let blocked = [
            // AWS
            "169.254.169.254",
            "fd00:ec2::254",
            // GCP
            "metadata.google.internal",
            "metadata.google",
            // Azure
            "metadata.azure.internal",
            "169.254.169.253",
            // 阿里云
            "100.100.100.200",
            // 腾讯云
            "metadata.tencentyun.com",
        ];
        for blocked_host in &blocked {
            if blocked_hosts_lower == *blocked_host {
                return Err(AiError::Config(format!("禁止访问元数据端点: {}", host_str)));
            }
        }
        Ok(())
    }

    /// 检查单个 IP 是否为私有/保留地址
    fn check_ip_private(ip: std::net::IpAddr, host_str: &str) -> Result<(), AiError> {
        let is_private = match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_private()
                    || v4.is_link_local()
                    || v4.is_loopback()
                    || v4.is_multicast()
                    || v4.is_broadcast()
                    || v4.is_documentation()
            }
            std::net::IpAddr::V6(v6) => {
                if let Some(v4) = v6.to_ipv4_mapped() {
                    v4.is_private()
                        || v4.is_link_local()
                        || v4.is_loopback()
                        || v4.is_multicast()
                        || v4.is_broadcast()
                        || v4.is_documentation()
                } else {
                    v6.is_loopback() || v6.is_multicast() || v6.is_unspecified()
                }
            }
        };
        if is_private || ip.is_unspecified() || ip.is_loopback() {
            return Err(AiError::Config(format!(
                "禁止访问私有/本地地址: {} (解析自 {})",
                ip, host_str
            )));
        }
        Ok(())
    }

    /// SEC-A01: 解析并校验 DNS 结果，阻断「域名始终解析到内网 IP」的攻击。
    ///
    /// H-01: 此方法对 DNS 解析返回的所有 IP 做私有地址校验，但不固定连接 IP。
    /// DNS 重绑定攻击（验证后 DNS 返回不同 IP）的彻底防护需要自定义 TLS connector，
    /// 属于架构级改造，暂不实施。当前校验作为纵深防御层保留。
    fn resolve_and_lock(url_str: &str) -> Result<ResolvedEndpoint, AiError> {
        let parsed =
            Url::parse(url_str).map_err(|e| AiError::Config(format!("无效的 URL: {}", e)))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| AiError::Config("URL 缺少主机名".to_string()))?;
        let port = parsed.port().unwrap_or(443);

        // 如果已经是 IP 地址，直接校验
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            Self::check_ip_private(ip, host)?;
            return Ok(ResolvedEndpoint {
                host: host.to_string(),
                port,
            });
        }

        // DNS 解析并校验所有 IP
        let addrs: Vec<std::net::SocketAddr> =
            std::net::ToSocketAddrs::to_socket_addrs(&(host, port))
                .map_err(|e| AiError::Config(format!("DNS 解析失败: {}", e)))?
                .collect();
        if addrs.is_empty() {
            return Err(AiError::Config(format!("DNS 解析无结果: {}", host)));
        }
        for addr in &addrs {
            Self::check_ip_private(addr.ip(), host)?;
        }
        Ok(ResolvedEndpoint {
            host: host.to_string(),
            port,
        })
    }

    /// SEC-C03: TOCTOU 二次 DNS 校验 — 在发起 HTTP 请求前调用
    /// 验证 DNS 解析结果未在两次查询间被篡改为私有地址
    fn validate_tcp_connect_target(url_str: &str) -> Result<(), AiError> {
        Self::resolve_and_lock(url_str).map(|_| ())
    }

    /// 安全读取响应体，限制最大 10MB
    fn read_limited_response(response: ureq::Response) -> Result<String, AiError> {
        let mut reader = response.into_reader();
        let mut buf = Vec::with_capacity(4096);
        let max_size = 10 * 1024 * 1024; // 10MB
        let mut total = 0usize;
        let mut chunk = [0u8; 4096];

        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    total += n;
                    if total > max_size {
                        return Err(AiError::Http("响应体超过 10MB 限制".to_string()));
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                Err(e) => return Err(AiError::Http(format!("读取响应失败: {}", e))),
            }
        }
        String::from_utf8(buf).map_err(|e| AiError::Parse(format!("UTF-8 解码失败: {}", e)))
    }

    /// H-21: 截断错误消息至 200 字符（在 UTF-8 字符边界上截断），防止大量响应体传入 UI
    fn truncate_error_message(text: &str) -> String {
        const MAX_ERR_LEN: usize = 200;
        if text.len() <= MAX_ERR_LEN {
            return text.to_string();
        }
        let safe_len = text.floor_char_boundary(MAX_ERR_LEN);
        let mut truncated = text[..safe_len].to_string();
        truncated.push_str("...(已截断)");
        truncated
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
        Self::validate_https(base_url)?;
        Self::validate_not_private_ip(base_url)?;

        // AI-M01: 空 API Key 前置检查
        if self.config.api_key.is_empty() {
            return Err(AiError::Config("API Key 未设置".to_string()));
        }

        // SEC-C03: TOCTOU 二次 DNS 校验，仅在请求前做 SSRF 校验，
        // 不再用解析到的 IP 直连，以保留 TLS 主机名证书校验
        Self::validate_tcp_connect_target(base_url)?;
        // 始终使用原始 base_url（含域名），TLS 证书验证才能匹配域名
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
            let text = Self::read_limited_response(response)?;
            // H-21: 截断 API 错误响应体至 200 字符，防止大量数据（可能含敏感信息）传入 UI
            return Err(AiError::Api {
                code: status,
                message: Self::truncate_error_message(&text),
            });
        }

        let text = Self::read_limited_response(response)?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| AiError::Parse(e.to_string()))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| AiError::Parse("Unexpected API response structure".to_string()))?
            .to_string();

        Ok(content)
    }

    fn complete_claude(&self, prompt: &str) -> Result<String, AiError> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com/v1");
        Self::validate_https(base_url)?;
        Self::validate_not_private_ip(base_url)?;

        // AI-M01: 空 API Key 前置检查
        if self.config.api_key.is_empty() {
            return Err(AiError::Config("API Key 未设置".to_string()));
        }

        // SEC-C03: TOCTOU 二次 DNS 校验，仅在请求前做 SSRF 校验，
        // 不再用解析到的 IP 直连，以保留 TLS 主机名证书校验
        Self::validate_tcp_connect_target(base_url)?;
        // 始终使用原始 base_url（含域名），TLS 证书验证才能匹配域名
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
            let text = Self::read_limited_response(response)?;
            return Err(AiError::Api {
                code: status,
                // H-21: 截断 API 错误响应体至 200 字符
                message: Self::truncate_error_message(&text),
            });
        }

        let text = Self::read_limited_response(response)?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| AiError::Parse(e.to_string()))?;

        let content = json["content"][0]["text"]
            .as_str()
            .ok_or_else(|| AiError::Parse("Unexpected API response structure".to_string()))?
            .to_string();

        Ok(content)
    }

    fn chat_openai_compatible(&self, messages: &[ChatMessage]) -> Result<String, AiError> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1");
        Self::validate_https(base_url)?;
        Self::validate_not_private_ip(base_url)?;

        // AI-M01: 空 API Key 前置检查
        if self.config.api_key.is_empty() {
            return Err(AiError::Config("API Key 未设置".to_string()));
        }

        // SEC-C03: TOCTOU 二次 DNS 校验，仅在请求前做 SSRF 校验，
        // 不再用解析到的 IP 直连，以保留 TLS 主机名证书校验
        Self::validate_tcp_connect_target(base_url)?;
        // 始终使用原始 base_url（含域名），TLS 证书验证才能匹配域名
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
            let text = Self::read_limited_response(response)?;
            return Err(AiError::Api {
                code: status,
                // H-21: 截断 API 错误响应体至 200 字符
                message: Self::truncate_error_message(&text),
            });
        }

        let text = Self::read_limited_response(response)?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| AiError::Parse(e.to_string()))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| AiError::Parse("Unexpected API response structure".to_string()))?
            .to_string();

        Ok(content)
    }

    fn chat_claude(&self, messages: &[ChatMessage]) -> Result<String, AiError> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com/v1");
        Self::validate_https(base_url)?;
        Self::validate_not_private_ip(base_url)?;

        // AI-M01: 空 API Key 前置检查
        if self.config.api_key.is_empty() {
            return Err(AiError::Config("API Key 未设置".to_string()));
        }

        // SEC-C03: TOCTOU 二次 DNS 校验，仅在请求前做 SSRF 校验，
        // 不再用解析到的 IP 直连，以保留 TLS 主机名证书校验
        Self::validate_tcp_connect_target(base_url)?;
        // 始终使用原始 base_url（含域名），TLS 证书验证才能匹配域名
        let url = format!("{}/messages", base_url);

        let (system, msgs) = split_claude_messages(messages);

        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": msgs,
            "max_tokens": 2048,
        });
        if let Some(system) = system {
            body["system"] = serde_json::json!(system);
        }

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
            let text = Self::read_limited_response(response)?;
            return Err(AiError::Api {
                code: status,
                // H-21: 截断 API 错误响应体至 200 字符
                message: Self::truncate_error_message(&text),
            });
        }

        let text = Self::read_limited_response(response)?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| AiError::Parse(e.to_string()))?;

        let content = json["content"][0]["text"]
            .as_str()
            .ok_or_else(|| AiError::Parse("Unexpected API response structure".to_string()))?
            .to_string();

        Ok(content)
    }

    /// 流式聊天补全。
    ///
    /// 返回一个 Receiver，后台线程会在每次收到 token 时发送 `AiStreamEvent::Token`，
    /// 流结束时发送 `AiStreamEvent::Done`，出错时发送 `AiStreamEvent::Error`。
    pub fn chat_completion_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<mpsc::Receiver<AiStreamEvent>, AiError> {
        match self.config.provider {
            AiProvider::Claude => self.stream_claude(messages),
            _ => self.stream_openai_compatible(messages),
        }
    }

    fn stream_openai_compatible(
        &self,
        messages: &[ChatMessage],
    ) -> Result<mpsc::Receiver<AiStreamEvent>, AiError> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1");
        Self::validate_https(base_url)?;
        Self::validate_not_private_ip(base_url)?;
        Self::validate_tcp_connect_target(base_url)?;

        if self.config.api_key.is_empty() {
            return Err(AiError::Config("API Key 未设置".to_string()));
        }

        let url = format!("{}/chat/completions", base_url);
        // system 消息由调用方在消息列表中构建（见 build_chat_prompt，固定为第一条），
        // 此处不再从 config.system_prompt 重复注入，避免同一提示词发送两遍。
        let body_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": body_messages,
            "stream": true,
        });
        if let Some(t) = self.config.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(m) = self.config.max_tokens {
            body["max_tokens"] = serde_json::json!(m);
        }

        let response = self
            .http
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.config.api_key))
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| AiError::Http(e.to_string()))?;

        Self::stream_response(response)
    }

    fn stream_claude(
        &self,
        messages: &[ChatMessage],
    ) -> Result<mpsc::Receiver<AiStreamEvent>, AiError> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com/v1");
        Self::validate_https(base_url)?;
        Self::validate_not_private_ip(base_url)?;
        Self::validate_tcp_connect_target(base_url)?;

        if self.config.api_key.is_empty() {
            return Err(AiError::Config("API Key 未设置".to_string()));
        }

        let url = format!("{}/messages", base_url);
        let (system, msgs) = split_claude_messages(messages);
        let max_tokens = self.config.max_tokens.unwrap_or(2048);

        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": msgs,
            "max_tokens": max_tokens,
            "stream": true,
        });
        if let Some(system) = system {
            body["system"] = serde_json::json!(system);
        }
        if let Some(t) = self.config.temperature {
            body["temperature"] = serde_json::json!(t);
        }

        let response = self
            .http
            .post(&url)
            .set("x-api-key", &self.config.api_key)
            .set("anthropic-version", "2023-06-01")
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| AiError::Http(e.to_string()))?;

        Self::stream_response(response)
    }

    fn stream_response(response: ureq::Response) -> Result<mpsc::Receiver<AiStreamEvent>, AiError> {
        let status = response.status();
        if status != 200 {
            let text = Self::read_limited_response(response)?;
            return Err(AiError::Api {
                code: status,
                message: text,
            });
        }

        let (tx, rx) = mpsc::channel::<AiStreamEvent>();
        std::thread::spawn(move || {
            let reader = response.into_reader();
            let mut buf = BufReader::new(reader);
            let mut data_buf = String::new();
            let mut line = String::new();
            let done = false;

            loop {
                line.clear();
                match buf.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) => {
                        let _ = tx.send(AiStreamEvent::Error(format!("读取流失败: {}", e)));
                        break;
                    }
                }

                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    if !data_buf.is_empty() {
                        if data_buf.trim() == "[DONE]" {
                            let _ = tx.send(AiStreamEvent::Done);
                            break;
                        } else {
                            match serde_json::from_str::<serde_json::Value>(&data_buf) {
                                Ok(json) => {
                                    if json.get("error").is_some() {
                                        let _ = tx.send(AiStreamEvent::Error(format!(
                                            "API error: {}",
                                            json["error"]
                                        )));
                                        break;
                                    }
                                    if let Some(reasoning) = json
                                        .pointer("/choices/0/delta/reasoning_content")
                                        .and_then(|v| v.as_str())
                                    {
                                        if !reasoning.is_empty() {
                                            let _ = tx.send(AiStreamEvent::Reasoning(
                                                reasoning.to_string(),
                                            ));
                                        }
                                    }
                                    if let Some(token) = Self::extract_stream_token(&json) {
                                        if !token.is_empty() {
                                            let _ = tx.send(AiStreamEvent::Token(token));
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = tx.send(AiStreamEvent::Error(format!(
                                        "解析 SSE JSON 失败: {}",
                                        e
                                    )));
                                }
                            }
                        }
                        data_buf.clear();
                    }
                    continue;
                }

                if let Some(data) = trimmed.strip_prefix("data:") {
                    data_buf.push_str(data.trim_start());
                }
            }

            if !done {
                let _ = tx.send(AiStreamEvent::Done);
            }
        });

        Ok(rx)
    }

    fn extract_stream_token(json: &serde_json::Value) -> Option<String> {
        // OpenAI / OpenAI-compatible: choices[0].delta.content
        if let Some(content) = json
            .pointer("/choices/0/delta/content")
            .and_then(|v| v.as_str())
        {
            return Some(content.to_string());
        }
        // Anthropic content_block_delta: delta.text
        if let Some(text) = json.pointer("/delta/text").and_then(|v| v.as_str()) {
            return Some(text.to_string());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== AiProvider ====================

    #[test]
    fn provider_from_str_openai_variants() {
        for s in ["openai", "gpt", "gpt-4", "gpt-3.5-turbo", "OPENAI", "Gpt"] {
            assert_eq!(
                AiProvider::from_str(s),
                AiProvider::OpenAi,
                "failed for {}",
                s
            );
        }
    }

    #[test]
    fn provider_from_str_claude_variants() {
        for s in ["claude", "anthropic", "Claude", "ANTHROPIC"] {
            assert_eq!(
                AiProvider::from_str(s),
                AiProvider::Claude,
                "failed for {}",
                s
            );
        }
    }

    #[test]
    fn provider_from_str_kimi_variants() {
        for s in ["kimi", "moonshot", "KIMI", "Moonshot"] {
            assert_eq!(
                AiProvider::from_str(s),
                AiProvider::Kimi,
                "failed for {}",
                s
            );
        }
    }

    #[test]
    fn provider_from_str_azure_variants() {
        for s in ["azure", "azure_openai", "azure-openai", "Azure"] {
            assert_eq!(
                AiProvider::from_str(s),
                AiProvider::Azure,
                "failed for {}",
                s
            );
        }
    }

    #[test]
    fn provider_from_str_custom_and_unknown() {
        for s in ["custom", "foo", "", "llama", "unknown"] {
            assert_eq!(
                AiProvider::from_str(s),
                AiProvider::Custom,
                "failed for {:?}",
                s
            );
        }
    }

    #[test]
    fn provider_default_base_url() {
        assert_eq!(
            AiProvider::OpenAi.default_base_url(),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            AiProvider::Claude.default_base_url(),
            "https://api.anthropic.com/v1"
        );
        assert_eq!(
            AiProvider::Kimi.default_base_url(),
            "https://api.moonshot.cn/v1"
        );
        assert_eq!(AiProvider::Azure.default_base_url(), "");
        assert_eq!(AiProvider::Custom.default_base_url(), "");
    }

    #[test]
    fn provider_default_model() {
        assert_eq!(AiProvider::OpenAi.default_model(), "gpt-4");
        assert_eq!(
            AiProvider::Claude.default_model(),
            "claude-3-sonnet-20240229"
        );
        assert_eq!(AiProvider::Kimi.default_model(), "moonshot-v1-8k");
        assert_eq!(AiProvider::Azure.default_model(), "gpt-4");
        assert_eq!(AiProvider::Custom.default_model(), "");
    }

    #[test]
    fn provider_as_str() {
        assert_eq!(AiProvider::OpenAi.as_str(), "openai");
        assert_eq!(AiProvider::Claude.as_str(), "claude");
        assert_eq!(AiProvider::Kimi.as_str(), "kimi");
        assert_eq!(AiProvider::Azure.as_str(), "azure");
        assert_eq!(AiProvider::Custom.as_str(), "custom");
    }

    #[test]
    fn provider_debug_and_clone_eq() {
        let p = AiProvider::Kimi;
        assert_eq!(format!("{:?}", p), "Kimi");
        assert_eq!(p.clone(), p);
    }

    // ==================== AiError ====================

    #[test]
    fn ai_error_display() {
        assert_eq!(
            format!("{}", AiError::Http("timeout".to_string())),
            "HTTP error: timeout"
        );
        assert_eq!(
            format!("{}", AiError::Parse("bad json".to_string())),
            "Parse error: bad json"
        );
        assert_eq!(
            format!("{}", AiError::Config("missing key".to_string())),
            "Config error: missing key"
        );
        assert_eq!(
            format!(
                "{}",
                AiError::Api {
                    code: 500,
                    message: "boom".to_string()
                }
            ),
            "API error 500: boom"
        );
    }

    // ==================== AiConfig ====================

    fn settings_with(
        provider: &str,
        api_key: &str,
        base_url: Option<&str>,
        model: &str,
    ) -> AiSettings {
        AiSettings {
            provider: provider.to_string(),
            api_key: api_key.to_string(),
            base_url: base_url.map(|s| s.to_string()),
            model: model.to_string(),
            temperature: None,
            max_tokens: None,
            system_prompt: None,
        }
    }

    #[test]
    fn config_from_settings_defaults() {
        let settings = settings_with("openai", "key", None, "");
        let config = AiConfig::from_settings(&settings);
        assert_eq!(config.provider, AiProvider::OpenAi);
        assert_eq!(config.api_key, "key");
        assert_eq!(
            config.base_url,
            Some("https://api.openai.com/v1".to_string())
        );
        assert_eq!(config.model, "gpt-4");
    }

    #[test]
    fn config_from_settings_custom_base_url_and_model() {
        let settings = settings_with(
            "claude",
            "secret",
            Some("https://example.com/v1"),
            "model-x",
        );
        let config = AiConfig::from_settings(&settings);
        assert_eq!(config.provider, AiProvider::Claude);
        assert_eq!(config.base_url, Some("https://example.com/v1".to_string()));
        assert_eq!(config.model, "model-x");
    }

    #[test]
    fn config_from_settings_empty_base_url_for_custom_provider() {
        // Custom provider has empty default base_url, so result should be None.
        let settings = settings_with("custom", "key", None, "");
        let config = AiConfig::from_settings(&settings);
        assert_eq!(config.provider, AiProvider::Custom);
        assert_eq!(config.base_url, None);
        assert_eq!(config.model, "");
    }

    #[test]
    fn config_from_settings_explicit_empty_base_url() {
        let settings = settings_with("openai", "key", Some(""), "");
        let config = AiConfig::from_settings(&settings);
        // An explicitly empty base_url is preserved as Some("") rather than falling back.
        assert_eq!(config.base_url, Some("".to_string()));
    }

    #[test]
    fn config_debug_hides_api_key_and_shows_system_prompt_presence() {
        let config = AiConfig {
            provider: AiProvider::OpenAi,
            api_key: "super-secret".to_string(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            model: "gpt-4".to_string(),
            temperature: Some(0.7),
            max_tokens: Some(100),
            system_prompt: Some("you are helpful".to_string()),
        };
        let out = format!("{:?}", config);
        assert!(!out.contains("super-secret"), "api_key leaked in Debug");
        assert!(out.contains("[REDACTED]"), "api_key not marked redacted");
        assert!(
            out.contains("[PRESENT]"),
            "system_prompt presence not indicated"
        );
        assert!(out.contains("gpt-4"));
    }

    // ==================== ChatMessage ====================

    #[test]
    fn chat_message_user_and_assistant() {
        let u = ChatMessage::user("hello");
        assert_eq!(u.role, "user");
        assert_eq!(u.content, "hello");

        let a = ChatMessage::assistant(String::from("hi there"));
        assert_eq!(a.role, "assistant");
        assert_eq!(a.content, "hi there");
    }

    #[test]
    fn split_claude_messages_extracts_system_to_top_level() {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "sys-1".to_string(),
            },
            ChatMessage::user("hello"),
            ChatMessage {
                role: "system".to_string(),
                content: "sys-2".to_string(),
            },
            ChatMessage::assistant("hi".to_string()),
        ];
        let (system, msgs) = split_claude_messages(&messages);
        // system 消息合并到顶层，messages 数组只含 user/assistant
        assert_eq!(system, Some("sys-1\n\nsys-2".to_string()));
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[1]["role"], "assistant");
    }

    #[test]
    fn split_claude_messages_without_system_returns_none() {
        let messages = vec![ChatMessage::user("hello")];
        let (system, msgs) = split_claude_messages(&messages);
        assert_eq!(system, None);
        assert_eq!(msgs.len(), 1);
    }

    // ==================== AiClient ====================

    #[test]
    fn client_new_preserves_config() {
        let settings = AiSettings {
            provider: "kimi".to_string(),
            api_key: "mk".to_string(),
            base_url: Some("https://api.moonshot.cn/v1".to_string()),
            model: "moonshot-v1-8k".to_string(),
            temperature: Some(0.5),
            max_tokens: Some(512),
            system_prompt: Some("sys".to_string()),
        };
        let client = AiClient::new(&settings);
        assert_eq!(client.config.provider, AiProvider::Kimi);
        assert_eq!(client.config.api_key, "mk");
        assert_eq!(
            client.config.base_url,
            Some("https://api.moonshot.cn/v1".to_string())
        );
        assert_eq!(client.config.model, "moonshot-v1-8k");
        assert_eq!(client.config.temperature, Some(0.5));
        assert_eq!(client.config.max_tokens, Some(512));
        assert_eq!(client.config.system_prompt, Some("sys".to_string()));
    }

    #[test]
    fn validate_https_rejects_http_and_accepts_https() {
        assert!(AiClient::validate_https("http://api.openai.com").is_err());
        assert!(AiClient::validate_https("https://").is_ok());
        assert!(AiClient::validate_https("https://api.openai.com/v1").is_ok());
        assert!(AiClient::validate_https("ftp://api.openai.com").is_err());
        assert!(AiClient::validate_https("").is_err());
    }

    #[test]
    fn check_ip_private_ipv4() {
        assert!(AiClient::check_ip_private("10.0.0.1".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("172.16.0.1".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("192.168.1.1".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("127.0.0.1".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("169.254.1.1".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("224.0.0.1".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("192.0.2.1".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("255.255.255.255".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("0.0.0.0".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("1.1.1.1".parse().unwrap(), "h").is_ok());
        assert!(AiClient::check_ip_private("8.8.8.8".parse().unwrap(), "h").is_ok());
    }

    #[test]
    fn check_ip_private_ipv6() {
        assert!(AiClient::check_ip_private("::1".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("::".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("ff02::1".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("::ffff:10.0.0.1".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("::ffff:127.0.0.1".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("::ffff:192.168.1.1".parse().unwrap(), "h").is_err());
        assert!(AiClient::check_ip_private("2001:4860:4860::8888".parse().unwrap(), "h").is_ok());
    }

    #[test]
    fn validate_not_private_ip_public_ip_passes() {
        assert!(AiClient::validate_not_private_ip("https://1.1.1.1").is_ok());
    }

    #[test]
    fn validate_not_private_ip_public_domain_passes() {
        // example.com is not blocked; if DNS is unavailable the function still returns Ok.
        assert!(AiClient::validate_not_private_ip("https://example.com").is_ok());
    }

    #[test]
    fn validate_not_private_ip_rejects_private_and_local() {
        assert!(AiClient::validate_not_private_ip("https://192.168.1.1").is_err());
        assert!(AiClient::validate_not_private_ip("https://127.0.0.1").is_err());
        assert!(AiClient::validate_not_private_ip("https://10.0.0.1").is_err());
        assert!(AiClient::validate_not_private_ip("https://[::1]").is_err());
        assert!(AiClient::validate_not_private_ip("https://[::ffff:192.168.1.1]").is_err());
    }

    #[test]
    fn validate_not_private_ip_rejects_metadata_endpoints() {
        assert!(AiClient::validate_not_private_ip("https://169.254.169.254").is_err());
        assert!(AiClient::validate_not_private_ip("https://metadata.google.internal").is_err());
        assert!(AiClient::validate_not_private_ip("https://metadata.google").is_err());
        assert!(AiClient::validate_not_private_ip("https://metadata.azure.internal").is_err());
        assert!(AiClient::validate_not_private_ip("https://100.100.100.200").is_err());
        assert!(AiClient::validate_not_private_ip("https://metadata.tencentyun.com").is_err());
    }

    #[test]
    fn validate_not_private_ip_rejects_bad_urls() {
        assert!(AiClient::validate_not_private_ip("not a url").is_err());
        assert!(AiClient::validate_not_private_ip("https://").is_err());
    }

    #[test]
    fn resolve_and_lock_public_ip_ok() {
        let ep = AiClient::resolve_and_lock("https://1.1.1.1").unwrap();
        assert_eq!(ep.host, "1.1.1.1");
        assert_eq!(ep.port, 443);
    }

    #[test]
    fn resolve_and_lock_custom_port() {
        let ep = AiClient::resolve_and_lock("https://8.8.8.8:8443").unwrap();
        assert_eq!(ep.host, "8.8.8.8");
        assert_eq!(ep.port, 8443);
    }

    #[test]
    fn resolve_and_lock_rejects_private_ip() {
        assert!(AiClient::resolve_and_lock("https://192.168.1.1").is_err());
        assert!(AiClient::resolve_and_lock("https://127.0.0.1:8080").is_err());
    }

    #[test]
    fn resolve_and_lock_rejects_bad_url() {
        assert!(AiClient::resolve_and_lock("not a url").is_err());
        assert!(AiClient::resolve_and_lock("https://").is_err());
    }

    #[test]
    fn validate_tcp_connect_target_matches_resolve_and_lock() {
        assert!(AiClient::validate_tcp_connect_target("https://1.1.1.1").is_ok());
        assert!(AiClient::validate_tcp_connect_target("https://127.0.0.1").is_err());
        assert!(AiClient::validate_tcp_connect_target("https://[::1]").is_err());
    }

    #[test]
    fn read_limited_response_empty() {
        let resp = ureq::Response::new(200, "OK", "").unwrap();
        let text = AiClient::read_limited_response(resp).unwrap();
        assert_eq!(text, "");
    }

    #[test]
    fn read_limited_response_normal_body() {
        let body = r#"{"choices":[{"message":{"content":"hi"}}]}"#;
        let resp = ureq::Response::new(200, "OK", body).unwrap();
        let text = AiClient::read_limited_response(resp).unwrap();
        assert_eq!(text, body);
    }

    fn client_with_empty_key(provider: AiProvider, base_url: &str) -> AiClient {
        let settings = AiSettings {
            provider: provider.as_str().to_string(),
            api_key: "".to_string(),
            base_url: Some(base_url.to_string()),
            model: "model".to_string(),
            temperature: None,
            max_tokens: None,
            system_prompt: None,
        };
        AiClient::new(&settings)
    }

    #[test]
    fn complete_rejects_empty_api_key_openai_compatible() {
        let client = client_with_empty_key(AiProvider::OpenAi, "https://1.1.1.1");
        let err = client.complete("prompt").unwrap_err();
        match err {
            AiError::Config(msg) => assert_eq!(msg, "API Key 未设置"),
            other => panic!("expected Config error, got {:?}", other),
        }
    }

    #[test]
    fn complete_rejects_empty_api_key_claude() {
        let client = client_with_empty_key(AiProvider::Claude, "https://1.1.1.1");
        let err = client.complete("prompt").unwrap_err();
        match err {
            AiError::Config(msg) => assert_eq!(msg, "API Key 未设置"),
            other => panic!("expected Config error, got {:?}", other),
        }
    }

    #[test]
    fn chat_completion_rejects_empty_api_key_openai_compatible() {
        let client = client_with_empty_key(AiProvider::Kimi, "https://1.1.1.1");
        let err = client
            .chat_completion(&[ChatMessage::user("hi")])
            .unwrap_err();
        match err {
            AiError::Config(msg) => assert_eq!(msg, "API Key 未设置"),
            other => panic!("expected Config error, got {:?}", other),
        }
    }

    #[test]
    fn chat_completion_rejects_empty_api_key_claude() {
        let client = client_with_empty_key(AiProvider::Claude, "https://1.1.1.1");
        let err = client
            .chat_completion(&[ChatMessage::user("hi")])
            .unwrap_err();
        match err {
            AiError::Config(msg) => assert_eq!(msg, "API Key 未设置"),
            other => panic!("expected Config error, got {:?}", other),
        }
    }

    #[test]
    fn chat_completion_stream_rejects_empty_api_key_openai_compatible() {
        let client = client_with_empty_key(AiProvider::Azure, "https://1.1.1.1");
        let err = client
            .chat_completion_stream(&[ChatMessage::user("hi")])
            .unwrap_err();
        match err {
            AiError::Config(msg) => assert_eq!(msg, "API Key 未设置"),
            other => panic!("expected Config error, got {:?}", other),
        }
    }

    #[test]
    fn chat_completion_stream_rejects_empty_api_key_claude() {
        let client = client_with_empty_key(AiProvider::Claude, "https://1.1.1.1");
        let err = client
            .chat_completion_stream(&[ChatMessage::user("hi")])
            .unwrap_err();
        match err {
            AiError::Config(msg) => assert_eq!(msg, "API Key 未设置"),
            other => panic!("expected Config error, got {:?}", other),
        }
    }

    #[test]
    fn chat_completion_stream_rejects_empty_api_key_custom() {
        let client = client_with_empty_key(AiProvider::Custom, "https://1.1.1.1");
        let err = client
            .chat_completion_stream(&[ChatMessage::user("hi")])
            .unwrap_err();
        match err {
            AiError::Config(msg) => assert_eq!(msg, "API Key 未设置"),
            other => panic!("expected Config error, got {:?}", other),
        }
    }

    // ==================== extract_stream_token ====================

    #[test]
    fn extract_stream_token_openai() {
        let json = serde_json::json!({
            "choices": [{"delta": {"content": "hello"}}]
        });
        assert_eq!(
            AiClient::extract_stream_token(&json),
            Some("hello".to_string())
        );
    }

    #[test]
    fn extract_stream_token_openai_empty_content() {
        let json = serde_json::json!({
            "choices": [{"delta": {"content": ""}}]
        });
        assert_eq!(AiClient::extract_stream_token(&json), Some("".to_string()));
    }

    #[test]
    fn extract_stream_token_openai_null_content() {
        let json = serde_json::json!({
            "choices": [{"delta": {"content": null}}]
        });
        assert_eq!(AiClient::extract_stream_token(&json), None);
    }

    #[test]
    fn extract_stream_token_anthropic() {
        let json = serde_json::json!({
            "delta": {"text": "world"}
        });
        assert_eq!(
            AiClient::extract_stream_token(&json),
            Some("world".to_string())
        );
    }

    #[test]
    fn extract_stream_token_unrelated() {
        let json = serde_json::json!({"foo": "bar"});
        assert_eq!(AiClient::extract_stream_token(&json), None);
    }

    // ==================== AiStreamEvent ====================

    #[test]
    fn ai_stream_event_clone_and_debug() {
        let e = AiStreamEvent::Token("tok".to_string());
        assert_eq!(format!("{:?}", e.clone()), format!("{:?}", e));

        let done = AiStreamEvent::Done;
        match done.clone() {
            AiStreamEvent::Done => {}
            _ => panic!("clone of Done should be Done"),
        }

        let err = AiStreamEvent::Error("oops".to_string());
        match err.clone() {
            AiStreamEvent::Error(msg) => assert_eq!(msg, "oops"),
            _ => panic!("clone of Error should be Error"),
        }

        let dbg = format!("{:?}", AiStreamEvent::Token("x".to_string()));
        assert!(dbg.contains("Token") && dbg.contains("x"));
    }
}
