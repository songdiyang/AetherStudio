use std::collections::HashMap;

use crate::permissions::PermissionManager;
use crate::runtime::{PluginId, PluginRuntime};

/// 已加载插件的元数据
#[derive(Clone, Debug)]
pub struct PluginMetadata {
    pub id: PluginId,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub permissions: PermissionManager,
}

/// 插件注册表
/// 管理所有已加载插件的生命周期和事件分发
pub struct PluginRegistry {
    runtime: PluginRuntime,
    plugins: HashMap<PluginId, PluginMetadata>,
    hooks: HashMap<String, Vec<PluginId>>,
}

impl PluginRegistry {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            runtime: PluginRuntime::new()?,
            plugins: HashMap::new(),
            hooks: HashMap::new(),
        })
    }

    /// 注册插件并加载
    pub fn register(&mut self, path: &std::path::Path) -> Result<PluginId, String> {
        let id = self.runtime.load_plugin(path)?;

        // 创建默认元数据
        let metadata = PluginMetadata {
            id,
            name: path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            version: "0.1.0".to_string(),
            description: String::new(),
            author: String::new(),
            permissions: PermissionManager::new(),
        };

        self.plugins.insert(id, metadata);
        Ok(id)
    }

    /// 卸载插件
    pub fn unregister(&mut self, id: PluginId) {
        self.runtime.unload_plugin(id);
        self.plugins.remove(&id);

        // 从所有钩子中移除
        for subscribers in self.hooks.values_mut() {
            subscribers.retain(|&sub_id| sub_id != id);
        }
    }

    /// 订阅钩子
    pub fn subscribe(&mut self, hook: &str, plugin_id: PluginId) {
        self.hooks
            .entry(hook.to_string())
            .or_default()
            .push(plugin_id);
    }

    /// 触发钩子，调用所有订阅的插件
    pub fn emit(
        &mut self,
        hook: &str,
        args: serde_json::Value,
    ) -> Vec<(PluginId, Result<serde_json::Value, String>)> {
        let mut results = Vec::new();

        if let Some(subscribers) = self.hooks.get(hook).cloned() {
            for plugin_id in subscribers {
                let result = self.runtime.call_hook(plugin_id, hook, args.clone());
                results.push((plugin_id, result));
            }
        }

        results
    }

    /// 获取已加载插件列表
    pub fn list_plugins(&self) -> Vec<&PluginMetadata> {
        self.plugins.values().collect()
    }

    /// 获取插件数量
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new().expect("Failed to create PluginRegistry")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_wasm_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("aether_plugin_registry_test_{}.wasm", name))
    }

    fn write_valid_wasm(path: &PathBuf) {
        fs::write(path, b"\x00asm").unwrap();
    }

    fn cleanup(path: &PathBuf) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn registry_new_succeeds() {
        let registry = PluginRegistry::new();
        assert!(registry.is_ok());
    }

    #[test]
    fn registry_default_succeeds() {
        let registry = PluginRegistry::default();
        assert_eq!(registry.plugin_count(), 0);
    }

    #[test]
    fn register_valid_plugin_succeeds() {
        let path = temp_wasm_path("register_valid");
        cleanup(&path);
        write_valid_wasm(&path);

        let mut registry = PluginRegistry::new().unwrap();
        let id = registry.register(&path).expect("应能注册有效插件");
        assert_eq!(registry.plugin_count(), 1);
        assert!(registry.list_plugins().iter().any(|m| m.id == id));

        cleanup(&path);
    }

    #[test]
    fn register_invalid_plugin_fails() {
        let path = temp_wasm_path("register_invalid");
        cleanup(&path);
        fs::write(&path, b"INVALID").unwrap();

        let mut registry = PluginRegistry::new().unwrap();
        let err = registry.register(&path).unwrap_err();
        assert!(err.contains("不是有效的 WASM 格式"), "错误应提示 WASM 格式无效: {}", err);
        assert_eq!(registry.plugin_count(), 0);

        cleanup(&path);
    }

    #[test]
    fn register_missing_plugin_fails() {
        let path = temp_wasm_path("register_missing");
        cleanup(&path);

        let mut registry = PluginRegistry::new().unwrap();
        let err = registry.register(&path).unwrap_err();
        assert!(err.contains("不存在"), "错误应提示文件不存在: {}", err);

        cleanup(&path);
    }

    #[test]
    fn unregister_removes_plugin_and_hooks() {
        let path = temp_wasm_path("unregister");
        cleanup(&path);
        write_valid_wasm(&path);

        let mut registry = PluginRegistry::new().unwrap();
        let id = registry.register(&path).unwrap();
        registry.subscribe("my_hook", id);
        assert!(registry.plugin_count() > 0);

        registry.unregister(id);
        assert_eq!(registry.plugin_count(), 0);
        assert!(registry.list_plugins().is_empty());
        // 钩子列表中不应再包含该插件
        let results = registry.emit("my_hook", serde_json::Value::Null);
        assert!(results.is_empty());

        cleanup(&path);
    }

    #[test]
    fn subscribe_and_emit_basic() {
        let path = temp_wasm_path("subscribe_emit");
        cleanup(&path);
        write_valid_wasm(&path);

        let mut registry = PluginRegistry::new().unwrap();
        let id = registry.register(&path).unwrap();
        registry.subscribe("event", id);

        let results = registry.emit("event", serde_json::Value::Null);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, id);
        // 默认 L1 权限，event 非预定义 hook，需要 L1，权限通过但 WASM 未集成
        let err = results[0].1.as_ref().unwrap_err();
        assert!(err.contains("WASM 运行时尚未集成"), "应返回 WASM 未集成错误: {}", err);

        cleanup(&path);
    }

    #[test]
    fn emit_with_no_subscribers_returns_empty() {
        let mut registry = PluginRegistry::new().unwrap();
        let results = registry.emit("nothing", serde_json::Value::Null);
        assert!(results.is_empty());
    }

    #[test]
    fn emit_permission_denied_for_l2_hook() {
        let path = temp_wasm_path("emit_denied");
        cleanup(&path);
        write_valid_wasm(&path);

        let mut registry = PluginRegistry::new().unwrap();
        let id = registry.register(&path).unwrap();
        registry.subscribe("write_file", id);

        let results = registry.emit("write_file", serde_json::Value::Null);
        assert_eq!(results.len(), 1);
        let err = results[0].1.as_ref().unwrap_err();
        assert!(err.contains("缺少执行"), "应提示缺少权限: {}", err);

        cleanup(&path);
    }

    #[test]
    fn list_plugins_returns_metadata() {
        let path = temp_wasm_path("list");
        cleanup(&path);
        write_valid_wasm(&path);

        let mut registry = PluginRegistry::new().unwrap();
        let id = registry.register(&path).unwrap();
        let plugins = registry.list_plugins();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].id, id);
        assert_eq!(plugins[0].name, "aether_plugin_registry_test_list");
        assert_eq!(plugins[0].version, "0.1.0");

        cleanup(&path);
    }

    #[test]
    fn plugin_count_tracks_register_and_unregister() {
        let path1 = temp_wasm_path("count_a");
        let path2 = temp_wasm_path("count_b");
        cleanup(&path1);
        cleanup(&path2);
        write_valid_wasm(&path1);
        write_valid_wasm(&path2);

        let mut registry = PluginRegistry::new().unwrap();
        assert_eq!(registry.plugin_count(), 0);
        let id1 = registry.register(&path1).unwrap();
        assert_eq!(registry.plugin_count(), 1);
        let id2 = registry.register(&path2).unwrap();
        assert_eq!(registry.plugin_count(), 2);
        registry.unregister(id1);
        assert_eq!(registry.plugin_count(), 1);
        registry.unregister(id2);
        assert_eq!(registry.plugin_count(), 0);

        cleanup(&path1);
        cleanup(&path2);
    }

    #[test]
    fn multiple_subscribers_same_hook() {
        let path1 = temp_wasm_path("multi_a");
        let path2 = temp_wasm_path("multi_b");
        cleanup(&path1);
        cleanup(&path2);
        write_valid_wasm(&path1);
        write_valid_wasm(&path2);

        let mut registry = PluginRegistry::new().unwrap();
        let id1 = registry.register(&path1).unwrap();
        let id2 = registry.register(&path2).unwrap();
        registry.subscribe("shared", id1);
        registry.subscribe("shared", id2);

        let results = registry.emit("shared", serde_json::Value::Null);
        assert_eq!(results.len(), 2);
        let ids: Vec<PluginId> = results.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));

        cleanup(&path1);
        cleanup(&path2);
    }
}
