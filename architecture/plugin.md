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
├── marketplace.rs      # Plugin marketplace integration
├── lifecycle.rs        # LifecycleHooks, typed hook I/O contracts (Phase 9)
├── policy.rs           # PluginLifecyclePolicy, hook gating and fail-open/fail-closed (Phase 9)
├── runtime/            # Plugin runtime abstraction (Phase 6)
│   ├── mod.rs          # PluginRuntime trait, RuntimeError, RuntimeLimits
│   ├── process.rs      # ProcessRuntime implementation
│   ├── wasm.rs         # WasmRuntime implementation (Phase 7)
│   └── wasm_cache.rs   # WasmModuleCache for compiled module caching
└── builtin/            # Built-in native Rust plugins
    ├── mod.rs          # BuiltinPlugin, handler registry, dispatch
    ├── copilot.rs      # GitHub Copilot auth provider
    ├── gitlab.rs       # GitLab auth provider
    ├── codex.rs        # OpenAI Codex integration
    └── poe.rs          # Poe API integration
```

## Key Types

### PluginManifest (Canonical Form, Phase 5)

The canonical manifest declares runtime, capabilities, and permissions:

```rust
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub api_version: u32,                        // manifest API version (e.g. 2)
    pub runtime: PluginRuntimeSpec,               // Builtin, Process, or Wasm
    pub capabilities: Vec<PluginCapability>,       // Command, Hook, Panel, etc.
    pub permissions: PluginPermissionSet,          // Filesystem and other permissions

    // Legacy fields (still accepted for backward compat)
    pub hooks: Vec<LegacyHookSpec>,
    pub config: HashMap<String, serde_json::Value>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
}
```

**Legacy types (backward compat):**

```rust
pub struct LegacyHookSpec {
    #[serde(rename = "type")]
    pub hook_type: String,
    pub priority: Option<i32>,
}

// LegacyManifest is an alias for the old flat form without api_version/runtime/capabilities.
// Parsed when manifest.toml lacks api_version; promoted to canonical on load.
pub type LegacyManifest = PluginManifest; // pre-Phase 5 shape
```

### PluginTrustClass

Each plugin is assigned a trust class that governs capability access:

```rust
pub enum PluginTrustClass {
    Builtin,         // Ships with Codegg, full access
    LocalProcess,    // Local process-backed command, filesystem access allowed
    SandboxedWasm,   // WASM plugin, restricted filesystem
    TrustedLocal,    // User-installed, explicitly trusted
}
```

### PluginRuntimeSpec

Declares how a plugin executes:

```rust
pub enum PluginRuntimeSpec {
    Builtin,                        // Native Rust handler
    Process { command: String },     // Local process execution
    Wasm { path: String },           // WASM module path
}
```

### PluginCapability

What a plugin can register:

```rust
pub enum PluginCapability {
    Command(PluginCommandSpec),
    Hook(HookSpec),
    Panel { name: String, placement: PanelPlacement },
    StatusWidget { name: String },
    EventSubscription { patterns: Vec<String> },
}

pub struct PluginCommandSpec {
    pub name: String,            // command name (leading `/` stripped)
    pub description: Option<String>,
    pub args: Option<String>,    // usage hint
    pub output: PluginOutputSurface,
}

pub enum PluginOutputSurface {
    Text,     // plain text to chat
    Dialog,   // opens a dialog
    Panel,    // renders in a side panel
    Toast,    // one-shot notification
}
```

### PluginPermissionSet / FilesystemPermission

```rust
pub struct PluginPermissionSet {
    pub filesystem: Vec<FilesystemPermission>,
    pub network: bool,
    pub shell: bool,
}

pub enum FilesystemPermission {
    Read(String),   // path glob
    Write(String),  // path glob
    None,
}
```

### PluginSource

Tracks where a plugin was installed from:

```rust
pub struct PluginSource {
    pub kind: PluginSourceKind,    // Path, Url, Registry
    pub resolved: PathBuf,         // canonical local path after install
}
```

### PluginDiagnostic / PluginDiagnosticLevel

Runtime diagnostics surfaced to users and the registry:

```rust
pub enum PluginDiagnosticLevel {
    Info,
    Warning,
    Error,
}

pub struct PluginDiagnostic {
    pub level: PluginDiagnosticLevel,
    pub message: String,
    pub plugin_id: String,
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

### PluginLifecyclePolicy (Phase 9)

Controls which hook types are allowed, which runtimes are allowed, and fail-open/fail-closed behavior:

```rust
pub struct PluginLifecyclePolicy {
    pub enable_observation_hooks: bool,     // Event, After, Config, TextComplete, Compacting
    pub enable_mutating_hooks: bool,        // MessagesTransform, ShellEnv, ChatParams, ChatHeaders, Provider, ToolDefinition
    pub enable_blocking_hooks: bool,        // ToolExecuteBefore, Auth
    pub allow_process_lifecycle_hooks: bool, // Allow process runtime for lifecycle hooks
    pub fail_open: bool,                    // true = skip failed hooks, false = fail the operation
}
```

Default is conservative: observation enabled, mutating/blocking disabled, process disabled.

### LifecycleHooks (Phase 9)

High-level dispatcher that wraps `PluginService` with policy evaluation. Provides typed methods for each hook type:

```rust
pub struct LifecycleHooks {
    service: Arc<PluginService>,
    policy: PluginLifecyclePolicy,
}

impl LifecycleHooks {
    pub fn new(service: Arc<PluginService>, policy: PluginLifecyclePolicy) -> Self;
    pub async fn emit_event(&self, input: EventHookInput) -> PluginHookOutcome<()>;
    pub async fn before_tool_execute(&self, input: ToolExecuteBeforeInput) -> PluginHookOutcome<ToolExecuteBeforeOutput>;
    pub async fn after_tool_execute(&self, input: ToolExecuteAfterInput) -> PluginHookOutcome<()>;
    pub async fn transform_messages(&self, input: MessagesTransformInput) -> PluginHookOutcome<MessagesTransformOutput>;
    pub async fn shell_env(&self, input: ShellEnvInput) -> PluginHookOutcome<ShellEnvOutput>;
}
```

Each method checks policy via `policy.is_hook_allowed(hook_type)`, serializes typed input to JSON, dispatches through `PluginService`, and converts `HookResult` to `PluginHookOutcome<T>`.

### PluginHookOutcome<T> (Phase 9)

Outcome enum for typed return values from lifecycle hooks:

```rust
pub enum PluginHookOutcome<T> {
    Ok(T),              // Hook succeeded with transformed output
    Skipped,            // Hook was skipped (policy denied or no hooks registered)
    Blocked { reason: String },  // Hook blocked the operation
    Failed { error: String },    // Hook execution failed (fail-open policy skips, fail-closed propagates)
}
```

## Components

### loader.rs - WASM Loading and Fuel Tracking (Legacy Shim)

**Location**: `src/plugin/loader.rs`

The loader is now a compatibility shim. `execute_wasm_hook` delegates to `WasmRuntime` (Phase 7). Historical WASM loading, execution, module caching, and fuel tracking logic has moved to `runtime/wasm.rs` and `runtime/wasm_cache.rs`.

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

**Fuel Flow** (`src/plugin/runtime/wasm.rs`):

1. **Reserve Fuel**: `module_cache::CACHE.reserve_fuel(plugin_id, fuel_for_this_call)`
   subtracts the full reserved amount from the plugin's budget atomically.
2. **Execute WASM** with `store.set_fuel(fuel_reserved)`.
3. **Return Fuel** on:
   - Normal completion: **unused fuel** (the `remaining` value from
     `store.get_fuel()`, capped by `fuel_reserved`). The per-plugin budget
     therefore decreases by exactly the consumed amount.
   - Error / timeout: full `fuel_reserved` (failed invocations do not burn
     fuel).

The `return_unused_fuel()` helper centralises the unused-fuel accounting
and caps the refund at `fuel_reserved` so buggy instrumentation cannot
over-credit the budget. Tests in `src/plugin/runtime/wasm.rs` and
`wasm_cache.rs` guard the contract.

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

    // Phase 5: command invocation
    pub async fn invoke_command(&self, name: &str, args: serde_json::Value) -> Result<PluginResponse, PluginError>;

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

> **Note**: Phase 9 adds `LifecycleHooks` in `lifecycle.rs` as a policy-aware wrapper around these dispatch methods. New code should prefer `LifecycleHooks` over calling `dispatch_*` directly.

**PluginResponse (Phase 5):**

```rust
pub struct PluginResponse {
    pub ok: bool,
    pub data: Option<serde_json::Value>,
    pub effects: Vec<UiEffect>,
    pub diagnostics: Vec<PluginDiagnostic>,
}
```

**PluginError (Phase 5):**

```rust
pub enum PluginError {
    NotFound(String),           // command or plugin not found
    ExecutionFailed(String),    // runtime error
    PermissionDenied(String),   // trust/permission check failed
    Timeout,
    Disabled,
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
    pub manifest: PluginManifest,  // canonical form (Phase 5)
    pub trust: PluginTrustClass,
    pub path: PathBuf,
    pub enabled: bool,
    pub error: Option<String>,
    pub diagnostics: Vec<PluginDiagnostic>,
}

pub struct PluginRegistry {
    plugins: RwLock<HashMap<String, PluginInfo>>,
    hooks: RwLock<Vec<HookRegistration>>,
    commands: RwLock<Vec<PluginCommandRegistration>>,
    panels: RwLock<Vec<PluginPanelRegistration>>,
    status_widgets: RwLock<Vec<PluginStatusRegistration>>,
    event_subscribers: RwLock<Vec<PluginEventRegistration>>,
}

// Capability-based registration structs (Phase 5)
pub struct PluginCommandRegistration {
    pub plugin_id: String,
    pub spec: PluginCommandSpec,
    pub trust: PluginTrustClass,
}

pub struct HookRegistration {
    pub plugin_id: String,
    pub hook_type: HookType,
    pub priority: i32,
}

pub struct PluginPanelRegistration {
    pub plugin_id: String,
    pub name: String,
    pub placement: PanelPlacement,
}

pub struct PluginStatusRegistration {
    pub plugin_id: String,
    pub name: String,
}

pub struct PluginEventRegistration {
    pub plugin_id: String,
    pub patterns: Vec<String>,
    pub priority: i32,
}
```

**Registry query methods (Phase 5):**

```rust
impl PluginRegistry {
    pub fn command(&self, name: &str) -> Option<PluginCommandRegistration>;
    pub fn commands(&self) -> Vec<PluginCommandRegistration>;
    pub fn panels(&self) -> Vec<PluginPanelRegistration>;
    pub fn status_widgets(&self) -> Vec<PluginStatusRegistration>;
    pub fn event_subscribers(&self) -> Vec<PluginEventRegistration>;
    // Existing
    pub fn hooks_for(&self, hook_type: HookType) -> Vec<HookRegistration>;
    pub fn plugins(&self) -> Vec<PluginInfo>;
}
```

**Plugin ID Prefixes:**
- WASM plugins: `plugin:{name}` (e.g., `plugin:my-plugin`)
- Built-in plugins: `builtin:{name}` (e.g., `builtin:copilot`)

### install.rs - Plugin Installation

**Location**: `src/plugin/install.rs`

```rust
pub fn plugins_dir() -> PathBuf;  // Cross-platform via dirs::data_local_dir()

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

### tui.rs - TUI Extensions (Legacy/Deprecated)

> **Note**: `tui.rs` is a legacy module. Panel and status widget registration is now handled through `PluginCapability` in the manifest and `PluginRegistry` methods (`panels()`, `status_widgets()`). This module is retained for backward compatibility but will be removed in a future phase.

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
~/.local/share/codegg/plugins/     (Linux)
~/Library/Application Support/codegg/plugins/  (macOS)
%LOCALAPPDATA%\codegg\plugins\     (Windows)
via dirs::data_local_dir()
├── my-plugin/
│   ├── manifest.toml
│   └── plugin.wasm
└── another-plugin/
    ├── manifest.toml
    └── plugin.wasm
```

### manifest.toml Example

**New canonical format (Phase 5):**

```toml
name = "my-plugin"
version = "1.0.0"
api_version = 2

[runtime]
type = "wasm"
path = "plugin.wasm"

[[capabilities.command]]
name = "my-cmd"
description = "Run my command"
output = "dialog"

[[capabilities.hook]]
type = "tool.execute.before"
priority = 0

[permissions]
network = false
shell = false

[[permissions.filesystem]]
type = "read"
path = "./config/**"
```

**Legacy format (still accepted):**

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
                          └─► WasmRuntime::invoke(PluginInvocation)
                                  │
                                  ├─► Check fuel budget (exhausted → early return)
                                  ├─► Reserve fuel from per-plugin budget
                                  ├─► Get/compile WASM module from WasmModuleCache
                                  ├─► Try modern ABI: codegg_plugin_invoke(ptr, len) -> i64
                                  │   (packed response: high 32 = ptr, low 32 = len)
                                  │   Falls back to legacy per-hook export if absent
                                  ├─► Allocate memory, write input JSON
                                  ├─► Call hook function (configurable timeout)
                                  ├─► Read output JSON
                                  ├─► Return unused fuel
                                  └─► Return PluginResponse
```

## Duplicate and Priority Rules (Phase 5)

- **Command name normalization**: Leading `/` is stripped and names are lowercased before lookup. `/MyCmd` and `mycmd` resolve to the same registration.
- **Built-in/static commands win**: When a built-in or statically registered command shares a normalized name with a plugin command, the built-in takes precedence. The plugin registration is retained but not surfaced in command completion or dispatch.
- **Duplicate plugin command registration is rejected**: If two plugins register the same normalized command name, the second registration returns an error diagnostic. The first successful registration wins.
- **Hooks sorted by priority ascending**: Lower numeric priority executes first. Registrations with equal priority are ordered by plugin registration order (FIFO).
- **Disabled plugins excluded**: Plugins with `enabled: false` are excluded from all capability queries (`commands()`, `panels()`, `status_widgets()`, `event_subscribers()`, `hooks_for()`). Re-enabling a plugin re-activates its registrations.

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

3. **After Execution** (`src/plugin/runtime/wasm.rs`, `return_unused_fuel()`):
   - On success: **unused fuel** is credited back. The helper computes
     `unused = remaining.min(reserved)` and calls
     `cache.return_fuel(plugin_id, unused)`. The per-plugin budget
     decreases by exactly `reserved - unused` (the consumed amount).
   - On error: `cache.return_fuel(plugin_id, fuel_reserved)` (full amount)
     so failed invocations do not burn fuel.
   - On timeout: `cache.return_fuel(plugin_id, fuel_reserved)` (full amount).

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
plugins = ["dep:wasmtime", "dep:wasmtime-wasi"]
```

When the `plugins` feature is disabled, `WasmRuntime::invoke` returns `RuntimeError::Unsupported`. The legacy `execute_wasm_hook` is a no-op stub that returns `HookResult::ok(ctx.input)`.

## WASM Plugin Contract

Plugins must export these functions:

| Export | Signature | Required | Description |
|--------|-----------|----------|-------------|
| `memory` | Memory | Yes | Wasmtime memory |
| `allocate` | `(i32) -> i32` | Yes | Allocate `len` bytes, return pointer |
| `deallocate` | `(i32, i32)` | No | Free memory |
| `codegg_plugin_invoke` | `(i32, i32) -> i64` | Recommended | Modern ABI entrypoint |
| Hook functions | See below | Legacy fallback | Per-hook exports |

Both ABIs use `allocate`/`deallocate` for memory management.

### Modern ABI (`codegg_plugin_invoke`)

Single entrypoint for all plugin invocations:

```
Input: (ptr: i32, len: i32) — pointer to serialized PluginInvocation JSON
Output: i64 — packed (high 32 bits = response pointer, low 32 bits = response length)
```

The host writes a `PluginInvocation` (from `codegg_protocol::plugin`) to WASM linear memory at `ptr`/`len`, then calls `codegg_plugin_invoke`. The plugin reads the invocation, performs its work, allocates response memory via `allocate`, writes a `PluginResponse` JSON, and returns the packed pointer/length.

If the module does not export `codegg_plugin_invoke`, the runtime falls back to the legacy per-hook ABI.

### Legacy ABI (per-hook exports)

Each hook type has its own export function:

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

**Legacy Hook Function Signature:**
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

**Memory Layout for Legacy Return Value:**
```
Offset 0: pointer to response (at offset 4)
Offset 4: length of response JSON (u32 le)
Offset 8: response JSON bytes
```

If result_ptr is 0, the original input is passed through unchanged.

## Runtime Limits

| Limit | Value | Notes |
|-------|-------|-------|
| Module size | 10 MiB | Maximum WASM module size (`MAX_WASM_SIZE`) |
| Output size | 1 MiB | Maximum output from a single WASM call |
| Fuel per call | 1,000,000 | Configurable via `WasmRuntimeSpec::fuel_per_call` |
| Memory max | 256 MiB | Configurable via `WasmRuntimeSpec::memory_max_mb`; not enforced on Config in wasmtime 36 |
| Fuel budget (global) | 10,000,000 | Per-plugin fuel budget (`MAX_PLUGIN_FUEL_BUDGET`) |
| Per-call timeout | 5s default | Configurable via `RuntimeLimits::timeout_ms` or `WasmRuntimeSpec::timeout_ms` |

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

## Protocol DTOs (Phase 1-5)

Phase 5 introduces the canonical `PluginManifest` with `api_version`, `runtime`, `capabilities`, and `permissions` fields. The registry is restructured around capability-based registration structs (`PluginCommandRegistration`, `PluginPanelRegistration`, etc.) and the `PluginTrustClass` system. Legacy manifests without `api_version` are accepted and promoted on load.

`crates/codegg-protocol/src/ui.rs` and `crates/codegg-protocol/src/plugin.rs` define frontend-neutral protocol types for plugin UI output and invocation. Phase 2 adds TUI-side consumption: `PluginUiState` (`src/tui/app/state/plugin_ui.rs`) stores plugin dialogs, panels, and status items. `PluginUiRenderer` (`src/tui/components/plugin_renderer.rs`) lowers `UiNode` trees into ratatui widgets and flat text lines. `App::apply_plugin_ui_effect()` centralizes effect routing. A single `Dialog::Plugin` variant handles all plugin dialogs without per-plugin enum entries. Phase 3 adds generic `TuiCommand` plugin variants (`PluginCommandRun`, `PluginCommandFinished`, `PluginUiEffect`) and `src/tui/commands/plugins.rs` with `start_plugin_command`, `apply_plugin_command_finished` (response application), and `apply_plugin_ui_effect` (direct effect dispatch). Phase 4 replaces the stub with real process execution: `start_plugin_command` accepts a `ProcessCommandSpec` and spawns a child process with timeout, output capping, and stdin piping.

**EmitChat visibility (Phase 11):** `App::apply_plugin_ui_effect()`
handles `UiEffect::EmitChat` directly rather than deferring it. The
content is rendered to the user via `show_short_or_info()`: short
blocks (≤3 lines) toast, longer blocks open the scrollable info dialog.
Both `ChatFormat::Plain` and `ChatFormat::Markdown` are lowered to
line-based text — markdown links and embedded escape sequences are not
executed. Output is **not** added to the model-visible chat transcript.
The lower-level `PluginUiState::apply_effect()` still returns
`PluginUiApplyResult::ChatRequested` for callers that route effects
without going through `App`. `PluginUiApplyResult::ChatApplied` is the
new variant returned by the App-level path.

### UI Types (`codegg_protocol::ui`)

- `UiNode` — Tree of display content: `Text`, `Markdown`, `Code`, `Table`, `KeyValue`, `Progress`, `Container`, `Empty`, `Unsupported`
- `UiEffect` — Side effects plugins can request: `EmitChat`, `ShowToast`, `OpenDialog`, `CloseDialog`, `OpenPanel`, `UpdatePanel`, `ClosePanel`, `AddStatusItem`, `UpdateStatusItem`, `RemoveStatusItem`
- Supporting types: `DialogSpec`, `PanelSpec`, `StatusItemSpec`, `ChatBlock`, `ToastSpec`, `PanelPlacement`, `StatusPlacement`
- TUI consumption (Phase 2): `PluginUiState` stores open/update/close effects; `PluginUiRenderer` renders `UiNode` to ratatui; `App::apply_plugin_ui_effect()` routes `ShowToast`/`EmitChat`/`OpenDialog`/`CloseDialog`. Panels and status items stored but not visually rendered yet.

### UI Effect Event Flow (Phase 10)

**Frontend-neutral transport**: Plugin UI effects travel through two channels — `CoreEvent::PluginUiEffect` (core event stream for remote TUI clients) and `TuiCommand::PluginUiEffect` (local TUI command channel). Both carry an `UiEffectEnvelope` wrapping the `UiEffect` with session, plugin, and invocation metadata.

**Event flow**: Lifecycle hooks produce `PluginResponse.effects` → converted to `HookResult.effects` → wrapped in `PluginHookOutcome::Ok(value, effects)` → agent loop emits `AppEvent::PluginUiEffect` → event bridge maps to `CoreEvent::PluginUiEffect` → remote clients receive via event log subscription. Local TUI routes through `TuiCommand::PluginUiEffect` to `App::apply_plugin_ui_effect()`.

**Capability negotiation**: `ClientCapabilities` carries `plugin_ui_*` boolean flags per surface type. `PluginUiCapabilities::supports_effect()` checks whether a client can render a given effect. `degrade_node_to_text()` provides deterministic fallback when a surface is unsupported.

**Degradation rules**: dialog→chat block, panel→chat block, table→markdown table, status item→omit, toast→always supported. Effects for unsupported surfaces are silently downgraded rather than dropped.

**TUI consumption**: Both local `TuiCommand::PluginUiEffect` and remote `CoreEvent::PluginUiEffect` route through `App::apply_plugin_ui_effect()`, which checks client capabilities before dispatching.

### Plugin Types (`codegg_protocol::plugin`)

- `PluginManifestDto` — Plugin metadata with runtime spec, capabilities, and permissions
- `PluginRuntimeSpec` — `Builtin`, `Process`, or `Wasm` runtime declaration
- `PluginCapability` — `Command`, `Hook`, `Panel`, `StatusWidget`, `EventSubscription`
- `PluginInvocation` — Request envelope for invoking a plugin capability
- `PluginResponse` — Response with `ok` flag, `effects: Vec<UiEffect>`, `data`, and `diagnostics`
- `PluginPermissionSet` / `FilesystemPermission` — Declared permission requirements
- `PluginDiagnostic` / `PluginDiagnosticLevel` — Diagnostic reporting

### Key Design Decisions

- Hook types are strings (not enums) in the protocol DTO; root crate maps to internal `HookType` enum
- `Unsupported` variant provides forward-compatible fallback for unknown UI node types
- `FilesystemPermission::None` is the default
- `PLUGIN_PROTOCOL_VERSION = 1` for versioning

## Process-Backed Commands (Phase 4)

Dynamic slash commands can declare `runtime: process` in their YAML frontmatter to execute a local process instead of rendering a template. This is the first plugin execution path: a developer can add a project-local `/quota`-style command that runs Python, shell, or another executable without recompiling Codegg.

### Frontmatter Schema

```yaml
---
description: Show quota
runtime: process
command: python3
args: ["scripts/quota.py"]
stdin: none        # none | json
stdout: auto       # text | json | auto
timeout_ms: 5000
cwd: /path/to/dir
env: ["KEY=VALUE"]
output: ["chat", "dialog"]
---
```

All process fields are optional. `command` is required when `runtime: process`.

### Config Types (`crates/codegg-config/src/schema.rs`)

- `CommandRuntimeKind` — `Template` (default) | `Process`
- `CommandStdinMode` — `None` (default) | `Json`
- `CommandStdoutMode` — `Text` | `Json` | `Auto` (default, tries JSON then falls back to text)
- `CommandConfig` gains: `runtime`, `command`, `args`, `stdin`, `stdout`, `timeout_ms`, `cwd`, `env`, `output`

### Internal Types (`src/command/mod.rs`)

- `ProcessCommandSpec` — Runtime execution spec: `command`, `args`, `stdin`, `stdout`, `timeout_ms`, `cwd`, `env`, `output`
- `Command.process: Option<ProcessCommandSpec>` — Set when `runtime: process`

### Execution (`src/tui/commands/plugins.rs`)

`start_plugin_command(spec, args)` delegates to `ProcessRuntime` via `execute_via_runtime()`:
- Converts `ProcessCommandSpec` to `ProcessRuntimeSpec`
- Creates a `ProcessRuntime` with default limits
- Builds a `PluginInvocation` and calls `runtime.invoke()`
- Posts `TuiCommand::PluginCommandFinished` with the structured `PluginResponse`

### TUI Integration (`src/tui/app/mod.rs`)

`execute_command` checks `cmd.process` before `cmd.template`. Process commands send `TuiCommand::PluginCommandRun { spec, args }` through the command channel. The dispatch handler calls `start_plugin_command`. Completion arrives as `TuiCommand::PluginCommandFinished` and is handled by `apply_plugin_command_finished`.

### Security

Process-backed commands are local executable code. Minimal safety controls: no shell execution unless explicitly configured, timeout, output caps, cwd control, explicit env variables. They are not sandboxed.

## Plugin Runtime Abstraction (Phase 6)

Phase 6 extracts process execution into a runtime-neutral abstraction layer. Process execution is no longer owned by `src/tui/commands/plugins.rs`. The TUI starts plugin commands, but execution is delegated to a runtime implementation that can later be used by TUI, core daemon, socket/stdio mode, tests, and installed plugin manifests.

### Runtime Module (`src/plugin/runtime/`)

- **`mod.rs`**: Defines `PluginRuntime` trait, `RuntimeError` enum, `RuntimeLimits` struct
- **`process.rs`**: `ProcessRuntime` implementation with `ProcessRuntimeSpec`

### `PluginRuntime` Trait

```rust
#[async_trait]
pub trait PluginRuntime: Send + Sync {
    async fn invoke(&self, invocation: PluginInvocation) -> Result<PluginResponse, RuntimeError>;
}
```

Implementations handle the actual execution of plugin commands (process, WASM, builtin) and return protocol-level responses. WASM implements this trait via `WasmRuntime` (Phase 7).

### `RuntimeError`

Covers: `Unsupported`, `Spawn`, `Timeout`, `NonZeroExit { code, stdout, stderr }`, `InvalidJson`, `Io`.

### `RuntimeLimits`

Default limits: timeout 5s, max stdout 1 MiB, max stderr 256 KiB.

### `ProcessRuntime`

- Takes `ProcessRuntimeSpec` (command, args, stdin mode, stdout mode, timeout, cwd, env)
- Converts from both `ProcessCommandSpec` and `PluginRuntimeSpec::Process`
- Handles child process spawning, stdin piping, timeout, output capping
- Parses stdout according to mode: `Text` → EmitChat effect, `Json` → structured response, `Auto` → try JSON then text
- Returns `PluginResponse` for all successful paths
- Non-zero exit returns `RuntimeError::NonZeroExit`

### WASM Runtime (Phase 7)

Phase 7 modernizes WASM execution by routing it through the same `PluginRuntime` trait used by `ProcessRuntime`. The legacy `loader.rs` `execute_wasm_hook` function is now a compatibility shim that delegates to `WasmRuntime`.

**`WasmRuntime`** implements the same `PluginRuntime` trait as `ProcessRuntime`:

```rust
pub struct WasmRuntime {
    spec: WasmRuntimeSpec,
    limits: RuntimeLimits,
}
```

**`WasmRuntimeSpec` Configuration:**

```rust
pub struct WasmRuntimeSpec {
    pub module_path: PathBuf,     // path to .wasm file
    pub timeout_ms: u64,          // per-call timeout
    pub memory_max_mb: u32,       // max memory in MB (configurable, not enforced on Config in wasmtime 36)
    pub fuel_per_call: u64,       // fuel per invocation (default 1,000,000)
    pub entrypoint: Option<String>, // entrypoint function name (default: "codegg_plugin_invoke")
}
```

**Dual ABI Support:**

- **Modern ABI** (`codegg_plugin_invoke`): Single entrypoint receives `PluginInvocation` JSON, returns `PluginResponse` JSON. Signature: `codegg_plugin_invoke(ptr: i32, len: i32) -> i64` where the returned i64 is packed (high 32 bits = response pointer, low 32 bits = response length).
- **Legacy ABI** (per-hook exports): Individual exports like `on_auth(ptr, len) -> i32`, `on_tool_execute_before(ptr, len) -> i32`, etc. Each receives `WasmHookResponse` JSON and returns a pointer to the legacy response format.
- Falls back to legacy ABI automatically when `codegg_plugin_invoke` is absent from the WASM module exports.

**`WasmModuleCache`** (`wasm_cache.rs`):

Provides mtime-based compiled module caching and per-plugin fuel budgets. Similar to the legacy `module_cache` in `loader.rs` but managed as a separate concern:

- Caches compiled `wasmtime::Module` keyed by file path and modification time
- Tracks per-plugin fuel budgets (`DashMap<String, AtomicU64>`)
- Provides `reserve_fuel` / `return_fuel` for budget management
- Hit/miss counters for observability

**Feature-gating:**

Requires the `plugins` feature for Wasmtime execution. Without it, `WasmRuntime::invoke` returns `RuntimeError::Unsupported`.

**`loader.rs` compatibility:**

`loader.rs` is now a compatibility shim. `execute_wasm_hook` delegates to `WasmRuntime` internally, preserving the existing hook-based calling convention while routing through the unified runtime abstraction.

### Response Type Unification

The local `PluginResponse` in `service.rs` is removed. `codegg_protocol::plugin::PluginResponse` (with `effects: Vec<UiEffect>`) is the single canonical type, re-exported from `plugin/mod.rs`. `PluginDiagnostic` is also unified via re-export from protocol.

### Registry Hardening

- `PluginRegistry::unregister()` now returns `Option<PluginInfo>` (previously returned `None`)
- Duplicate command/panel/status checking is global (all registered plugins, not just enabled)
- `set_enabled(true)` validates that enabling won't create duplicate commands/panels/status widgets
- **`enabled_plugin_ids()` snapshot (Phase 11):** Capability queries
  (`hooks_for`, `command`, `commands`, `panels`, `status_widgets`,
  `event_subscribers`) and the duplicate checks in `set_enabled()` no
  longer use `try_read()`. They acquire a single read guard on
  `plugins` via `enabled_plugin_ids()` to snapshot the set of enabled
  plugin ids, then filter against that snapshot. Visibility now depends
  only on actual enabled state, not on lock contention. A structural
  regression test (`registry_does_not_use_try_read_as_code`) prevents
  the `try_read()` pattern from being reintroduced.

### Service Dispatch

`PluginService::invoke_command()` dispatches to the appropriate runtime:
- **Builtin**: Returns structured response with handler info (command invocation not yet wired)
- **Process**: Creates `ProcessRuntime`, invokes via `PluginRuntime` trait, returns `PluginResponse`
- **Wasm**: Creates `WasmRuntime`, invokes via `PluginRuntime` trait, returns `PluginResponse`

## Builtin Runtime (Phase 8)

Phase 8 promotes built-in plugins from the legacy `BUILTIN_HANDLERS` static to a first-class `BuiltinRuntime` that implements the `PluginRuntime` trait alongside `ProcessRuntime` and `WasmRuntime`.

### BuiltinRuntime and BuiltinHandlerRegistry

`BuiltinRuntime` (`src/plugin/runtime/builtin.rs`) implements `PluginRuntime` and dispatches `PluginInvocation` through a `BuiltinHandlerRegistry`. The registry maps handler IDs (e.g., `"copilot"`, `"gitlab"`) to native Rust `fn(HookContext) -> HookResult` functions.

```rust
pub type BuiltinHookHandler = fn(HookContext) -> HookResult;

pub struct BuiltinHandlerRegistry {
    handlers: HashMap<String, BuiltinHookHandler>,
}

pub struct BuiltinRuntime {
    handlers: Arc<BuiltinHandlerRegistry>,
}

impl PluginRuntime for BuiltinRuntime {
    async fn invoke(&self, invocation: PluginInvocation) -> Result<PluginResponse, RuntimeError>;
}
```

### Adapter Functions

Two adapter functions bridge the hook handler model with the runtime model:

- **`invocation_to_hook_context()`**: Converts a `PluginInvocation` (with
  `PluginCapabilityInvocation::Hook`) into a `HookContext`, extracting the
  `HookType` from the capability string. **Phase 11 strictness:**
  rejects `PluginCapabilityInvocation::Command`, `Panel`, `StatusWidget`,
  `Event`, and unknown hook type strings with `RuntimeError::Unsupported`.
  The previous `unwrap_or(HookType::Auth)` fallback has been removed.
- **`hook_result_to_plugin_response()`**: Converts a `HookResult` into a `PluginResponse`, mapping errors to diagnostics and blocked state to `ok: false`.

**Hook-only scope:** `BuiltinRuntime` dispatches only hook invocations.
There is no builtin command handler registry; builtin plugin
capabilities must be `PluginCapability::Hook`, not `PluginCapability::Command`.

### Canonical Sources

- **`builtin_plugin_manifests()`**: Returns `Vec<PluginManifest>` for all four builtins. Each manifest declares `runtime: PluginRuntimeSpec::Builtin { handler }` and hook capabilities. This is the canonical source for builtin metadata.
- **`builtin_runtime_registry()`**: Builds a `BuiltinHandlerRegistry` populated with all four handlers. The returned registry can be wrapped in `Arc` and passed to `BuiltinRuntime::new()`.
- **`make_builtin_info()`**: Returns `(PluginInfo, Vec<HookRegistration>)`.
  **Phase 11:** unknown hook type strings are skipped via `filter_map`
  rather than silently falling back to `HookType::Auth`.

### Individual Builtin Modules

Each builtin module (`copilot.rs`, `gitlab.rs`, `codex.rs`, `poe.rs`) exports:

- `PLUGIN_ID: &str` — e.g., `"builtin:copilot"`
- `HANDLER_ID: &str` — e.g., `"copilot"`
- `manifest() -> PluginManifest` — canonical manifest with `runtime: Builtin { handler }` and `PluginCapability::Hook` (no command capabilities)
- `handle_hook(HookContext) -> HookResult` — the actual handler

### Service Integration

`PluginService` accepts an optional `Arc<BuiltinRuntime>` via `with_builtin_runtime()`. When a builtin runtime is present, `invoke_command()` dispatches builtin plugin invocations through `BuiltinRuntime::invoke()`. **Phase 11:** if no builtin runtime is registered, `invoke_command()` for a builtin plugin returns `PluginError::Runtime` instead of a success placeholder. Builtin plugins that declare a command capability but have no command runtime handler will therefore fail loudly rather than silently succeed.

### Tests

- `builtin_plugin_manifests_declare_builtin_runtime`: All manifests use `PluginRuntimeSpec::Builtin` and `PluginTrustClass::Builtin`.
- `builtin_runtime_registry_contains_all_handlers`: Registry has entries for all four builtins.
- `builtin_runtime_registry_handlers_work`: Handlers produce correct `HookResult` output.
- `builtin_runtime_rejects_command_invocation` (Phase 11): command invocations on the builtin runtime return `RuntimeError::Unsupported`.
- `builtin_runtime_rejects_unknown_hook_type` (Phase 11): unknown hook type strings are rejected (no Auth fallback).
- `invocation_to_hook_context_unknown_hook_type_is_rejected` (Phase 11): same guarantee at the adapter layer.
- `builtin_command_invocation_is_rejected_by_service` (Phase 11): service-level rejection of builtin commands.
- Unit tests in `runtime/builtin.rs` verify dispatch, unknown handler errors, non-`builtin:` prefix rejection, and adapter function correctness.

### Acceptance

Builtins now register through the unified plugin registry with `runtime = Builtin` and dispatch through `BuiltinRuntime` (or fallback to direct `builtin_hook_handler()` lookup when no runtime is provided). The runtime is hook-only; command invocation against builtin plugins is rejected.

## Lifecycle Hooks (Phase 9)

Phase 9 wires plugin lifecycle hooks into the core execution paths where they matter: provider/auth resolution, tool execution before/after, chat params/headers, message transforms, session compaction, shell env, and event publication.

### Key Additions

- `lifecycle.rs`: `LifecycleHooks` dispatcher with typed I/O contracts for each hook type
- `policy.rs`: `PluginLifecyclePolicy` for gating hook execution by type and runtime
- `PluginService` is now created and wired into `AgentLoop` via `TurnRunInput`
- Shell env hooks are dispatched before process spawn in `ShellRuntime`
- Message transform hooks run before provider calls in the agent loop
- Pre/post tool hooks and compaction hooks were already wired but now active

### Policy Defaults

| Hook Category | Default | Policy Field |
|---------------|---------|--------------|
| Observation (Event, After, Config, TextComplete, Compacting) | Enabled | `enable_observation_hooks` |
| Mutating (MessagesTransform, ShellEnv, ChatParams, ChatHeaders, Provider, ToolDefinition) | Disabled | `enable_mutating_hooks` |
| Blocking (ToolExecuteBefore, Auth) | Disabled | `enable_blocking_hooks` |
| Process runtime | Disabled | `allow_process_lifecycle_hooks` |

### Hook Pipeline

1. Caller creates typed input (e.g., `EventHookInput`)
2. `LifecycleHooks` checks `policy.is_hook_allowed(hook_type)` → returns `Skipped` if denied
3. Input is serialized to JSON
4. `PluginService::dispatch_hook()` iterates registered hooks by priority
5. Each hook is dispatched through the appropriate runtime (Builtin, WASM, Process)
6. Results are threaded through (pipeline pattern: each hook's output becomes next hook's input)
7. Final `HookResult` is converted to `PluginHookOutcome<T>` using fail-open/fail-closed policy

## Phase 12: Plugin Management UX

First-class plugin management commands and UI surfaces for local observability and controlled management.

### Files

- `src/plugin/management.rs` — `PluginManager`, `PluginManagementView`, `PluginDoctorReport`, `resolve_plugin_selector()`
- `src/plugin/management_ui.rs` — `plugins_table()`, `plugin_info_node()`, `doctor_report_node()` returning `UiNode`
- `src/tui/commands/plugin_management.rs` — TUI command handlers

### Commands

| Command | Description |
|---------|-------------|
| `/plugins` | List installed and built-in plugins |
| `/plugin-info <id>` | Show plugin runtime, capabilities, trust, diagnostics |
| `/plugin-enable <id>` | Enable a plugin |
| `/plugin-disable <id>` | Disable a plugin |
| `/plugin-doctor [id]` | Diagnose plugin configuration and runtime health |
| `/plugin-remove <id>` | Remove a local installed plugin |
| `/plugin-install <path>` | Install a plugin from a local path |

### Selector Resolution

Plugins can be referenced by:
1. Exact plugin id
2. Exact manifest name
3. Unique prefix match on id (case-insensitive)
4. Unique prefix match on name (case-insensitive)

Ambiguous or missing selectors produce clear error messages.

### Safety

- Enable/disable persists to `disabled_plugins.toml` in the plugins directory
- Remove only deletes from the canonical plugin install directory
- Install validates manifests before copying and refuses to overwrite existing plugins
- Doctor checks are read-only and never execute plugin code

### Tests

- 30 management tests (selector resolution, view construction, doctor checks, last_error)
- 16 management_ui tests (table rendering, key-value, doctor reports, last_error display)
- 31 TUI plugin management tests (format helpers, resolve, persistence, apply handler routing)

## Security Policy (Phase 12)

`PluginPolicy` in `src/plugin/policy.rs` is a composite policy that gates plugin invocations against manifest declarations and trust requirements.

### Sub-Policies

| Sub-Policy | Default | What It Controls |
|------------|---------|-----------------|
| `PluginLifecyclePolicy` | Observation hooks allowed; mutating/blocking/process denied | Hook type + runtime gating |
| `PluginUiPolicy` | Dialog and toast allowed; panel/status denied | UI effect surface gating |
| `PluginPermissionPolicy` | All capabilities denied unless declared | Command/hook invocation declarations |
| `PluginInstallPolicy` | Env passthrough denied | Environment variable access from plugins |
| `PluginRuntimePolicy` | Secrets denied; auth-hook requires high trust | Secret access and high-trust gating |

All sub-policies default to conservative. A plugin must explicitly declare capabilities and permissions in its manifest to pass policy checks.

### PolicyDecision

```rust
pub enum PolicyDecision {
    Allow,
    Deny { reason: String },
    Degrade { reason: String },
}
```

Four check functions validate invocations against policy:

- **`check_invocation_allowed(manifest, invocation, trust, policy)`** — validates that a command or hook invocation matches a declared capability in the manifest.
- **`check_ui_effect_allowed(manifest, effect, policy)`** — gates UI effects by output surface declarations (e.g., panel requires `PluginCapability::Panel`).
- **`check_lifecycle_hook_allowed(hook_type, trust, policy)`** — validates hook type and trust class; auth hooks require high trust.
- **`check_secret_access_allowed(manifest, secret_name, policy)`** — validates secret access against declared permissions.

### Integration

`PluginService` accepts an optional `Arc<PluginPolicy>` via `with_policy()`. When set:
- `invoke_command()` rejects undeclared commands with a policy error
- `dispatch_hook()` checks hook type, trust class, and auth-hook high-trust requirement before dispatch; denied hooks are skipped with logging

When policy is absent, all checks pass (backward compatible).

### Safety Note

Process plugins are local executable code. They are not sandboxed. They are suitable for explicit user-invoked local commands, not silent lifecycle interception by default.

## SDKs and Examples (Phase 13)

The `examples/plugins/` directory contains runnable reference plugins
and helper SDKs. Each is small, self-contained, and tested. Use them
as templates rather than reverse-engineering the protocol from
`codegg-protocol`.

### Examples

| Path | Runtime | Demonstrates |
|------|---------|--------------|
| `process-quota-text/` | process (stdout) | Zero-SDK path: emit plain text and let auto-detection surface it as `EmitChat`. |
| `process-quota-json/` | process (JSON) | Read `PluginInvocation` from stdin; emit `PluginResponse` with `OpenDialog` + `EmitChat` to stdout. |
| `wasm-command-table/` | wasm | Modern `codegg_plugin_invoke` ABI; returns a dialog containing a Table `UiNode`. |
| `wasm-hook-message-transform/` | wasm | Event-subscription observation hook (default policy permits it without config). |
| `wasm-status-widget/` | wasm | Panel + status widget via separate capabilities. |
| `builtin-reference/` | builtin (in-tree Rust) | Walk-through of the `BuiltinRuntime` pattern for codegg contributors. |

### SDKs

#### `sdk-rust/`

A Rust helper crate compiled to `wasm32-unknown-unknown`. Depends on
`codegg-protocol` by path so the wire format cannot drift.

Key entry points:

- `codegg_plugin!(handler_fn)` — macro that exports `allocate`,
  `deallocate`, and `codegg_plugin_invoke`.
- `builders::text_node`, `builders::table_node`, `builders::key_value_node`,
  `builders::markdown_node`, `builders::code_node`, `builders::progress_node`,
  `builders::container_node` — typed `UiNode` constructors.
- `builders::response_chat`, `builders::response_chat_markdown`,
  `builders::response_dialog`, `builders::response_panel`,
  `builders::response_status` — typed `PluginResponse` constructors.
- `builders::ok_response`, `builders::error_response`,
  `builders::diagnostic` — response helpers.

The allocator is a 1 MiB bump allocator (`src/abi.rs`); memory is not
freed per-pointer but the heap is reset per invocation since Wasmtime
re-instantiates the module between calls. This is sufficient for
short-lived plugin responses.

Tested via `cargo test --manifest-path examples/plugins/sdk-rust/Cargo.toml`
(11 tests, one wasm-only test marked `#[ignore]`).

#### `sdk-python/`

A stdlib-only Python helper package, vendorable or pip-installable from
path. Mirrors the Rust builders but emits dict literals matching the
exact wire format.

```python
from codegg_plugin import read_invocation, ok_response, write_response, emit_chat, open_dialog, table_node

inv = read_invocation()
write_response(ok_response(
    effects=[
        emit_chat("Hello"),
        open_dialog("d1", "Title", table_node(["A"], [["1"]]), modal=True),
    ],
    data={"ok": True},
))
```

Tested via `PYTHONPATH=examples/plugins/sdk-python python3 -m unittest
discover examples/plugins/sdk-python/tests -v` (24 tests).

### Wire Format Quick Reference

The full schema lives in `crates/codegg-protocol/src/plugin.rs` and
`crates/codegg-protocol/src/ui.rs`. Plugin authors should re-export the
protocol types from their SDK rather than redefining them.

```json
{
  "protocol_version": 1,
  "invocation_id": "uuid-or-string",
  "plugin_id": "plugin:<name>",
  "capability": {"type": "command", "name": "greet"},
  "args": [],
  "input": {},
  "context": {
    "session_id": null,
    "turn_id": null,
    "project_dir": null,
    "model": null,
    "agent": null,
    "frontend_capabilities": [],
    "metadata": {}
  }
}
```

```json
{
  "ok": true,
  "effects": [
    {"type": "open_dialog", "dialog": {"id": "d1", "title": "T", "body": {"kind": "table", "columns": ["a"], "rows": [["1"]]}, "modal": true}}
  ],
  "data": null,
  "diagnostics": [{"level": "info", "message": "ok"}]
}
```

### Build Validation

```bash
# Python SDK
PYTHONPATH=examples/plugins/sdk-python \
  python3 -m unittest discover examples/plugins/sdk-python/tests -v

# Rust SDK
cargo test --manifest-path examples/plugins/sdk-rust/Cargo.toml

# WASM examples (one-time: rustup target add wasm32-unknown-unknown)
cargo build --target wasm32-unknown-unknown \
  --manifest-path examples/plugins/wasm-command-table/Cargo.toml --release
```

### Safety

- Process plugins are local executables; not sandboxed. Treat them like
  any locally runnable command.
- WASM plugins run inside Wasmtime with per-plugin fuel budgets
  (`MAX_PLUGIN_FUEL_BUDGET = 10_000_000`, default
  `WASM_FUEL_PER_HOOK = 1_000_000`) and memory caps (default 256 MiB).
- All examples in `examples/plugins/` are local-only: no network, no
  secrets, no filesystem writes outside the documented paths.
- The `wasm-hook-message-transform` example uses an observation-only
  event subscription; mutating or blocking lifecycle hooks remain
  denied by default under `PluginPolicy`.

## See Also

- [hooks.md](hooks.md) - External hooks system
- [agent.md](agent.md) - AgentLoop integration with plugins
- [tool.md](tool.md) - Tool execution hooks
- [provider.md](provider.md) - Provider middleware hooks
