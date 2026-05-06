use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;

use crate::plugin::hooks::{HookRegistration, HookType};
use crate::plugin::manifest::PluginManifest;

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub manifest: PluginManifest,
    pub path: PathBuf,
    pub enabled: bool,
    pub error: Option<String>,
}

pub struct PluginRegistry {
    plugins: RwLock<HashMap<String, PluginInfo>>,
    hooks: RwLock<Vec<HookRegistration>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: RwLock::new(HashMap::new()),
            hooks: RwLock::new(Vec::new()),
        }
    }

    pub async fn register(&self, info: PluginInfo, hook_specs: Vec<HookRegistration>) {
        let id = info.id.clone();
        self.plugins.write().await.insert(id.clone(), info);
        self.hooks.write().await.extend(hook_specs);
        self.sort_hooks().await;
    }

    pub async fn unregister(&self, id: &str) {
        self.plugins.write().await.remove(id);
        self.hooks.write().await.retain(|h| h.plugin_id != id);
    }

    pub async fn get(&self, id: &str) -> Option<PluginInfo> {
        self.plugins.read().await.get(id).cloned()
    }

    pub async fn list(&self) -> Vec<PluginInfo> {
        self.plugins.read().await.values().cloned().collect()
    }

    pub async fn enabled_plugins(&self) -> Vec<PluginInfo> {
        self.plugins
            .read()
            .await
            .values()
            .filter(|p| p.enabled)
            .cloned()
            .collect()
    }

    pub async fn hooks_for(&self, hook_type: HookType) -> Vec<HookRegistration> {
        let mut hooks = self
            .hooks
            .read()
            .await
            .iter()
            .filter(|h| h.hook_type == hook_type)
            .cloned()
            .collect::<Vec<_>>();
        hooks.sort_by_key(|h| h.priority);
        hooks
    }

    pub async fn is_enabled(&self, id: &str) -> bool {
        self.plugins
            .read()
            .await
            .get(id)
            .map(|p| p.enabled)
            .unwrap_or(false)
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) {
        if let Some(info) = self.plugins.write().await.get_mut(id) {
            info.enabled = enabled;
        }
    }

    async fn sort_hooks(&self) {
        self.hooks.write().await.sort_by_key(|h| h.priority);
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}
