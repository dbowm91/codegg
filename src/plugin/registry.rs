use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::plugin::hooks::{HookRegistration, HookType};
use crate::plugin::manifest::{
    PluginCapability, PluginDiagnostic, PluginManifest, PluginTrustClass,
};

/// Information about a registered plugin.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub manifest: PluginManifest,
    pub enabled: bool,
    pub trust: PluginTrustClass,
    pub diagnostics: Vec<PluginDiagnostic>,
}

/// A command contributed by a plugin.
#[derive(Debug, Clone)]
pub struct PluginCommandRegistration {
    pub plugin_id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub handler: Option<String>,
}

/// A hook contributed by a plugin.
#[derive(Debug, Clone)]
pub struct PluginHookRegistration {
    pub plugin_id: String,
    pub hook_type: HookType,
    pub priority: i32,
    pub handler: Option<String>,
}

/// A panel contributed by a plugin.
#[derive(Debug, Clone)]
pub struct PluginPanelRegistration {
    pub plugin_id: String,
    pub id: String,
    pub title: String,
    pub placement: String,
    pub handler: Option<String>,
}

/// A status widget contributed by a plugin.
#[derive(Debug, Clone)]
pub struct PluginStatusRegistration {
    pub plugin_id: String,
    pub id: String,
    pub label: Option<String>,
    pub placement: String,
    pub refresh_ms: Option<u64>,
    pub handler: Option<String>,
}

/// An event subscription contributed by a plugin.
#[derive(Debug, Clone)]
pub struct PluginEventRegistration {
    pub plugin_id: String,
    pub event_type: String,
    pub handler: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginRegistryError {
    #[error("plugin already registered: {0}")]
    AlreadyRegistered(String),
    #[error("plugin not found: {0}")]
    NotFound(String),
    #[error("duplicate command name: '{0}' (owned by '{1}')")]
    DuplicateCommand(String, String),
    #[error("duplicate panel id: '{0}' (owned by '{1}')")]
    DuplicatePanel(String, String),
    #[error("duplicate status widget id: '{0}' (owned by '{1}')")]
    DuplicateStatusWidget(String, String),
}

pub struct PluginRegistry {
    plugins: RwLock<HashMap<String, PluginInfo>>,
    hooks: RwLock<Vec<HookRegistration>>,
    commands: RwLock<Vec<PluginCommandRegistration>>,
    panels: RwLock<Vec<PluginPanelRegistration>>,
    status_widgets: RwLock<Vec<PluginStatusRegistration>>,
    event_subscribers: RwLock<Vec<PluginEventRegistration>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: RwLock::new(HashMap::new()),
            hooks: RwLock::new(Vec::new()),
            commands: RwLock::new(Vec::new()),
            panels: RwLock::new(Vec::new()),
            status_widgets: RwLock::new(Vec::new()),
            event_subscribers: RwLock::new(Vec::new()),
        }
    }

    /// Register a plugin with its full manifest.
    ///
    /// Extracts capabilities from the manifest and indexes them.
    /// Rejects duplicate command names (unless the owning plugin is disabled).
    pub async fn register(
        &self,
        info: PluginInfo,
    ) -> Result<(), PluginRegistryError> {
        let id = info.id.clone();

        // Check for duplicate registration
        if self.plugins.read().await.contains_key(&id) {
            return Err(PluginRegistryError::AlreadyRegistered(id));
        }

        // Extract capability registrations from manifest
        let hook_specs = self.extract_hooks(&id, &info.manifest).await?;
        let command_specs = self.extract_commands(&id, &info.manifest).await?;
        let panel_specs = self.extract_panels(&id, &info.manifest).await;
        let status_specs = self.extract_status_widgets(&id, &info.manifest).await;
        let event_specs = self.extract_event_subscribers(&id, &info.manifest).await;

        // Check command duplicate rules
        self.check_command_duplicates(&command_specs).await?;

        // Store plugin
        self.plugins.write().await.insert(id.clone(), info);

        // Index capabilities
        self.hooks.write().await.extend(hook_specs);
        self.commands.write().await.extend(command_specs);
        self.panels.write().await.extend(panel_specs);
        self.status_widgets.write().await.extend(status_specs);
        self.event_subscribers.write().await.extend(event_specs);

        self.sort_hooks().await;

        Ok(())
    }

    /// Register a plugin from legacy format (backward compatible).
    pub async fn register_legacy(
        &self,
        id: String,
        manifest: PluginManifest,
        _path: std::path::PathBuf,
        hook_specs: Vec<HookRegistration>,
    ) {
        let info = PluginInfo {
            id: id.clone(),
            manifest,
            enabled: true,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
        };

        // Best-effort: ignore duplicate errors for legacy registration
        let _ = self.register(info).await;

        // For legacy, also add raw hook registrations
        self.hooks.write().await.extend(hook_specs);
        self.sort_hooks().await;
    }

    /// Register with hook specs (backward compatible with existing PluginService API).
    pub async fn register_with_hooks(
        &self,
        info: PluginInfo,
        hook_specs: Vec<HookRegistration>,
    ) -> Result<(), PluginRegistryError> {
        self.register(info).await?;

        // Add additional hook registrations (e.g., from builtin registration)
        self.hooks.write().await.extend(hook_specs);
        self.sort_hooks().await;

        Ok(())
    }

    pub async fn unregister(&self, id: &str) -> Option<PluginInfo> {
        self.plugins.write().await.remove(id);
        self.hooks.write().await.retain(|h| h.plugin_id != id);
        self.commands.write().await.retain(|c| c.plugin_id != id);
        self.panels.write().await.retain(|p| p.plugin_id != id);
        self.status_widgets
            .write()
            .await
            .retain(|s| s.plugin_id != id);
        self.event_subscribers
            .write()
            .await
            .retain(|e| e.plugin_id != id);
        // Note: we return None here because the old API returned Option<PluginInfo>
        // but we consumed it. The caller can use `get` before `unregister` if needed.
        None
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

    pub async fn is_enabled(&self, id: &str) -> bool {
        self.plugins
            .read()
            .await
            .get(id)
            .map(|p| p.enabled)
            .unwrap_or(false)
    }

    pub async fn set_enabled(
        &mut self,
        id: &str,
        enabled: bool,
    ) -> Result<(), PluginRegistryError> {
        if let Some(info) = self.plugins.write().await.get_mut(id) {
            info.enabled = enabled;
            Ok(())
        } else {
            Err(PluginRegistryError::NotFound(id.to_string()))
        }
    }

    // --- Capability queries ---

    /// Get all hook registrations for a hook type (from enabled plugins only).
    pub async fn hooks_for(&self, hook_type: HookType) -> Vec<HookRegistration> {
        self.hooks
            .read()
            .await
            .iter()
            .filter(|h| h.hook_type == hook_type)
            .filter(|h| self.is_enabled_sync(&h.plugin_id))
            .cloned()
            .collect::<Vec<_>>()
    }

    /// Get all hook registrations (including from disabled plugins, for inspection).
    pub async fn all_hooks_for(&self, hook_type: HookType) -> Vec<HookRegistration> {
        self.hooks
            .read()
            .await
            .iter()
            .filter(|h| h.hook_type == hook_type)
            .cloned()
            .collect()
    }

    /// Look up a command by name (from enabled plugins only).
    pub async fn command(&self, name: &str) -> Option<PluginCommandRegistration> {
        let normalized = normalize_command_name(name);
        self.commands
            .read()
            .await
            .iter()
            .filter(|c| self.is_enabled_sync(&c.plugin_id))
            .find(|c| {
                normalize_command_name(&c.name) == normalized
                    || c.aliases
                        .iter()
                        .any(|a| normalize_command_name(a) == normalized)
            })
            .cloned()
    }

    /// Get all registered commands (from enabled plugins only).
    pub async fn commands(&self) -> Vec<PluginCommandRegistration> {
        self.commands
            .read()
            .await
            .iter()
            .filter(|c| self.is_enabled_sync(&c.plugin_id))
            .cloned()
            .collect()
    }

    /// Get all registered commands (including from disabled plugins).
    pub async fn all_commands(&self) -> Vec<PluginCommandRegistration> {
        self.commands.read().await.clone()
    }

    /// Get all panels (from enabled plugins only).
    pub async fn panels(&self) -> Vec<PluginPanelRegistration> {
        self.panels
            .read()
            .await
            .iter()
            .filter(|p| self.is_enabled_sync(&p.plugin_id))
            .cloned()
            .collect()
    }

    /// Get all status widgets (from enabled plugins only).
    pub async fn status_widgets(&self) -> Vec<PluginStatusRegistration> {
        self.status_widgets
            .read()
            .await
            .iter()
            .filter(|s| self.is_enabled_sync(&s.plugin_id))
            .cloned()
            .collect()
    }

    /// Get event subscribers for a specific event type (from enabled plugins only).
    pub async fn event_subscribers(
        &self,
        event_type: &str,
    ) -> Vec<PluginEventRegistration> {
        self.event_subscribers
            .read()
            .await
            .iter()
            .filter(|e| self.is_enabled_sync(&e.plugin_id))
            .filter(|e| e.event_type == event_type || e.event_type == "*")
            .cloned()
            .collect()
    }

    // --- Internal helpers ---

    fn is_enabled_sync(&self, plugin_id: &str) -> bool {
        // This is a sync helper that checks the in-memory state.
        // Since we're called from async context and hold a read guard,
        // we need a separate mechanism. For now, we use try_read.
        if let Ok(plugins) = self.plugins.try_read() {
            plugins
                .get(plugin_id)
                .map(|p| p.enabled)
                .unwrap_or(false)
        } else {
            // If we can't get the lock (we're in a write context),
            // default to enabled to avoid silent failures.
            true
        }
    }

    async fn extract_hooks(
        &self,
        plugin_id: &str,
        manifest: &PluginManifest,
    ) -> Result<Vec<HookRegistration>, PluginRegistryError> {
        let mut hooks = Vec::new();

        // From capability declarations
        for cap in &manifest.capabilities {
            if let PluginCapability::Hook(spec) = cap {
                if let Some(hook_type) = HookType::parse(&spec.hook_type) {
                    hooks.push(HookRegistration {
                        plugin_id: plugin_id.to_string(),
                        hook_type,
                        priority: spec.priority,
                    });
                }
            }
        }

        // Also from legacy hooks field
        for legacy_hook in &manifest.hooks {
            if let Some(hook_type) = HookType::parse(&legacy_hook.hook_type) {
                hooks.push(HookRegistration {
                    plugin_id: plugin_id.to_string(),
                    hook_type,
                    priority: legacy_hook.priority.unwrap_or(0),
                });
            }
        }

        Ok(hooks)
    }

    async fn extract_commands(
        &self,
        plugin_id: &str,
        manifest: &PluginManifest,
    ) -> Result<Vec<PluginCommandRegistration>, PluginRegistryError> {
        let mut commands = Vec::new();
        for cap in &manifest.capabilities {
            if let PluginCapability::Command(spec) = cap {
                commands.push(PluginCommandRegistration {
                    plugin_id: plugin_id.to_string(),
                    name: spec.name.clone(),
                    aliases: spec.aliases.clone(),
                    description: spec.description.clone(),
                    handler: spec.handler.clone(),
                });
            }
        }
        Ok(commands)
    }

    async fn extract_panels(
        &self,
        plugin_id: &str,
        manifest: &PluginManifest,
    ) -> Vec<PluginPanelRegistration> {
        manifest
            .capabilities
            .iter()
            .filter_map(|cap| {
                if let PluginCapability::Panel(spec) = cap {
                    Some(PluginPanelRegistration {
                        plugin_id: plugin_id.to_string(),
                        id: spec.id.clone(),
                        title: spec.title.clone(),
                        placement: spec.placement.clone(),
                        handler: spec.handler.clone(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    async fn extract_status_widgets(
        &self,
        plugin_id: &str,
        manifest: &PluginManifest,
    ) -> Vec<PluginStatusRegistration> {
        manifest
            .capabilities
            .iter()
            .filter_map(|cap| {
                if let PluginCapability::StatusWidget(spec) = cap {
                    Some(PluginStatusRegistration {
                        plugin_id: plugin_id.to_string(),
                        id: spec.id.clone(),
                        label: spec.label.clone(),
                        placement: spec.placement.clone(),
                        refresh_ms: spec.refresh_ms,
                        handler: spec.handler.clone(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    async fn extract_event_subscribers(
        &self,
        plugin_id: &str,
        manifest: &PluginManifest,
    ) -> Vec<PluginEventRegistration> {
        manifest
            .capabilities
            .iter()
            .filter_map(|cap| {
                if let PluginCapability::EventSubscription(spec) = cap {
                    Some(PluginEventRegistration {
                        plugin_id: plugin_id.to_string(),
                        event_type: spec.event_type.clone(),
                        handler: spec.handler.clone(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check for duplicate command names across enabled plugins.
    async fn check_command_duplicates(
        &self,
        new_commands: &[PluginCommandRegistration],
    ) -> Result<(), PluginRegistryError> {
        let existing = self.commands.read().await;

        for new_cmd in new_commands {
            let new_normalized = normalize_command_name(&new_cmd.name);

            // Check against existing enabled commands
            for existing_cmd in existing.iter().filter(|c| self.is_enabled_sync(&c.plugin_id)) {
                let existing_normalized = normalize_command_name(&existing_cmd.name);

                if existing_normalized == new_normalized {
                    return Err(PluginRegistryError::DuplicateCommand(
                        new_cmd.name.clone(),
                        existing_cmd.plugin_id.clone(),
                    ));
                }

                // Check aliases
                for alias in &existing_cmd.aliases {
                    if normalize_command_name(alias) == new_normalized {
                        return Err(PluginRegistryError::DuplicateCommand(
                            new_cmd.name.clone(),
                            existing_cmd.plugin_id.clone(),
                        ));
                    }
                }

                // Check new command's aliases against existing name
                for alias in &new_cmd.aliases {
                    if normalize_command_name(alias) == existing_normalized {
                        return Err(PluginRegistryError::DuplicateCommand(
                            alias.clone(),
                            existing_cmd.plugin_id.clone(),
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    async fn sort_hooks(&self) {
        self.hooks.write().await.sort_by_key(|h| h.priority);
    }
}

/// Normalize a command name for comparison: trim leading `/` and lowercase.
pub fn normalize_command_name(name: &str) -> String {
    name.trim_start_matches('/').to_lowercase()
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plugin_info(id: &str, name: &str) -> PluginInfo {
        PluginInfo {
            id: id.to_string(),
            manifest: PluginManifest {
                name: name.to_string(),
                version: "1.0.0".into(),
                ..Default::default()
            },
            enabled: true,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
        }
    }

    #[tokio::test]
    async fn register_and_list() {
        let mut registry = PluginRegistry::new();
        let info = make_plugin_info("test:1", "test");
        registry.register(info).await.unwrap();
        let list = registry.list().await;
        assert_eq!(list.len(), 1);
    }

    #[tokio::test]
    async fn duplicate_registration_rejected() {
        let mut registry = PluginRegistry::new();
        let info1 = make_plugin_info("test:1", "test");
        let info2 = make_plugin_info("test:1", "test2");
        registry.register(info1).await.unwrap();
        let result = registry.register(info2).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn command_lookup_by_name() {
        let mut registry = PluginRegistry::new();
        let manifest = PluginManifest {
            name: "my-plugin".into(),
            capabilities: vec![PluginCapability::Command(
                crate::plugin::manifest::PluginCommandSpec {
                    name: "deploy".into(),
                    aliases: vec!["d".into()],
                    description: None,
                    handler: None,
                    output: Vec::new(),
                },
            )],
            ..Default::default()
        };
        let info = PluginInfo {
            id: "test:1".into(),
            manifest,
            enabled: true,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
        };
        registry.register(info).await.unwrap();

        // Lookup by name
        let cmd = registry.command("deploy").await;
        assert!(cmd.is_some());
        assert_eq!(cmd.unwrap().name, "deploy");

        // Lookup by alias
        let cmd = registry.command("d").await;
        assert!(cmd.is_some());

        // Lookup by normalized name
        let cmd = registry.command("/deploy").await;
        assert!(cmd.is_some());
    }

    #[tokio::test]
    async fn disabled_plugin_excluded_from_queries() {
        let mut registry = PluginRegistry::new();
        let manifest = PluginManifest {
            name: "my-plugin".into(),
            capabilities: vec![PluginCapability::Command(
                crate::plugin::manifest::PluginCommandSpec {
                    name: "test-cmd".into(),
                    aliases: Vec::new(),
                    description: None,
                    handler: None,
                    output: Vec::new(),
                },
            )],
            ..Default::default()
        };
        let info = PluginInfo {
            id: "test:1".into(),
            manifest,
            enabled: true,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
        };
        registry.register(info).await.unwrap();

        // Command is visible
        assert!(registry.command("test-cmd").await.is_some());

        // Disable plugin
        registry.set_enabled("test:1", false).await.unwrap();

        // Command is no longer visible
        assert!(registry.command("test-cmd").await.is_none());
    }

    #[tokio::test]
    async fn normalize_command_name_works() {
        assert_eq!(normalize_command_name("/Deploy"), "deploy");
        assert_eq!(normalize_command_name("DEPLOY"), "deploy");
        assert_eq!(normalize_command_name("/deploy"), "deploy");
    }
}
