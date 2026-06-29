/// 权限级别（L1-L4）
/// L1: 只读 UI 访问
/// L2: 文件读写
/// L3: 网络访问
/// L4: 系统命令执行
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PermissionLevel {
    L1_ReadOnly,
    L2_FileIO,
    L3_Network,
    L4_System,
}

impl PermissionLevel {
    /// 获取权限级别描述
    pub fn description(&self) -> &'static str {
        match self {
            Self::L1_ReadOnly => "只读 UI 访问",
            Self::L2_FileIO => "文件读写",
            Self::L3_Network => "网络访问",
            Self::L4_System => "系统命令执行",
        }
    }

    /// 检查是否包含另一级别权限
    /// 使用显式匹配替代枚举数值比较，避免重排变体导致静默破坏
    pub fn contains(&self, other: PermissionLevel) -> bool {
        match (self, other) {
            // L4 包含所有权限
            (PermissionLevel::L4_System, _) => true,
            // L3 包含 L3/L2/L1
            (PermissionLevel::L3_Network, PermissionLevel::L3_Network)
            | (PermissionLevel::L3_Network, PermissionLevel::L2_FileIO)
            | (PermissionLevel::L3_Network, PermissionLevel::L1_ReadOnly) => true,
            // L2 包含 L2/L1
            (PermissionLevel::L2_FileIO, PermissionLevel::L2_FileIO)
            | (PermissionLevel::L2_FileIO, PermissionLevel::L1_ReadOnly) => true,
            // L1 仅包含 L1
            (PermissionLevel::L1_ReadOnly, PermissionLevel::L1_ReadOnly) => true,
            // 其他组合均不满足
            _ => false,
        }
    }
}

/// 权限授予记录
#[derive(Clone, Debug)]
pub struct PermissionGrant {
    pub level: PermissionLevel,
    pub granted_at: std::time::SystemTime,
    pub expires_at: Option<std::time::SystemTime>,
    pub reason: String,
}

/// 插件权限管理器
#[derive(Clone, Debug)]
pub struct PermissionManager {
    grants: Vec<PermissionGrant>,
}

impl PermissionManager {
    pub fn new() -> Self {
        Self { grants: Vec::new() }
    }

    /// 检查是否已授予指定权限
    pub fn is_granted(&self, level: PermissionLevel) -> bool {
        self.grants
            .iter()
            .any(|g| g.level.contains(level) && !Self::is_expired(g))
    }

    /// 授予权限
    pub fn grant(&mut self, grant: PermissionGrant) {
        self.grants.push(grant);
    }

    /// 撤销所有权限
    pub fn revoke_all(&mut self) {
        self.grants.clear();
    }

    fn is_expired(grant: &PermissionGrant) -> bool {
        if let Some(expires) = grant.expires_at {
            // SEC-H06: 检查 expires_at 不能是过去时间（防止"已过期但永久有效"的权限）
            // 同时也验证 granted_at 不能是未来时间
            let now = std::time::SystemTime::now();
            now > expires || grant.granted_at > now
        } else {
            false
        }
    }
}

impl Default for PermissionManager {
    fn default() -> Self {
        Self::new()
    }
}
