use codegg::plugin::{
    install_from_path, ApiVersion, HookContext, HookResult, HookType, PluginCapability,
    PluginCommandSpec, PluginHookSpec, PluginManifest, PluginPermissionSet, PluginTrustClass,
    Stability, API_VERSION,
};
use std::path::PathBuf;

#[test]
fn test_api_version_current() {
    let version = ApiVersion::current();
    assert_eq!(version.version, API_VERSION);
    assert!(version.stability.is_stable());
}

#[test]
fn test_stability_is_stable() {
    assert!(Stability::Stable.is_stable());
    assert!(!Stability::Beta.is_stable());
    assert!(!Stability::Alpha.is_stable());
}

#[test]
fn test_plugin_manifest_default() {
    let manifest = PluginManifest::default();
    assert!(manifest.name.is_empty());
    assert_eq!(manifest.api_version, 1);
    assert!(manifest.capabilities.is_empty());
}

#[test]
fn test_hook_type_variants() {
    assert!(matches!(HookType::Auth, HookType::Auth));
    assert!(matches!(HookType::Provider, HookType::Provider));
    assert!(matches!(HookType::ToolDefinition, HookType::ToolDefinition));
    assert!(matches!(
        HookType::ToolExecuteBefore,
        HookType::ToolExecuteBefore
    ));
    assert!(matches!(
        HookType::ToolExecuteAfter,
        HookType::ToolExecuteAfter
    ));
}

#[test]
fn test_hook_result_ok() {
    let result = HookResult::ok(serde_json::json!({}));
    assert!(!result.blocked);
    assert!(result.error.is_none());
}

#[test]
fn test_hook_result_blocked() {
    let result = HookResult::blocked();
    assert!(result.blocked);
}

#[test]
fn test_hook_result_error() {
    let result = HookResult::error("test error message");
    assert!(!result.blocked);
    assert!(result.error.is_some());
    assert_eq!(result.error.unwrap(), "test error message");
}

#[tokio::test]
async fn test_install_from_path_nonexistent() {
    let path = PathBuf::from("/nonexistent/path");
    let result = install_from_path(&path).await;
    assert!(result.is_err());
}

#[test]
fn test_hook_context_new() {
    let context = HookContext {
        hook_type: HookType::ToolExecuteBefore,
        input: serde_json::json!({}),
    };
    assert!(matches!(context.hook_type, HookType::ToolExecuteBefore));
}

#[test]
fn test_canonical_process_manifest_parses() {
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
    let cmd = m.commands().next().unwrap();
    assert_eq!(cmd.name, "quota");
}

#[test]
fn test_canonical_wasm_hook_manifest_parses() {
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
    assert_eq!(m.runtime_kind(), "wasm");
    assert_eq!(m.trust_class(), PluginTrustClass::SandboxedWasm);
    let hook = m.hooks_capabilities().next().unwrap();
    assert_eq!(hook.hook_type, "tool.execute.before");
    assert_eq!(hook.priority, -10);
}

#[test]
fn test_legacy_manifest_auto_converts_hooks_to_capabilities() {
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
    assert_eq!(m.runtime_kind(), "builtin");
}

#[test]
fn test_trust_class_inferred_from_runtime() {
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
}

#[test]
fn test_permission_set_defaults() {
    let perm = PluginPermissionSet::default();
    assert!(!perm.network);
    assert!(!perm.session_messages);
    assert!(!perm.tool_interception);
    assert!(perm.env.is_empty());
    assert!(perm.secrets.is_empty());
}

#[test]
fn test_plugin_manifest_commands_iterator() {
    let manifest = PluginManifest {
        name: "test".into(),
        capabilities: vec![
            PluginCapability::Command(PluginCommandSpec {
                name: "cmd1".into(),
                aliases: vec![],
                description: None,
                handler: None,
                output: vec![],
            }),
            PluginCapability::Hook(PluginHookSpec {
                hook_type: "auth".into(),
                priority: 0,
                handler: None,
            }),
            PluginCapability::Command(PluginCommandSpec {
                name: "cmd2".into(),
                aliases: vec!["c2".into()],
                description: None,
                handler: None,
                output: vec![],
            }),
        ],
        ..Default::default()
    };
    let cmds: Vec<_> = manifest.commands().collect();
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].name, "cmd1");
    assert_eq!(cmds[1].name, "cmd2");
}
