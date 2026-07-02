---
name: plugin
description: Plugin system, WASM execution, hooks, fuel tracking, capability-based registry, runtime abstraction (Process/Wasm/Builtin), EmitChat UI rendering, Phase 11 corrective hardening, Phase 12 management UX + policy, Phase 13 SDKs and examples
version: 1.4.0
tags:
  - plugin
  - wasm
  - hooks
  - fuel
  - wasmtime
  - sdk
  - examples
---

# Plugin System Guide

This skill covers the plugin system in opencode-rs, which enables extending the agent with WASM-based plugins and hooks.

## Architecture Overview

```
Plugin System
├── PluginLoader (WASM execution)
├── HookRegistry (hook registration)
├── FuelTracking (resource management)
├── PluginService (management APIs)
├── BuiltinPlugins (copilot, gitlab, codex, poe)
├── MarketplaceService (local plugin discovery)
├── PluginEventBus (event subscriptions)
├── LifecycleHooks (policy-aware hook dispatch, Phase 9)
└── PluginLifecyclePolicy (hook type/runtime gating, Phase 9)
```

## Project Structure

```
src/plugin/
├── mod.rs           # Main module, PluginService, exports
├── loader.rs        # WASM loading, execution, module caching
├── hooks.rs         # Hook types and HookContext/HookResult
├── registry.rs      # PluginRegistry for hook registration
├── manifest.rs      # PluginManifest parsing
├── service.rs       # PluginService implementation
├── install.rs       # Plugin installation from various sources
├── api.rs           # External API types (ApiVersion, Stability)
├── tui.rs           # TUI plugin registry for routes/components (deprecated/legacy, superseded by Phase 5 capability-based registry)
├── event_bus.rs     # Event bus integration
├── marketplace.rs   # Local plugin discovery service
├── lifecycle.rs  # LifecycleHooks, typed hook I/O contracts (Phase 9)
├── policy.rs     # PluginLifecyclePolicy, hook gating (Phase 9)
├── runtime/         # Plugin runtime abstraction (Phase 6+8)
│   ├── mod.rs       # PluginRuntime trait, RuntimeError, RuntimeLimits
│   ├── builtin.rs   # BuiltinRuntime for native Rust handlers (Phase 8)
│   ├── process.rs   # ProcessRuntime implementation
│   ├── wasm.rs      # WasmRuntime for WASM plugins (feature-gated)
│   └── wasm_cache.rs # WASM module caching with fuel budgets
└── builtin/         # Built-in plugins
    ├── mod.rs       # Plugin registry and builtin handler map
    ├── copilot.rs   # GitHub Copilot integration
    ├── codex.rs     # Anthropic Codex integration
    ├── gitlab.rs    # GitLab MR integration
    └── poe.rs       # Poe API integration
```

## Hook Types

Hooks are extension points in the agent lifecycle. Hook types use dot notation (e.g., `tool.execute.before`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, Display, EnumIter)]
#[strum(serialize_all = "snake_case")]
pub enum HookType {
    Auth,              // "auth"
    Provider,          // "provider"
    ToolDefinition,    // "tool.definition"
    ToolExecuteBefore, // "tool.execute.before"
    ToolExecuteAfter,  // "tool.execute.after"
    ChatParams,       // "chat.params"
    ChatHeaders,      // "chat.headers"
    Event,            // "event"
    Config,           // "config"
    ShellEnv,         // "shell.env"
    TextComplete,     // "text.complete"
    SessionCompacting,// "session.compacting"
    MessagesTransform,// "messages.transform"
}

impl HookType {
    pub fn as_str(&self) -> &'static str { ... }  // Returns dot notation
    pub fn parse(s: &str) -> Option<Self> { ... } // Parses dot notation
}
```

### Hook Context

```rust
pub struct HookContext {
    pub hook_type: HookType,
    pub input: serde_json::Value,
}

pub struct HookResult {
    pub output: serde_json::Value,
    pub blocked: bool,
    pub error: Option<String>,
}

impl HookResult {
    pub fn ok(output: serde_json::Value) -> Self;
    pub fn blocked() -> Self;
    pub fn error(msg: impl Into<String>) -> Self;
}
```

## Plugin Manifest

Plugins require `manifest.toml`. Phase 5 introduces a canonical capability-based format; the legacy `[[hooks]]` format is still supported for backward compatibility.

### Canonical Format (Phase 5+)

```toml
name = "my-plugin"
version = "1.0.0"
description = "My plugin description"
author = "Author Name"
homepage = "https://example.com"
license = "MIT"
api_version = "1"

[runtime]
type = "wasm"  # "builtin", "process", or "wasm"

[[capabilities]]
type = "command"
name = "my-command"

[[capabilities]]
type = "hook"
hook_type = "tool.execute.before"
priority = 0

[[capabilities]]
type = "hook"
hook_type = "tool.execute.after"
priority = 0

[[capabilities]]
type = "panel"
name = "my-panel"
route = "/my-plugin"

[[capabilities]]
type = "status_widget"
name = "my-status"

[[capabilities]]
type = "event_subscription"
event_type = "session.created"

[permissions]
filesystem = ["read", "write"]

[config]
some_setting = "value"
```

### Legacy Format (pre-Phase 5)

```toml
name = "my-plugin"
version = "1.0.0"
description = "My plugin description"
author = "Author Name"
homepage = "https://example.com"
license = "MIT"

[[hooks]]
type = "tool.execute.before"
priority = 0

[[hooks]]
type = "tool.execute.after"
priority = 0

[config]
some_setting = "value"
```

### Manifest Structure

```rust
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub api_version: Option<String>,          // Phase 5: manifest schema version
    pub runtime: Option<PluginRuntimeSpec>,    // Phase 5: how the plugin executes
    pub capabilities: Vec<PluginCapability>,   // Phase 5: what the plugin provides
    pub permissions: Option<PluginPermissionSet>, // Phase 5: declared permissions
    pub hooks: Vec<HookSpec>,                 // Legacy: hook specifications (pre-Phase 5)
    pub config: HashMap<String, serde_json::Value>,
}

pub struct HookSpec {
    #[serde(rename = "type")]
    pub hook_type: String,  // dot notation, e.g., "tool.execute.before"
    pub priority: Option<i32>,
}
```

## WASM Execution

Plugins execute via WASMtime with module caching. The execution path for WASM plugins:

```rust
pub async fn execute_wasm_hook(plugin_id: &str, ctx: HookContext) -> HookResult {
    use wasmtime::{Config, Engine, Linker, Module, Store, WasmBacktraceDetails};

    static ENGINE: once_cell::sync::Lazy<Engine> = once_cell::sync::Lazy::new(|| {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.wasm_backtrace_details(WasmBacktraceDetails::Disable);
        Engine::new(&config).unwrap()
    });

    // Check per-plugin fuel budget
    let current_plugin_fuel = module_cache::CACHE.get_plugin_fuel(plugin_id);
    if current_plugin_fuel >= MAX_PLUGIN_FUEL_BUDGET {
        tracing::warn!(plugin = plugin_id, "plugin fuel budget exhausted");
        return HookResult::ok(ctx.input);
    }

    let fuel_for_this_call = WASM_FUEL_PER_HOOK.min(current_plugin_fuel);

    // Reserve fuel for this call
    let Some(fuel_reserved) = module_cache::CACHE.reserve_fuel(plugin_id, fuel_for_this_call) else {
        tracing::warn!(plugin = plugin_id, "plugin fuel reservation failed");
        return HookResult::ok(ctx.input);
    };

    // Build WASM path: plugins/{plugin_name}/plugin.wasm
    let plugin_name = plugin_id.strip_prefix("plugin:").unwrap_or(plugin_id);
    let wasm_path = crate::plugin::install::plugins_dir().join(plugin_name).join("plugin.wasm");
    let wasm_path_str = wasm_path.to_string_lossy();

    // Check WASM size limit (10MB)
    let metadata = std::fs::metadata(&wasm_path)?;
    if metadata.len() > MAX_WASM_SIZE as u64 { ... }

    // Get module from cache (or compile if mtime changed)
    let module = module_cache::CACHE.get_or_compile(&wasm_path_str, || {
        let wasm_bytes = std::fs::read(&wasm_path).ok()?;
        Module::new(&ENGINE, &wasm_bytes).ok()
    })?;

    // Execute with fuel and timeout
    let hook_result = timeout(WASM_HOOK_TIMEOUT, async {
        let mut store = Store::new(&ENGINE, ());
        store.set_fuel(fuel_reserved).ok();

        let mut linker = Linker::new(&ENGINE);
        linker.allow_shadowing(true);
        let instance = linker.instantiate(&mut store, &module)?;

        // Map HookType to WASM function name
        let func_name = match ctx.hook_type {
            HookType::Auth => "on_auth",
            HookType::Provider => "on_provider",
            HookType::ToolDefinition => "on_tool_definition",
            HookType::ToolExecuteBefore => "on_tool_execute_before",
            HookType::ToolExecuteAfter => "on_tool_execute_after",
            HookType::ChatParams => "on_chat_params",
            HookType::ChatHeaders => "on_chat_headers",
            HookType::Event => "on_event",
            HookType::Config => "on_config",
            HookType::ShellEnv => "on_shell_env",
            HookType::TextComplete => "on_text_complete",
            HookType::SessionCompacting => "on_session_compacting",
            HookType::MessagesTransform => "on_messages_transform",
        };

        // Get memory and allocate function
        let memory = instance.get_memory(&mut store, "memory")?;
        let alloc_func = instance.get_func(&mut store, "allocate")?;

        // Serialize input, write to WASM memory, call hook, read output
        // ...
    }).await;

    // Handle result and return fuel.
    //
    // On success: return UNUSED fuel (`remaining`) — not consumed fuel.
    // The helper `return_unused_fuel()` caps the refund at `fuel_reserved`
    // so the budget can never be over-credited past what was reserved.
    match hook_result {
        Ok(Ok((result, remaining))) => {
            return_unused_fuel(&module_cache::CACHE, plugin_id, fuel_reserved, remaining);
            result
        }
        Ok(Err(e)) => {
            module_cache::CACHE.return_fuel(plugin_id, fuel_reserved);
            HookResult::error(format!("WASM hook execution error: {}", e))
        }
        Err(_) => {
            module_cache::CACHE.return_fuel(plugin_id, fuel_reserved);
            HookResult::error(format!("{}: hook timeout: {}", plugin_id, "execution timed out"))
        }
    }
}
```

**WASM Plugin Contract:**
- Plugin must export `memory` (Wasmtime memory)
- Plugin must export `allocate(ptr, len) -> ptr` function
- Plugin may export `deallocate(ptr, len)` function
- Hook function names: `on_auth`, `on_provider`, `on_tool_execute_before`, etc.
- Hook returns JSON: `{"output": {...}, "blocked": false, "error": null}`

**Fuel Accounting (Phase 11):**
- On successful execution, return the **unused** fuel (`remaining`),
  capped by the reserved amount. The per-plugin budget therefore
  decreases by exactly the consumed amount. The `return_unused_fuel()`
  helper in `src/plugin/runtime/wasm.rs` centralises this.
- On error / timeout, return the full `fuel_reserved` so failed
  invocations do not burn fuel.

**Fuel Leak Prevention**:
The `execute_wasm_hook()` function returns fuel on ALL early exits after `fuel_reserved` is set:
1. Hook function not found → return fuel before returning HookResult::ok
2. No memory export → return fuel before returning error
3. No allocate function → return fuel before returning error
4. Allocate returned no value → return fuel before returning error
5. Input exceeds memory bounds → return fuel before returning error
6. Timeout → return fuel in the Err(_) match arm
7. Execution error → return fuel in the Ok(Err(e)) match arm

The previous "consumed-fuel" return behavior (Phase 10 and earlier) has
been corrected in Phase 11. The contract is now: per-plugin fuel
budgets decrease by exactly the fuel consumed by successful
invocations.

## Fuel Tracking

### Per-Plugin Fuel Budget

```rust
const MAX_PLUGIN_FUEL_BUDGET: u64 = 10_000_000;
const WASM_FUEL_PER_HOOK: u64 = 1_000_000;
```

**Fuel Logic:**
- Per-plugin fuel budgets tracked in `ModuleCache::fuel_budgets` (DashMap)
- Each hook reserves fuel via `ModuleCache::reserve_fuel()` before execution
- WASM fuel is set on the store via `store.set_fuel()`
- After execution, unused fuel is returned via `ModuleCache::return_fuel()`
- Returns early if budget exhausted
- **Important**: `return_fuel()` initializes new plugin entries with `MAX_PLUGIN_FUEL_BUDGET` (not 0) to ensure proper fuel tracking for plugins that haven't been seen before
- **Fuel leak prevention**: `return_fuel()` is called on ALL exit paths in `execute_wasm_hook()` (success, error, and early returns) to prevent fuel leaks
- **Known issue**: `load_plugin()` at `loader.rs:255-285` has fuel leaks on early error returns (metadata failure, size check failure, compilation failure)

## WASM Module Caching

Module caching with mtime-based invalidation (feature-gated with `plugins`):

```rust
#[cfg(feature = "plugins")]
mod module_cache {
    use dashmap::DashMap;
    use wasmtime::Module;

    pub struct ModuleCache {
        modules: DashMap<String, (Module, u64)>,  // path -> (module, mtime)
        hits: AtomicU64,
        misses: AtomicU64,
    }

    impl ModuleCache {
        pub fn new() -> Self;
        
        pub fn get_or_compile<F>(&self, path: &str, compile_fn: F) -> Option<Module>
        where
            F: FnOnce() -> Option<Module>,
        {
            // Get current file mtime
            let mtime = std::fs::metadata(path).ok()?.modified().ok()?.elapsed().ok()?.as_secs();
            
            // Check cache
            if let Some(entry) = self.modules.get(path) {
                if entry.value().1 == mtime {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    return Some(entry.value().0.clone());  // Cache hit
                }
            }
            
            // Recompile and cache
            if let Some(module) = compile_fn() {
                self.misses.fetch_add(1, Ordering::Relaxed);
                self.modules.insert(path.to_string(), (module.clone(), mtime));
                return Some(module);
            }
            
            None
        }
        
        pub fn stats(&self) -> (u64, u64);  // Returns (hits, misses)
    }
    
    pub static CACHE: once_cell::sync::Lazy<ModuleCache> = 
        once_cell::sync::Lazy::new(ModuleCache::new);
}
```

**Usage in `execute_wasm_hook()`:**
```rust
let module = module_cache::CACHE.get_or_compile(&wasm_path, || {
    let wasm_bytes = std::fs::read(&wasm_path).ok()?;
    Module::new(&ENGINE, &wasm_bytes).ok()
});
```

## Hook Execution Flow

```
AgentLoop (or other component)
  ↓
PluginService::dispatch_hook(ctx)
  ↓
PluginRegistry.hooks_for(hook_type) - get sorted hooks
  ↓
for hook in hooks:
    if plugin enabled:
        execute_hook_with_timeout(plugin_id, ctx)
          ↓
        Loader::execute_wasm_hook(plugin_id, ctx)
          ↓
        timeout(WASM_HOOK_TIMEOUT, ...)
          ↓
        HookResult
  ↓
return final HookResult
```

### Dispatching Hooks

```rust
pub async fn dispatch_hook(&self, ctx: HookContext) -> HookResult {
    let hook_type = ctx.hook_type;
    let hooks = self.registry.hooks_for(hook_type).await;
    
    if hooks.is_empty() {
        return HookResult::ok(ctx.input);
    }
    
    let mut current_input = ctx.input;
    
    for hook in hooks {
        if !self.registry.is_enabled(&hook.plugin_id).await {
            continue;
        }
        
        let hook_ctx = HookContext {
            hook_type,
            input: current_input.clone(),
        };
        
        let result = self.execute_hook_with_timeout(&hook.plugin_id, hook_ctx).await;
        
        match result {
            Ok(res) => {
                if res.blocked {
                    return res;
                }
                if let Some(err) = &res.error {
                    tracing::warn!(plugin = hook.plugin_id, error = err, "hook execution error");
                    return res;
                }
                current_input = res.output;
            }
            Err(_) => return HookResult::ok(current_input),
        }
    }
    
    HookResult::ok(current_input)
}
```

## Plugin Registration

### Loading Plugins

```rust
pub async fn load_plugin(path: &Path) -> Result<LoadedPlugin, LoadError>
```

1. Find `manifest.toml` in plugin directory
2. Parse manifest
3. Find `.wasm` file
4. Return LoadedPlugin

### Installing Plugins

```rust
pub async fn install(&self, source: &str) -> Result<PluginId, InstallError>
```

 Sources:
 - Local path
 - GitHub URL
 - npm package

### Enabling Hooks

```rust
pub fn register_hook(&self, plugin_id: &str, hook: HookType) -> Result<(), RegistryError>
```

## Phase 5: Capability-Based Registry

Phase 5 replaces the hook-only registry with a capability-based system that supports commands, hooks, panels, status widgets, and event subscriptions.

### Runtime Spec

```rust
pub enum PluginRuntimeSpec {
    Builtin,              // Native Rust, no WASM
    Process {             // External process
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    Wasm {                // WASM module (default)
        path: Option<String>,
    },
}
```

### Capability Types

```rust
pub enum PluginCapability {
    Command(PluginCommandSpec),       // Invocable command
    Hook {                            // Lifecycle hook
        hook_type: HookType,
        priority: Option<i32>,
    },
    Panel {                           // TUI panel
        name: String,
        route: String,
    },
    StatusWidget {                    // Status bar widget
        name: String,
    },
    EventSubscription {               // Event listener
        event_type: String,
    },
}

pub struct PluginCommandSpec {
    pub name: String,                 // Unique command name
    pub description: Option<String>,
    pub args: Vec<CommandArg>,        // Argument definitions
}

pub struct CommandArg {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
    pub default: Option<serde_json::Value>,
}
```

### Trust Classes

```rust
pub enum PluginTrustClass {
    Builtin,            // Built-in plugins (highest trust)
    LocalProcess,       // Local process plugins
    SandboxedWasm,      // Sandboxed WASM plugins
    TrustedLocal,       // User-trusted local plugins
}
```

### Registry Methods

```rust
impl PluginRegistry {
    // Query capabilities
    pub fn command(&self, name: &str) -> Option<&PluginCommandSpec>;
    pub fn commands(&self) -> Vec<(&str, &PluginCommandSpec)>;
    pub fn panels(&self) -> Vec<(&str, &str)>;       // (name, route)
    pub fn status_widgets(&self) -> Vec<&str>;
    pub fn event_subscribers(&self, event_type: &str) -> Vec<&str>;

    // Registration
    pub fn register_plugin(&self, plugin_id: &str, manifest: &PluginManifest, trust: PluginTrustClass) -> Result<(), RegistryError>;
    pub fn register_manifest(&self, plugin_id: &str, manifest: &PluginManifest) -> Result<(), RegistryError>;

    // Enable/disable affects capability queries
    pub fn set_enabled(&self, plugin_id: &str, enabled: bool);
    pub fn is_enabled(&self, plugin_id: &str) -> bool;
}
```

### Duplicate Command Name Rejection

When registering a plugin that declares a `Command` capability, the registry checks for name collisions with already-registered commands. If a duplicate name is found, registration fails with `RegistryError::DuplicateCommand(String)`.

### Enable/Disable Semantics

Disabling a plugin excludes its capabilities from all query methods (`command()`, `commands()`, `panels()`, `status_widgets()`, `event_subscribers()`). The plugin remains registered but its capabilities are invisible until re-enabled.

## Built-in Plugins

Built-in plugins are native Rust implementations that don't require WASM. They are registered via `register_builtins()` and use a static handler map for dispatch.

### Handler Registration

```rust
// In builtin/mod.rs
static BUILTIN_HANDLERS: std::sync::LazyLock<
    RwLock<HashMap<String, fn(HookContext) -> HookResult>>,
> = std::sync::LazyLock::new(|| {
    let mut handlers = HashMap::new();
    handlers.insert("copilot".to_string(), copilot::handle_hook);
    handlers.insert("gitlab".to_string(), gitlab::handle_hook);
    handlers.insert("codex".to_string(), codex::handle_hook);
    handlers.insert("poe".to_string(), poe::handle_hook);
    RwLock::new(handlers)
});

pub fn builtin_hook_handler(plugin_name: &str, ctx: HookContext) -> HookResult {
    if let Ok(handlers) = BUILTIN_HANDLERS.read() {
        if let Some(handler) = handlers.get(plugin_name) {
            return handler(ctx);
        }
    }
    HookResult::error(format!("unknown builtin plugin: {}", plugin_name))
}
```

### Available Builtins

- **copilot**: GitHub Copilot authentication provider. Handles `auth` hook by injecting `Bearer {token}` into Authorization header when provider is "copilot" or "github".
- **gitlab**: GitLab authentication provider. Handles `auth` hook by injecting `Bearer {token}` into Authorization header when provider is "gitlab".
- **codex**: OpenAI Codex authentication provider. Handles `auth` hook by injecting `Bearer {token}` into Authorization header when provider is "codex" or "openai".
- **poe**: Poe API authentication provider. Handles `auth` hook by injecting `Bearer {token}` into Authorization header when provider is "poe".

All builtins handle only their specific provider and pass through unchanged for others via `HookResult::ok(ctx.input)`.

## Plugin Service

```rust
pub struct PluginService {
    registry: Arc<PluginRegistry>,
    hook_timeout: Duration,  // default 5 seconds
}
```

### Methods

```rust
impl PluginService {
    pub fn new(registry: Arc<PluginRegistry>) -> Self;
    pub fn with_hook_timeout(mut self, timeout: Duration) -> Self;
    pub fn registry(&self) -> &Arc<PluginRegistry>;

    pub async fn load_and_register(&self, loaded: LoadedPlugin) -> Result<(), LoadError>;
    pub async fn dispatch_hook(&self, ctx: HookContext) -> HookResult;

    // Phase 5: capability-based methods
    pub async fn invoke_command(&self, name: &str, args: serde_json::Value) -> Result<PluginResponse, PluginError>;
    pub fn register_plugin(&self, plugin_id: &str, manifest: &PluginManifest, trust: PluginTrustClass) -> Result<(), RegistryError>;
    pub fn register_manifest(&self, plugin_id: &str, manifest: &PluginManifest) -> Result<(), RegistryError>;

    // Individual dispatch methods for each hook type:
    pub async fn dispatch_auth(&self, input: serde_json::Value) -> HookResult;
    pub async fn dispatch_provider(&self, input: serde_json::Value) -> HookResult;
    pub async fn dispatch_tool_definition(&self, input: serde_json::Value) -> HookResult;
    pub async fn dispatch_tool_execute_before(&self, input: serde_json::Value) -> HookResult;
    pub async fn dispatch_tool_execute_after(&self, input: serde_json::Value) -> HookResult;
    pub async fn dispatch_chat_params(&self, input: serde_json::Value) -> HookResult;
    pub async fn dispatch_chat_headers(&self, input: serde_json::Value) -> HookResult;
    pub async fn dispatch_event(&self, input: serde_json::Value) -> HookResult;
    pub async fn dispatch_config(&self, input: serde_json::Value) -> HookResult;
    pub async fn dispatch_shell_env(&self, input: serde_json::Value) -> HookResult;
    pub async fn dispatch_text_complete(&self, input: serde_json::Value) -> HookResult;
    pub async fn dispatch_session_compacting(&self, input: serde_json::Value) -> HookResult;
    pub async fn dispatch_messages_transform(&self, input: serde_json::Value) -> HookResult;
}
```

**Plugin IDs:**
- WASM plugins: `plugin:{name}` (e.g., `plugin:my-plugin`)
- Built-in plugins: `builtin:{name}` (e.g., `builtin:copilot`)

**Note:** `PluginService` no longer holds a loader or event bus directly. Loading is done via `loader::load_plugin()`, and events use the global `PluginEventBus`.

## Runtime Abstraction (Phase 6)

Phase 6 extracts process execution into a runtime-neutral abstraction. The `src/plugin/runtime/` module defines:

### `PluginRuntime` Trait (`src/plugin/runtime/mod.rs`)

```rust
#[async_trait]
pub trait PluginRuntime: Send + Sync {
    async fn invoke(&self, invocation: PluginInvocation) -> Result<PluginResponse, RuntimeError>;
}
```

### `RuntimeError` Enum

- `Unsupported(String)` — runtime type not supported
- `Spawn(String)` — failed to spawn process
- `Timeout { timeout_ms: u64 }` — execution timed out
- `NonZeroExit { code: i32, stdout: String, stderr: String }` — process exited with error
- `InvalidJson(String)` — response JSON parse failure
- `Io(String)` — I/O error

### `ProcessRuntime` (`src/plugin/runtime/process.rs`)

- Takes `ProcessRuntimeSpec` (command, args, stdin mode, stdout mode, timeout, cwd, env)
- Converts from `ProcessCommandSpec` and `PluginRuntimeSpec::Process`
- Handles spawning, stdin piping, timeout, output capping, stdout mode parsing
- Returns `PluginResponse` for all successful paths

### `BuiltinRuntime` (`src/plugin/runtime/builtin.rs`) — Phase 8

First-class runtime for native Rust builtin plugins, alongside `ProcessRuntime` and `WasmRuntime`.

- `BuiltinHandlerRegistry` maps handler IDs to `fn(HookContext) -> HookResult` functions
- `BuiltinRuntime` implements `PluginRuntime` trait; dispatches `PluginInvocation` through registered handlers
- Adapter functions bridge the hook handler model with the runtime model:
  - `invocation_to_hook_context()` converts `PluginInvocation` → `HookContext`
  - `hook_result_to_plugin_response()` converts `HookResult` → `PluginResponse`
- Plugin ID format: `builtin:<name>` (e.g., `builtin:copilot`)
- Individual builtin modules expose `PLUGIN_ID`, `HANDLER_ID`, and `manifest()` functions
- `builtin_plugin_manifests()` and `builtin_runtime_registry()` provide canonical metadata sources
- `PluginService::with_builtin_runtime()` accepts an `Arc<BuiltinRuntime>` for runtime dispatch

**Hook-only scope (Phase 11):** `BuiltinRuntime` dispatches only
`PluginCapabilityInvocation::Hook` invocations. The following are
rejected with `RuntimeError::Unsupported`:

- `PluginCapabilityInvocation::Command` (no builtin command handler exists)
- `PluginCapabilityInvocation::Panel`, `StatusWidget`, `Event`
- Unknown hook type strings (e.g. `"command"`) — they no longer
  silently fall back to `HookType::Auth`
- Plugin IDs that do not start with the `builtin:` prefix

`make_builtin_info()` in `src/plugin/builtin/mod.rs` skips unknown hook
types via `filter_map` rather than falling back to `HookType::Auth`.

`PluginService::invoke_command()` for a builtin plugin returns
`PluginError::Runtime` if no command runtime handler is registered
(instead of the previous success placeholder).

### Response Type Unification

`codegg_protocol::plugin::PluginResponse` is the single canonical type (re-exported from `plugin/mod.rs`). The local `PluginResponse` in `service.rs` is removed. `PluginDiagnostic` is also unified via re-export from protocol.

## Lifecycle Hooks (Phase 9)

Phase 9 wires plugin lifecycle hooks into core execution paths via a policy-aware dispatcher.

### LifecycleHooks (`src/plugin/lifecycle.rs`)

High-level dispatcher wrapping `PluginService` with policy evaluation. Provides typed methods:
- `emit_event(EventHookInput)` - observation hook, always fails open
- `before_tool_execute(ToolBeforeHookInput)` - may block/modify, policy-gated
- `after_tool_execute(ToolAfterHookInput)` - observation, fails open
- `transform_messages(MessageTransformInput)` - mutating, returns transformed messages
- `shell_env(ShellEnvHookInput)` - mutating, returns env additions/removals

### PluginLifecyclePolicy (`src/plugin/policy.rs`)

Controls which hook types and runtimes are allowed:
- `enable_observation_hooks` (default: true) - Event, After, Config, TextComplete, Compacting
- `enable_mutating_hooks` (default: false) - MessagesTransform, ShellEnv, ChatParams, etc.
- `enable_blocking_hooks` (default: false) - ToolExecuteBefore, Auth
- `allow_process_lifecycle_hooks` (default: false) - Process runtime

### PluginHookOutcome<T>

Typed outcome enum: `Ok(T)`, `Skipped`, `Blocked{reason}`, `Failed{error}`.
Fail-open/fail-closed behavior is controlled by the policy.

### Integration Points

- `PluginService` is created in `CoreDaemon` and passed via `TurnRunInput`
- `AgentLoop::set_plugin_service()` wires it into the agent loop
- Shell env hooks dispatch before process spawn in `ShellRuntime::spawn()`
- Message transform hooks run before provider calls in `AgentLoop::run()`
- Pre/post tool hooks and compaction hooks run in `execute_tool_calls()`

## Built-in Plugin Struct

The `BuiltinPlugin` struct is defined in `src/plugin/builtin/mod.rs`:

```rust
pub struct BuiltinPlugin {
    pub manifest: PluginManifest,
    pub handler: fn(HookContext) -> HookResult,
}
```

**Note:** This struct is NOT re-exported from the main `plugin` module (`src/plugin/mod.rs`). It exists only in the `builtin` submodule and is used internally by `get_builtin_plugins()`.

## TUI Integration

Plugins can register TUI routes and components:

```rust
pub struct TuiPluginRegistry {
    routes: Arc<RwLock<Vec<TuiRoute>>>,
    components: Arc<RwLock<Vec<TuiComponent>>>,
    plugin_configs: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

pub struct TuiRoute {
    pub path: String,
    pub label: String,
    pub plugin_id: String,
    pub icon: Option<String>,
}

pub struct TuiComponent {
    pub name: String,
    pub plugin_id: String,
    pub config: serde_json::Value,
}
```

## Marketplace Service

Local plugin discovery via filesystem scanning:

```rust
pub struct MarketplaceService {
    plugins_dir: PathBuf,  // ~/.local/share/codegg/plugins
}

impl MarketplaceService {
    pub async fn list_local_plugins(&self) -> Vec<MarketplacePlugin>;
    pub async fn search_plugins(&self, query: &str) -> Vec<MarketplacePlugin>;
    pub fn list_official_plugins() -> Vec<MarketplacePlugin>;  // TODO: not implemented
    pub fn list_repository_plugins() -> Vec<MarketplacePlugin>;  // TODO: not implemented
}
```

**Note**: `list_official_plugins()` and `list_repository_plugins()` are TODO stubs that are not yet implemented. They currently return empty vectors.

## Security Considerations

1. **Fuel Limits**: Per-plugin budgets prevent runaway plugins. Unused fuel is returned after execution.
2. **Timeout**: 5 second timeout per hook (configurable via `with_hook_timeout()`), 30 second timeout for WASM hook execution.
3. **Timeout error includes plugin_id**: Error message format is `"{plugin_id}: hook timeout: {err}"`.
4. **Memory Bounds**: Input validated against memory bounds before writing to WASM.
5. **Output Size**: 10MB max output size from WASM.
6. **WASM Size**: 10MB max module size.
7. **Path Traversal**: Archive extraction validates paths stay within destination.
8. **Symlink Protection**: Symlinks not allowed in plugin archives or during installation.

## Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("manifest error: {0}")]
    Manifest(String),
    #[error("wasm error: {0}")]
    Wasm(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("install error: {0}")]
    Install(#[from] InstallError),
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("plugin already installed: {0}")]
    AlreadyInstalled(String),
    #[error("invalid plugin path: {0}")]
    InvalidPath(String),
    #[error("download failed: {0}")]
    DownloadFailed(String),
    #[error("manifest error: {0}")]
    Manifest(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
```

## Configuration

**Plugin Directory:** `~/.local/share/codegg/plugins/` (via `dirs::data_local_dir()`)

**Plugin Structure:**
```
~/.local/share/codegg/plugins/
├── my-plugin/
│   ├── manifest.toml
│   └── plugin.wasm    # or plugin.wasm32-wasi.wasm
└── another-plugin/
    ├── manifest.toml
    └── plugin.wasm
```

**Installation:**
```rust
// From local path
install_from_path(path: &Path) -> Result<PathBuf, InstallError>

// From URL (downloads .wasm or .tar.gz)
install_from_url(url: &str) -> Result<PathBuf, InstallError>

// Uninstall
uninstall(plugin_name: &str) -> Result<(), InstallError>

// Get plugins directory
plugins_dir() -> PathBuf  // ~/.local/share/codegg/plugins
```

## Configuration

Plugins configured in `config.json`:

```json
{
  "plugins": {
    "enabled": ["my-plugin"],
    "directory": "~/.config/opencode/plugins"
  }
}
```

## Protocol DTOs (Phase 1-10)

`codegg-protocol` now ships frontend-neutral plugin and UI protocol types in `crates/codegg-protocol/src/ui.rs` and `crates/codegg-protocol/src/plugin.rs`. These are available as `codegg_protocol::ui` and `codegg_protocol::plugin`.

Key types:
- `UiNode` — Display tree nodes (Text, Markdown, Code, Table, KeyValue, Progress, Container, Empty, Unsupported)
- `UiEffect` — Plugin side effects (EmitChat, ShowToast, OpenDialog/CloseDialog, OpenPanel/UpdatePanel/ClosePanel, AddStatusItem/UpdateStatusItem/RemoveStatusItem)
- `UiEffectEnvelope` / `UiEffectSource` — Typed transport wrappers for frontend-neutral effect delivery
- `PluginManifestDto` — Plugin metadata with runtime, capabilities, permissions
- `PluginInvocation` — Request to invoke a plugin capability
- `PluginResponse` — Response with effects, data, and diagnostics
- `PluginPermissionSet` / `FilesystemPermission` — Declared permissions

These are protocol-only types. They do not execute plugins or render UI. Phase 2 (TUI Renderer Adapter) consumes them via `PluginUiState` and `PluginUiRenderer`. Phase 3 adds generic `TuiCommand` plugin variants and `src/tui/commands/plugins.rs` for response application. Phase 5 redesigns the manifest and registry to be capability-based: `PluginManifestDto` now carries `runtime: PluginRuntimeSpec`, `capabilities: Vec<PluginCapability>`, and `permissions: Option<PluginPermissionSet>`, replacing the legacy hook-only model. The registry exposes capability queries (`commands()`, `panels()`, `status_widgets()`, `event_subscribers()`) and rejects duplicate command names at registration time.

### Phase 10: Frontend-Neutral UI Events

Phase 10 adds protocol-level transport for plugin UI effects so they can be delivered to any frontend without TUI-specific coupling.

- **`UiEffectEnvelope`** / **`UiEffectSource`** in `codegg_protocol::ui` wrap `UiEffect` with session and plugin metadata for typed transport.
- **`CoreEvent::PluginUiEffect`** and **`TuiMessage::PluginUiEffect`** carry the envelope through the event bus and command channel respectively.
- **`HookResult.effects`** and **`PluginHookOutcome::Ok(T, Vec<UiEffect>)`** allow hooks to return UI effects alongside their normal result.
- **`ClientCapabilities`** includes `plugin_ui_dialogs`, `plugin_ui_panels`, and `plugin_ui_status_widgets` flags for frontend capability negotiation.

### Phase 15: Multi-Frontend Readiness

Phase 15 turns the Phase 10 envelope transport into a stable multi-frontend contract with strict validation and durable snapshots.

- **`UiLimits`** (`balanced()` / `text_only()`) defines validated caps: `max_effects_per_response`, `max_effect_bytes`, `max_node_depth`, `max_table_rows`, `max_table_columns`, `max_string_len`, `max_panels_per_plugin`, `max_status_items_per_plugin`, `max_open_dialogs_global`, `max_snapshot_body_bytes`. `UiLimits::validate_effect(effect)` and `validate_effects(effects)` are the canonical validation gates.
- **`UiValidationError`** (`TooManyEffects`, `EffectTooLarge`, `StringTooLong`, `TooDeep`, `TableTooLarge`) surfaces structured reasons for rejections.
- **`App::apply_plugin_ui_envelope(envelope)`** is the canonical dispatch entry point for both local TUI commands and remote WebSocket events. It runs the session guard, validates against `UiLimits::balanced()`, enforces plugin surface-ownership rules, and delegates to `App::apply_plugin_ui_effect(effect, plugin_id_opt)`. Use this for any new effect-delivery path.
- **`App::validate_plugin_ui_effects(effects)`** is the batch validator used by lifecycle hooks and the event bridge before publishing effects on the bus.
- **`RemotePanelView`** and **`RemoteStatusItemView`** carry optional `source_plugin_id` and `body: Option<UiNode>` fields. The remote snapshot builder includes the body only when its serialized size ≤ `SNAPSHOT_BODY_LIMIT` (16 KiB). Helper `plugin_id_from_surface_id(id)` extracts the owning plugin from the surface id (`<plugin>:<command>` form; `command:local:...` has no plugin owner). Legacy snapshots deserialize cleanly via `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- Wire versions: `PROTOCOL_VERSION = 2` for core events, `REMOTE_TUI_PROTOCOL_VERSION = 3` for the remote TUI channel.

### Phase 11: Corrective Hardening

Four correctness fixes close gaps in the plugin UI/runtime integration:

1. **WASM fuel accounting** — `return_unused_fuel()` returns the unused
   portion of the reservation, not the consumed amount. Per-plugin
   budgets decrease by exactly the consumed fuel. On error, the full
   reserved amount is returned.
2. **Builtin runtime strictness** — `BuiltinRuntime` rejects
   `PluginCapabilityInvocation::Command` and unknown hook type strings
   with `RuntimeError::Unsupported`. `make_builtin_info()` skips unknown
   hook types instead of falling back to `HookType::Auth`.
   `PluginService::invoke_command()` for a builtin plugin returns
   `PluginError::Runtime` if no command runtime handler is registered.
3. **EmitChat visibility** — `App::apply_plugin_ui_effect()` renders
   `UiEffect::EmitChat` to the toast / info-dialog surface directly.
   Short blocks toast, long blocks open the scrollable info dialog.
   Both `ChatFormat::Plain` and `ChatFormat::Markdown` are lowered to
   line-based text — markdown links and embedded escape sequences are
   not executed. Output is **not** added to the model-visible chat
   transcript.
4. **Registry snapshot filtering** — `PluginRegistry::enabled_plugin_ids()`
   acquires a single read guard on `plugins` to snapshot the set of
   enabled plugin ids. Capability queries (`hooks_for`, `command`,
   `commands`, `panels`, `status_widgets`, `event_subscribers`) and the
   duplicate checks in `set_enabled()` filter against this snapshot
   instead of using `try_read()`. Visibility depends only on actual
   enabled state, not lock contention. The regression test
   `registry_does_not_use_try_read_as_code` prevents the bug from
   being reintroduced.
- **`degrade_node_to_text()`** converts unsupported `UiNode` variants to plain text for frontends that lack full rendering support.

### Phase 12: Plugin Management UX

First-class slash commands for local plugin management and observability.

**Files added:**
- `src/plugin/management.rs` — `PluginManager`, `PluginManagementView`, `PluginDoctorReport`, `resolve_plugin_selector()`
- `src/plugin/management_ui.rs` — `plugins_table()`, `plugin_info_node()`, `doctor_report_node()` returning `UiNode`
- `src/tui/commands/plugin_management.rs` — TUI command handlers

**Commands:**
| Command | Description |
|---------|-------------|
| `/plugins` (aliases `/plugin-list`, `/plugin-ls`) | List installed and built-in plugins |
| `/plugin-info <id>` | Show plugin runtime, capabilities, trust, diagnostics |
| `/plugin-enable <id>` | Enable a plugin |
| `/plugin-disable <id>` | Disable a plugin |
| `/plugin-doctor [id]` | Diagnose plugin configuration and runtime health |
| `/plugin-remove <id>` | Remove a local installed plugin |
| `/plugin-install <path>` | Install a plugin from a local path |

**Selector resolution order:**
1. Exact plugin id
2. Exact manifest name
3. Unique prefix match on id (case-insensitive)
4. Unique prefix match on name (case-insensitive)

**Safety semantics:**
- Enable/disable persists to `disabled_plugins.toml` in the plugins directory
- Remove only deletes from the canonical plugin install directory
- Install validates manifests before copying and refuses to overwrite existing plugins
- Doctor checks are read-only and never execute plugin code

**Key types:** `PluginManager`, `PluginManagementView`, `PluginDoctorReport`

### Security Policy (Phase 12)

`PluginPolicy` in `src/plugin/policy.rs` is a composite policy combining five sub-policies with conservative defaults:

- `PluginLifecyclePolicy` — observation hooks allowed; mutating/blocking/process hooks denied
- `PluginUiPolicy` — dialog/toast allowed; panel/status effects denied
- `PluginPermissionPolicy` — undeclared capabilities denied
- `PluginInstallPolicy` — env passthrough denied
- `PluginRuntimePolicy` — secrets denied; auth-hook requires high trust

`PluginService` accepts `Option<Arc<PluginPolicy>>` via `with_policy()`. When set, `invoke_command()` checks command declarations and `dispatch_hook()` checks hook type + trust class before dispatch. Policy is opt-in: when absent, all checks pass (backward compatible).

`src/plugin/permission.rs` provides `PolicyDecision` (Allow/Deny/Degrade) with four check functions:
- `check_invocation_allowed` — validates command/hook invocations against manifest declarations
- `check_ui_effect_allowed` — gates UI effects by output surface declarations
- `check_lifecycle_hook_allowed` — validates hook type, trust class, auth-hook high-trust requirement
- `check_secret_access_allowed` — validates secret access against declared permissions

Process plugins are local executable code. They are not sandboxed. They are suitable for explicit user-invoked local commands, not silent lifecycle interception by default.

### Phase 13: SDKs and Examples

`examples/plugins/` ships six runnable reference plugins plus two helper SDKs. Use them as templates instead of reverse-engineering protocol types. Each example is independent (own `Cargo.toml` with `[workspace]` isolation or its own directory); the root workspace is unmodified.

**Process examples** (project-local commands via `command/*.md` frontmatter):

| Path | Demonstrates |
|------|--------------|
| `process-quota-text/` | Zero-SDK path. Plain stdout; auto-detected as EmitChat. |
| `process-quota-json/` | Reads `PluginInvocation` from stdin; writes `PluginResponse` with effects to stdout. |

**WASM examples** (install to the platform plugins directory):

| Path | Demonstrates |
|------|--------------|
| `wasm-command-table/` | Modern `codegg_plugin_invoke` ABI; returns OpenDialog + Table. |
| `wasm-hook-message-transform/` | Observation-only `event_subscription` hook (default-policy-permitted). |
| `wasm-status-widget/` | Panel + status widget via separate capabilities. |

**Documentation-only example:**

| Path | Demonstrates |
|------|--------------|
| `builtin-reference/` | Walkthrough of the `BuiltinRuntime` pattern for codegg contributors. |

**SDKs:**

| Path | Provides |
|------|----------|
| `sdk-python/` (stdlib-only, 24 tests) | `read_invocation`, `write_response`, builders for every `UiEffect`/`UiNode` variant. |
| `sdk-rust/` (11 tests, 1 wasm-only `#[ignore]`) | `codegg_plugin!` macro exporting `allocate`/`deallocate`/`codegg_plugin_invoke`; typed builders. Uses a 1 MiB bump allocator. |

**Wire format reference:** `crates/codegg-protocol/src/plugin.rs` (`PLUGIN_PROTOCOL_VERSION = 1`) and `crates/codegg-protocol/src/ui.rs`. Both SDKs re-export protocol types by path dependency so the format cannot drift.

**Build / validation:**

```bash
PYTHONPATH=examples/plugins/sdk-python \
  python3 -m unittest discover examples/plugins/sdk-python/tests -v

cargo test --manifest-path examples/plugins/sdk-rust/Cargo.toml

rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown \
  --manifest-path examples/plugins/wasm-command-table/Cargo.toml --release
```

**WASM ABI summary** (modern):

| Export | Signature | Required |
|--------|-----------|----------|
| `memory` | linear memory | yes |
| `allocate` | `(i32) -> i32` | yes |
| `deallocate` | `(i32, i32)` | optional |
| `codegg_plugin_invoke` | `(i32, i32) -> i64` | yes (modern) |

The packed i64 return is `(response_ptr << 32) | response_len`. The host reads `len` bytes from `ptr`, deserializes JSON, then optionally calls `deallocate`.

**Manual testing for the JSON process example:**

```bash
cat examples/plugins/process-quota-json/sample_invocation.json | \
  python3 examples/plugins/process-quota-json/scripts/quota_json.py | \
  python3 -m json.tool
```

### Phase 14: TUI Component Modularization

`UiNodeRenderer` (`src/tui/components/ui_node_renderer.rs`) is the canonical `UiNode` lowering adapter — both plugin and first-party informational surfaces flow through it. The legacy `PluginUiRenderer` is a compat alias at `src/tui/components/plugin_renderer.rs`. `UiNodeDialog` (`src/tui/components/dialogs/ui_node.rs`) is a generic scrollable dialog that accepts a `UiNode` directly and reuses `DialogType::Plugin` in the focus manager.

First-party builders live in `src/tui/ui_builders/`: `stats.rs` (`stats_node` + `TaskSummaryView` DTO), `plugins.rs` (re-export shim for `management_ui` builders), `shell.rs` (`shell_detail_node`). The plugin management builders (`plugins_table`, `plugin_info_node`, `doctor_report_node`, `node_to_lines`) remain in `src/plugin/management_ui.rs` — the `ui_builders/plugins.rs` shim gives first-party callers a clean import path.

Renderer hardening: empty-table fallback message, key-value alignment to longest key, column-width cap of 60 chars with `…` truncation, ANSI/CSI and control-character sanitization on line output, safe `total=0` percentage. Interactive components (permission/question/command-palette/file-diff/source-preview/tree/security-review) MUST NOT be migrated to `UiNode` — keep them as native ratatui components.