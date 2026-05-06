pub mod codex;
pub mod copilot;
pub mod gitlab;
pub mod poe;

use crate::plugin::hooks::{HookContext, HookResult, HookType};
use crate::plugin::manifest::{HookSpec, PluginManifest};
use crate::plugin::registry::PluginInfo;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct BuiltinPlugin {
    pub manifest: PluginManifest,
    pub handler: fn(HookContext) -> HookResult,
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
            path: PathBuf::from("<builtin>"),
            enabled: true,
            error: None,
        };

        registry.register(info, hook_specs).await;
        register_builtin_handler(&id, bp.handler);
    }
}

fn register_builtin_handler(_id: &str, _handler: fn(HookContext) -> HookResult) {
    tracing::info!(id = _id, "registered builtin plugin handler");
}

pub fn builtin_hook_handler(plugin_name: &str, ctx: HookContext) -> HookResult {
    match plugin_name {
        "copilot" => copilot::handle_hook(ctx),
        "gitlab" => gitlab::handle_hook(ctx),
        "codex" => codex::handle_hook(ctx),
        "poe" => poe::handle_hook(ctx),
        _ => HookResult::error(format!("unknown builtin plugin: {}", plugin_name)),
    }
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
