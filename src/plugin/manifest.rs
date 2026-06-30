use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Trust class for a plugin, inferred from its runtime kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginTrustClass {
    /// Built-in native Rust plugin.
    Builtin,
    /// Process-backed plugin executing a local command.
    LocalProcess,
    /// WASM plugin running in a sandboxed Wasmtime environment.
    SandboxedWasm,
    /// Reserved for future embedded/PyO3 runtimes.
    TrustedLocal,
}

impl PluginTrustClass {
    /// Infer trust class from a runtime kind string.
    pub fn from_runtime_kind(kind: &str) -> Self {
        match kind {
            "builtin" => PluginTrustClass::Builtin,
            "process" => PluginTrustClass::LocalProcess,
            "wasm" => PluginTrustClass::SandboxedWasm,
            _ => PluginTrustClass::TrustedLocal,
        }
    }
}

/// Source/installation metadata for a plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PluginSource {
    /// Path on disk or URL from which the plugin was installed.
    pub path: Option<String>,
    /// The original source type (e.g., "path", "url", "builtin").
    #[serde(default)]
    pub source_type: String,
}

/// Diagnostic message from a plugin.
///
/// Re-exported from `codegg_protocol::plugin` for canonical type identity.
pub use crate::protocol::plugin::{PluginDiagnostic, PluginDiagnosticLevel};

/// Runtime specification for a plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginRuntimeSpec {
    Builtin {
        handler: String,
    },
    Process {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        timeout_ms: Option<u64>,
    },
    Wasm {
        module: String,
        timeout_ms: Option<u64>,
        memory_max_mb: Option<u64>,
        fuel_per_call: Option<u64>,
    },
}

impl Default for PluginRuntimeSpec {
    fn default() -> Self {
        PluginRuntimeSpec::Builtin {
            handler: String::new(),
        }
    }
}

/// A capability contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginCapability {
    Command(PluginCommandSpec),
    Hook(PluginHookSpec),
    Panel(PluginPanelContribution),
    StatusWidget(PluginStatusContribution),
    EventSubscription(PluginEventSubscriptionSpec),
}

/// Declares a slash command contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginCommandSpec {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub handler: Option<String>,
    #[serde(default)]
    pub output: Vec<PluginOutputSurface>,
}

/// Declares a hook contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginHookSpec {
    pub hook_type: String,
    #[serde(default)]
    pub priority: i32,
    pub handler: Option<String>,
}

/// Declares a TUI panel contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginPanelContribution {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub placement: String,
    pub handler: Option<String>,
}

/// Declares a TUI status widget contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginStatusContribution {
    pub id: String,
    pub label: Option<String>,
    #[serde(default)]
    pub placement: String,
    pub refresh_ms: Option<u64>,
    pub handler: Option<String>,
}

/// Declares an event subscription contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginEventSubscriptionSpec {
    pub event_type: String,
    pub handler: Option<String>,
}

/// Output surface for a plugin command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PluginOutputSurface {
    Chat,
    Toast,
    Dialog,
    Panel,
    Status,
}

/// Permission set declared by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PluginPermissionSet {
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub filesystem: FilesystemPermission,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default)]
    pub session_messages: bool,
    #[serde(default)]
    pub tool_interception: bool,
}

/// Filesystem permission level.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemPermission {
    #[default]
    None,
    ProjectRead,
    ProjectWrite,
    Full,
}

/// Canonical plugin manifest (Phase 5).
///
/// This is the canonical internal representation that maps to the protocol
/// `PluginManifestDto`. It supports both legacy hook-only manifests and the
/// new capability-based manifests with runtime declarations.
///
/// # TOML Examples
///
/// **Process-backed plugin:**
/// ```toml
/// name = "quota"
/// version = "0.1.0"
/// api_version = 1
///
/// [runtime]
/// kind = "process"
/// command = "python3"
/// args = ["quota.py"]
/// timeout_ms = 5000
///
/// [[capabilities]]
/// type = "command"
/// name = "quota"
/// description = "Show provider quota"
/// output = ["chat", "dialog"]
///
/// [permissions]
/// network = false
/// filesystem = "none"
/// ```
///
/// **Legacy hook-only manifest:**
/// ```toml
/// name = "my-plugin"
/// version = "1.0.0"
///
/// [[hooks]]
/// type = "tool.execute.before"
/// priority = 0
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    #[serde(default = "default_api_version")]
    pub api_version: u32,
    #[serde(default)]
    pub runtime: PluginRuntimeSpec,
    #[serde(default)]
    pub capabilities: Vec<PluginCapability>,
    #[serde(default)]
    pub permissions: PluginPermissionSet,

    // Legacy fields (kept for backwards compatibility)
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,

    // Legacy hooks (converted to capabilities during parsing)
    #[serde(default)]
    pub hooks: Vec<LegacyHookSpec>,

    // Legacy config
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

fn default_api_version() -> u32 {
    1
}

/// Legacy hook specification from old-style manifests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LegacyHookSpec {
    #[serde(rename = "type")]
    pub hook_type: String,
    pub priority: Option<i32>,
}

impl Default for PluginManifest {
    fn default() -> Self {
        Self {
            name: String::new(),
            version: String::new(),
            api_version: 1,
            runtime: PluginRuntimeSpec::Builtin {
                handler: String::new(),
            },
            capabilities: Vec::new(),
            permissions: PluginPermissionSet::default(),
            description: None,
            author: None,
            homepage: None,
            license: None,
            hooks: Vec::new(),
            config: HashMap::new(),
        }
    }
}

impl PluginManifest {
    /// Parse a TOML manifest string into a canonical `PluginManifest`.
    ///
    /// Supports both legacy `[[hooks]]` format and new `[[capabilities]]` format.
    /// Legacy hooks are automatically converted to `PluginCapability::Hook` entries.
    pub fn from_toml_str(s: &str) -> Result<Self, String> {
        let mut manifest: Self =
            toml::from_str(s).map_err(|e| format!("manifest parse error: {}", e))?;

        // If no capabilities are declared but legacy hooks exist, convert them
        if manifest.capabilities.is_empty() && !manifest.hooks.is_empty() {
            manifest.capabilities = manifest
                .hooks
                .iter()
                .map(|h| {
                    PluginCapability::Hook(PluginHookSpec {
                        hook_type: h.hook_type.clone(),
                        priority: h.priority.unwrap_or(0),
                        handler: None,
                    })
                })
                .collect();
        }

        // If no runtime is declared, infer from legacy hooks presence
        if manifest.runtime
            == (PluginRuntimeSpec::Builtin {
                handler: String::new(),
            })
            && !manifest.hooks.is_empty()
        {
            // Legacy hook-only manifest — treat as builtin with no specific handler
            manifest.runtime = PluginRuntimeSpec::Builtin {
                handler: manifest.name.clone(),
            };
        }

        Ok(manifest)
    }

    /// Convert a legacy `LegacyManifest` into this canonical form.
    pub fn from_legacy(legacy: &LegacyManifest) -> Self {
        let capabilities: Vec<PluginCapability> = legacy
            .hooks
            .iter()
            .map(|h| {
                PluginCapability::Hook(PluginHookSpec {
                    hook_type: h.hook_type.clone(),
                    priority: h.priority.unwrap_or(0),
                    handler: None,
                })
            })
            .collect();

        let runtime = PluginRuntimeSpec::Builtin {
            handler: legacy.name.clone(),
        };

        Self {
            name: legacy.name.clone(),
            version: legacy.version.clone(),
            api_version: 1,
            runtime,
            capabilities,
            permissions: PluginPermissionSet::default(),
            description: legacy.description.clone(),
            author: legacy.author.clone(),
            homepage: legacy.homepage.clone(),
            license: legacy.license.clone(),
            hooks: legacy.hooks.clone(),
            config: legacy.config.clone(),
        }
    }

    /// Get the runtime kind string.
    pub fn runtime_kind(&self) -> &str {
        match &self.runtime {
            PluginRuntimeSpec::Builtin { .. } => "builtin",
            PluginRuntimeSpec::Process { .. } => "process",
            PluginRuntimeSpec::Wasm { .. } => "wasm",
        }
    }

    /// Infer trust class from the runtime kind.
    pub fn trust_class(&self) -> PluginTrustClass {
        PluginTrustClass::from_runtime_kind(self.runtime_kind())
    }

    /// Get all command capabilities.
    pub fn commands(&self) -> impl Iterator<Item = &PluginCommandSpec> {
        self.capabilities.iter().filter_map(|c| match c {
            PluginCapability::Command(cmd) => Some(cmd),
            _ => None,
        })
    }

    /// Get all hook capabilities.
    pub fn hooks_capabilities(&self) -> impl Iterator<Item = &PluginHookSpec> {
        self.capabilities.iter().filter_map(|c| match c {
            PluginCapability::Hook(hook) => Some(hook),
            _ => None,
        })
    }

    /// Get all panel capabilities.
    pub fn panels(&self) -> impl Iterator<Item = &PluginPanelContribution> {
        self.capabilities.iter().filter_map(|c| match c {
            PluginCapability::Panel(panel) => Some(panel),
            _ => None,
        })
    }

    /// Get all status widget capabilities.
    pub fn status_widgets(&self) -> impl Iterator<Item = &PluginStatusContribution> {
        self.capabilities.iter().filter_map(|c| match c {
            PluginCapability::StatusWidget(sw) => Some(sw),
            _ => None,
        })
    }

    /// Get all event subscription capabilities.
    pub fn event_subscriptions(&self) -> impl Iterator<Item = &PluginEventSubscriptionSpec> {
        self.capabilities.iter().filter_map(|c| match c {
            PluginCapability::EventSubscription(es) => Some(es),
            _ => None,
        })
    }
}

/// Legacy plugin manifest format (pre-Phase 5).
///
/// This struct is kept for backwards compatibility with existing manifest.toml files.
/// New plugins should use the canonical `PluginManifest` with runtime + capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LegacyManifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    #[serde(default)]
    pub hooks: Vec<LegacyHookSpec>,
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_manifest() {
        let m = PluginManifest::default();
        assert!(m.name.is_empty());
        assert!(m.capabilities.is_empty());
    }

    #[test]
    fn canonical_process_manifest_parses() {
        let toml_str = r#"
name = "quota"
version = "0.1.0"
api_version = 1

[runtime]
kind = "process"
command = "python3"
args = ["quota.py"]
timeout_ms = 5000

[[capabilities]]
type = "command"
name = "quota"
description = "Show provider quota"
output = ["chat", "dialog"]

[permissions]
network = false
filesystem = "none"
"#;
        let m = PluginManifest::from_toml_str(toml_str).unwrap();
        assert_eq!(m.name, "quota");
        assert_eq!(m.version, "0.1.0");
        assert_eq!(m.api_version, 1);
        assert_eq!(m.runtime_kind(), "process");
        assert_eq!(m.trust_class(), PluginTrustClass::LocalProcess);
        assert_eq!(m.capabilities.len(), 1);
        assert!(m.commands().next().is_some());
        assert_eq!(m.commands().next().unwrap().name, "quota");
    }

    #[test]
    fn canonical_wasm_hook_manifest_parses() {
        let toml_str = r#"
name = "policy-filter"
version = "0.1.0"
api_version = 1

[runtime]
kind = "wasm"
module = "plugin.wasm"
timeout_ms = 1000
memory_max_mb = 16
fuel_per_call = 1000000

[[capabilities]]
type = "hook"
hook_type = "tool.execute.before"
priority = -10
"#;
        let m = PluginManifest::from_toml_str(toml_str).unwrap();
        assert_eq!(m.name, "policy-filter");
        assert_eq!(m.runtime_kind(), "wasm");
        assert_eq!(m.trust_class(), PluginTrustClass::SandboxedWasm);
        assert_eq!(m.capabilities.len(), 1);
        let hook = m.hooks_capabilities().next().unwrap();
        assert_eq!(hook.hook_type, "tool.execute.before");
        assert_eq!(hook.priority, -10);
    }

    #[test]
    fn legacy_manifest_converts_to_canonical() {
        let legacy = LegacyManifest {
            name: "old-plugin".into(),
            version: "1.0.0".into(),
            description: Some("An old plugin".into()),
            hooks: vec![
                LegacyHookSpec {
                    hook_type: "auth".into(),
                    priority: Some(5),
                },
                LegacyHookSpec {
                    hook_type: "tool.execute.before".into(),
                    priority: None,
                },
            ],
            ..Default::default()
        };

        let m = PluginManifest::from_legacy(&legacy);
        assert_eq!(m.name, "old-plugin");
        assert_eq!(m.capabilities.len(), 2);
        assert_eq!(m.hooks.len(), 2);
        assert_eq!(m.runtime_kind(), "builtin");
        assert_eq!(m.trust_class(), PluginTrustClass::Builtin);
    }

    #[test]
    fn legacy_hooks_auto_convert_to_capabilities() {
        let toml_str = r#"
name = "legacy-plugin"
version = "2.0.0"

[[hooks]]
type = "auth"
priority = 10

[[hooks]]
type = "tool.execute.after"
"#;
        let m = PluginManifest::from_toml_str(toml_str).unwrap();
        assert_eq!(m.capabilities.len(), 2);
        assert_eq!(m.hooks.len(), 2);
        // Hooks should be converted to capabilities
        let hook_caps: Vec<_> = m.hooks_capabilities().collect();
        assert_eq!(hook_caps.len(), 2);
    }

    #[test]
    fn trust_class_from_runtime_kind() {
        assert_eq!(
            PluginTrustClass::from_runtime_kind("builtin"),
            PluginTrustClass::Builtin
        );
        assert_eq!(
            PluginTrustClass::from_runtime_kind("process"),
            PluginTrustClass::LocalProcess
        );
        assert_eq!(
            PluginTrustClass::from_runtime_kind("wasm"),
            PluginTrustClass::SandboxedWasm
        );
        assert_eq!(
            PluginTrustClass::from_runtime_kind("pyo3"),
            PluginTrustClass::TrustedLocal
        );
    }

    #[test]
    fn builtin_manifest_represents_existing_plugins() {
        let m = PluginManifest {
            name: "copilot".into(),
            version: "1.0.0".into(),
            runtime: PluginRuntimeSpec::Builtin {
                handler: "copilot_auth".into(),
            },
            capabilities: vec![PluginCapability::Hook(PluginHookSpec {
                hook_type: "auth".into(),
                priority: 0,
                handler: None,
            })],
            ..Default::default()
        };
        assert_eq!(m.runtime_kind(), "builtin");
        assert_eq!(m.trust_class(), PluginTrustClass::Builtin);
    }
}
