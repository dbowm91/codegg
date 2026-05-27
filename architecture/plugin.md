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
- Fuel tracking to prevent infinite loops in WASM plugins

## Technology

Uses **Wasmtime** runtime for WASM execution (feature-gated with `plugins` flag).

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
└── builtin/            # Built-in native Rust plugins
    ├── mod.rs          # BuiltinPlugin, handler registry, dispatch
    ├── copilot.rs      # GitHub Copilot auth provider
    ├── gitlab.rs       # GitLab auth provider
    ├── codex.rs        # OpenAI Codex integration
    └── poe.rs          # Poe API integration
```

## Key Types

### PluginManifest

Parsed from `manifest.toml` in each plugin directory:

```rust
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    #[serde(default)]
    pub hooks: Vec<HookSpec>,        // Hook specifications
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,  // Plugin configuration
}

pub struct HookSpec {
    #[serde(rename = "type")]
    pub hook_type: String,           // e.g., "tool.execute.before"
    pub priority: Option<i32>,        // Lower = earlier execution
}
```

### HookType

All hook types use snake_case serialization (via `strum` derive):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, Display, EnumIter)]
#[strum(serialize_all = "snake_case")]
pub enum HookType {
    Auth,              // "auth" - Authentication provider injection
    Provider,          // "provider" - Provider selection/modification
    ToolDefinition,     // "tool.definition" - Modify tool definitions
    ToolExecuteBefore,  // "tool.execute.before" - Pre-tool execution
    ToolExecuteAfter,   // "tool.execute.after" - Post-tool execution
    ChatParams,        // "chat.params" - Chat parameters
    ChatHeaders,       // "chat.headers" - HTTP headers for chat
    Event,             // "event" - General event handling
    Config,            // "config" - Configuration hooks
    ShellEnv,          // "shell.env" - Shell environment
    TextComplete,      // "text.complete" - Text completion
    SessionCompacting,  // "session.compacting" - Before session compaction
    MessagesTransform,  // "messages.transform" - Transform messages
}
```

### HookContext

Passed to every hook:

```rust
pub struct HookContext {
    pub hook_type: HookType,
    pub input: serde_json::Value,  // JSON input data (varies by hook type)
}
```

### HookResult

Returned by every hook handler:

```rust
pub struct HookResult {
    pub output: serde_json::Value,  // Transformed output (passed to next hook)
    pub blocked: bool,              // If true, stops hook chain
    pub error: Option<String>,      // Error message if any
}

impl HookResult {
    pub fn ok(output: serde_json::Value) -> Self;
    pub fn blocked() -> Self;
    pub fn error(msg: impl Into<String>) -> Self;
}
```

## Components

### loader.rs - WASM Loading and Fuel Tracking

**Location**: `src/plugin/loader.rs`

The loader handles WASM plugin execution with module caching and fuel tracking.

**Key Functions:**

```rust
pub async fn load_plugin(path: &Path) -> Result<LoadedPlugin, LoadError>
pub async fn execute_wasm_hook(plugin_id: &str, ctx: HookContext) -> HookResult
```

**Module Cache** (`loader.rs:103-218`):

```rust
#[cfg(feature = "plugins")]
mod module_cache {
    pub struct ModuleCache {
        modules: DashMap<String, (Module, u64)>,  // path -> (WASM module, mtime)
        hits: AtomicU64,
        misses: AtomicU64,
        fuel_budgets: DashMap<String, AtomicU64>,  // plugin_id -> remaining fuel
    }

    impl ModuleCache {
        pub fn get_or_compile<F>(&self, path: &str, compile_fn: F) -> Option<Module>;
        pub fn get_plugin_fuel(&self, plugin_id: &str) -> u64;
        pub fn reserve_fuel(&self, plugin_id: &str, fuel_needed: u64) -> Option<u64>;
        pub fn return_fuel(&self, plugin_id: &str, fuel: u64);
    }

    pub static CACHE: once_cell::sync::Lazy<ModuleCache> = once_cell::sync::Lazy::new(ModuleCache::new);
}
```

**Fuel Tracking Constants** (`loader.rs:8-15`):

```rust
const MAX_WASM_SIZE: usize = 10 * 1024 * 1024;       // 10MB max WASM module
const WASM_FUEL_PER_HOOK: u64 = 1_000_000;            // 1M fuel per hook call
const WASM_HOOK_TIMEOUT: Duration = Duration::from_secs(30);  // 30s timeout for WASM exec
const MAX_PLUGIN_FUEL_BUDGET: u64 = 10_000_000;        // 10M initial budget per plugin
```

**Fuel Flow** (`loader.rs:222-519`):

1. **Reserve Fuel** (line 244): `module_cache::CACHE.reserve_fuel(plugin_id, fuel_for_this_call)`
2. **Execute WASM** with `store.set_fuel(fuel_reserved)`
3. **Return Fuel** on:
   - Normal completion: consumed fuel (reserved - remaining)
   - Early returns (lines 258, 270, 286, 329, 338, 353, 371, 386, 406, 431): full `fuel_reserved`
   - Timeout: full `fuel_reserved` (line 510)
   - WASM execution error: full `fuel_reserved` (line 505)

All early error returns at lines 255-285 correctly return fuel before exiting.

### service.rs - Plugin Service and Hook Dispatch

**Location**: `src/plugin/service.rs`

**PluginService Structure:**

```rust
pub struct PluginService {
    registry: Arc<PluginRegistry>,
    hook_timeout: Duration,  // Outer timeout for hook dispatch (default 5s)
}
```

**Hook Timeout Hierarchy:**
- Outer timeout (service.rs:18): **5 seconds** - `hook_timeout` in `PluginService::new()`
- Inner timeout (loader.rs:13): **30 seconds** - `WASM_HOOK_TIMEOUT` in `execute_wasm_hook()`

**Key Methods:**

```rust
impl PluginService {
    pub fn new(registry: Arc<PluginRegistry>) -> Self;
    pub fn with_hook_timeout(mut self, timeout: Duration) -> Self;
    pub async fn load_and_register(&self, loaded: LoadedPlugin) -> Result<(), LoadError>;
    pub async fn dispatch_hook(&self, ctx: HookContext) -> HookResult;
    
    // Individual dispatch methods
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

**Hook Execution Flow** (`service.rs:63-114`):

1. Get all hook registrations for the hook type from registry
2. Sort by priority (lower first - done at registration time)
3. For each hook:
   - Check if plugin is enabled
   - Execute with timeout
   - If blocked, return immediately
   - If error, return immediately
   - Otherwise, pass output to next hook

### registry.rs - Plugin Registry

**Location**: `src/plugin/registry.rs`

```rust
pub struct PluginInfo {
    pub id: String,              // "plugin:name" or "builtin:name"
    pub manifest: PluginManifest,
    pub path: PathBuf,
    pub enabled: bool,
    pub error: Option<String>,
}

pub struct PluginRegistry {
    plugins: RwLock<HashMap<String, PluginInfo>>,
    hooks: RwLock<Vec<HookRegistration>>,
}

pub struct HookRegistration {
    pub plugin_id: String,
    pub hook_type: HookType,
    pub priority: i32,
}
```

**Plugin ID Prefixes:**
- WASM plugins: `plugin:{name}` (e.g., `plugin:my-plugin`)
- Built-in plugins: `builtin:{name}` (e.g., `builtin:copilot`)

### install.rs - Plugin Installation

**Location**: `src/plugin/install.rs`

```rust
pub fn plugins_dir() -> PathBuf;  // ~/.local/share/codegg/plugins

pub async fn install_from_path(path: &Path) -> Result<PathBuf, InstallError>;
pub async fn install_from_url(url: &str) -> Result<PathBuf, InstallError>;
pub async fn uninstall(plugin_name: &str) -> Result<(), InstallError>;
```

**Security Measures:**
- Symlinks not allowed in archives or installation
- Path canonicalization checks prevent path traversal attacks
- HTTP download support for `.wasm` files or `.tar.gz` archives

### Path Canonicalization Security (`install.rs:136-156`)

The installation process validates extracted paths to prevent directory traversal attacks:

```rust
fn validate_extracted_path(dest: &Path, entry_path: &Path) -> Result<PathBuf, InstallError> {
    // Canonicalize the destination directory
    let dest_canonical = dest.canonicalize()
        .map_err(|e| InstallError::InvalidPath(format!("dest: {}", e)))?;

    // Canonicalize the entry path (resolved against dest)
    let entry_full = dest.join(entry_path);
    let entry_canonical = entry_full.canonicalize()
        .map_err(|e| InstallError::InvalidPath(format!("entry {}: {}", entry_path.display(), e)))?;

    // Ensure the canonical path starts with the destination directory
    if !entry_canonical.starts_with(&dest_canonical) {
        return Err(InstallError::PathTraversal);
    }

    Ok(entry_canonical)
}
```

This prevents attacks where malicious archive entries like `../../etc/passwd` could write outside the plugin directory.

### Symlink Prevention (`install.rs:183-212`)

Archive extraction rejects symlinks to prevent:
- Symlink attacks: extracting `plugin.wasm` -> `/etc/passwd`
- Time-of-check-time-of-use (TOCTOU) issues with relative path resolution
- Arbitrary file overwrite via crafted archives

The check verifies `entry.file_type().is_symlink()` returns false for all archive entries.

### tui.rs - TUI Extensions

**Location**: `src/plugin/tui.rs`

Allows plugins to register TUI routes and components:

```rust
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

pub struct TuiPluginRegistry {
    routes: Arc<RwLock<Vec<TuiRoute>>>,
    components: Arc<RwLock<Vec<TuiComponent>>>,
    plugin_configs: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

impl TuiPluginRegistry {
    pub async fn register_route(&self, route: TuiRoute);
    pub async fn register_component(&self, component: TuiComponent);
    pub async fn set_plugin_config(&self, plugin_id: &str, config: serde_json::Value);
    pub async fn get_plugin_config(&self, plugin_id: &str) -> Option<serde_json::Value>;
    pub async fn routes(&self) -> Vec<TuiRoute>;
    pub async fn components(&self) -> Vec<TuiComponent>;
    pub async fn routes_for_plugin(&self, plugin_id: &str) -> Vec<TuiRoute>;
    pub async fn components_for_plugin(&self, plugin_id: &str) -> Vec<TuiComponent>;
    pub async fn find_route(&self, path: &str) -> Option<TuiRoute>;
}
```

### event_bus.rs - Plugin Event Bus

**Location**: `src/plugin/event_bus.rs`

Allows plugins to subscribe to app events:

```rust
pub struct PluginEventSubscription {
    pub plugin_id: String,
    pub event_patterns: Vec<String>,  // e.g., ["agent.*", "tool.*"]
    pub priority: i32,
}

pub struct PluginEventBus {
    subscriptions: Arc<RwLock<Vec<PluginEventSubscription>>>,
    event_log: Arc<RwLock<Vec<AppEvent>>>,  // Circular buffer
    max_log_size: usize,
}

impl PluginEventBus {
    pub async fn subscribe(&self, subscription: PluginEventSubscription);
    pub async fn unsubscribe(&self, plugin_id: &str);
    pub async fn publish(&self, event: AppEvent);
    pub async fn subscriptions(&self) -> Vec<PluginEventSubscription>;
}
```

### builtin/mod.rs - Built-in Plugins

**Location**: `src/plugin/builtin/mod.rs` (lines 1-137)

Built-in plugins are native Rust handlers, not WASM:

```rust
pub struct BuiltinPlugin {
    pub manifest: PluginManifest,
    pub handler: fn(HookContext) -> HookResult,
}

// Handler registry
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

**Built-in Plugin Handlers:**

| Plugin | File | Hook Type | Purpose |
|--------|------|-----------|---------|
| copilot | builtin/copilot.rs | Auth | Injects Bearer token for GitHub Copilot provider |
| gitlab | builtin/gitlab.rs | Auth | Injects Bearer token for GitLab provider |
| codex | builtin/codex.rs | Auth | Injects Bearer token for OpenAI Codex provider |
| poe | builtin/poe.rs | Auth | Injects Bearer token for Poe API provider |

All built-in plugins handle the `auth` hook type and inject `Authorization: Bearer {token}` headers when the matching provider is detected.

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
  │    (hooks sorted by priority at registration time)
  │
  └──► For each hook registration:
          │
          ├──► Check if plugin is enabled?
          │
          └──► execute_hook_with_timeout(plugin_id, ctx)
                  │
                  ├─► If builtin:* → builtin_hook_handler(name, ctx)
                  │        │
                  │        └─► Returns HookResult directly (no fuel tracking)
                  │
                  └─► Else (WASM plugin):
                          └─► execute_wasm_hook(plugin_id, ctx)
                                  │
                                  ├─► Check fuel budget (exhausted → early return)
                                  ├─► Reserve fuel from per-plugin budget
                                  ├─► Get/compile WASM module from cache
                                  ├─► Allocate memory, write input JSON
                                  ├─► Call hook function (30s timeout)
                                  ├─► Read output JSON
                                  ├─► Return unused fuel
                                  └─► Return HookResult
```

## Fuel Tracking Mechanism

**Fuel Constants:**
| Constant | Value | Purpose |
|----------|-------|---------|
| `MAX_WASM_SIZE` | 10 MB | Maximum WASM module size |
| `WASM_FUEL_PER_HOOK` | 1,000,000 | Fuel allocated per hook call |
| `WASM_HOOK_TIMEOUT` | 30 seconds | Inner timeout for WASM execution |
| `MAX_PLUGIN_FUEL_BUDGET` | 10,000,000 | Initial fuel budget per plugin |

**Fuel Flow:**

1. **Initialization** (`loader.rs:236-248`):
   - Get current plugin fuel from `module_cache::CACHE.get_plugin_fuel()`
   - If budget exhausted, return early with `HookResult::ok(ctx.input)`
   - Calculate `fuel_for_this_call = min(WASM_FUEL_PER_HOOK, current_plugin_fuel)`
   - Reserve fuel via `module_cache::CACHE.reserve_fuel()`

2. **During Execution** (`loader.rs:292-293`):
   - Set store fuel: `store.set_fuel(fuel_reserved).ok()`

3. **After Execution** (`loader.rs:496-518`):
   - On success: `consumed = fuel_reserved - remaining; return_fuel(plugin_id, consumed)`
   - On error: `return_fuel(plugin_id, fuel_reserved)` (full amount)
   - On timeout: `return_fuel(plugin_id, fuel_reserved)` (full amount)

**All Early Return Paths with Fuel Return:**
- Line 258: metadata read failure
- Line 270: WASM size exceeds max
- Line 286: module cache failure
- Line 329: hook function not found
- Line 338: no memory export
- Line 353: no allocate function
- Line 371: allocate returned no value
- Line 386: input exceeds memory bounds
- Line 406: hook returned no value
- Line 431: output exceeds size limit
- Line 505: WASM execution error
- Line 510: hook timeout

## Security

| Feature | Implementation |
|---------|---------------|
| Fuel Limits | Per-plugin budgets in `ModuleCache::fuel_budgets` prevent infinite loops |
| Outer Timeout | 5s `hook_timeout` in `PluginService` (service.rs:18) |
| Inner Timeout | 30s `WASM_HOOK_TIMEOUT` for WASM execution (loader.rs:13) |
| Memory Bounds | Input validated before WASM memory write (loader.rs:384) |
| Output Size Limit | 10MB max from WASM output (loader.rs:424) |
| WASM Size Limit | 10MB max module size (loader.rs:263) |
| Path Traversal | Archive extraction validates canonical paths (install.rs:136-156) |
| Symlink Prevention | Not allowed in archives or installation (install.rs:191, 143) |

## Feature Flag

Requires `plugins` feature in `Cargo.toml`:

```toml
[features]
plugins = ["dep:wasmtime", "dep:wasmtime-cache", "dep:wasmtime-wasi"]
```

When the `plugins` feature is disabled, `execute_wasm_hook` is a no-op stub that returns `HookResult::ok(ctx.input)` (loader.rs:521-524).

## WASM Plugin Contract

Plugins must export these functions:

| Export | Signature | Required | Description |
|--------|-----------|----------|-------------|
| `memory` | Memory | Yes | Wasmtime memory |
| `allocate` | `(i32) -> i32` | Yes | Allocate `len` bytes, return pointer |
| `deallocate` | `(i32, i32)` | No | Free memory |
| Hook functions | See below | At least one | Handle specific hook types |

**Hook Function Naming Convention:**

| HookType | Function Name |
|----------|---------------|
| Auth | `on_auth` |
| Provider | `on_provider` |
| ToolDefinition | `on_tool_definition` |
| ToolExecuteBefore | `on_tool_execute_before` |
| ToolExecuteAfter | `on_tool_execute_after` |
| ChatParams | `on_chat_params` |
| ChatHeaders | `on_chat_headers` |
| Event | `on_event` |
| Config | `on_config` |
| ShellEnv | `on_shell_env` |
| TextComplete | `on_text_complete` |
| SessionCompacting | `on_session_compacting` |
| MessagesTransform | `on_messages_transform` |

**Hook Function Signature:**
```rust
// Input: (input_ptr: i32, input_len: i32) - pointer to JSON input
// Output: result_ptr i32 - pointer to serialized WasmHookResponse

#[derive(serde::Deserialize)]
struct WasmHookResponse {
    output: serde_json::Value,
    blocked: Option<bool>,  // defaults to false
    error: Option<String>,
}
```

**Memory Layout for Return Value:**
```
Offset 0: pointer to response (at offset 4)
Offset 4: length of response JSON (u32 le)
Offset 8: response JSON bytes
```

If result_ptr is 0, the original input is passed through unchanged.

## API Version

Current API version is `1.0.0` (api.rs:3):

```rust
pub const API_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiVersion {
    pub version: String,
    pub stability: Stability,
    pub features: Vec<String>,  // ["hooks", "custom_tools", "provider_middleware"]
}

impl ApiVersion {
    pub fn current() -> Self {
        Self {
            version: API_VERSION.to_string(),
            stability: Stability::Stable,
            features: vec![
                "hooks".to_string(),
                "custom_tools".to_string(),
                "provider_middleware".to_string(),
            ],
        }
    }
}
```

## See Also

- [hooks.md](hooks.md) - External hooks system
- [agent.md](agent.md) - AgentLoop integration with plugins
- [tool.md](tool.md) - Tool execution hooks
- [provider.md](provider.md) - Provider middleware hooks
