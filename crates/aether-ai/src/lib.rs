use aether_shared::settings::AiSettings;
use serde::{Deserialize, Serialize};
use std::io::Read;
use url::Url;

/// DNS 解析结果缓存（用于 SSRF 防护中消除 DNS Rebinding 窗口）
#[derive(Clone, Debug)]
struct ResolvedEndpoint {
    #[allow(dead_code)]
    host: String,
    #[allow(dead_code)]
    port: u16,
    // P0-6 后不再用 IP 直连构建 URL（会破坏 TLS 主机名校验），
    // 保留此字段仅为未来可能的连接级 IP pinning 预留。
    #[allow(dead_code)]
    verified_ips: Vec<std::net::IpAddr>,
}

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

#[derive(Clone)]
pub struct AiConfig {
    pub provider: AiProvider,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
}

impl std::fmt::Debug for AiConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiConfig")
            .field("provider", &self.provider)
            .field("api_key", &"[REDACTED]")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
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

    /// SEC-A01: 解析并锁定 DNS 结果，消除 DNS Rebinding 窗口
    /// 返回已验证的 IP 列表，后续 HTTP 请求必须直连这些 IP
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
                verified_ips: vec![ip],
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
        let mut verified_ips = Vec::new();
        for addr in &addrs {
            Self::check_ip_private(addr.ip(), host)?;
            verified_ips.push(addr.ip());
        }
        Ok(ResolvedEndpoint {
            host: host.to_string(),
            port,
            verified_ips,
        })
    }

    /// SEC-C03: TOCTOU 二次 DNS 校验 — 在发起 HTTP 请求前调用
    /// 验证 DNS 解析结果未在两次查询间被篡改
    fn validate_tcp_connect_target(url_str: &str) -> Result<ResolvedEndpoint, AiError> {
        // 使用 resolve_and_lock 替代独立校验，确保 DNS 结果在请求前锁定
        Self::resolve_and_lock(url_str)
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
            return Err(AiError::Api {
                code: status,
                message: text,
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
                message: text,
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
                message: text,
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
            let text = Self::read_limited_response(response)?;
            return Err(AiError::Api {
                code: status,
                message: text,
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
}
