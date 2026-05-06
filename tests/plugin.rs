use codegg::plugin::{
    install_from_path, ApiVersion, HookContext, HookResult, HookType, PluginManifest,
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
    assert!(manifest.name.is_empty() || manifest.name == "unnamed");
}

#[test]
fn test_hook_type_variants() {
    assert!(matches!(HookType::Auth, HookType::Auth));
    assert!(matches!(HookType::Provider, HookType::Provider));
    assert!(matches!(HookType::ToolDefinition, HookType::ToolDefinition));
    assert!(matches!(HookType::ToolExecuteBefore, HookType::ToolExecuteBefore));
    assert!(matches!(HookType::ToolExecuteAfter, HookType::ToolExecuteAfter));
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
