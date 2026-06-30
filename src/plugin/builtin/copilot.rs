use crate::plugin::builtin::make_builtin_info;
use crate::plugin::hooks::{HookContext, HookResult};
use crate::plugin::manifest::{
    PluginCapability, PluginHookSpec, PluginManifest, PluginRuntimeSpec,
};

pub const PLUGIN_ID: &str = "builtin:copilot";
pub const HANDLER_ID: &str = "copilot";

/// Return the canonical manifest for the Copilot builtin plugin.
pub fn manifest() -> PluginManifest {
    PluginManifest {
        name: "copilot".into(),
        version: "0.1.0".into(),
        description: Some("GitHub Copilot authentication provider".into()),
        author: Some("codegg".into()),
        hooks: vec![crate::plugin::manifest::LegacyHookSpec {
            hook_type: "auth".into(),
            priority: Some(0),
        }],
        runtime: PluginRuntimeSpec::Builtin {
            handler: HANDLER_ID.into(),
        },
        capabilities: vec![PluginCapability::Hook(PluginHookSpec {
            hook_type: "auth".into(),
            priority: 0,
            handler: None,
        })],
        ..Default::default()
    }
}

pub fn plugin() -> crate::plugin::builtin::BuiltinPlugin {
    crate::plugin::builtin::BuiltinPlugin {
        manifest: manifest(),
        handler: handle_hook,
    }
}

pub fn handle_hook(ctx: HookContext) -> HookResult {
    match ctx.hook_type {
        crate::plugin::hooks::HookType::Auth => {
            let input = ctx.input;
            let provider = input.get("provider").and_then(|v| v.as_str()).unwrap_or("");

            if provider != "copilot" && provider != "github" {
                return HookResult::ok(input);
            }

            let mut output = input.clone();
            if let Some(headers) = output.get_mut("headers").and_then(|h| h.as_object_mut()) {
                if let Some(token) = input.get("token").and_then(|t| t.as_str()) {
                    headers.insert(
                        "Authorization".into(),
                        serde_json::Value::String(format!("Bearer {}", token)),
                    );
                }
            }

            HookResult::ok(output)
        }
        _ => HookResult::ok(ctx.input),
    }
}

pub fn plugin_info() -> (
    crate::plugin::registry::PluginInfo,
    Vec<crate::plugin::hooks::HookRegistration>,
) {
    make_builtin_info("copilot", "0.1.0", vec![("auth", 0)])
}
