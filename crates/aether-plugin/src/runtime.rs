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

        if header != *WASM_MAGIC {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    fn temp_wasm_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("aether_plugin_runtime_test_{}.wasm", name))
    }

    fn write_valid_wasm(path: &PathBuf, extra: &[u8]) {
        let mut data = WASM_MAGIC.to_vec();
        data.extend_from_slice(extra);
        fs::write(path, &data).unwrap();
    }

    fn cleanup(path: &PathBuf) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn plugin_runtime_new_succeeds() {
        let runtime = PluginRuntime::new();
        assert!(runtime.is_ok());
    }

    #[test]
    fn plugin_runtime_default_succeeds() {
        let runtime = PluginRuntime::default();
        assert_eq!(runtime.plugin_count(), 0);
    }

    #[test]
    fn plugin_id_traits() {
        let a = PluginId(1);
        let b = PluginId(1);
        let c = PluginId(2);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.clone(), a);
        assert_eq!(format!("{:?}", a), "PluginId(1)");

        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h1 = DefaultHasher::new();
        a.hash(&mut h1);
        let mut h2 = DefaultHasher::new();
        b.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }

    #[test]
    fn load_plugin_with_valid_wasm_succeeds() {
        let path = temp_wasm_path("valid");
        cleanup(&path);
        write_valid_wasm(&path, b"\x01\x00\x00\x00");

        let mut runtime = PluginRuntime::new().unwrap();
        let id = runtime.load_plugin(&path).expect("应能加载有效 WASM 文件");
        assert_eq!(runtime.plugin_count(), 1);
        assert!(runtime._plugins.contains_key(&id));

        cleanup(&path);
    }

    #[test]
    fn load_plugin_missing_file_fails() {
        let path = temp_wasm_path("missing");
        cleanup(&path);

        let mut runtime = PluginRuntime::new().unwrap();
        let err = runtime.load_plugin(&path).unwrap_err();
        assert!(err.contains("不存在"), "错误应提示文件不存在: {}", err);

        cleanup(&path);
    }

    #[test]
    fn load_plugin_invalid_magic_fails() {
        let path = temp_wasm_path("invalid_magic");
        cleanup(&path);
        fs::write(&path, b"NOTWASM").unwrap();

        let mut runtime = PluginRuntime::new().unwrap();
        let err = runtime.load_plugin(&path).unwrap_err();
        assert!(err.contains("不是有效的 WASM 格式"), "错误应提示 WASM 格式无效: {}", err);

        cleanup(&path);
    }

    #[test]
    fn load_plugin_too_large_fails() {
        let path = temp_wasm_path("too_large");
        cleanup(&path);

        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)
                .unwrap();
            file.write_all(WASM_MAGIC).unwrap();
            file.set_len(MAX_PLUGIN_SIZE + 1).unwrap();
        }

        let mut runtime = PluginRuntime::new().unwrap();
        let err = runtime.load_plugin(&path).unwrap_err();
        assert!(err.contains("过大"), "错误应提示文件过大: {}", err);

        cleanup(&path);
    }

    #[test]
    fn load_plugin_id_exhaustion_fails() {
        let path = temp_wasm_path("exhaust");
        cleanup(&path);
        write_valid_wasm(&path, b"\x01\x00\x00\x00");

        let mut runtime = PluginRuntime::new().unwrap();
        runtime._next_id = u32::MAX;
        let err = runtime.load_plugin(&path).unwrap_err();
        assert!(err.contains("ID 已耗尽"), "错误应提示 ID 耗尽: {}", err);

        cleanup(&path);
    }

    #[test]
    fn unload_plugin_removes_plugin_and_permissions() {
        let path = temp_wasm_path("unload");
        cleanup(&path);
        write_valid_wasm(&path, b"\x01\x00\x00\x00");

        let mut runtime = PluginRuntime::new().unwrap();
        let id = runtime.load_plugin(&path).unwrap();
        assert_eq!(runtime.plugin_count(), 1);
        runtime.unload_plugin(id);
        assert_eq!(runtime.plugin_count(), 0);
        assert!(!runtime._plugins.contains_key(&id));
        assert!(!runtime.permissions.contains_key(&id));

        cleanup(&path);
    }

    #[test]
    fn grant_permission_success() {
        let path = temp_wasm_path("grant_ok");
        cleanup(&path);
        write_valid_wasm(&path, b"\x01\x00\x00\x00");

        let mut runtime = PluginRuntime::new().unwrap();
        let id = runtime.load_plugin(&path).unwrap();
        let future = SystemTime::now() + Duration::from_secs(3600);
        let result = runtime.grant_permission(id, PermissionLevel::L3_Network, "network", Some(future));
        assert!(result.is_ok());

        let perm_mgr = runtime.permissions.get(&id).unwrap();
        assert!(perm_mgr.is_granted(PermissionLevel::L3_Network));

        cleanup(&path);
    }

    #[test]
    fn grant_permission_past_expiry_fails() {
        let path = temp_wasm_path("grant_past");
        cleanup(&path);
        write_valid_wasm(&path, b"\x01\x00\x00\x00");

        let mut runtime = PluginRuntime::new().unwrap();
        let id = runtime.load_plugin(&path).unwrap();
        let past = SystemTime::now() - Duration::from_secs(1);
        let err = runtime
            .grant_permission(id, PermissionLevel::L2_FileIO, "file", Some(past))
            .unwrap_err();
        assert!(err.contains("过去时间"), "错误应提示过期时间不能是过去时间: {}", err);

        cleanup(&path);
    }

    #[test]
    fn grant_permission_unknown_plugin_fails() {
        let mut runtime = PluginRuntime::new().unwrap();
        let err = runtime
            .grant_permission(PluginId(999), PermissionLevel::L1_ReadOnly, "x", None)
            .unwrap_err();
        assert!(err.contains("未加载"), "错误应提示插件未加载: {}", err);
    }

    #[test]
    fn revoke_all_permissions_clears_them() {
        let path = temp_wasm_path("revoke");
        cleanup(&path);
        write_valid_wasm(&path, b"\x01\x00\x00\x00");

        let mut runtime = PluginRuntime::new().unwrap();
        let id = runtime.load_plugin(&path).unwrap();
        runtime
            .grant_permission(id, PermissionLevel::L4_System, "admin", None)
            .unwrap();
        assert!(runtime.permissions.get(&id).unwrap().is_granted(PermissionLevel::L4_System));
        runtime.revoke_all_permissions(id);
        assert!(!runtime.permissions.get(&id).unwrap().is_granted(PermissionLevel::L1_ReadOnly));

        cleanup(&path);
    }

    #[test]
    fn call_hook_plugin_not_loaded_fails() {
        let mut runtime = PluginRuntime::new().unwrap();
        let err = runtime.call_hook(PluginId(42), "on_activate", serde_json::Value::Null).unwrap_err();
        assert!(err.contains("未加载"), "错误应提示插件未加载: {}", err);
    }

    #[test]
    fn call_hook_permission_denied_for_l2() {
        let path = temp_wasm_path("hook_denied");
        cleanup(&path);
        write_valid_wasm(&path, b"\x01\x00\x00\x00");

        let mut runtime = PluginRuntime::new().unwrap();
        let id = runtime.load_plugin(&path).unwrap();
        let err = runtime.call_hook(id, "write_file", serde_json::Value::Null).unwrap_err();
        assert!(err.contains("缺少执行"), "错误应提示缺少权限: {}", err);
        assert!(err.contains("L2_FileIO"), "错误应包含所需权限级别: {}", err);

        cleanup(&path);
    }

    #[test]
    fn call_hook_permission_allowed_returns_not_integrated() {
        let path = temp_wasm_path("hook_allowed");
        cleanup(&path);
        write_valid_wasm(&path, b"\x01\x00\x00\x00");

        let mut runtime = PluginRuntime::new().unwrap();
        let id = runtime.load_plugin(&path).unwrap();
        let err = runtime.call_hook(id, "on_activate", serde_json::Value::Null).unwrap_err();
        assert!(err.contains("无法执行"), "错误应提示钩子无法执行: {}", err);
        assert!(err.contains("WASM 运行时尚未集成"), "错误应说明 WASM 未集成: {}", err);

        cleanup(&path);
    }

    #[test]
    fn call_hook_with_granted_l2_succeeds_permission_check() {
        let path = temp_wasm_path("hook_granted");
        cleanup(&path);
        write_valid_wasm(&path, b"\x01\x00\x00\x00");

        let mut runtime = PluginRuntime::new().unwrap();
        let id = runtime.load_plugin(&path).unwrap();
        runtime.grant_permission(id, PermissionLevel::L2_FileIO, "file", None).unwrap();

        let err = runtime.call_hook(id, "write_file", serde_json::Value::Null).unwrap_err();
        // 权限通过，但 WASM 未集成
        assert!(err.contains("WASM 运行时尚未集成"), "应通过权限检查并返回未集成错误: {}", err);

        cleanup(&path);
    }

    #[test]
    fn call_hook_unknown_hook_defaults_to_l1() {
        let path = temp_wasm_path("hook_unknown");
        cleanup(&path);
        write_valid_wasm(&path, b"\x01\x00\x00\x00");

        let mut runtime = PluginRuntime::new().unwrap();
        let id = runtime.load_plugin(&path).unwrap();
        // 默认拥有 L1，因此权限检查通过，但 WASM 未集成
        let err = runtime.call_hook(id, "unknown_hook", serde_json::Value::Null).unwrap_err();
        assert!(err.contains("WASM 运行时尚未集成"), "未知 hook 默认 L1，应通过权限检查: {}", err);

        cleanup(&path);
    }

    #[test]
    fn required_permission_for_hook_mapping() {
        let runtime = PluginRuntime::new().unwrap();
        let cases: Vec<(&str, PermissionLevel)> = vec![
            ("on_activate", PermissionLevel::L1_ReadOnly),
            ("on_deactivate", PermissionLevel::L1_ReadOnly),
            ("get_theme", PermissionLevel::L1_ReadOnly),
            ("get_language", PermissionLevel::L1_ReadOnly),
            ("on_save", PermissionLevel::L2_FileIO),
            ("on_open", PermissionLevel::L2_FileIO),
            ("read_file", PermissionLevel::L2_FileIO),
            ("write_file", PermissionLevel::L2_FileIO),
            ("fetch", PermissionLevel::L3_Network),
            ("http_request", PermissionLevel::L3_Network),
            ("websocket", PermissionLevel::L3_Network),
            ("exec", PermissionLevel::L4_System),
            ("spawn", PermissionLevel::L4_System),
            ("shell", PermissionLevel::L4_System),
            ("run_command", PermissionLevel::L4_System),
            ("totally_unknown", PermissionLevel::L1_ReadOnly),
        ];
        for (hook, expected) in cases {
            assert_eq!(
                runtime.required_permission_for_hook(hook),
                expected,
                "hook '{}' 权限映射错误",
                hook
            );
        }
    }

    #[test]
    fn plugin_count_tracks_load_and_unload() {
        let path1 = temp_wasm_path("count1");
        let path2 = temp_wasm_path("count2");
        cleanup(&path1);
        cleanup(&path2);
        write_valid_wasm(&path1, b"\x01\x00\x00\x00");
        write_valid_wasm(&path2, b"\x02\x00\x00\x00");

        let mut runtime = PluginRuntime::new().unwrap();
        assert_eq!(runtime.plugin_count(), 0);
        let id1 = runtime.load_plugin(&path1).unwrap();
        assert_eq!(runtime.plugin_count(), 1);
        let id2 = runtime.load_plugin(&path2).unwrap();
        assert_eq!(runtime.plugin_count(), 2);
        runtime.unload_plugin(id1);
        assert_eq!(runtime.plugin_count(), 1);
        runtime.unload_plugin(id2);
        assert_eq!(runtime.plugin_count(), 0);

        cleanup(&path1);
        cleanup(&path2);
    }
}
