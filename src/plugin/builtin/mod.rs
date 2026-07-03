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
use crate::plugin::registry::{PluginInfo, PluginSourceMetadata};
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
    registry.register("poe".to_string(), poe::handle_hook as BuiltinHookHandler);
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

        let info = PluginInfo {
            id: id.clone(),
            manifest: bp.manifest,
            enabled: true,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
            source: Some(PluginSourceMetadata::builtin()),
        };

        // register() extracts hooks from both capabilities and legacy hooks fields.
        // No need to pass explicit hook_specs — they would be double-counted.
        let _ = registry.register(info).await;
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
        .filter_map(|hs| {
            HookType::parse(&hs.hook_type).map(|ht| crate::plugin::hooks::HookRegistration {
                plugin_id: format!("builtin:{}", name),
                hook_type: ht,
                priority: hs.priority.unwrap_or(0),
            })
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
        source: Some(PluginSourceMetadata::builtin()),
    };

    (info, registrations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::runtime::builtin::BuiltinRuntime;
    use std::sync::Arc;

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

    #[tokio::test]
    async fn register_builtins_populates_registry_hook_indexes() {
        let registry = crate::plugin::registry::PluginRegistry::new();
        register_builtins(&registry).await;

        let auth_hooks = registry.hooks_for(HookType::Auth).await;
        assert_eq!(auth_hooks.len(), 4, "should have 4 builtin auth hooks");

        let plugin_ids: Vec<&str> = auth_hooks.iter().map(|h| h.plugin_id.as_str()).collect();
        assert!(plugin_ids.contains(&"builtin:copilot"));
        assert!(plugin_ids.contains(&"builtin:codex"));
        assert!(plugin_ids.contains(&"builtin:gitlab"));
        assert!(plugin_ids.contains(&"builtin:poe"));
    }

    #[tokio::test]
    async fn disabling_builtin_excludes_its_capabilities_from_queries() {
        let registry = crate::plugin::registry::PluginRegistry::new();
        register_builtins(&registry).await;

        let auth_hooks_before = registry.hooks_for(HookType::Auth).await;
        assert_eq!(auth_hooks_before.len(), 4);

        registry
            .set_enabled("builtin:copilot", false)
            .await
            .unwrap();

        let auth_hooks_after = registry.hooks_for(HookType::Auth).await;
        assert_eq!(auth_hooks_after.len(), 3, "copilot should be excluded");

        let plugin_ids: Vec<&str> = auth_hooks_after
            .iter()
            .map(|h| h.plugin_id.as_str())
            .collect();
        assert!(!plugin_ids.contains(&"builtin:copilot"));
        assert!(plugin_ids.contains(&"builtin:codex"));
        assert!(plugin_ids.contains(&"builtin:gitlab"));
        assert!(plugin_ids.contains(&"builtin:poe"));
    }

    #[tokio::test]
    async fn invoking_builtin_through_plugin_service_reaches_builtin_runtime() {
        let registry = Arc::new(crate::plugin::registry::PluginRegistry::new());
        register_builtins(&registry).await;

        let handler_registry = Arc::new(builtin_runtime_registry());
        let builtin_rt = Arc::new(BuiltinRuntime::new(handler_registry));

        let service =
            crate::plugin::service::PluginService::new(registry).with_builtin_runtime(builtin_rt);

        let result = service
            .dispatch_auth(serde_json::json!({"provider": "copilot", "token": "t", "headers": {}}))
            .await;
        assert!(!result.blocked, "builtin auth hook should not block");
        assert!(result.error.is_none(), "builtin auth hook should not error");
        assert!(
            result.output.get("headers").is_some(),
            "response should contain transformed headers"
        );
    }

    /// `PluginService::invoke_command` for a builtin plugin must reject with
    /// a runtime error (no builtin command handler exists). This guards
    /// against the old placeholder-success behavior.
    #[tokio::test]
    async fn builtin_command_invocation_is_rejected_by_service() {
        use crate::plugin::runtime::builtin::{BuiltinHandlerRegistry, BuiltinRuntime};
        let registry = Arc::new(crate::plugin::registry::PluginRegistry::new());
        register_builtins(&registry).await;

        let handler_registry = Arc::new(BuiltinHandlerRegistry::new());
        let builtin_rt = Arc::new(BuiltinRuntime::new(handler_registry));

        let service =
            crate::plugin::service::PluginService::new(registry).with_builtin_runtime(builtin_rt);

        let result = service
            .invoke_command("fake-builtin-cmd", vec![], serde_json::json!({}))
            .await;
        // Builtins don't currently register any commands, so this should be
        // CommandNotFound at the registry layer. Either CommandNotFound or
        // PluginError::Runtime is acceptable; the key invariant is that we
        // do NOT return a successful placeholder response for builtin commands.
        match result {
            Err(crate::plugin::service::PluginError::CommandNotFound(_))
            | Err(crate::plugin::service::PluginError::Runtime(_)) => {}
            Ok(resp) => panic!(
                "builtin command invocation must not silently succeed, got: {:?}",
                resp
            ),
            Err(other) => panic!(
                "expected CommandNotFound or Runtime error, got: {:?}",
                other
            ),
        }
    }
}
