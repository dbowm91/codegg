use std::sync::Arc;

use crate::plugin::manifest::{PluginManifest, PluginRuntimeSpec, PluginTrustClass};
use crate::plugin::registry::{PluginInfo, PluginRegistry, PluginRegistryError};
use crate::plugin::service::{PluginError, PluginService};

/// A flattened view of a plugin for management and display purposes.
#[derive(Debug, Clone)]
pub struct PluginManagementView {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    pub enabled: bool,
    pub runtime_kind: String,
    pub trust: PluginTrustClass,
    pub source_path: Option<String>,
    pub command_count: usize,
    pub hook_count: usize,
    pub panel_count: usize,
    pub status_widget_count: usize,
    pub event_subscription_count: usize,
    pub permissions_summary: String,
    pub diagnostic_count: usize,
    pub description: Option<String>,
}

impl PluginManagementView {
    /// Build a view from a [`PluginInfo`].
    pub fn from_info(info: &PluginInfo) -> Self {
        let manifest = &info.manifest;
        let permissions_summary = summarize_permissions(manifest);

        Self {
            id: info.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            api_version: manifest.api_version,
            enabled: info.enabled,
            runtime_kind: manifest.runtime_kind().to_string(),
            trust: info.trust,
            source_path: None,
            command_count: manifest.commands().count(),
            hook_count: manifest.hooks_capabilities().count() + manifest.hooks.len(),
            panel_count: manifest.panels().count(),
            status_widget_count: manifest.status_widgets().count(),
            event_subscription_count: manifest.event_subscriptions().count(),
            permissions_summary,
            diagnostic_count: info.diagnostics.len(),
            description: manifest.description.clone(),
        }
    }
}

fn summarize_permissions(manifest: &PluginManifest) -> String {
    let perms = &manifest.permissions;
    let mut parts = Vec::new();
    if perms.network {
        parts.push("network".to_string());
    }
    match perms.filesystem {
        crate::plugin::manifest::FilesystemPermission::None => {}
        ref fs => parts.push(format!("fs:{:?}", fs)),
    }
    if !perms.env.is_empty() {
        parts.push(format!("env:{}", perms.env.join(",")));
    }
    if !perms.secrets.is_empty() {
        parts.push(format!("secrets:{}", perms.secrets.join(",")));
    }
    if perms.session_messages {
        parts.push("session_messages".to_string());
    }
    if perms.tool_interception {
        parts.push("tool_interception".to_string());
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(", ")
    }
}

/// Errors from plugin management operations.
#[derive(Debug, thiserror::Error)]
pub enum PluginManagementError {
    #[error("plugin not found: {0}")]
    NotFound(String),
    #[error("ambiguous selector '{0}': matches {1:?}")]
    Ambiguous(String, Vec<String>),
    #[error("no plugin matches selector: {0}")]
    NoMatch(String),
    #[error("registry error: {0}")]
    Registry(#[from] PluginRegistryError),
    #[error("service error: {0}")]
    Service(String),
    #[error("install error: {0}")]
    Install(String),
}

impl From<PluginError> for PluginManagementError {
    fn from(e: PluginError) -> Self {
        PluginManagementError::Service(e.to_string())
    }
}

/// A single diagnostic check within a doctor report.
#[derive(Debug, Clone)]
pub struct PluginDoctorCheck {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

/// Doctor report for a single plugin.
#[derive(Debug, Clone)]
pub struct PluginDoctorReport {
    pub plugin_id: String,
    pub plugin_name: String,
    pub checks: Vec<PluginDoctorCheck>,
}

/// Resolve a user-provided selector string to a concrete plugin.
///
/// Resolution order:
/// 1. Exact plugin id match
/// 2. Exact manifest name match
/// 3. Unique prefix match on id (case-insensitive)
/// 4. Unique prefix match on name (case-insensitive)
/// 5. Error on none or ambiguous
pub async fn resolve_plugin_selector(
    registry: &PluginRegistry,
    selector: &str,
) -> Result<PluginInfo, PluginManagementError> {
    let all = registry.list().await;
    let selector_lower = selector.to_lowercase();

    // 1. Exact plugin id match
    if let Some(info) = all.iter().find(|p| p.id == selector) {
        return Ok(info.clone());
    }

    // 2. Exact manifest name match
    if let Some(info) = all.iter().find(|p| p.manifest.name == selector) {
        return Ok(info.clone());
    }

    // 3. Unique prefix match on id (case-insensitive)
    let id_prefixes: Vec<&PluginInfo> = all
        .iter()
        .filter(|p| p.id.to_lowercase().starts_with(&selector_lower))
        .collect();
    if id_prefixes.len() == 1 {
        return Ok(id_prefixes[0].clone());
    }
    if id_prefixes.len() > 1 {
        return Err(PluginManagementError::Ambiguous(
            selector.to_string(),
            id_prefixes.iter().map(|p| p.id.clone()).collect(),
        ));
    }

    // 4. Unique prefix match on name (case-insensitive)
    let name_prefixes: Vec<&PluginInfo> = all
        .iter()
        .filter(|p| p.manifest.name.to_lowercase().starts_with(&selector_lower))
        .collect();
    if name_prefixes.len() == 1 {
        return Ok(name_prefixes[0].clone());
    }
    if name_prefixes.len() > 1 {
        return Err(PluginManagementError::Ambiguous(
            selector.to_string(),
            name_prefixes.iter().map(|p| p.id.clone()).collect(),
        ));
    }

    // 5. No match
    Err(PluginManagementError::NoMatch(selector.to_string()))
}

/// High-level plugin management API.
pub struct PluginManager {
    service: Arc<PluginService>,
}

impl PluginManager {
    pub fn new(service: Arc<PluginService>) -> Self {
        Self { service }
    }

    /// List all registered plugins.
    pub async fn list(&self) -> Vec<PluginManagementView> {
        let infos = self.service.registry().list().await;
        infos.iter().map(PluginManagementView::from_info).collect()
    }

    /// Get detailed info for a single plugin by selector.
    pub async fn info(
        &self,
        selector: &str,
    ) -> Result<PluginManagementView, PluginManagementError> {
        let info = resolve_plugin_selector(self.service.registry(), selector).await?;
        Ok(PluginManagementView::from_info(&info))
    }

    /// Enable a plugin by selector.
    ///
    /// Returns the updated view after enabling.
    ///
    /// NOTE: This requires `PluginRegistry::set_enabled` to take `&self`
    /// instead of `&mut self`. The current registry signature requires
    /// `&mut self` which is incompatible with `Arc<PluginRegistry>`.
    /// The registry method should be updated to take `&self` since all
    /// mutations go through internal `RwLock`s.
    ///
    /// Until the registry is updated, this returns a `Service` error.
    pub async fn enable(
        &self,
        selector: &str,
    ) -> Result<PluginManagementView, PluginManagementError> {
        let info = resolve_plugin_selector(self.service.registry(), selector).await?;
        // TODO: Once PluginRegistry::set_enabled takes `&self`, call:
        //   self.service.registry().set_enabled(&info.id, true).await?;
        // For now, the TUI handler should call registry.set_enabled() directly
        // when it has mutable access.
        let _ = &info;
        Err(PluginManagementError::Service(
            "enable requires registry.set_enabled to take &self; \
             use the TUI handler with direct registry access for now"
                .to_string(),
        ))
    }

    /// Disable a plugin by selector.
    ///
    /// Returns the updated view after disabling.
    ///
    /// See [`Self::enable`] for the `set_enabled` signature limitation.
    pub async fn disable(
        &self,
        selector: &str,
    ) -> Result<PluginManagementView, PluginManagementError> {
        let info = resolve_plugin_selector(self.service.registry(), selector).await?;
        let _ = &info;
        Err(PluginManagementError::Service(
            "disable requires registry.set_enabled to take &self; \
             use the TUI handler with direct registry access for now"
                .to_string(),
        ))
    }

    /// Uninstall a plugin by selector (removes from registry).
    pub async fn uninstall(
        &self,
        selector: &str,
    ) -> Result<PluginManagementView, PluginManagementError> {
        let info = resolve_plugin_selector(self.service.registry(), selector).await?;
        let removed = self
            .service
            .registry()
            .unregister(&info.id)
            .await
            .ok_or_else(|| {
                PluginManagementError::NotFound(format!(
                    "plugin '{}' not found during unregister",
                    info.id
                ))
            })?;
        Ok(PluginManagementView::from_info(&removed))
    }

    /// Run diagnostic checks on a plugin.
    pub async fn doctor(
        &self,
        selector: &str,
    ) -> Result<PluginDoctorReport, PluginManagementError> {
        let info = resolve_plugin_selector(self.service.registry(), selector).await?;
        let view = PluginManagementView::from_info(&info);
        let mut checks = Vec::new();

        // Check: plugin is registered
        checks.push(PluginDoctorCheck {
            name: "registered".to_string(),
            passed: true,
            message: format!("Plugin '{}' is registered", info.id),
        });

        // Check: manifest has required fields
        let manifest_ok = !info.manifest.name.is_empty() && !info.manifest.version.is_empty();
        checks.push(PluginDoctorCheck {
            name: "manifest_valid".to_string(),
            passed: manifest_ok,
            message: if manifest_ok {
                "Manifest has name and version".to_string()
            } else {
                "Manifest is missing name or version".to_string()
            },
        });

        // Check: has at least one capability
        let has_caps = !info.manifest.capabilities.is_empty() || !info.manifest.hooks.is_empty();
        checks.push(PluginDoctorCheck {
            name: "has_capabilities".to_string(),
            passed: has_caps,
            message: if has_caps {
                format!(
                    "Plugin declares {} capability(ies)",
                    info.manifest.capabilities.len()
                )
            } else {
                "Plugin declares no capabilities".to_string()
            },
        });

        // Check: runtime is declared
        let runtime_declared = matches!(
            &info.manifest.runtime,
            PluginRuntimeSpec::Process { .. } | PluginRuntimeSpec::Wasm { .. }
        ) || matches!(&info.manifest.runtime, PluginRuntimeSpec::Builtin { handler } if !handler.is_empty());
        checks.push(PluginDoctorCheck {
            name: "runtime_configured".to_string(),
            passed: runtime_declared,
            message: if runtime_declared {
                format!("Runtime kind: {}", view.runtime_kind)
            } else {
                "Runtime not configured".to_string()
            },
        });

        // Check: no diagnostics at error level
        let error_diags: Vec<_> = info
            .diagnostics
            .iter()
            .filter(|d| d.level == crate::plugin::PluginDiagnosticLevel::Error)
            .collect();
        checks.push(PluginDoctorCheck {
            name: "no_error_diagnostics".to_string(),
            passed: error_diags.is_empty(),
            message: if error_diags.is_empty() {
                "No error-level diagnostics".to_string()
            } else {
                format!("{} error-level diagnostic(s)", error_diags.len())
            },
        });

        // Check: permissions are non-default
        let perms = &info.manifest.permissions;
        let perms_default = !perms.network
            && matches!(perms.filesystem, crate::plugin::manifest::FilesystemPermission::None)
            && perms.env.is_empty()
            && perms.secrets.is_empty()
            && !perms.session_messages
            && !perms.tool_interception;
        checks.push(PluginDoctorCheck {
            name: "permissions_declared".to_string(),
            passed: true, // informational, not a failure
            message: if perms_default {
                "No permissions declared (default)".to_string()
            } else {
                format!("Permissions: {}", view.permissions_summary)
            },
        });

        let all_passed = checks.iter().all(|c| c.passed);

        if !all_passed {
            tracing::warn!(
                plugin = info.id,
                failed_checks = checks.iter().filter(|c| !c.passed).count(),
                "plugin doctor found issues"
            );
        }

        Ok(PluginDoctorReport {
            plugin_id: info.id,
            plugin_name: info.manifest.name,
            checks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manifest::{
        PluginCapability, PluginCommandSpec, PluginHookSpec, PluginManifest,
        PluginPanelContribution, PluginStatusContribution,
    };

    fn make_manifest(name: &str, capabilities: Vec<PluginCapability>) -> PluginManifest {
        PluginManifest {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            capabilities,
            ..Default::default()
        }
    }

    fn make_info(id: &str, name: &str, enabled: bool) -> PluginInfo {
        PluginInfo {
            id: id.to_string(),
            manifest: make_manifest(name, Vec::new()),
            enabled,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
        }
    }

    fn make_info_with_caps(
        id: &str,
        name: &str,
        enabled: bool,
        capabilities: Vec<PluginCapability>,
    ) -> PluginInfo {
        PluginInfo {
            id: id.to_string(),
            manifest: make_manifest(name, capabilities),
            enabled,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
        }
    }

    #[tokio::test]
    async fn resolve_exact_id() {
        let registry = PluginRegistry::new();
        registry.register(make_info("my-plugin:1", "my-plugin", true)).await.unwrap();

        let result = resolve_plugin_selector(&registry, "my-plugin:1").await.unwrap();
        assert_eq!(result.id, "my-plugin:1");
    }

    #[tokio::test]
    async fn resolve_exact_name() {
        let registry = PluginRegistry::new();
        registry.register(make_info("plugin:my-plugin", "my-plugin", true)).await.unwrap();

        let result = resolve_plugin_selector(&registry, "my-plugin").await.unwrap();
        assert_eq!(result.manifest.name, "my-plugin");
    }

    #[tokio::test]
    async fn resolve_unique_id_prefix() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("my-plugin-alpha:1", "alpha", true))
            .await
            .unwrap();
        registry
            .register(make_info("my-plugin-beta:1", "beta", true))
            .await
            .unwrap();

        let result = resolve_plugin_selector(&registry, "my-plugin-a").await.unwrap();
        assert_eq!(result.id, "my-plugin-alpha:1");
    }

    #[tokio::test]
    async fn resolve_unique_name_prefix() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("p:alpha", "alpha-plugin", true))
            .await
            .unwrap();
        registry
            .register(make_info("p:beta", "beta-plugin", true))
            .await
            .unwrap();

        let result = resolve_plugin_selector(&registry, "alpha").await.unwrap();
        assert_eq!(result.manifest.name, "alpha-plugin");
    }

    #[tokio::test]
    async fn ambiguous_prefix_returns_error() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("my-plugin-a:1", "a", true))
            .await
            .unwrap();
        registry
            .register(make_info("my-plugin-b:1", "b", true))
            .await
            .unwrap();

        let result = resolve_plugin_selector(&registry, "my-plugin-").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            PluginManagementError::Ambiguous(sel, matches) => {
                assert_eq!(sel, "my-plugin-");
                assert_eq!(matches.len(), 2);
            }
            other => panic!("expected Ambiguous, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn no_match_returns_error() {
        let registry = PluginRegistry::new();
        registry.register(make_info("a:1", "alpha", true)).await.unwrap();

        let result = resolve_plugin_selector(&registry, "nonexistent").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            PluginManagementError::NoMatch(sel) => assert_eq!(sel, "nonexistent"),
            other => panic!("expected NoMatch, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn resolve_case_insensitive_id_prefix() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("MyPlugin:1", "test", true))
            .await
            .unwrap();

        let result = resolve_plugin_selector(&registry, "myplugin").await.unwrap();
        assert_eq!(result.id, "MyPlugin:1");
    }

    #[test]
    fn view_from_info_counts_capabilities() {
        let caps = vec![
            PluginCapability::Command(PluginCommandSpec {
                name: "deploy".into(),
                aliases: Vec::new(),
                description: None,
                handler: None,
                output: Vec::new(),
            }),
            PluginCapability::Hook(PluginHookSpec {
                hook_type: "auth".into(),
                priority: 0,
                handler: None,
            }),
            PluginCapability::Panel(PluginPanelContribution {
                id: "panel-1".into(),
                title: "Panel".into(),
                placement: "sidebar".into(),
                handler: None,
            }),
            PluginCapability::StatusWidget(PluginStatusContribution {
                id: "widget-1".into(),
                label: None,
                placement: "statusbar".into(),
                refresh_ms: None,
                handler: None,
            }),
        ];
        let info = make_info_with_caps("test:1", "test", true, caps);
        let view = PluginManagementView::from_info(&info);

        assert_eq!(view.command_count, 1);
        assert_eq!(view.hook_count, 1);
        assert_eq!(view.panel_count, 1);
        assert_eq!(view.status_widget_count, 1);
        assert_eq!(view.event_subscription_count, 0);
    }

    #[test]
    fn view_from_info_permissions_summary() {
        let mut manifest = make_manifest("test", Vec::new());
        manifest.permissions.network = true;
        manifest.permissions.env = vec!["API_KEY".to_string()];
        let info = PluginInfo {
            id: "test:1".into(),
            manifest,
            enabled: true,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
        };
        let view = PluginManagementView::from_info(&info);
        assert!(view.permissions_summary.contains("network"));
        assert!(view.permissions_summary.contains("env:API_KEY"));
    }

    #[test]
    fn view_from_info_permissions_default_is_none() {
        let info = make_info("test:1", "test", true);
        let view = PluginManagementView::from_info(&info);
        assert_eq!(view.permissions_summary, "none");
    }

    #[tokio::test]
    async fn doctor_report_checks_registered_plugin() {
        let registry = PluginRegistry::new();
        let caps = vec![PluginCapability::Command(PluginCommandSpec {
            name: "test-cmd".into(),
            aliases: Vec::new(),
            description: None,
            handler: None,
            output: Vec::new(),
        })];
        let manifest = PluginManifest {
            name: "test".into(),
            version: "1.0.0".into(),
            runtime: PluginRuntimeSpec::Builtin {
                handler: "test_handler".into(),
            },
            capabilities: caps,
            ..Default::default()
        };
        registry
            .register(PluginInfo {
                id: "test:1".into(),
                manifest,
                enabled: true,
                trust: PluginTrustClass::Builtin,
                diagnostics: Vec::new(),
            })
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let report = manager.doctor("test:1").await.unwrap();
        assert_eq!(report.plugin_id, "test:1");
        assert_eq!(report.plugin_name, "test");
        assert!(!report.checks.is_empty());
        assert!(report.checks.iter().all(|c| c.passed));
    }

    #[tokio::test]
    async fn doctor_report_detects_empty_manifest() {
        let registry = PluginRegistry::new();
        registry
            .register(PluginInfo {
                id: "bad:1".into(),
                manifest: PluginManifest::default(),
                enabled: true,
                trust: PluginTrustClass::Builtin,
                diagnostics: Vec::new(),
            })
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let report = manager.doctor("bad:1").await.unwrap();
        let manifest_check = report.checks.iter().find(|c| c.name == "manifest_valid").unwrap();
        assert!(!manifest_check.passed);
    }

    #[tokio::test]
    async fn list_returns_all_plugins() {
        let registry = PluginRegistry::new();
        registry.register(make_info("a:1", "alpha", true)).await.unwrap();
        registry.register(make_info("b:1", "beta", false)).await.unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let views = manager.list().await;
        assert_eq!(views.len(), 2);
    }
}
