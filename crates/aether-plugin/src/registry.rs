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
