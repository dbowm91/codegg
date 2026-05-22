---
name: plugin
description: Plugin system, WASM execution, hooks, fuel tracking
version: 1.0.0
tags:
  - plugin
  - wasm
  - hooks
  - fuel
  - wasmtime
---

# Plugin System Guide

This skill covers the plugin system in opencode-rs, which enables extending the agent with WASM-based plugins and hooks.

## Architecture Overview

```
Plugin System
├── PluginLoader (WASM execution)
├── HookRegistry (hook registration)
├── FuelTracking (resource management)
└── PluginService (management APIs)
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
├── api.rs           # External API for plugin management
├── tui.rs           # TUI dialog for plugin management
├── event_bus.rs     # Event bus integration
└── builtin/         # Built-in plugins
    ├── mod.rs
    ├── copilot.rs   # GitHub Copilot integration
    ├── codex.rs     # Anthropic Codex integration
    ├── gitlab.rs    # GitLab MR integration
    └── poe.rs       # Poe API integration
```

## Hook Types

Hooks are extension points in the agent lifecycle:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, Display, EnumIter)]
pub enum HookType {
    Auth,              # Authentication
    Provider,          # Provider selection
    ToolDefinition,    # Tool definition modification
    ToolExecuteBefore, # Before tool execution
    ToolExecuteAfter,  # After tool execution
    ChatParams,       # Chat parameters
    ChatHeaders,      # Chat headers
    Event,            # Event handling
    Config,           # Configuration
    ShellEnv,         # Shell environment
    TextComplete,     # Text completion
    SessionCompacting,# Session compaction
    MessagesTransform,# Message transformation
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

Plugins require `manifest.toml`:

```toml
name = "my-plugin"
version = "1.0.0"
description = "My plugin description"
author = "Author Name"
hooks = ["tool.execute.before", "tool.execute.after"]
```

### Manifest Structure

```rust
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub hooks: Vec<String>,
}
```

## WASM Execution

Plugins execute via WASMtime with module caching:

```rust
pub async fn execute_wasm_hook(plugin_id: &str, ctx: HookContext) -> HookResult {
    use wasmtime::{Config, Engine, Linker, Module, Store};

    static ENGINE: once_cell::sync::Lazy<Engine> = once_cell::sync::Lazy::new(|| {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.wasm_backtrace_details(WasmBacktraceDetails::Disable);
        Engine::new(&config).unwrap()
    });

    // Check fuel budget
    check_and_reset_fuel_budget();
    let current_budget = PLUGIN_FUEL_BUDGET.load(Ordering::Relaxed);
    if current_budget >= MAX_PLUGIN_FUEL_BUDGET {
        return HookResult::ok(ctx.input);  // Budget exhausted
    }

    // Get module from cache (or compile if not cached/mtime changed)
    let module = module_cache::CACHE.get_or_compile(&wasm_path, || {
        let wasm_bytes = std::fs::read(&wasm_path).ok()?;
        Module::new(&ENGINE, &wasm_bytes).ok()
    });

    // Create store with fuel
    let mut store = Store::new(&ENGINE, ());
    let fuel_for_this_call = WASM_FUEL_PER_HOOK.min(MAX_PLUGIN_FUEL_BUDGET - current_budget);
    store.set_fuel(fuel_for_this_call).ok();

    // Instantiate and call hook function with timeout
    let linker = Linker::new(&ENGINE);
    let instance = linker.instantiate(&mut store, &module)?;
    
    // Map HookType to WASM function name
    let func_name = match ctx.hook_type {
        HookType::Auth => "on_auth",
        HookType::ToolExecuteBefore => "on_tool_execute_before",
        // ... etc
    };
    
    timeout(WASM_HOOK_TIMEOUT, async {
        let func = instance.get_func(&mut store, func_name)?;
        func.call(&mut store, &[...], &mut result)?;
        Ok(())
    }).await?;
}
```

## Fuel Tracking

### Global Fuel Budget

```rust
static PLUGIN_FUEL_BUDGET: AtomicU64 = AtomicU64::new(10_000_000);
static PLUGIN_FUEL_LAST_RESET: AtomicU64 = AtomicU64::new(0);

const MAX_PLUGIN_FUEL_BUDGET: u64 = 10_000_000;
const WASM_FUEL_PER_HOOK: u64 = 1_000_000;
const FUEL_RESET_INTERVAL_SECS: u64 = 60;
```

**Fuel Logic:**
- Per-plugin fuel budgets tracked in `ModuleCache::fuel_budgets` (DashMap)
- Each hook reserves fuel via `ModuleCache::reserve_fuel()` before execution
- WASM fuel is set on the store via `store.set_fuel()`
- After execution, unused fuel is returned via `ModuleCache::return_fuel()`
- Budget resets every 60 seconds via `check_and_reset_fuel_budget()`
- Returns early if budget exhausted
- **Important**: `return_fuel()` initializes new plugin entries with `MAX_PLUGIN_FUEL_BUDGET` (not 0) to ensure proper fuel tracking for plugins that haven't been seen before

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

## Built-in Plugins

### Copilot

GitHub Copilot integration.

### Codex

Anthropic Codex integration.

### GitLab

GitLab MR integration.

### Poe

Poe API integration.

## Plugin Service

```rust
pub struct PluginService {
    registry: Arc<PluginRegistry>,
    hook_timeout: Duration,
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
}
```

**Note:** `PluginService` no longer holds a loader or event bus directly. Loading is done via `loader::load_plugin()`, and events use the global `PluginEventBus`.

## TUI Integration

The TUI can manage plugins via the PluginDialog:

```rust
pub struct PluginDialog {
    plugins: Vec<PluginInfo>,
    selected: usize,
    scroll: CenteredScroll,
}
```

## Security Considerations

1. **Fuel Limits**: Per-plugin budgets prevent runaway plugins. Unused fuel is returned after execution.
2. **Timeout**: 5 second timeout per hook (configurable via `with_hook_timeout()`).
3. **Timeout error includes plugin_id**: Error message format is `"{plugin_id}: hook timeout: {err}"`.
4. **Memory Bounds**: Input validated against memory bounds
5. **Output Size**: 10MB max output size
6. **WASM Size**: 10MB max module size

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
}

pub enum InstallError {
    #[error("download error: {0}")]
    Download(String),
    #[error("invalid source: {0}")]
    InvalidSource(String),
}
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