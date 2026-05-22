# Plugin Module

The `plugin` module provides a WASM-based plugin system for extending agent capabilities with built-in and custom plugins.

## Overview

**Location**: `src/plugin/`

**Key Responsibilities**:
- WASM plugin loading and execution via Wasmtime
- Plugin manifest parsing (TOML format)
- Hook system for agent lifecycle events
- Built-in plugin support (copilot, gitlab, codex, poe)
- Plugin installation and registry
- TUI extensions for plugins
- Marketplace for local plugin discovery

## Technology

Uses **Wasmtime** runtime for WASM execution (feature-gated with `plugins` flag, not `plugin`).

## Project Structure

```
src/plugin/
├── mod.rs              # Main module, exports
├── loader.rs           # WASM loading, execution, module caching, fuel tracking
├── hooks.rs            # HookType enum, HookContext, HookResult, HookRegistration
├── registry.rs         # PluginRegistry, PluginInfo
├── manifest.rs         # PluginManifest, HookSpec (TOML parsing)
├── service.rs          # PluginService, hook dispatch methods
├── install.rs          # Installation from path/URL, uninstallation
├── api.rs              # ApiVersion, Stability, API types
├── tui.rs              # TuiPluginRegistry, TuiRoute, TuiComponent
├── event_bus.rs        # PluginEventBus, PluginEventSubscription
├── marketplace.rs      # MarketplaceService for local plugin discovery
└── builtin/            # Built-in native Rust plugins
    ├── mod.rs          # BuiltinPlugin, handler registry, dispatch
    ├── copilot.rs      # GitHub Copilot auth provider
    ├── gitlab.rs       # GitLab auth provider
    ├── codex.rs        # Anthropic Codex integration
    └── poe.rs          # Poe API integration
```

## Key Types

### PluginManifest

```rust
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    #[serde(default)]
    pub hooks: Vec<HookSpec>,
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

pub struct HookSpec {
    #[serde(rename = "type")]
    pub hook_type: String,  // dot notation, e.g., "tool.execute.before"
    pub priority: Option<i32>,
}
```

### LoadedPlugin

```rust
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub wasm_path: PathBuf,
    pub plugin_dir: PathBuf,
}
```

### HookType

Hook types use dot notation for serialization:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, Display, EnumIter)]
#[strum(serialize_all = "snake_case")]
pub enum HookType {
    Auth,              // "auth"
    Provider,          // "provider"
    ToolDefinition,    // "tool.definition"
    ToolExecuteBefore, // "tool.execute.before"
    ToolExecuteAfter,  // "tool.execute.after"
    ChatParams,        // "chat.params"
    ChatHeaders,       // "chat.headers"
    Event,             // "event"
    Config,            // "config"
    ShellEnv,          // "shell.env"
    TextComplete,      // "text.complete"
    SessionCompacting, // "session.compacting"
    MessagesTransform, // "messages.transform"
}
```

### HookContext

```rust
pub struct HookContext {
    pub hook_type: HookType,
    pub input: serde_json::Value,
}
```

### HookResult

```rust
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

## Components

### loader.rs - WASM Loading

WASM loading with module caching and fuel tracking:

```rust
pub async fn load_plugin(path: &Path) -> Result<LoadedPlugin, LoadError>

pub async fn execute_wasm_hook(plugin_id: &str, ctx: HookContext) -> HookResult
```

**Module Cache:**
```rust
#[cfg(feature = "plugins")]
mod module_cache {
    pub struct ModuleCache {
        modules: DashMap<String, (Module, u64)>,  // path -> (module, mtime)
        hits: AtomicU64,
        misses: AtomicU64,
        fuel_budgets: DashMap<String, AtomicU64>,
    }

    pub fn get_or_compile<F>(&self, path: &str, compile_fn: F) -> Option<Module>;
    pub fn get_plugin_fuel(&self, plugin_id: &str) -> u64;
    pub fn reserve_fuel(&self, plugin_id: &str, fuel_needed: u64) -> Option<u64>;
    pub fn return_fuel(&self, plugin_id: &str, fuel: u64);
}
```

### manifest.rs - Manifest Parsing

Parses plugin metadata from `manifest.toml`:

```rust
pub fn parse_manifest(toml: &str) -> Result<PluginManifest>;
```

### hooks.rs - Hook System

Hook registration and types:

```rust
pub struct HookRegistration {
    pub plugin_id: String,
    pub hook_type: HookType,
    pub priority: i32,
}

impl HookType {
    pub fn as_str(&self) -> &'static str;  // Returns dot notation
    pub fn parse(s: &str) -> Option<Self>;  // Parses dot notation
}
```

### service.rs - Plugin Service

Main service for hook dispatch:

```rust
pub struct PluginService {
    registry: Arc<PluginRegistry>,
    hook_timeout: Duration,  // default 5 seconds
}

impl PluginService {
    pub fn new(registry: Arc<PluginRegistry>) -> Self;
    pub fn with_hook_timeout(mut self, timeout: Duration) -> Self;

    pub async fn load_and_register(&self, loaded: LoadedPlugin) -> Result<(), LoadError>;
    pub async fn dispatch_hook(&self, ctx: HookContext) -> HookResult;

    // Individual dispatch methods
    pub async fn dispatch_auth(&self, input: serde_json::Value) -> HookResult;
    pub async fn dispatch_tool_execute_before(&self, input: serde_json::Value) -> HookResult;
    pub async fn dispatch_tool_execute_after(&self, input: serde_json::Value) -> HookResult;
    // ... other hook types
}
```

### registry.rs - Plugin Registry

```rust
pub struct PluginInfo {
    pub id: String,
    pub manifest: PluginManifest,
    pub path: PathBuf,
    pub enabled: bool,
    pub error: Option<String>,
}

pub struct PluginRegistry {
    plugins: RwLock<HashMap<String, PluginInfo>>,
    hooks: RwLock<Vec<HookRegistration>>,
}

impl PluginRegistry {
    pub async fn register(&self, info: PluginInfo, hook_specs: Vec<HookRegistration>);
    pub async fn unregister(&self, id: &str);
    pub async fn hooks_for(&self, hook_type: HookType) -> Vec<HookRegistration>;
    pub async fn is_enabled(&self, id: &str) -> bool;
    pub async fn set_enabled(&self, id: &str, enabled: bool);
}
```

### install.rs - Plugin Installation

```rust
pub fn plugins_dir() -> PathBuf;  // ~/.local/share/codegg/plugins

pub async fn install_from_path(path: &Path) -> Result<PathBuf, InstallError>;
pub async fn install_from_url(url: &str) -> Result<PathBuf, InstallError>;
pub async fn uninstall(plugin_name: &str) -> Result<(), InstallError>;
```

### tui.rs - TUI Extensions

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

### builtin/mod.rs - Built-in Plugins

Native Rust plugin handlers:

```rust
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

pub fn builtin_hook_handler(plugin_name: &str, ctx: HookContext) -> HookResult;

pub async fn register_builtins(registry: &PluginRegistry);
```

### marketplace.rs - Marketplace Service

```rust
pub struct MarketplaceService {
    plugins_dir: PathBuf,
}

impl MarketplaceService {
    pub async fn list_local_plugins(&self) -> Vec<MarketplacePlugin>;
    pub async fn search_plugins(&self, query: &str) -> Vec<MarketplacePlugin>;
    pub fn list_official_plugins() -> Vec<MarketplacePlugin>;  // TODO
    pub fn list_repository_plugins() -> Vec<MarketplacePlugin>;  // TODO
}
```

## Plugin Directory Structure

```
~/.local/share/codegg/plugins/     (via dirs::data_local_dir())
├── my-plugin/
│   ├── manifest.toml
│   └── plugin.wasm
└── another-plugin/
    ├── manifest.toml
    └── plugin.wasm
```

### manifest.toml Example

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
setting = "value"
```

## Hook Flow

```
AgentLoop (or other component)
  │
  ▼
PluginService::dispatch_tool_execute_before(input)
  │
  ▼
PluginService::dispatch_hook(ctx)
  │
  ├──► PluginRegistry::hooks_for(hook_type) → Vec<HookRegistration>
  │    (sorted by priority)
  │
  └──► For each hook registration:
          │
          ├──► Check if plugin is enabled?
          │
          └──► execute_hook_with_timeout(plugin_id, ctx)
                  │
                  ├─► If builtin:* → builtin_hook_handler(name, ctx)
                  │        │
                  │        └─► Returns HookResult directly
                  │
                  └─► Else (WASM plugin):
                          └─► execute_wasm_hook(plugin_id, ctx)
                                  │
                                  ├─► Reserve fuel
                                  ├─► Get/compile WASM module
                                  ├─► Allocate memory, write input
                                  ├─► Call hook function
                                  ├─► Read output, return fuel
                                  └─► Return HookResult
```

## Fuel Tracking

Global and per-plugin fuel budgets:

```rust
// Constants
const MAX_WASM_SIZE: usize = 10 * 1024 * 1024;  // 10MB
const WASM_FUEL_PER_HOOK: u64 = 1_000_000;
const WASM_HOOK_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PLUGIN_FUEL_BUDGET: u64 = 10_000_000;
const FUEL_RESET_INTERVAL_SECS: u64 = 60;

// Global budget (auto-resets every 60s)
static PLUGIN_FUEL_BUDGET: AtomicU64 = AtomicU64::new(10_000_000);
static PLUGIN_FUEL_LAST_RESET: AtomicU64 = AtomicU64::new(0);

// Per-plugin budgets (DashMap in ModuleCache)
```

**Fuel Logic:**
- Each hook reserves fuel before execution
- WASM fuel set on Store via `store.set_fuel()`
- Unused fuel returned after execution
- Budget exhausted → returns `HookResult::ok(ctx.input)` early

## Security

- **Fuel Limits**: Per-plugin budgets prevent infinite loops
- **Timeout**: 5s per hook dispatch, 30s for WASM execution
- **Memory Bounds**: Input validated before WASM memory write
- **Output Size**: 10MB max from WASM output
- **WASM Size**: 10MB max module size
- **Path Traversal**: Archive extraction validates paths
- **Symlinks**: Not allowed in archives or installation

## Feature Flag

Requires `plugins` feature in `Cargo.toml`:

```toml
[features]
plugins = ["dep:wasmtime", "dep:wasmtime-cache", "dep:wasmtime-wasi"]
```

## Plugin IDs

- **WASM plugins**: `plugin:{name}` (e.g., `plugin:my-plugin`)
- **Built-in plugins**: `builtin:{name}` (e.g., `builtin:copilot`)

## WASM Plugin Contract

Plugins must export:
- `memory`: Wasmtime memory
- `allocate(ptr, len) -> ptr`: Allocate memory in plugin
- `deallocate(ptr, len)`: Optional, free memory
- Hook functions: `on_auth`, `on_tool_execute_before`, etc.

Hook function signature:
```rust
// Input: (input_ptr: i32, input_len: i32)
// Output: result_ptr i32 (points to serialized HookResponse)

#[derive(serde::Deserialize)]
struct WasmHookResponse {
    output: serde_json::Value,
    blocked: Option<bool>,
    error: Option<String>,
}
```

## See Also

- [.opencode/skills/plugin/SKILL.md](../.opencode/skills/plugin/SKILL.md) - Detailed plugin skill guide
- [hooks.md](hooks.md) - Hook system details (external hooks)
- [agent.md](agent.md) - AgentLoop integration
- [tool.md](tool.md) - Tool execution