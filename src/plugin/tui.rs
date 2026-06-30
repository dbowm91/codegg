//! Legacy TUI plugin registry.
//!
//! **Deprecated:** This module contains the pre-Phase 5 TUI extension types.
//! New plugin UI contributions should be declared via `PluginCapability::Panel`
//! and `PluginCapability::StatusWidget` in the plugin manifest, and consumed
//! through the protocol `UiEffect` system. Do not add new functionality here.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiRoute {
    pub path: String,
    pub label: String,
    pub plugin_id: String,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiComponent {
    pub name: String,
    pub plugin_id: String,
    pub config: serde_json::Value,
}

pub struct TuiPluginRegistry {
    routes: Arc<RwLock<Vec<TuiRoute>>>,
    components: Arc<RwLock<Vec<TuiComponent>>>,
    plugin_configs: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

impl TuiPluginRegistry {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(Vec::new())),
            components: Arc::new(RwLock::new(Vec::new())),
            plugin_configs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register_route(&self, route: TuiRoute) {
        self.routes.write().await.push(route);
    }

    pub async fn register_component(&self, component: TuiComponent) {
        self.components.write().await.push(component);
    }

    pub async fn set_plugin_config(&self, plugin_id: &str, config: serde_json::Value) {
        self.plugin_configs
            .write()
            .await
            .insert(plugin_id.to_string(), config);
    }

    pub async fn get_plugin_config(&self, plugin_id: &str) -> Option<serde_json::Value> {
        self.plugin_configs.read().await.get(plugin_id).cloned()
    }

    pub async fn routes(&self) -> Vec<TuiRoute> {
        self.routes.read().await.clone()
    }

    pub async fn components(&self) -> Vec<TuiComponent> {
        self.components.read().await.clone()
    }

    pub async fn routes_for_plugin(&self, plugin_id: &str) -> Vec<TuiRoute> {
        self.routes
            .read()
            .await
            .iter()
            .filter(|r| r.plugin_id == plugin_id)
            .cloned()
            .collect()
    }

    pub async fn components_for_plugin(&self, plugin_id: &str) -> Vec<TuiComponent> {
        self.components
            .read()
            .await
            .iter()
            .filter(|c| c.plugin_id == plugin_id)
            .cloned()
            .collect()
    }

    pub async fn find_route(&self, path: &str) -> Option<TuiRoute> {
        self.routes
            .read()
            .await
            .iter()
            .find(|r| r.path == path)
            .cloned()
    }
}

impl Default for TuiPluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}
