use std::path::PathBuf;
use std::sync::Arc;

use crate::plugin::manifest::{PluginManifest, PluginRuntimeSpec, PluginTrustClass};
use crate::plugin::marketplace::MarketplacePlugin;
use crate::plugin::policy::PluginInstallPolicy;
use crate::plugin::registry::{
    PluginInfo, PluginRegistry, PluginRegistryError, PluginSourceMetadata,
};
use crate::plugin::service::{PluginError, PluginService};

/// A flattened view of a plugin for management and display purposes.
///
/// `source_path` is derived from `PluginInfo::source.install_path` when the
/// plugin was installed from a local path or discovered from the registry
/// plugins directory. It is `None` for builtins and registry-only plugins
/// whose install location was never recorded.
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
    /// The message from the most recent error-level diagnostic, if any.
    pub last_error: Option<String>,
}

/// Structured result of a plugin uninstall operation.
///
/// `unregistered` is true if the plugin was successfully removed from the
/// live registry. `removed_files` is true if the install directory was
/// successfully deleted from disk. The two flags are independent: a builtin
/// may be unregistered but have no files to remove, or a plugin may be
/// unregistered but its directory could not be deleted (in which case
/// `warning` carries the error).
#[derive(Debug, Clone)]
pub struct PluginUninstallResult {
    pub view: PluginManagementView,
    pub unregistered: bool,
    pub removed_files: bool,
    pub install_path: Option<PathBuf>,
    pub warning: Option<String>,
}

impl PluginManagementView {
    /// Build a view from a [`PluginInfo`].
    pub fn from_info(info: &PluginInfo) -> Self {
        let manifest = &info.manifest;
        let permissions_summary = summarize_permissions(manifest);

        let last_error = info
            .diagnostics
            .iter()
            .rfind(|d| d.level == crate::plugin::PluginDiagnosticLevel::Error)
            .map(|d| d.message.clone());

        Self {
            id: info.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            api_version: manifest.api_version,
            enabled: info.enabled,
            runtime_kind: manifest.runtime_kind().to_string(),
            trust: info.trust,
            source_path: source_path_from_metadata(info.source.as_ref()),
            command_count: manifest.commands().count(),
            hook_count: manifest.hooks_capabilities().count() + manifest.hooks.len(),
            panel_count: manifest.panels().count(),
            status_widget_count: manifest.status_widgets().count(),
            event_subscription_count: manifest.event_subscriptions().count(),
            permissions_summary,
            diagnostic_count: info.diagnostics.len(),
            description: manifest.description.clone(),
            last_error,
        }
    }

    /// Build a view from a [`MarketplacePlugin`] (filesystem-discovered plugin)
    /// combined with its current enabled state.
    pub fn from_marketplace(plugin: &MarketplacePlugin, enabled: bool) -> Self {
        Self {
            id: plugin.id.clone(),
            name: plugin.name.clone(),
            version: plugin.version.clone(),
            api_version: 1, // marketplace plugins don't carry API version separately
            enabled,
            runtime_kind: "marketplace".to_string(),
            trust: PluginTrustClass::TrustedLocal,
            source_path: Some(format!("{}/{}", "plugins", plugin.id)),
            command_count: 0,
            hook_count: plugin.hooks.len(),
            panel_count: 0,
            status_widget_count: 0,
            event_subscription_count: 0,
            permissions_summary: "n/a (marketplace listing)".to_string(),
            diagnostic_count: 0,
            description: plugin.description.clone(),
            last_error: None,
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

/// Extract a displayable source path from plugin source metadata, if any.
///
/// Returns the install path string when the plugin recorded one. Falls back
/// to the original source path when only that is available. Returns `None`
/// when there is no metadata or no path at all (e.g., for builtins).
fn source_path_from_metadata(source: Option<&PluginSourceMetadata>) -> Option<String> {
    let meta = source?;
    if let Some(install) = meta.install_path.as_ref() {
        return Some(install.display().to_string());
    }
    if let Some(original) = meta.original_source_path.as_ref() {
        return Some(original.display().to_string());
    }
    None
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

/// High-level plugin management API.
#[derive(Clone)]
pub struct PluginManager {
    service: Arc<PluginService>,
}

impl PluginManager {
    pub fn new(service: Arc<PluginService>) -> Self {
        Self { service }
    }

    /// Access the underlying [`PluginService`].
    pub fn service(&self) -> &Arc<PluginService> {
        &self.service
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
        let info = self
            .service
            .registry()
            .resolve_plugin_selector(selector)
            .await
            .map_err(registry_error_to_management)?;
        Ok(PluginManagementView::from_info(&info))
    }

    /// Enable a plugin by selector.
    ///
    /// Returns the updated view after enabling.
    pub async fn enable(
        &self,
        selector: &str,
    ) -> Result<PluginManagementView, PluginManagementError> {
        let info = self
            .service
            .registry()
            .resolve_plugin_selector(selector)
            .await
            .map_err(registry_error_to_management)?;
        self.service.registry().set_enabled(&info.id, true).await?;
        let updated = self
            .service
            .registry()
            .get(&info.id)
            .await
            .ok_or_else(|| PluginManagementError::NotFound(info.id.clone()))?;
        Ok(PluginManagementView::from_info(&updated))
    }

    /// Disable a plugin by selector.
    ///
    /// Returns the updated view after disabling.
    pub async fn disable(
        &self,
        selector: &str,
    ) -> Result<PluginManagementView, PluginManagementError> {
        let info = self
            .service
            .registry()
            .resolve_plugin_selector(selector)
            .await
            .map_err(registry_error_to_management)?;
        self.service.registry().set_enabled(&info.id, false).await?;
        let updated = self
            .service
            .registry()
            .get(&info.id)
            .await
            .ok_or_else(|| PluginManagementError::NotFound(info.id.clone()))?;
        Ok(PluginManagementView::from_info(&updated))
    }

    /// Remove (unregister) a plugin from the registry by selector.
    ///
    /// This does NOT delete files on disk. For safe filesystem removal,
    /// the TUI handler should validate the path against the canonical
    /// plugins directory before calling `remove`.
    pub async fn remove(
        &self,
        selector: &str,
    ) -> Result<PluginManagementView, PluginManagementError> {
        let info = self
            .service
            .registry()
            .resolve_plugin_selector(selector)
            .await
            .map_err(registry_error_to_management)?;
        let removed = self
            .service
            .registry()
            .unregister(&info.id)
            .await
            .ok_or_else(|| PluginManagementError::NotFound(info.id.clone()))?;
        Ok(PluginManagementView::from_info(&removed))
    }

    /// Install a plugin from a local filesystem path and register it
    /// in the live registry so subsequent `list()` calls include it.
    ///
    /// The source directory must contain a `manifest.toml`. The plugin
    /// is copied into the canonical plugins directory, the manifest is
    /// parsed, and the plugin is registered in the registry.
    pub async fn install_from_path(
        &self,
        path: &std::path::Path,
    ) -> Result<PluginManagementView, PluginManagementError> {
        let dest = crate::plugin::install::install_from_path(path)
            .await
            .map_err(|e| PluginManagementError::Install(e.to_string()))?;

        let manifest_path = dest.join("manifest.toml");
        let content = tokio::fs::read_to_string(&manifest_path)
            .await
            .map_err(|e| PluginManagementError::Install(format!("failed to read manifest: {e}")))?;
        let manifest: PluginManifest = toml::from_str(&content)
            .map_err(|e| PluginManagementError::Install(format!("invalid manifest: {e}")))?;

        let plugin_id = format!("plugin:{}", manifest.name);
        let original_canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let info = PluginInfo {
            id: plugin_id.clone(),
            manifest,
            enabled: true,
            trust: PluginTrustClass::TrustedLocal,
            diagnostics: Vec::new(),
            source: Some(PluginSourceMetadata::local_path(
                original_canonical,
                dest.clone(),
            )),
        };

        self.service
            .registry()
            .register(info)
            .await
            .map_err(PluginManagementError::Registry)?;

        let updated = self
            .service
            .registry()
            .get(&plugin_id)
            .await
            .ok_or(PluginManagementError::NotFound(plugin_id))?;
        Ok(PluginManagementView::from_info(&updated))
    }

    /// Uninstall a plugin: unregister from the registry and remove
    /// its filesystem directory if it is under the canonical plugins dir.
    ///
    /// Removal ordering follows the plan's preferred flow:
    /// 1. Resolve the plugin and capture its source metadata.
    /// 2. If a recorded install path exists, validate it against the canonical
    ///    plugins directory (`validate_uninstall_target`).
    /// 3. Unregister from the live registry.
    /// 4. Delete the install directory. If the delete fails, the plugin is
    ///    already unregistered — the warning surfaces this in the result.
    ///
    /// Builtins (`installed_by = Builtin`) are never deleted from disk.
    /// Plugins without a recorded install path are unregistered but no
    /// filesystem removal is attempted.
    pub async fn uninstall(
        &self,
        selector: &str,
    ) -> Result<PluginUninstallResult, PluginManagementError> {
        let info = self
            .service
            .registry()
            .resolve_plugin_selector(selector)
            .await
            .map_err(registry_error_to_management)?;

        let install_path = info.source.as_ref().and_then(|s| s.install_path.clone());

        let is_builtin = matches!(
            info.source.as_ref().map(|s| s.installed_by),
            Some(crate::plugin::registry::PluginInstallKind::Builtin)
        );

        // Step 1: validate the install target if we have one and the plugin
        // is not a builtin. A failed validation aborts uninstall without
        // touching the registry.
        let mut removed_files = false;
        let mut warning: Option<String> = None;

        if let Some(target) = install_path.as_ref() {
            if is_builtin {
                // Builtins should never have an install_path recorded, but
                // guard against drift defensively.
                tracing::warn!(
                    plugin = info.id,
                    path = %target.display(),
                    "builtin plugin has install_path recorded; skipping filesystem removal"
                );
            } else if !target.exists() {
                // No files to remove.
            } else {
                let policy = PluginInstallPolicy::default();
                if let Err(e) = crate::plugin::install::validate_uninstall_target(target, &policy) {
                    return Err(PluginManagementError::Install(e.to_string()));
                }
            }
        }

        // Step 2: unregister from the live registry.
        let removed = self
            .service
            .registry()
            .unregister(&info.id)
            .await
            .ok_or_else(|| PluginManagementError::NotFound(info.id.clone()))?;
        let unregistered = true;

        // Step 3: attempt filesystem removal if we have a validated target
        // and the plugin is not a builtin.
        if !is_builtin {
            if let Some(target) = install_path.as_ref() {
                if target.exists() {
                    if let Err(e) = tokio::fs::remove_dir_all(target).await {
                        tracing::warn!(
                            plugin = info.id,
                            error = %e,
                            "failed to remove plugin directory after unregister"
                        );
                        warning = Some(e.to_string());
                    } else {
                        removed_files = true;
                    }
                }
            }
        }

        let view = PluginManagementView::from_info(&removed);
        Ok(PluginUninstallResult {
            view,
            unregistered,
            removed_files,
            install_path,
            warning,
        })
    }

    /// Run diagnostic checks on a plugin.
    pub async fn doctor(
        &self,
        selector: &str,
    ) -> Result<PluginDoctorReport, PluginManagementError> {
        let info = self
            .service
            .registry()
            .resolve_plugin_selector(selector)
            .await
            .map_err(registry_error_to_management)?;
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

        // Check: API version compatibility
        let api_compat_ok = info.manifest.api_version == SUPPORTED_API_VERSION;
        checks.push(PluginDoctorCheck {
            name: "api_version".to_string(),
            passed: api_compat_ok,
            message: if api_compat_ok {
                format!("API version {} is supported", info.manifest.api_version)
            } else {
                format!(
                    "API version {} differs from supported {}",
                    info.manifest.api_version, SUPPORTED_API_VERSION
                )
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

        // Check: runtime availability (process executable or wasm feature)
        let runtime_avail_check = check_runtime_availability(&info.manifest);
        checks.push(runtime_avail_check);

        // Check: plugin enable state
        checks.push(PluginDoctorCheck {
            name: "enable_state".to_string(),
            passed: true, // informational
            message: if info.enabled {
                "Plugin is enabled".to_string()
            } else {
                "Plugin is disabled".to_string()
            },
        });

        // Check: duplicate capability conflicts
        let dup_check = check_duplicate_capabilities(&self.service.registry().clone(), &info).await;
        checks.push(dup_check);

        // Check: permission/trust warnings (informational)
        let perms = &info.manifest.permissions;
        let perms_default = !perms.network
            && matches!(
                perms.filesystem,
                crate::plugin::manifest::FilesystemPermission::None
            )
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

        // Check: declared output surfaces
        let output_count = count_output_surfaces(&info.manifest);
        checks.push(PluginDoctorCheck {
            name: "output_surfaces".to_string(),
            passed: true, // informational
            message: format!("Plugin declares {} output surface(s)", output_count),
        });

        // Check: stale/inaccessible install path (only if source_path is set)
        if let Some(ref path) = view.source_path {
            let path_exists = std::path::Path::new(path).exists();
            checks.push(PluginDoctorCheck {
                name: "install_path".to_string(),
                passed: path_exists,
                message: if path_exists {
                    format!("Install path '{}' is accessible", path)
                } else {
                    format!("Install path '{}' is stale or inaccessible", path)
                },
            });
        } else {
            checks.push(PluginDoctorCheck {
                name: "install_path".to_string(),
                passed: true, // unknown, not a failure
                message: "Install path not tracked".to_string(),
            });
        }

        // Check: no error-level diagnostics
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

        // Check: registry index consistency
        let registry_infos = self.service.registry().list().await;
        let in_registry = registry_infos.iter().any(|i| i.id == info.id);
        checks.push(PluginDoctorCheck {
            name: "registry_consistency".to_string(),
            passed: in_registry,
            message: if in_registry {
                format!(
                    "Plugin appears in registry index ({})",
                    registry_infos.len()
                )
            } else {
                "Plugin missing from registry index".to_string()
            },
        });

        // --- Policy diagnostics (informational) ---
        if let Some(policy) = self.service.policy() {
            // Check: process lifecycle hooks
            if matches!(&info.manifest.runtime, PluginRuntimeSpec::Process { .. }) {
                let process_hooks_ok = policy.lifecycle.allow_process_lifecycle_hooks;
                checks.push(PluginDoctorCheck {
                    name: "policy_process_hooks".to_string(),
                    passed: process_hooks_ok,
                    message: if process_hooks_ok {
                        "Process lifecycle hooks are allowed by policy".to_string()
                    } else {
                        "Process lifecycle hooks denied by policy (default)".to_string()
                    },
                });
            }

            // Check: undeclared capabilities
            if policy.runtime.deny_undeclared_capabilities {
                checks.push(PluginDoctorCheck {
                    name: "policy_undeclared_capabilities".to_string(),
                    passed: true, // informational
                    message: "Undeclared capabilities are denied by policy".to_string(),
                });
            }

            // Check: secret access
            if policy.permissions.deny_secrets_by_default {
                let declared_secrets = info.manifest.permissions.secrets.len();
                checks.push(PluginDoctorCheck {
                    name: "policy_secret_access".to_string(),
                    passed: true, // informational
                    message: if declared_secrets > 0 {
                        format!(
                            "Secret access denied by default; {} secret(s) declared",
                            declared_secrets
                        )
                    } else {
                        "Secret access denied by default; no secrets declared".to_string()
                    },
                });
            }

            // Check: env passthrough
            if policy.permissions.deny_env_passthrough_by_default
                && matches!(&info.manifest.runtime, PluginRuntimeSpec::Process { .. })
            {
                checks.push(PluginDoctorCheck {
                    name: "policy_env_passthrough".to_string(),
                    passed: true, // informational
                    message: "Env passthrough denied by default for process plugins".to_string(),
                });
            }

            // Check: high-risk permissions
            let high_risk = info.manifest.permissions.network
                || info.manifest.permissions.tool_interception
                || info.manifest.permissions.session_messages;
            if high_risk {
                let mut risks = Vec::new();
                if info.manifest.permissions.network {
                    risks.push("network");
                }
                if info.manifest.permissions.tool_interception {
                    risks.push("tool_interception");
                }
                if info.manifest.permissions.session_messages {
                    risks.push("session_messages");
                }
                checks.push(PluginDoctorCheck {
                    name: "policy_high_risk_grants".to_string(),
                    passed: false, // flagged for user review
                    message: format!("High-risk permission(s) declared: {}", risks.join(", ")),
                });
            }

            // Check: UI effect restrictions
            if !policy.ui.allow_panel {
                checks.push(PluginDoctorCheck {
                    name: "policy_ui_panel_denied".to_string(),
                    passed: true, // informational
                    message: "Panel UI effects denied by policy".to_string(),
                });
            }
            if !policy.ui.allow_status {
                checks.push(PluginDoctorCheck {
                    name: "policy_ui_status_denied".to_string(),
                    passed: true, // informational
                    message: "Status UI effects denied by policy".to_string(),
                });
            }

            // Check: auth hook trust requirement
            if policy.permissions.require_high_trust_for_auth_hooks
                && !matches!(info.trust, PluginTrustClass::Builtin)
            {
                let has_auth_hook = info
                    .manifest
                    .hooks_capabilities()
                    .any(|h| h.hook_type == "auth" || h.hook_type == "provider")
                    || info
                        .manifest
                        .hooks
                        .iter()
                        .any(|h| h.hook_type == "auth" || h.hook_type == "provider");
                if has_auth_hook {
                    checks.push(PluginDoctorCheck {
                        name: "policy_auth_hook_trust".to_string(),
                        passed: false,
                        message:
                            "Plugin declares auth/provider hook but is not Builtin trust class"
                                .to_string(),
                    });
                }
            }
        }

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

    /// Run diagnostic checks on all registered plugins.
    pub async fn doctor_all(&self) -> Vec<PluginDoctorReport> {
        let infos = self.service.registry().list().await;
        let mut reports = Vec::with_capacity(infos.len());
        for info in &infos {
            if let Ok(report) = self.doctor(&info.id).await {
                reports.push(report);
            }
        }
        reports
    }
}

/// Supported plugin API version. Plugins with a different value get a
/// doctor warning but are still allowed to operate.
pub const SUPPORTED_API_VERSION: u32 = 1;

/// Convert a `PluginRegistryError` into a `PluginManagementError`.
///
/// NotFound errors carry ambiguous/no-match info from `resolve_plugin_selector`
/// that should be surfaced as `Ambiguous` or `NoMatch` when possible. Since
/// `resolve_plugin_selector` only emits `NotFound`, we keep the conversion
/// straightforward here and rely on the structured error messages.
fn registry_error_to_management(e: PluginRegistryError) -> PluginManagementError {
    match e {
        PluginRegistryError::NotFound(msg) => {
            if msg.contains("ambiguous") {
                PluginManagementError::Ambiguous(msg, Vec::new())
            } else {
                PluginManagementError::NoMatch(msg)
            }
        }
        other => PluginManagementError::Registry(other),
    }
}

/// Check whether the plugin's runtime is available.
///
/// - Builtin: always available (first-party code)
/// - Process: requires the executable to be present on PATH or absolute
/// - Wasm: requires the `plugins` feature to be enabled at compile time
fn check_runtime_availability(manifest: &PluginManifest) -> PluginDoctorCheck {
    match &manifest.runtime {
        PluginRuntimeSpec::Builtin { .. } => PluginDoctorCheck {
            name: "runtime_available".to_string(),
            passed: true,
            message: "Builtin runtime is always available".to_string(),
        },
        PluginRuntimeSpec::Process { command, .. } => {
            let exists = command_exists(command);
            PluginDoctorCheck {
                name: "runtime_available".to_string(),
                passed: exists,
                message: if exists {
                    format!("Process executable '{}' is available", command)
                } else {
                    format!("Process executable '{}' not found on PATH", command)
                },
            }
        }
        PluginRuntimeSpec::Wasm { .. } => {
            let wasm_enabled = cfg!(feature = "plugins");
            PluginDoctorCheck {
                name: "runtime_available".to_string(),
                passed: wasm_enabled,
                message: if wasm_enabled {
                    "WASM runtime is enabled (feature 'plugins' on)".to_string()
                } else {
                    "WASM runtime disabled — rebuild with --features plugins".to_string()
                },
            }
        }
    }
}

/// Check if a process command exists on PATH or as an absolute path.
fn command_exists(command: &str) -> bool {
    use std::path::Path;
    let p = Path::new(command);
    if p.is_absolute() || command.contains('/') {
        return p.exists();
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            if dir.join(command).exists() {
                return true;
            }
        }
    }
    false
}

/// Check whether this plugin's capabilities collide with already-registered ones.
async fn check_duplicate_capabilities(
    _registry: &Arc<PluginRegistry>,
    info: &PluginInfo,
) -> PluginDoctorCheck {
    // We approximate duplicate detection by counting command names declared
    // by this plugin. The registry itself rejects duplicates on register/enable,
    // so any duplicates here indicate either stale info or a pre-existing
    // conflict that the registry has already resolved.
    let cmd_names: Vec<&str> = info.manifest.commands().map(|c| c.name.as_str()).collect();
    let dup_count = cmd_names.len()
        - cmd_names
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
    PluginDoctorCheck {
        name: "no_duplicate_capabilities".to_string(),
        passed: dup_count == 0,
        message: if dup_count == 0 {
            format!("{} unique command name(s)", cmd_names.len())
        } else {
            format!("{} duplicate command name(s) within plugin", dup_count)
        },
    }
}

/// Count the total number of output surfaces declared by a manifest.
fn count_output_surfaces(manifest: &PluginManifest) -> usize {
    // Output surfaces are capabilities that produce UI output (panels, status widgets)
    manifest.panels().count() + manifest.status_widgets().count()
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
            source: None,
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
            source: None,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn enable_sets_state() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("test:1", "test", false))
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let view = manager.enable("test:1").await.unwrap();
        assert!(view.enabled);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn disable_sets_state() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("test:1", "test", true))
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let view = manager.disable("test:1").await.unwrap();
        assert!(!view.enabled);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn registry_rejects_duplicate_command() {
        // The registry enforces global command-name uniqueness at register
        // time. Two plugins with the same command name cannot both register;
        // the second registration fails deterministically.
        let registry = PluginRegistry::new();
        let cap = PluginCapability::Command(PluginCommandSpec {
            name: "deploy".into(),
            aliases: Vec::new(),
            description: None,
            handler: None,
            output: Vec::new(),
        });
        let info_a = PluginInfo {
            id: "a:1".into(),
            manifest: PluginManifest {
                name: "alpha".into(),
                version: "1.0.0".into(),
                api_version: SUPPORTED_API_VERSION,
                runtime: PluginRuntimeSpec::Builtin {
                    handler: "h".into(),
                },
                capabilities: vec![cap.clone()],
                ..Default::default()
            },
            enabled: true,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
            source: None,
        };
        let info_b = PluginInfo {
            id: "b:1".into(),
            manifest: PluginManifest {
                name: "beta".into(),
                version: "1.0.0".into(),
                api_version: SUPPORTED_API_VERSION,
                runtime: PluginRuntimeSpec::Builtin {
                    handler: "h".into(),
                },
                capabilities: vec![cap],
                ..Default::default()
            },
            enabled: false,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
            source: None,
        };
        registry.register(info_a).await.unwrap();
        let result = registry.register(info_b).await;
        assert!(matches!(
            result,
            Err(PluginRegistryError::DuplicateCommand(_, _))
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remove_unregisters_plugin() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("test:1", "test", true))
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let _view = manager.remove("test:1").await.unwrap();
        // After remove, looking up the same id should fail
        let lookup = manager.info("test:1").await;
        assert!(lookup.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_via_registry() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("my-plugin:1", "my-plugin", true))
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let view = manager.info("my-plugin:1").await.unwrap();
        assert_eq!(view.id, "my-plugin:1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn doctor_reports_process_executable_missing() {
        let registry = PluginRegistry::new();
        let manifest = PluginManifest {
            name: "proc-test".into(),
            version: "1.0.0".into(),
            api_version: SUPPORTED_API_VERSION,
            runtime: PluginRuntimeSpec::Process {
                command: "definitely-nonexistent-cmd-xyz".into(),
                args: Vec::new(),
                timeout_ms: None,
            },
            capabilities: vec![PluginCapability::Command(PluginCommandSpec {
                name: "go".into(),
                aliases: Vec::new(),
                description: None,
                handler: None,
                output: Vec::new(),
            })],
            ..Default::default()
        };
        registry
            .register(PluginInfo {
                id: "proc:1".into(),
                manifest,
                enabled: true,
                trust: PluginTrustClass::LocalProcess,
                diagnostics: Vec::new(),
                source: None,
            })
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let report = manager.doctor("proc:1").await.unwrap();
        let rt_check = report
            .checks
            .iter()
            .find(|c| c.name == "runtime_available")
            .unwrap();
        assert!(!rt_check.passed);
        assert!(rt_check.message.contains("not found"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn doctor_reports_wasm_disabled_when_feature_off() {
        let registry = PluginRegistry::new();
        let manifest = PluginManifest {
            name: "wasm-test".into(),
            version: "1.0.0".into(),
            api_version: SUPPORTED_API_VERSION,
            runtime: PluginRuntimeSpec::Wasm {
                module: "plugin.wasm".into(),
                timeout_ms: None,
                memory_max_mb: None,
                fuel_per_call: None,
            },
            ..Default::default()
        };
        registry
            .register(PluginInfo {
                id: "wasm:1".into(),
                manifest,
                enabled: true,
                trust: PluginTrustClass::SandboxedWasm,
                diagnostics: Vec::new(),
                source: None,
            })
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let report = manager.doctor("wasm:1").await.unwrap();
        let rt_check = report
            .checks
            .iter()
            .find(|c| c.name == "runtime_available")
            .unwrap();
        if !cfg!(feature = "plugins") {
            assert!(!rt_check.passed);
            assert!(rt_check.message.contains("disabled"));
        } else {
            assert!(rt_check.passed);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn doctor_reports_api_version_mismatch() {
        let registry = PluginRegistry::new();
        let mut manifest = make_manifest("old", Vec::new());
        manifest.api_version = SUPPORTED_API_VERSION + 99;
        registry
            .register(PluginInfo {
                id: "old:1".into(),
                manifest,
                enabled: true,
                trust: PluginTrustClass::Builtin,
                diagnostics: Vec::new(),
                source: None,
            })
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let report = manager.doctor("old:1").await.unwrap();
        let api_check = report
            .checks
            .iter()
            .find(|c| c.name == "api_version")
            .unwrap();
        assert!(!api_check.passed);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn doctor_builtin_runtime_is_always_available() {
        let registry = PluginRegistry::new();
        let manifest = PluginManifest {
            name: "builtin-test".into(),
            version: "1.0.0".into(),
            api_version: SUPPORTED_API_VERSION,
            runtime: PluginRuntimeSpec::Builtin {
                handler: "h".into(),
            },
            ..Default::default()
        };
        registry
            .register(PluginInfo {
                id: "builtin:1".into(),
                manifest,
                enabled: true,
                trust: PluginTrustClass::Builtin,
                diagnostics: Vec::new(),
                source: None,
            })
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let report = manager.doctor("builtin:1").await.unwrap();
        let rt_check = report
            .checks
            .iter()
            .find(|c| c.name == "runtime_available")
            .unwrap();
        assert!(rt_check.passed);
        assert!(rt_check.message.contains("always available"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn doctor_includes_registry_consistency_check() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("test:1", "test", true))
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let report = manager.doctor("test:1").await.unwrap();
        let consistency_check = report
            .checks
            .iter()
            .find(|c| c.name == "registry_consistency")
            .unwrap();
        assert!(consistency_check.passed);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn doctor_includes_enable_state_check() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("test:1", "test", false))
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let report = manager.doctor("test:1").await.unwrap();
        let enable_check = report
            .checks
            .iter()
            .find(|c| c.name == "enable_state")
            .unwrap();
        assert!(enable_check.message.contains("disabled"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn doctor_includes_output_surfaces_check() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("test:1", "test", true))
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let report = manager.doctor("test:1").await.unwrap();
        let output_check = report
            .checks
            .iter()
            .find(|c| c.name == "output_surfaces")
            .unwrap();
        assert!(output_check.passed);
        assert!(output_check.message.contains("output surface"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_exact_id() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("my-plugin:1", "my-plugin", true))
            .await
            .unwrap();

        let result = registry
            .resolve_plugin_selector("my-plugin:1")
            .await
            .unwrap();
        assert_eq!(result.id, "my-plugin:1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_exact_name() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("plugin:my-plugin", "my-plugin", true))
            .await
            .unwrap();

        let result = registry.resolve_plugin_selector("my-plugin").await.unwrap();
        assert_eq!(result.manifest.name, "my-plugin");
    }

    #[tokio::test(flavor = "current_thread")]
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

        let result = registry
            .resolve_plugin_selector("my-plugin-a")
            .await
            .unwrap();
        assert_eq!(result.id, "my-plugin-alpha:1");
    }

    #[tokio::test(flavor = "current_thread")]
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

        let result = registry.resolve_plugin_selector("alpha").await.unwrap();
        assert_eq!(result.manifest.name, "alpha-plugin");
    }

    #[tokio::test(flavor = "current_thread")]
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

        let result = registry.resolve_plugin_selector("my-plugin-").await;
        assert!(result.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn no_match_returns_error() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("a:1", "alpha", true))
            .await
            .unwrap();

        let result = registry.resolve_plugin_selector("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_case_insensitive_id_prefix() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("MyPlugin:1", "test", true))
            .await
            .unwrap();

        let result = registry.resolve_plugin_selector("myplugin").await.unwrap();
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
            source: None,
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

    #[tokio::test(flavor = "current_thread")]
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
            api_version: SUPPORTED_API_VERSION,
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
                source: None,
            })
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let report = manager.doctor("test:1").await.unwrap();
        assert_eq!(report.plugin_id, "test:1");
        assert_eq!(report.plugin_name, "test");
        assert!(!report.checks.is_empty());
        // The registered builtin should pass the runtime_available check.
        let rt = report
            .checks
            .iter()
            .find(|c| c.name == "runtime_available")
            .unwrap();
        assert!(rt.passed);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn doctor_report_detects_empty_manifest() {
        let registry = PluginRegistry::new();
        registry
            .register(PluginInfo {
                id: "bad:1".into(),
                manifest: PluginManifest::default(),
                enabled: true,
                trust: PluginTrustClass::Builtin,
                diagnostics: Vec::new(),
                source: None,
            })
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let report = manager.doctor("bad:1").await.unwrap();
        let manifest_check = report
            .checks
            .iter()
            .find(|c| c.name == "manifest_valid")
            .unwrap();
        assert!(!manifest_check.passed);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_returns_all_plugins() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("a:1", "alpha", true))
            .await
            .unwrap();
        registry
            .register(make_info("b:1", "beta", false))
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let views = manager.list().await;
        assert_eq!(views.len(), 2);
    }

    #[test]
    fn command_exists_finds_absolute_path() {
        // /bin/sh should exist on any Unix system used for testing
        assert!(command_exists("/bin/sh"));
    }

    #[test]
    fn command_exists_rejects_missing() {
        assert!(!command_exists("definitely-not-a-real-binary-zzz"));
    }

    #[test]
    fn view_from_info_populates_last_error_from_diagnostics() {
        let mut manifest = make_manifest("test", Vec::new());
        manifest.api_version = SUPPORTED_API_VERSION;
        let info = PluginInfo {
            id: "test:1".into(),
            manifest,
            enabled: true,
            trust: PluginTrustClass::Builtin,
            diagnostics: vec![
                crate::plugin::PluginDiagnostic {
                    level: crate::plugin::PluginDiagnosticLevel::Warning,
                    message: "just a warning".to_string(),
                },
                crate::plugin::PluginDiagnostic {
                    level: crate::plugin::PluginDiagnosticLevel::Error,
                    message: "something broke".to_string(),
                },
            ],
            source: None,
        };
        let view = PluginManagementView::from_info(&info);
        assert_eq!(view.last_error.as_deref(), Some("something broke"));
    }

    #[test]
    fn view_from_info_last_error_none_when_no_errors() {
        let info = make_info("test:1", "test", true);
        let view = PluginManagementView::from_info(&info);
        assert!(view.last_error.is_none());
    }

    #[test]
    fn view_from_marketplace_last_error_is_none() {
        let plugin = MarketplacePlugin {
            id: "test".into(),
            name: "Test".into(),
            version: "1.0.0".into(),
            description: None,
            author: None,
            homepage: None,
            tier: crate::plugin::marketplace::PluginTier::Personal,
            hooks: Vec::new(),
        };
        let view = PluginManagementView::from_marketplace(&plugin, true);
        assert!(view.last_error.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn doctor_includes_policy_checks_when_policy_set() {
        use crate::plugin::policy::PluginPolicy;
        let registry = PluginRegistry::new();
        let manifest = PluginManifest {
            name: "proc-test".into(),
            version: "1.0.0".into(),
            api_version: SUPPORTED_API_VERSION,
            runtime: PluginRuntimeSpec::Process {
                command: "echo".into(),
                args: vec![],
                timeout_ms: None,
            },
            ..Default::default()
        };
        registry
            .register(PluginInfo {
                id: "proc:1".into(),
                manifest,
                enabled: true,
                trust: PluginTrustClass::LocalProcess,
                diagnostics: Vec::new(),
                source: None,
            })
            .await
            .unwrap();
        let service = Arc::new(
            PluginService::new(Arc::new(registry)).with_policy(Arc::new(PluginPolicy::default())),
        );
        let manager = PluginManager::new(service);

        let report = manager.doctor("proc:1").await.unwrap();
        let policy_check = report
            .checks
            .iter()
            .find(|c| c.name == "policy_process_hooks");
        assert!(policy_check.is_some());
        let check = policy_check.unwrap();
        assert!(!check.passed); // process hooks denied by default
        assert!(check.message.contains("denied by policy"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn doctor_includes_env_passthrough_check_for_process_plugins() {
        use crate::plugin::policy::PluginPolicy;
        let registry = PluginRegistry::new();
        let manifest = PluginManifest {
            name: "proc-test".into(),
            version: "1.0.0".into(),
            api_version: SUPPORTED_API_VERSION,
            runtime: PluginRuntimeSpec::Process {
                command: "echo".into(),
                args: vec![],
                timeout_ms: None,
            },
            ..Default::default()
        };
        registry
            .register(PluginInfo {
                id: "proc:1".into(),
                manifest,
                enabled: true,
                trust: PluginTrustClass::LocalProcess,
                diagnostics: Vec::new(),
                source: None,
            })
            .await
            .unwrap();
        let service = Arc::new(
            PluginService::new(Arc::new(registry)).with_policy(Arc::new(PluginPolicy::default())),
        );
        let manager = PluginManager::new(service);

        let report = manager.doctor("proc:1").await.unwrap();
        let env_check = report
            .checks
            .iter()
            .find(|c| c.name == "policy_env_passthrough");
        assert!(env_check.is_some());
        assert!(env_check.unwrap().message.contains("passthrough"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn doctor_no_policy_checks_when_no_policy() {
        let registry = PluginRegistry::new();
        registry
            .register(make_info("test:1", "test", true))
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let report = manager.doctor("test:1").await.unwrap();
        let policy_checks: Vec<_> = report
            .checks
            .iter()
            .filter(|c| c.name.starts_with("policy_"))
            .collect();
        assert!(policy_checks.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn doctor_reports_high_risk_permissions() {
        use crate::plugin::policy::PluginPolicy;
        let registry = PluginRegistry::new();
        let mut manifest = make_manifest("risky", Vec::new());
        manifest.permissions.network = true;
        manifest.permissions.tool_interception = true;
        manifest.api_version = SUPPORTED_API_VERSION;
        registry
            .register(PluginInfo {
                id: "risky:1".into(),
                manifest,
                enabled: true,
                trust: PluginTrustClass::LocalProcess,
                diagnostics: Vec::new(),
                source: None,
            })
            .await
            .unwrap();
        let service = Arc::new(
            PluginService::new(Arc::new(registry)).with_policy(Arc::new(PluginPolicy::default())),
        );
        let manager = PluginManager::new(service);

        let report = manager.doctor("risky:1").await.unwrap();
        let risk_check = report
            .checks
            .iter()
            .find(|c| c.name == "policy_high_risk_grants");
        assert!(risk_check.is_some());
        let check = risk_check.unwrap();
        assert!(!check.passed);
        assert!(check.message.contains("network"));
        assert!(check.message.contains("tool_interception"));
    }

    // ---- Plugin source metadata (Workstream A) ----

    #[test]
    fn view_from_info_populates_source_path_from_local_path_metadata() {
        use crate::plugin::registry::{PluginInstallKind, PluginSourceMetadata};
        let install = std::path::PathBuf::from("/data/codegg/plugins/my-plugin");
        let original = std::path::PathBuf::from("/Users/me/dev/my-plugin");
        let info = PluginInfo {
            id: "plugin:my-plugin".into(),
            manifest: make_manifest("my-plugin", Vec::new()),
            enabled: true,
            trust: PluginTrustClass::TrustedLocal,
            diagnostics: Vec::new(),
            source: Some(PluginSourceMetadata {
                install_path: Some(install.clone()),
                original_source_path: Some(original),
                installed_by: PluginInstallKind::LocalPath,
            }),
        };
        let view = PluginManagementView::from_info(&info);
        assert_eq!(view.source_path.as_deref(), Some(install.to_str().unwrap()));
    }

    #[test]
    fn view_from_info_builtin_has_no_source_path() {
        use crate::plugin::registry::{PluginInstallKind, PluginSourceMetadata};
        let info = PluginInfo {
            id: "builtin:codex".into(),
            manifest: make_manifest("codex", Vec::new()),
            enabled: true,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
            source: Some(PluginSourceMetadata {
                install_path: None,
                original_source_path: None,
                installed_by: PluginInstallKind::Builtin,
            }),
        };
        let view = PluginManagementView::from_info(&info);
        assert!(view.source_path.is_none());
    }

    #[test]
    fn view_from_info_falls_back_to_original_when_install_missing() {
        use crate::plugin::registry::{PluginInstallKind, PluginSourceMetadata};
        let original = std::path::PathBuf::from("/legacy/path/my-plugin");
        let info = PluginInfo {
            id: "plugin:my-plugin".into(),
            manifest: make_manifest("my-plugin", Vec::new()),
            enabled: true,
            trust: PluginTrustClass::TrustedLocal,
            diagnostics: Vec::new(),
            source: Some(PluginSourceMetadata {
                install_path: None,
                original_source_path: Some(original.clone()),
                installed_by: PluginInstallKind::RegistryLoaded,
            }),
        };
        let view = PluginManagementView::from_info(&info);
        assert_eq!(
            view.source_path.as_deref(),
            Some(original.to_str().unwrap())
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uninstall_source_less_plugin_reports_unregister_only() {
        use crate::plugin::registry::{PluginInstallKind, PluginSourceMetadata};
        let registry = PluginRegistry::new();
        registry
            .register(PluginInfo {
                id: "plugin:sourceless".into(),
                manifest: make_manifest("sourceless", Vec::new()),
                enabled: true,
                trust: PluginTrustClass::TrustedLocal,
                diagnostics: Vec::new(),
                source: Some(PluginSourceMetadata {
                    install_path: None,
                    original_source_path: None,
                    installed_by: PluginInstallKind::Unknown,
                }),
            })
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let result = manager.uninstall("plugin:sourceless").await.unwrap();
        assert!(result.unregistered);
        assert!(!result.removed_files);
        assert!(result.install_path.is_none());
        assert!(result.warning.is_none());

        // And the plugin is gone from the registry.
        let lookup = manager.info("plugin:sourceless").await;
        assert!(lookup.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uninstall_builtin_does_not_attempt_filesystem_removal() {
        use crate::plugin::registry::{PluginInstallKind, PluginSourceMetadata};
        let registry = PluginRegistry::new();
        registry
            .register(PluginInfo {
                id: "builtin:codex".into(),
                manifest: make_manifest("codex", Vec::new()),
                enabled: true,
                trust: PluginTrustClass::Builtin,
                diagnostics: Vec::new(),
                source: Some(PluginSourceMetadata {
                    install_path: None,
                    original_source_path: None,
                    installed_by: PluginInstallKind::Builtin,
                }),
            })
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let result = manager.uninstall("builtin:codex").await.unwrap();
        assert!(result.unregistered);
        assert!(!result.removed_files);
        assert!(result.install_path.is_none());
        assert!(result.warning.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uninstall_with_install_path_outside_plugins_dir_is_rejected() {
        use crate::plugin::registry::{PluginInstallKind, PluginSourceMetadata};
        let registry = PluginRegistry::new();
        // Use a path that does exist and is not under plugins_dir().
        let outside = std::env::temp_dir();
        registry
            .register(PluginInfo {
                id: "plugin:bad".into(),
                manifest: make_manifest("bad", Vec::new()),
                enabled: true,
                trust: PluginTrustClass::TrustedLocal,
                diagnostics: Vec::new(),
                source: Some(PluginSourceMetadata {
                    install_path: Some(outside),
                    original_source_path: None,
                    installed_by: PluginInstallKind::LocalPath,
                }),
            })
            .await
            .unwrap();
        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let manager = PluginManager::new(service);

        let result = manager.uninstall("plugin:bad").await;
        assert!(
            matches!(result, Err(PluginManagementError::Install(_))),
            "expected Install error for outside plugins_dir, got: {result:?}"
        );

        // Plugin should still be registered because validation failed before unregister.
        let lookup = manager.info("plugin:bad").await;
        assert!(lookup.is_ok());
    }
}
