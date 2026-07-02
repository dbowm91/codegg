# builtin-reference — Anatomy of a codegg Builtin Plugin

This is a documentation-only example. It walks through the pattern used
by built-in plugins like `copilot`, showing how a native Rust handler is
compiled into codegg and dispatched at runtime.

For external plugin authors: use the **process** or **WASM** runtimes
instead. Builtins are reserved for codegg contributors.

## The four exported functions

Every builtin module exports these items:

| Export | Purpose |
|--------|---------|
| `PLUGIN_ID` | Stable string ID, e.g. `"builtin:copilot"` |
| `HANDLER_ID` | Registry key matching the `BuiltinHandlerRegistry` map |
| `manifest()` | Returns a `PluginManifest` describing the plugin |
| `plugin()` | Returns a `BuiltinPlugin` (manifest + handler fn) |

## Canonical reference: `copilot.rs`

```rust
use crate::plugin::builtin::make_builtin_info;
use crate::plugin::hooks::{HookContext, HookResult};
use crate::plugin::manifest::{
    PluginCapability, PluginHookSpec, PluginManifest, PluginRuntimeSpec,
};

pub const PLUGIN_ID: &str = "builtin:copilot";
pub const HANDLER_ID: &str = "copilot";

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
            let provider = input.get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if provider != "copilot" && provider != "github" {
                return HookResult::ok(input);
            }

            let mut output = input.clone();
            if let Some(headers) = output.get_mut("headers")
                .and_then(|h| h.as_object_mut())
            {
                if let Some(token) = input.get("token")
                    .and_then(|t| t.as_str())
                {
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
```

## Step-by-step: how a builtin gets registered

### 1. Module lives under `src/plugin/builtin/`

Each builtin is a single `.rs` file (e.g. `copilot.rs`, `codex.rs`,
`gitlab.rs`, `poe.rs`).

### 2. `builtin/mod.rs` maintains the handler map

```rust
static BUILTIN_HANDLERS: LazyLock<RwLock<HashMap<String, fn(HookContext) -> HookResult>>> =
    LazyLock::new(|| {
        let mut handlers = HashMap::new();
        handlers.insert("copilot".into(), copilot::handle_hook);
        handlers.insert("codex".into(), codex::handle_hook);
        handlers.insert("gitlab".into(), gitlab::handle_hook);
        handlers.insert("poe".into(), poe::handle_hook);
        RwLock::new(handlers)
    });
```

### 3. `BuiltinRuntime` dispatches via the trait

`BuiltinRuntime` (`src/plugin/runtime/builtin.rs`) implements
`PluginRuntime::invoke()`. It converts the incoming `PluginInvocation`
into a `HookContext`, looks up the handler by ID, and calls it.

### 4. `PluginService::with_builtin_runtime()` wires it in

The daemon creates a `BuiltinRuntime` and passes it to `PluginService`.
When a hook invocation targets a builtin plugin, `PluginService` routes
it through `BuiltinRuntime` instead of spawning a process.

### 5. Builtin manifests are indexed by the registry

`builtin_plugin_manifests()` provides all builtin manifests for
`PluginRegistry::register_manifest()` calls at startup.

## Minimal echo builtin

Here is the smallest possible builtin — an echo hook that passes its
input through unchanged, plus a command capability stub:

```rust
use crate::plugin::hooks::{HookContext, HookResult};
use crate::plugin::manifest::{
    PluginCapability, PluginCommandSpec, PluginHookSpec,
    PluginManifest, PluginOutputSurface, PluginRuntimeSpec,
};

pub const PLUGIN_ID: &str = "builtin:echo";
pub const HANDLER_ID: &str = "echo";

pub fn manifest() -> PluginManifest {
    PluginManifest {
        name: "echo".into(),
        version: "0.1.0".into(),
        description: Some("Minimal echo builtin for testing".into()),
        author: Some("codegg".into()),
        runtime: PluginRuntimeSpec::Builtin {
            handler: HANDLER_ID.into(),
        },
        capabilities: vec![
            PluginCapability::Hook(PluginHookSpec {
                hook_type: "auth".into(),
                priority: 0,
                handler: None,
            }),
            PluginCapability::Command(PluginCommandSpec {
                name: "echo".into(),
                aliases: vec![],
                description: Some("Echo back the input".into()),
                handler: None,
                output: vec![PluginOutputSurface::Chat],
            }),
        ],
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
        crate::plugin::hooks::HookType::Auth => HookResult::ok(ctx.input),
        _ => HookResult::ok(ctx.input),
    }
}
```

## Key notes

- **BuiltinRuntime is hook-only (Phase 11):** Command invocations to
  builtins return `RuntimeError::Unsupported`. Builtins cannot currently
  serve as command plugins.
- **Trust class is `Builtin`:** Builtins get the highest trust level.
  They bypass lifecycle policy gating for observation hooks.
- **No WASM, no process spawn:** The handler function runs directly in
  the tokio runtime. Keep it synchronous and fast.
- **External authors should not use this path.** Use `runtime: process`
  (script) or `runtime: wasm` (compiled module) for your own plugins.
