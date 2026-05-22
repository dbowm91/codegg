pub mod codex;
pub mod copilot;
pub mod gitlab;
pub mod poe;

use crate::plugin::hooks::{HookContext, HookResult, HookType};
use crate::plugin::manifest::{HookSpec, PluginManifest};
use crate::plugin::registry::PluginInfo;
use std::collections::HashMap;
use std::path::PathBuf;
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
            path: PathBuf::from("<builtin>"),
            enabled: true,
            error: None,
        };

        registry.register(info, hook_specs).await;
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
        .map(|(ht, pri)| HookSpec {
            hook_type: ht.to_string(),
            priority: Some(*pri),
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
    };

    let info = PluginInfo {
        id: format!("builtin:{}", name),
        manifest,
        path: PathBuf::from("<builtin>"),
        enabled: true,
        error: None,
    };

    (info, registrations)
}
