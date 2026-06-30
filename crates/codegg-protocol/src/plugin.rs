use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ui::UiEffect;

pub const PLUGIN_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginManifestDto {
    pub name: String,
    pub version: String,
    pub api_version: u32,
    pub runtime: PluginRuntimeSpec,
    pub capabilities: Vec<PluginCapability>,
    pub permissions: PluginPermissionSet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginRuntimeSpec {
    Builtin {
        handler: String,
    },
    Process {
        command: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginCapability {
    Command(PluginCommandSpec),
    Hook(PluginHookSpec),
    Panel(PluginPanelContribution),
    StatusWidget(PluginStatusContribution),
    EventSubscription(PluginEventSubscription),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginCommandSpec {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub handler: Option<String>,
    pub output: Vec<PluginOutputSurface>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginHookSpec {
    pub hook_type: String,
    pub priority: i32,
    pub handler: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginPanelContribution {
    pub id: String,
    pub title: String,
    pub placement: String,
    pub handler: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginStatusContribution {
    pub id: String,
    pub label: Option<String>,
    pub placement: String,
    pub refresh_ms: Option<u64>,
    pub handler: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginEventSubscription {
    pub event_type: String,
    pub handler: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PluginOutputSurface {
    Chat,
    Toast,
    Dialog,
    Panel,
    Status,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PluginPermissionSet {
    pub network: bool,
    pub filesystem: FilesystemPermission,
    pub env: Vec<String>,
    pub secrets: Vec<String>,
    pub session_messages: bool,
    pub tool_interception: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemPermission {
    #[default]
    None,
    ProjectRead,
    ProjectWrite,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginInvocation {
    pub protocol_version: u32,
    pub invocation_id: String,
    pub plugin_id: String,
    pub capability: PluginCapabilityInvocation,
    pub args: Vec<String>,
    pub input: serde_json::Value,
    pub context: PluginContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginCapabilityInvocation {
    Command { name: String },
    Hook { hook_type: String },
    Panel { id: String },
    StatusWidget { id: String },
    Event { event_type: String },
}

impl PluginCapabilityInvocation {
    /// Get the hook type string if this is a Hook invocation, or a debug description.
    pub fn hook_type_string(&self) -> String {
        match self {
            Self::Hook { hook_type } => hook_type.clone(),
            other => format!("{other:?}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PluginContext {
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub project_dir: Option<String>,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub frontend_capabilities: Vec<String>,
    pub metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginResponse {
    pub ok: bool,
    pub effects: Vec<UiEffect>,
    pub data: serde_json::Value,
    pub diagnostics: Vec<PluginDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginDiagnostic {
    pub level: PluginDiagnosticLevel,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PluginDiagnosticLevel {
    Debug,
    Info,
    Warning,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{ChatBlock, ChatFormat, DialogSpec, TextNode, UiNode};

    #[test]
    fn plugin_invocation_command_round_trip() {
        let inv = PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id: "inv-1".into(),
            plugin_id: "my-plugin".into(),
            capability: PluginCapabilityInvocation::Command {
                name: "greet".into(),
            },
            args: vec!["world".into()],
            input: serde_json::json!({"name": "world"}),
            context: PluginContext::default(),
        };
        let json = serde_json::to_string(&inv).unwrap();
        assert!(json.contains("inv-1"));
        assert!(json.contains("my-plugin"));
        let back: PluginInvocation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, inv);
    }

    #[test]
    fn plugin_response_with_effects_round_trip() {
        let resp = PluginResponse {
            ok: true,
            effects: vec![
                UiEffect::EmitChat {
                    block: ChatBlock {
                        format: ChatFormat::Markdown,
                        content: "Hello!".into(),
                    },
                },
                UiEffect::OpenDialog {
                    dialog: DialogSpec {
                        id: "dlg-1".into(),
                        title: "Info".into(),
                        body: UiNode::Text(TextNode {
                            text: "details".into(),
                        }),
                        modal: true,
                    },
                },
            ],
            data: serde_json::json!({"result": "ok"}),
            diagnostics: vec![PluginDiagnostic {
                level: PluginDiagnosticLevel::Info,
                message: "ran successfully".into(),
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("emit_chat"));
        assert!(json.contains("open_dialog"));
        let back: PluginResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back, resp);
    }

    #[test]
    fn default_filesystem_permission_is_none() {
        let perm = FilesystemPermission::default();
        assert_eq!(perm, FilesystemPermission::None);
    }

    #[test]
    fn default_plugin_permission_set() {
        let perm = PluginPermissionSet::default();
        assert!(!perm.network);
        assert_eq!(perm.filesystem, FilesystemPermission::None);
        assert!(perm.env.is_empty());
        assert!(perm.secrets.is_empty());
        assert!(!perm.session_messages);
        assert!(!perm.tool_interception);
    }

    #[test]
    fn plugin_runtime_spec_builtin_round_trip() {
        let spec = PluginRuntimeSpec::Builtin {
            handler: "copilot_auth".into(),
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("builtin"));
        let back: PluginRuntimeSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back, spec);
    }

    #[test]
    fn plugin_runtime_spec_process_round_trip() {
        let spec = PluginRuntimeSpec::Process {
            command: "python".into(),
            args: vec!["-m".into(), "my_plugin".into()],
            timeout_ms: Some(5000),
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("process"));
        let back: PluginRuntimeSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back, spec);
    }

    #[test]
    fn plugin_runtime_spec_wasm_round_trip() {
        let spec = PluginRuntimeSpec::Wasm {
            module: "plugin.wasm".into(),
            timeout_ms: Some(10000),
            memory_max_mb: Some(256),
            fuel_per_call: Some(1_000_000),
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("wasm"));
        let back: PluginRuntimeSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back, spec);
    }

    #[test]
    fn plugin_capability_command_round_trip() {
        let cap = PluginCapability::Command(PluginCommandSpec {
            name: "deploy".into(),
            aliases: vec!["d".into()],
            description: Some("Deploy the project".into()),
            handler: Some("handle_deploy".into()),
            output: vec![PluginOutputSurface::Chat, PluginOutputSurface::Toast],
        });
        let json = serde_json::to_string(&cap).unwrap();
        assert!(json.contains("command"));
        let back: PluginCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cap);
    }

    #[test]
    fn plugin_capability_hook_round_trip() {
        let cap = PluginCapability::Hook(PluginHookSpec {
            hook_type: "auth".into(),
            priority: 10,
            handler: Some("on_auth".into()),
        });
        let json = serde_json::to_string(&cap).unwrap();
        let back: PluginCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cap);
    }

    #[test]
    fn plugin_manifest_dto_round_trip() {
        let manifest = PluginManifestDto {
            name: "my-plugin".into(),
            version: "0.1.0".into(),
            api_version: 1,
            runtime: PluginRuntimeSpec::Builtin {
                handler: "test".into(),
            },
            capabilities: vec![PluginCapability::Command(PluginCommandSpec {
                name: "test-cmd".into(),
                aliases: vec![],
                description: None,
                handler: None,
                output: vec![],
            })],
            permissions: PluginPermissionSet::default(),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let back: PluginManifestDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back, manifest);
    }

    #[test]
    fn plugin_capability_invocation_hook_round_trip() {
        let cap = PluginCapabilityInvocation::Hook {
            hook_type: "auth".into(),
        };
        let json = serde_json::to_string(&cap).unwrap();
        assert!(json.contains("hook"));
        let back: PluginCapabilityInvocation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cap);
    }

    #[test]
    fn plugin_context_default() {
        let ctx = PluginContext::default();
        assert!(ctx.session_id.is_none());
        assert!(ctx.turn_id.is_none());
        assert!(ctx.project_dir.is_none());
        assert!(ctx.model.is_none());
        assert!(ctx.agent.is_none());
        assert!(ctx.frontend_capabilities.is_empty());
        assert!(ctx.metadata.is_empty());
    }

    #[test]
    fn plugin_protocol_version_is_set() {
        assert_eq!(PLUGIN_PROTOCOL_VERSION, 1);
    }
}
