# Plugin System

codegg supports WASM-based plugins that can hook into the agent lifecycle and extend functionality.

## Architecture

The plugin module (`src/plugin/`) consists of:

- **`mod.rs`** - Main exports and `PluginService`
- **`loader.rs`** - WASM module loading with caching and fuel tracking
- **`hooks.rs`** - `HookType` enum and hook context/result types
- **`registry.rs`** - `PluginRegistry` for managing installed plugins
- **`service.rs`** - `PluginService` for hook dispatch and execution
- **`manifest.rs`** - `PluginManifest` parsing from `manifest.toml`
- **`install.rs`** - Plugin installation from path or URL
- **`event_bus.rs`** - Event bus integration for plugins
- **`tui.rs`** - TUI component extensions
- **`builtin/`** - Built-in plugins (poe, gitlab, copilot, codex)

## Plugin Manifest

Each plugin requires a `manifest.toml`:

```toml
name = "my-plugin"
version = "1.0.0"
description = "A sample plugin"
api_version = "0.1.0"

[hooks]
auth = "on_auth"
provider = "on_provider"
tool_definition = "on_tool_definition"
tool_execute_before = "on_tool_execute_before"
tool_execute_after = "on_tool_execute_after"
chat_params = "on_chat_params"
chat_headers = "on_chat_headers"
event = "on_event"
config = "on_config"
shell_env = "on_shell_env"
text_complete = "on_text_complete"
session_compacting = "on_session_compacting"
messages_transform = "on_messages_transform"
```

## Hook System

Plugins register hooks to intercept and modify behavior at various points:

### Hook Types

```rust
pub enum HookType {
    Auth,               // Authentication hooks
    Provider,           // Provider selection
    ToolDefinition,     // Modify tool definitions
    ToolExecuteBefore,  // Before tool execution
    ToolExecuteAfter,   // After tool execution
    ChatParams,         // Modify chat parameters
    ChatHeaders,        // Modify request headers
    Event,              // Global event handling
    Config,             // Configuration loading
    ShellEnv,           // Shell environment
    TextComplete,       // Text completion
    SessionCompacting,  // Context compaction
    MessagesTransform,  // Transform messages
}
```

### Hook Context and Result

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
```

## WASM Execution

Plugins are executed in a WASM sandbox using Wasmtime:

### Fuel Tracking

Each plugin has a fuel budget to prevent infinite loops:

```rust
const MAX_PLUGIN_FUEL_BUDGET: u64 = 10_000_000;  // 10M fuel units
const WASM_FUEL_PER_HOOK: u64 = 1_000_000;       // 1M per hook call
const FUEL_RESET_INTERVAL_SECS: u64 = 60;         // Reset every 60s
```

Fuel is:
1. Reserved before hook execution
2. Consumed during execution (Wasmtime tracks actual fuel usage)
3. Returned for unused portion after completion

### Module Cache

WASM modules are cached with mtime-based invalidation:

```rust
pub struct ModuleCache {
    modules: DashMap<String, (Module, u64)>,  // path -> (module, mtime)
    hits: AtomicU64,
    misses: AtomicU64,
    fuel_budgets: DashMap<String, AtomicU64>,
}
```

### Execution Flow

1. Check plugin has remaining fuel budget
2. Reserve fuel for this call
3. Load/check module from cache
4. Create WASM store with fuel tracking
5. Call hook function with serialized input
6. Parse hook response
7. Return unused fuel

### WASM Memory Model

Plugins use a memory-first interface:
1. Plugin exports `allocate(size)` function
2. codegg writes input JSON to allocated memory
3. Plugin processes and writes output
4. codegg reads output (prefixed with length)
5. codegg calls `deallocate(ptr, size)` if exported

## Plugin Service

The `PluginService` dispatches hooks to registered plugins:

```rust
pub struct PluginService {
    registry: PluginRegistry,
    api_version: ApiVersion,
}
```

Key methods:
- `dispatch_hook()` - Execute a single hook
- `dispatch_tool_definition()` - Modify tool definitions
- `register_plugin()` - Add a plugin to the registry

## Installation

Plugins can be installed from:

### Local Path
```rust
install_from_path(&path, &mut registry)
```

### Remote URL
```rust
install_from_url(url, &mut registry).await
```

## TUI Extensions

Plugins can register custom TUI components:

```rust
pub struct TuiComponent {
    pub name: String,
    pub route: TuiRoute,
    pub render_fn: Box<dyn Fn(&mut Frame, &AppState) + Send + Sync>,
}
```

## Security

- WASM plugins are fuel-bounded
- Fuel prevents infinite loops
- Memory access is sandboxed
- Module cache invalidates on file change

## Built-in Plugins

The `builtin/` directory contains:
- `poe.rs` - Poe API integration
- `gitlab.rs` - GitLab integration
- `copilot.rs` - GitHub Copilot integration
- `codex.rs` - OpenAI Codex integration
