#![allow(clippy::type_complexity)]

pub mod codex;
pub mod copilot;
pub mod gitlab;
pub mod poe;

use crate::plugin::hooks::{HookContext, HookResult, HookType};
use crate::plugin::manifest::PluginTrustClass;
use crate::plugin::manifest::{
    LegacyHookSpec, PluginCapability, PluginHookSpec, PluginManifest, PluginRuntimeSpec,
};
use crate::plugin::registry::PluginInfo;
use crate::plugin::runtime::builtin::{BuiltinHandlerRegistry, BuiltinHookHandler};
use std::collections::HashMap;
use std::sync::RwLock;

pub struct BuiltinPlugin {
    pub manifest: PluginManifest,
    pub handler: fn(HookContext) -> HookResult,
}

static BUILTIN_HANDLERS: std::sync::LazyLock<
    RwLock<HashMap<String, fn(HookContext) -> HookResult>>,
> = std::sync::LazyLock::new(|| {
    let mut handlers = HashMap::new();
    handlers.insert(
        "copilot".to_string(),
        copilot::handle_hook as fn(HookContext) -> HookResult,
    );
    handlers.insert(
        "gitlab".to_string(),
        gitlab::handle_hook as fn(HookContext) -> HookResult,
    );
    handlers.insert(
        "codex".to_string(),
        codex::handle_hook as fn(HookContext) -> HookResult,
    );
    handlers.insert(
        "poe".to_string(),
        poe::handle_hook as fn(HookContext) -> HookResult,
    );
    RwLock::new(handlers)
});

/// Return manifests for all known builtin plugins.
///
/// Each manifest declares `runtime = Builtin { handler }` and the appropriate
/// hook capabilities. This is the canonical source for builtin plugin metadata.
pub fn builtin_plugin_manifests() -> Vec<PluginManifest> {
    vec![
        copilot::manifest(),
        gitlab::manifest(),
        codex::manifest(),
        poe::manifest(),
    ]
}

/// Build a `BuiltinHandlerRegistry` populated with all builtin plugin handlers.
///
/// The returned registry can be wrapped in `Arc` and passed to `BuiltinRuntime::new()`.
pub fn builtin_runtime_registry() -> BuiltinHandlerRegistry {
    let mut registry = BuiltinHandlerRegistry::new();
    registry.register(
        "copilot".to_string(),
        copilot::handle_hook as BuiltinHookHandler,
    );
    registry.register(
        "gitlab".to_string(),
        gitlab::handle_hook as BuiltinHookHandler,
    );
    registry.register(
        "codex".to_string(),
        codex::handle_hook as BuiltinHookHandler,
    );
    registry.register(
        "poe".to_string(),
        poe::handle_hook as BuiltinHookHandler,
    );
    registry
}

pub fn get_builtin_plugins() -> Vec<BuiltinPlugin> {
    vec![
        copilot::plugin(),
        gitlab::plugin(),
        codex::plugin(),
        poe::plugin(),
    ]
}

pub async fn register_builtins(registry: &crate::plugin::registry::PluginRegistry) {
    let plugins = get_builtin_plugins();
    for bp in plugins {
        let id = format!("builtin:{}", bp.manifest.name);
        let hook_specs: Vec<_> = bp
            .manifest
            .hooks
            .iter()
            .map(|hs| crate::plugin::hooks::HookRegistration {
                plugin_id: id.clone(),
                hook_type: HookType::parse(&hs.hook_type).unwrap_or(HookType::Auth),
                priority: hs.priority.unwrap_or(0),
            })
            .collect();

        let info = PluginInfo {
            id: id.clone(),
            manifest: bp.manifest,
            enabled: true,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
        };

        // Best-effort registration (ignore duplicate errors for builtins)
        let _ = registry.register_with_hooks(info, hook_specs).await;
        register_builtin_handler(&id, bp.handler);
    }
}

fn register_builtin_handler(id: &str, handler: fn(HookContext) -> HookResult) {
    let plugin_name = id.strip_prefix("builtin:").unwrap_or(id);
    if let Ok(mut handlers) = BUILTIN_HANDLERS.write() {
        handlers.insert(plugin_name.to_string(), handler);
    }
    tracing::info!(id = id, "registered builtin plugin handler");
}

pub fn builtin_hook_handler(plugin_name: &str, ctx: HookContext) -> HookResult {
    if let Ok(handlers) = BUILTIN_HANDLERS.read() {
        if let Some(handler) = handlers.get(plugin_name) {
            return handler(ctx);
        }
    }
    HookResult::error(format!("unknown builtin plugin: {}", plugin_name))
}

pub fn make_builtin_info(
    name: &str,
    version: &str,
    hooks: Vec<(&str, i32)>,
) -> (PluginInfo, Vec<crate::plugin::hooks::HookRegistration>) {
    let hook_specs: Vec<_> = hooks
        .iter()
        .map(|(ht, pri)| LegacyHookSpec {
            hook_type: ht.to_string(),
            priority: Some(*pri),
        })
        .collect();

    let capabilities: Vec<PluginCapability> = hook_specs
        .iter()
        .map(|hs| {
            PluginCapability::Hook(PluginHookSpec {
                hook_type: hs.hook_type.clone(),
                priority: hs.priority.unwrap_or(0),
                handler: None,
            })
        })
        .collect();

    let registrations: Vec<_> = hook_specs
        .iter()
        .map(|hs| crate::plugin::hooks::HookRegistration {
            plugin_id: format!("builtin:{}", name),
            hook_type: HookType::parse(&hs.hook_type).unwrap_or(HookType::Auth),
            priority: hs.priority.unwrap_or(0),
        })
        .collect();

    let manifest = PluginManifest {
        name: name.to_string(),
        version: version.to_string(),
        description: None,
        author: None,
        homepage: None,
        license: None,
        hooks: hook_specs,
        config: HashMap::new(),
        runtime: PluginRuntimeSpec::Builtin {
            handler: name.to_string(),
        },
        capabilities,
        ..Default::default()
    };

    let info = PluginInfo {
        id: format!("builtin:{}", name),
        manifest,
        enabled: true,
        trust: PluginTrustClass::Builtin,
        diagnostics: Vec::new(),
    };

    (info, registrations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_plugin_manifests_returns_all_builtins() {
        let manifests = builtin_plugin_manifests();
        assert_eq!(manifests.len(), 4);

        let names: Vec<&str> = manifests.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"copilot"));
        assert!(names.contains(&"codex"));
        assert!(names.contains(&"gitlab"));
        assert!(names.contains(&"poe"));
    }

    #[test]
    fn builtin_plugin_manifests_declare_builtin_runtime() {
        let manifests = builtin_plugin_manifests();
        for m in &manifests {
            assert_eq!(m.runtime_kind(), "builtin");
            assert_eq!(m.trust_class(), PluginTrustClass::Builtin);
            match &m.runtime {
                PluginRuntimeSpec::Builtin { handler } => {
                    assert!(!handler.is_empty());
                }
                _ => panic!("expected Builtin runtime spec for {}", m.name),
            }
        }
    }

    #[test]
    fn builtin_plugin_manifests_declare_hook_capabilities() {
        let manifests = builtin_plugin_manifests();
        for m in &manifests {
            let hooks: Vec<_> = m.hooks_capabilities().collect();
            assert!(
                !hooks.is_empty(),
                "builtin {} should declare hook capabilities",
                m.name
            );
        }
    }

    #[test]
    fn builtin_runtime_registry_contains_all_handlers() {
        let registry = builtin_runtime_registry();
        assert_eq!(registry.len(), 4);
        assert!(registry.contains("copilot"));
        assert!(registry.contains("codex"));
        assert!(registry.contains("gitlab"));
        assert!(registry.contains("poe"));
    }

    #[test]
    fn builtin_runtime_registry_handlers_work() {
        let registry = builtin_runtime_registry();
        let handler = registry.get("copilot").unwrap();
        let ctx = HookContext {
            hook_type: HookType::Auth,
            input: serde_json::json!({"provider": "copilot", "token": "test", "headers": {}}),
        };
        let result = handler(ctx);
        assert!(!result.blocked);
        assert!(result.error.is_none());
        assert_eq!(
            result.output,
            serde_json::json!({"provider": "copilot", "token": "test", "headers": {"Authorization": "Bearer test"}})
        );
    }
}
