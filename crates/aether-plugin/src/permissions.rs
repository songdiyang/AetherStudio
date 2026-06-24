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
    pub fn contains(&self, other: PermissionLevel) -> bool {
        let self_level = *self as u8;
        let other_level = other as u8;
        self_level >= other_level
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
            std::time::SystemTime::now() > expires
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
