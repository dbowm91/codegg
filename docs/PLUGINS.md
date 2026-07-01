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
1. Reserved before hook execution (full amount subtracted from budget)
2. Consumed during execution (Wasmtime tracks actual fuel usage)
3. **Unused portion returned to the budget** after completion — never the consumed amount

The fuel budget decreases by exactly the consumed amount per invocation.
On error, the full reserved amount is returned to the budget so failed
invocations do not burn fuel.

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

## Plugin UI Output Surfaces

Plugins can emit UI effects through lifecycle hooks and process command responses:

| Surface | Description | Capabilities Flag |
|---------|-------------|-------------------|
| **Toast** | Transient notification | `toast` |
| **Dialog** | Modal dialog with title and body | `dialog` |
| **Panel** | Persistent side panel (Left/Right) | `panel` |
| **Status Item** | Status bar indicator | `status_item` |
| **Chat Block** | Inline chat message rendered to user (toast for short blocks, info dialog for long blocks) | `toast` (shares flag) |

**Chat Block visibility semantics:** EmitChat effects are rendered visibly
to the user via the toast / info-dialog surface. They are **not** added to
the model-visible chat transcript unless a downstream integration
explicitly promotes them. Both `ChatFormat::Plain` and `ChatFormat::Markdown`
are lowered to line-based text — markdown links and embedded escape
sequences are not executed.

Surface IDs are namespaced by plugin ID: `plugin:<plugin-name>:<surface-id>`. Cross-plugin surface ID usage is rejected at apply time.

## Degradation Rules

When a client does not support a given surface type, effects degrade deterministically via `degrade_node_to_text()`:

| Effect | Unsupported Behavior |
|--------|---------------------|
| Dialog | Chat block or toast summary |
| Panel | Chat block |
| Table | Markdown table |
| Status Item | Omitted unless important |
| Markdown | Plain text |
| Code | Plain text with language header |
| Progress | Text percentage |

The TUI reference client supports all surface types. Remote/external clients negotiate capabilities via `ClientCapabilities` flags.

## Capability Enforcement

`PluginUiCapabilities` tracks which surface types a client supports. Effects are checked against capabilities before application:
- Unsupported effects return `PluginUiApplyResult::Unsupported`
- Toasts and EmitChat always pass (universal support)
- Cross-plugin ID spoofing is rejected when a `source_plugin_id` is provided

## Built-in Plugins

The `builtin/` directory contains:
- `poe.rs` - Poe API integration
- `gitlab.rs` - GitLab integration
- `copilot.rs` - GitHub Copilot integration
- `codex.rs` - OpenAI Codex integration

Built-in plugins now use `BuiltinRuntime` as a first-class runtime (Phase 8). They register through the unified plugin registry with `runtime = "builtin"` and dispatch through `BuiltinRuntime`, sharing the same invocation path as process and WASM plugins.

**Builtin runtime scope:** `BuiltinRuntime` is **hook-only**. It dispatches
`PluginCapabilityInvocation::Hook` invocations only. The following are
explicitly rejected with `RuntimeError::Unsupported`:

- `PluginCapabilityInvocation::Command` (no builtin command handler exists)
- `PluginCapabilityInvocation::Panel`, `StatusWidget`, `Event`
- Unknown hook type strings (e.g. `"command"`) — they no longer silently
  fall back to `HookType::Auth`
- Plugin IDs that do not start with the `builtin:` prefix

Builtin plugins that declare a command capability but do not provide a
runtime command handler will fail at `PluginService::invoke_command` with
a `PluginError::Runtime` rather than silently returning a success
placeholder.

## Plugin Management Commands

First-class slash commands for local plugin management:

- `/plugins` — List all installed and built-in plugins with status and capability summary
- `/plugin-info <id>` — Show detailed plugin info (runtime, capabilities, trust, permissions, diagnostics)
- `/plugin-enable <id>` — Enable a plugin (persisted to disabled_plugins.toml)
- `/plugin-disable <id>` — Disable a plugin (persisted to disabled_plugins.toml)
- `/plugin-doctor [id]` — Run diagnostic checks on plugin configuration and health
- `/plugin-remove <id>` — Remove a locally installed plugin (safe: only from plugin install dir)
- `/plugin-install <path>` — Install a plugin from a local directory path

### Selector Resolution

Plugin selectors resolve in order: exact id → exact name → unique id prefix → unique name prefix. Ambiguous matches produce clear error messages.

### Safety

- Enable/disable state persists across sessions
- Remove only deletes from the canonical plugin directory (`~/.local/share/codegg/plugins/`)
- Install validates manifest.toml before copying and rejects invalid manifests
- Doctor checks are read-only and never execute plugin code by default

## Security Policy (Phase 12)

`PluginPolicy` (`src/plugin/policy.rs`) is an opt-in composite policy that gates plugin invocations against manifest declarations and trust requirements.

### Sub-Policy Defaults

| Sub-Policy | Default | Controls |
|------------|---------|----------|
| `PluginLifecyclePolicy` | Observation hooks allowed; mutating/blocking/process denied | Hook type + runtime gating |
| `PluginUiPolicy` | Dialog/toast allowed; panel/status denied | UI effect surfaces |
| `PluginPermissionPolicy` | All capabilities denied unless declared | Command/hook declarations |
| `PluginInstallPolicy` | Env passthrough denied | Environment variable access |
| `PluginRuntimePolicy` | Secrets denied; auth-hook requires high trust | Secret access, high-trust gating |

### PolicyDecision

```rust
pub enum PolicyDecision {
    Allow,
    Deny { reason: String },
    Degrade { reason: String },
}
```

Four check functions validate invocations:
- `check_invocation_allowed` — command/hook must match a declared capability
- `check_ui_effect_allowed` — UI effects gated by output surface declarations
- `check_lifecycle_hook_allowed` — hook type and trust class validated; auth hooks require high trust
- `check_secret_access_allowed` — secret access must match declared permissions

### Integration

`PluginService::with_policy(Arc<PluginPolicy>)` enables policy enforcement. When set:
- `invoke_command()` rejects undeclared commands
- `dispatch_hook()` checks hook type, trust class, and auth-hook high-trust requirement; denied hooks are skipped with logging

When policy is absent, all checks pass (backward compatible).

> **Process plugins are local executable code. They are not sandboxed.** They are suitable for explicit user-invoked local commands, not silent lifecycle interception by default.
