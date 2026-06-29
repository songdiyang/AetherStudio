use std::collections::HashMap;
use std::path::Path;

use crate::permissions::{PermissionGrant, PermissionLevel, PermissionManager};

const MAX_PLUGIN_SIZE: u64 = 50 * 1024 * 1024; // 50MB
const WASM_MAGIC: &[u8] = &[0x00, 0x61, 0x73, 0x6d]; // \0asm

/// 插件运行时标识
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PluginId(pub u32);

/// WASM 插件运行时（占位符实现）
/// 基于 Wasmtime 引擎，提供安全的插件执行环境
/// 注：完整实现需要 wasmtime 依赖，当前为架构占位
pub struct PluginRuntime {
    _next_id: u32,
    _plugins: HashMap<PluginId, String>, // 存储插件路径
    /// SEC-C06: 每个插件的权限管理器
    permissions: HashMap<PluginId, PermissionManager>,
}

impl PluginRuntime {
    /// 创建新的插件运行时
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            _next_id: 1,
            _plugins: HashMap::new(),
            permissions: HashMap::new(),
        })
    }

    /// 验证 WASM 文件魔数和大小
    fn validate_wasm(path: &Path) -> Result<(), String> {
        use std::io::Read;

        let metadata = std::fs::metadata(path).map_err(|e| format!("无法读取插件文件: {}", e))?;

        if metadata.len() > MAX_PLUGIN_SIZE {
            return Err(format!(
                "插件文件过大: {} bytes (最大 {})",
                metadata.len(),
                MAX_PLUGIN_SIZE
            ));
        }

        let mut file = std::fs::File::open(path).map_err(|e| format!("无法打开插件文件: {}", e))?;
        let mut header = [0u8; 4];
        file.read_exact(&mut header)
            .map_err(|e| format!("无法读取插件文件头: {}", e))?;

        if &header != WASM_MAGIC {
            return Err("插件文件不是有效的 WASM 格式".to_string());
        }

        Ok(())
    }

    /// 加载 WASM 插件
    pub fn load_plugin(&mut self, path: &Path) -> Result<PluginId, String> {
        // 验证文件存在性和格式 (H-15)
        if !path.exists() {
            return Err(format!("插件文件不存在: {}", path.display()));
        }
        Self::validate_wasm(path)?;

        // 检查 ID 溢出 (M-18)
        if self._next_id == u32::MAX {
            return Err("插件 ID 已耗尽".to_string());
        }

        let id = PluginId(self._next_id);
        self._next_id = self._next_id.wrapping_add(1);
        self._plugins.insert(id, path.to_string_lossy().to_string());

        // SEC-C06: 新加载插件默认仅有 L1_ReadOnly 权限
        let mut perm_mgr = PermissionManager::new();
        perm_mgr.grant(PermissionGrant {
            level: PermissionLevel::L1_ReadOnly,
            granted_at: std::time::SystemTime::now(),
            expires_at: None,
            reason: "插件加载时自动授予基础只读权限".to_string(),
        });
        self.permissions.insert(id, perm_mgr);

        Ok(id)
    }

    /// 卸载插件
    pub fn unload_plugin(&mut self, id: PluginId) {
        self._plugins.remove(&id);
        self.permissions.remove(&id);
    }

    /// 为插件授予权限
    pub fn grant_permission(
        &mut self,
        id: PluginId,
        level: PermissionLevel,
        reason: &str,
        expires_at: Option<std::time::SystemTime>,
    ) -> Result<(), String> {
        let perm_mgr = self.permissions.get_mut(&id).ok_or("插件未加载")?;
        let now = std::time::SystemTime::now();
        // SEC-H06: 拒绝过去时间的权限
        if let Some(exp) = expires_at {
            if exp <= now {
                return Err("权限过期时间不能是过去时间".to_string());
            }
        }
        perm_mgr.grant(PermissionGrant {
            level,
            granted_at: now,
            expires_at,
            reason: reason.to_string(),
        });
        Ok(())
    }

    /// 撤销插件的所有权限
    pub fn revoke_all_permissions(&mut self, id: PluginId) {
        if let Some(perm_mgr) = self.permissions.get_mut(&id) {
            perm_mgr.revoke_all();
        }
    }

    /// 调用插件生命周期钩子
    ///
    /// 注意：当前为占位实现。权限检查会照常执行（通过则继续，失败则返回 Err），
    /// 但实际的 WASM 函数调用尚未接入（需要 wasmtime 依赖）。
    /// 因此权限检查通过后返回显式错误，避免调用方误以为钩子已成功执行。
    pub fn call_hook(
        &mut self,
        id: PluginId,
        hook: &str,
        _args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        // SEC-C06: 接入权限检查 — 根据 hook 类型确定所需权限级别
        let required_level = self.required_permission_for_hook(hook);
        if let Some(perm_mgr) = self.permissions.get(&id) {
            if !perm_mgr.is_granted(required_level) {
                return Err(format!(
                    "插件 {} 缺少执行 '{}' 所需的 {:?} 权限",
                    id.0, hook, required_level
                ));
            }
        } else {
            return Err(format!("插件 {} 未加载", id.0));
        }

        // 权限检查通过，但 WASM 运行时尚未集成
        // 返回显式错误而非 Ok(Null)，避免调用方误判钩子已执行
        Err(format!(
            "插件 {} 的 '{}' 钩子无法执行：WASM 运行时尚未集成（需要 wasmtime 依赖）",
            id.0, hook
        ))
    }

    /// 根据 hook 名称确定所需权限级别
    fn required_permission_for_hook(&self, hook: &str) -> PermissionLevel {
        match hook {
            // 只读操作 → L1
            "on_activate" | "on_deactivate" | "get_theme" | "get_language" => {
                PermissionLevel::L1_ReadOnly
            }
            // 文件操作 → L2
            "on_save" | "on_open" | "read_file" | "write_file" => PermissionLevel::L2_FileIO,
            // 网络操作 → L3
            "fetch" | "http_request" | "websocket" => PermissionLevel::L3_Network,
            // 系统操作 → L4
            "exec" | "spawn" | "shell" | "run_command" => PermissionLevel::L4_System,
            // 未知 hook 默认要求 L1（最安全）
            _ => PermissionLevel::L1_ReadOnly,
        }
    }

    /// 获取已加载插件数量
    pub fn plugin_count(&self) -> usize {
        self._plugins.len()
    }
}

impl Default for PluginRuntime {
    fn default() -> Self {
        Self::new().expect("Failed to create PluginRuntime")
    }
}
