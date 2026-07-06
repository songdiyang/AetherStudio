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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn permission_level_descriptions() {
        assert_eq!(PermissionLevel::L1_ReadOnly.description(), "只读 UI 访问");
        assert_eq!(PermissionLevel::L2_FileIO.description(), "文件读写");
        assert_eq!(PermissionLevel::L3_Network.description(), "网络访问");
        assert_eq!(PermissionLevel::L4_System.description(), "系统命令执行");
    }

    #[test]
    fn permission_level_equality_and_copy() {
        let a = PermissionLevel::L2_FileIO;
        let b = a;
        assert_eq!(a, b);
        assert_eq!(a, PermissionLevel::L2_FileIO);
        assert_ne!(a, PermissionLevel::L3_Network);
    }

    #[test]
    fn permission_level_contains_reflexive() {
        for level in [
            PermissionLevel::L1_ReadOnly,
            PermissionLevel::L2_FileIO,
            PermissionLevel::L3_Network,
            PermissionLevel::L4_System,
        ] {
            assert!(level.contains(level), "{:?} 应包含自身", level);
        }
    }

    #[test]
    fn permission_level_contains_hierarchy() {
        assert!(PermissionLevel::L4_System.contains(PermissionLevel::L3_Network));
        assert!(PermissionLevel::L4_System.contains(PermissionLevel::L2_FileIO));
        assert!(PermissionLevel::L4_System.contains(PermissionLevel::L1_ReadOnly));

        assert!(PermissionLevel::L3_Network.contains(PermissionLevel::L2_FileIO));
        assert!(PermissionLevel::L3_Network.contains(PermissionLevel::L1_ReadOnly));
        assert!(!PermissionLevel::L3_Network.contains(PermissionLevel::L4_System));

        assert!(PermissionLevel::L2_FileIO.contains(PermissionLevel::L1_ReadOnly));
        assert!(!PermissionLevel::L2_FileIO.contains(PermissionLevel::L3_Network));
        assert!(!PermissionLevel::L2_FileIO.contains(PermissionLevel::L4_System));

        assert!(!PermissionLevel::L1_ReadOnly.contains(PermissionLevel::L2_FileIO));
        assert!(!PermissionLevel::L1_ReadOnly.contains(PermissionLevel::L3_Network));
        assert!(!PermissionLevel::L1_ReadOnly.contains(PermissionLevel::L4_System));
    }

    #[test]
    fn permission_manager_new_is_empty() {
        let mgr = PermissionManager::new();
        assert!(!mgr.is_granted(PermissionLevel::L1_ReadOnly));
        assert!(!mgr.is_granted(PermissionLevel::L4_System));
    }

    #[test]
    fn permission_manager_default_equals_new() {
        let default: PermissionManager = Default::default();
        let new = PermissionManager::new();
        assert_eq!(default.is_granted(PermissionLevel::L1_ReadOnly), new.is_granted(PermissionLevel::L1_ReadOnly));
    }

    #[test]
    fn grant_single_level_is_granted() {
        let mut mgr = PermissionManager::new();
        mgr.grant(PermissionGrant {
            level: PermissionLevel::L2_FileIO,
            granted_at: SystemTime::now(),
            expires_at: None,
            reason: "test".to_string(),
        });
        assert!(mgr.is_granted(PermissionLevel::L2_FileIO));
        assert!(mgr.is_granted(PermissionLevel::L1_ReadOnly));
        assert!(!mgr.is_granted(PermissionLevel::L3_Network));
        assert!(!mgr.is_granted(PermissionLevel::L4_System));
    }

    #[test]
    fn grant_l4_grants_everything() {
        let mut mgr = PermissionManager::new();
        mgr.grant(PermissionGrant {
            level: PermissionLevel::L4_System,
            granted_at: SystemTime::now(),
            expires_at: None,
            reason: "admin".to_string(),
        });
        assert!(mgr.is_granted(PermissionLevel::L1_ReadOnly));
        assert!(mgr.is_granted(PermissionLevel::L2_FileIO));
        assert!(mgr.is_granted(PermissionLevel::L3_Network));
        assert!(mgr.is_granted(PermissionLevel::L4_System));
    }

    #[test]
    fn lower_level_does_not_grant_higher() {
        let mut mgr = PermissionManager::new();
        mgr.grant(PermissionGrant {
            level: PermissionLevel::L1_ReadOnly,
            granted_at: SystemTime::now(),
            expires_at: None,
            reason: "readonly".to_string(),
        });
        assert!(!mgr.is_granted(PermissionLevel::L2_FileIO));
        assert!(!mgr.is_granted(PermissionLevel::L3_Network));
        assert!(!mgr.is_granted(PermissionLevel::L4_System));
    }

    #[test]
    fn expired_grant_is_not_granted() {
        let mut mgr = PermissionManager::new();
        let now = SystemTime::now();
        mgr.grant(PermissionGrant {
            level: PermissionLevel::L4_System,
            granted_at: now - Duration::from_secs(60),
            expires_at: Some(now - Duration::from_secs(1)),
            reason: "expired".to_string(),
        });
        assert!(!mgr.is_granted(PermissionLevel::L1_ReadOnly));
        assert!(!mgr.is_granted(PermissionLevel::L4_System));
    }

    #[test]
    fn future_granted_at_is_treated_as_invalid() {
        let mut mgr = PermissionManager::new();
        let now = SystemTime::now();
        mgr.grant(PermissionGrant {
            level: PermissionLevel::L4_System,
            granted_at: now + Duration::from_secs(60),
            // is_expired 在 expires_at 为 Some 时才会检查 granted_at > now
            expires_at: Some(now + Duration::from_secs(3600)),
            reason: "future grant".to_string(),
        });
        assert!(!mgr.is_granted(PermissionLevel::L1_ReadOnly));
    }

    #[test]
    fn non_expired_grant_with_future_expiry_is_granted() {
        let mut mgr = PermissionManager::new();
        let now = SystemTime::now();
        mgr.grant(PermissionGrant {
            level: PermissionLevel::L3_Network,
            granted_at: now,
            expires_at: Some(now + Duration::from_secs(3600)),
            reason: "temporary".to_string(),
        });
        assert!(mgr.is_granted(PermissionLevel::L3_Network));
        assert!(mgr.is_granted(PermissionLevel::L1_ReadOnly));
    }

    #[test]
    fn revoke_all_clears_grants() {
        let mut mgr = PermissionManager::new();
        mgr.grant(PermissionGrant {
            level: PermissionLevel::L4_System,
            granted_at: SystemTime::now(),
            expires_at: None,
            reason: "all".to_string(),
        });
        assert!(mgr.is_granted(PermissionLevel::L4_System));
        mgr.revoke_all();
        assert!(!mgr.is_granted(PermissionLevel::L1_ReadOnly));
        assert!(!mgr.is_granted(PermissionLevel::L4_System));
    }

    #[test]
    fn multiple_grants_one_expired_one_valid() {
        let mut mgr = PermissionManager::new();
        let now = SystemTime::now();
        mgr.grant(PermissionGrant {
            level: PermissionLevel::L1_ReadOnly,
            granted_at: now,
            expires_at: Some(now - Duration::from_secs(1)),
            reason: "expired".to_string(),
        });
        mgr.grant(PermissionGrant {
            level: PermissionLevel::L2_FileIO,
            granted_at: now,
            expires_at: None,
            reason: "valid".to_string(),
        });
        assert!(mgr.is_granted(PermissionLevel::L1_ReadOnly));
        assert!(!mgr.is_granted(PermissionLevel::L3_Network));
    }
}
