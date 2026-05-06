use crate::plugin::builtin::make_builtin_info;
use crate::plugin::hooks::{HookContext, HookResult};

pub fn plugin() -> crate::plugin::builtin::BuiltinPlugin {
    let manifest = crate::plugin::manifest::PluginManifest {
        name: "gitlab".into(),
        version: "0.1.0".into(),
        description: Some("GitLab authentication provider".into()),
        author: Some("codegg".into()),
        homepage: None,
        license: None,
        hooks: vec![crate::plugin::manifest::HookSpec {
            hook_type: "auth".into(),
            priority: Some(0),
        }],
        config: Default::default(),
    };

    crate::plugin::builtin::BuiltinPlugin {
        manifest,
        handler: handle_hook,
    }
}

pub fn handle_hook(ctx: HookContext) -> HookResult {
    match ctx.hook_type {
        crate::plugin::hooks::HookType::Auth => {
            let input = ctx.input;
            let provider = input.get("provider").and_then(|v| v.as_str()).unwrap_or("");

            if provider != "gitlab" {
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
    make_builtin_info("gitlab", "0.1.0", vec![("auth", 0)])
}
