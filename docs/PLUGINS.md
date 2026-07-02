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

Each installed plugin requires a `manifest.toml`. The canonical (Phase 5) format
declares runtime, capabilities, and permissions explicitly:

```toml
name = "my-plugin"
version = "0.1.0"
api_version = 1

[runtime]
kind = "wasm"
module = "plugin.wasm"
timeout_ms = 5000
memory_max_mb = 16
fuel_per_call = 1000000

[[capabilities]]
type = "command"
name = "greet"
aliases = ["hi"]
description = "Print a greeting"
output = ["chat", "dialog"]

[permissions]
network = false
filesystem = "none"
```

For a process-backed plugin, use `kind = "process"` and provide `command` and
`args`:

```toml
[runtime]
kind = "process"
command = "python3"
args = ["scripts/quota.py"]
timeout_ms = 5000
```

For a builtin plugin (compiled into codegg), use `kind = "builtin"` and
identify the handler:

```toml
[runtime]
kind = "builtin"
handler = "copilot"
```

The legacy flat format (a top-level `[hooks]` table mapping hook type to export
name) is still accepted and is auto-converted to `[[capabilities]]` entries on
load. New manifests should use the canonical form above.

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

## Frontend Compatibility (Phase 15)

Phase 15 makes plugin UI, management, lifecycle effects, and durable plugin surfaces robust across embedded TUI, remote TUI, daemon/socket clients, and future GUI/web/mobile frontends.

### Supported Frontend Classes

| Frontend | Transport | Renders |
|----------|-----------|---------|
| **Embedded TUI** | In-process | All surfaces (dialogs, panels, status items, tables, markdown, code, progress) |
| **Remote TUI** | WebSocket/socket | All surfaces via `TuiMessage::PluginUiEffect` and snapshots |
| **CLI/Automation** | stdio | Degraded text output; dialogs/panels/status items omitted |
| **GUI/Web/Mobile** | Future | Consumes `UiNode`/`UiEffect` protocol without TUI-specific naming |

### Capability Negotiation

Clients declare their capabilities via `ClientCapabilities` (in `crates/codegg-protocol/src/frames.rs`). The protocol defines:

- `plugin_ui_dialogs`, `plugin_ui_panels`, `plugin_ui_status_items` — surface types
- `plugin_ui_tables`, `plugin_ui_markdown`, `plugin_ui_code`, `plugin_ui_progress` — node types
- `visual_notifications`, `desktop_notifications`, `audio`, `tts`, `multi_session_view` — general

All fields default to `false`. `ClientCapabilities::plugin_ui_capabilities()` converts the `plugin_ui_*` fields into a `PluginUiCapabilities` struct for capability-aware degradation.

### Effect Transport

Plugin UI effects cross process/core/frontend boundaries wrapped in a typed envelope:

```rust
pub struct UiEffectEnvelope {
    pub session_id: Option<String>,
    pub source: UiEffectSource,    // Plugin { plugin_id } | Core | Tui
    pub invocation_id: Option<String>,
    pub effect: UiEffect,
}
```

**Transport rules:**

- **Session-scoped effects** flow through core event transport: `PluginRuntime → PluginResponse.effects → AppEvent::PluginUiEffect → CoreEvent::PluginUiEffect → subscribed clients`.
- **Local-only effects** (e.g. from `/plugins`) use `UiEffectSource::Tui` and stay local.
- **Durable surfaces** (panels, status items) are included in snapshots for reconnect fidelity.
- **Transient surfaces** (dialogs, toasts) are not persisted and are not in reconnect snapshots.

### Size Limits

`UiLimits` in `crates/codegg-protocol/src/ui.rs` defines bounded resource caps:

| Limit | Default (balanced) | text_only | Purpose |
|-------|-------------------|-----------|---------|
| `max_effects_per_response` | 32 | 8 | Prevents effect flooding |
| `max_effect_bytes` | 64 KiB | 16 KiB | Per-effect serialization cap |
| `max_node_depth` | 16 | 4 | Recursive node depth |
| `max_table_rows` | 256 | 16 | Table size cap |
| `max_table_columns` | 32 | 8 | Table width cap |
| `max_string_len` | 8192 | 1024 | Per-string truncation |
| `max_panels_per_plugin` | 8 | 0 | Durable panel count |
| `max_status_items_per_plugin` | 8 | 0 | Status item count |
| `max_open_dialogs_global` | 4 | 0 | Global dialog cap |
| `max_snapshot_body_bytes` | 16 KiB | 4 KiB | Snapshot body serialization cap |

Effects exceeding limits are rejected with `UiValidationError`. Policy never panics — it denies or truncates with diagnostics.

### Source Attribution and Ownership

Every plugin UI effect carries source metadata when crossing boundaries:

- `plugin_id` — owning plugin (from `UiEffectSource::Plugin`)
- `invocation_id` — correlates effect with the original invocation
- `session_id` — scopes delivery to session subscribers

**Surface-ownership rules:**

- `source.plugin_id` must match durable surface id namespace (e.g. panel id `my-plugin:stats` must come from `plugin_id == "my-plugin"`).
- Cross-plugin updates are rejected at apply time.
- Missing source id for durable effects is rejected or namespaced under a safe synthetic source.
- `UiEffectSource::Core` effects skip plugin-ownership checks (trusted core-originated effects).

### Snapshot Durability

Remote snapshots (`RemoteTuiStateSnapshot`) include durable plugin surface metadata:

- **Panels**: `id`, `title`, `placement`, `source_plugin_id`, `body` (if size-safe)
- **Status items**: `id`, `label`, `placement`, `source_plugin_id`, `body` (if size-safe)

The snapshot builder populates `body` only when the serialized size is ≤ `SNAPSHOT_BODY_LIMIT` (16 KiB). Bodies exceeding the cap are omitted; metadata alone is sufficient for clients to fetch the body via replay/resync.

Both `source_plugin_id` and `body` are optional with `skip_serializing_if`, so legacy snapshots without these fields deserialize cleanly.

### Multi-Client Behavior

When multiple frontends are connected:

- Session-scoped plugin effects are delivered to subscribers of that session.
- Durable state changes update snapshots for new/reconnected clients via `RequestSnapshot`.
- Local-only effects (from `UiEffectSource::Tui`) do not leak to remote clients.
- Automation clients with limited capabilities receive degraded text or have effects ignored safely.
- Unsupported clients never block on UI effects — validation/degradation is synchronous.

### Canonical Entry Point

`App::apply_plugin_ui_envelope(envelope)` in `src/tui/app/mod.rs` is the canonical entry point for all plugin UI effects regardless of transport (local TUI command or remote WebSocket). It:

1. Derives `source_plugin_id` from the envelope.
2. Runs the session guard (drops effects for non-matching session).
3. Validates against `UiLimits::balanced()`.
4. Enforces surface-ownership rules.
5. Delegates to `App::apply_plugin_ui_effect(effect, plugin_id_opt)`.

`App::validate_plugin_ui_effects(effects)` is the batch validator used by lifecycle hooks and the event bridge.

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

## Quickstart Examples and SDKs (Phase 13)

The `examples/plugins/` directory contains small, tested reference
plugins and SDK helpers. Use them as templates rather than reverse
engineering the protocol from `codegg-protocol`.

| Path | What it demonstrates |
|------|---------------------|
| `examples/plugins/process-quota-text/` | Zero-SDK process plugin emitting plain text stdout (auto-detected as EmitChat). |
| `examples/plugins/process-quota-json/` | Process plugin reading `PluginInvocation` JSON from stdin and emitting `PluginResponse` JSON with effects. |
| `examples/plugins/wasm-command-table/` | WASM plugin using the modern `codegg_plugin_invoke` ABI; returns an OpenDialog with a Table. |
| `examples/plugins/wasm-hook-message-transform/` | WASM observation hook via the `event_subscription` capability (low-risk, default-policy-permitted). |
| `examples/plugins/wasm-status-widget/` | WASM plugin contributing an OpenPanel and an AddStatusItem. |
| `examples/plugins/builtin-reference/` | Walk-through of the builtin pattern for codegg contributors (not for external plugin authors). |
| `examples/plugins/sdk-python/` | Vendorable Python helper package (stdlib only): protocol I/O + builders. |
| `examples/plugins/sdk-rust/` | Rust helper crate with the `codegg_plugin!` macro and typed builders for WASM plugins. |

### Wire format

All process and WASM plugins exchange JSON `PluginInvocation` and
`PluginResponse` objects. Both use snake_case field names; tagged enums
use different tag keys (`kind` for `UiNode`, `type` for `UiEffect` and
`PluginCapabilityInvocation`, `kind` for `PluginRuntimeSpec`). The
canonical definitions live in:

- `crates/codegg-protocol/src/plugin.rs` — `PluginInvocation`,
  `PluginResponse`, `PluginCapabilityInvocation`, `PluginContext`,
  `PluginDiagnostic`.
- `crates/codegg-protocol/src/ui.rs` — `UiNode`, `UiEffect`, `ChatBlock`,
  `DialogSpec`, `PanelSpec`, `StatusItemSpec`.

The current protocol version is `PLUGIN_PROTOCOL_VERSION = 1`. Process
plugins must validate `protocol_version` and emit a structured error
response (not a crash) on mismatch.

### Build matrix

```bash
# Python SDK (24 tests)
PYTHONPATH=examples/plugins/sdk-python \
  python3 -m unittest discover examples/plugins/sdk-python/tests -v

# Rust SDK (11 tests, 1 wasm-only ignored)
cargo test --manifest-path examples/plugins/sdk-rust/Cargo.toml

# WASM examples (require wasm32-unknown-unknown target)
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown \
  --manifest-path examples/plugins/wasm-command-table/Cargo.toml --release
```

### Installing an example

Process-based project-local commands live in `command/*.md` next to the
project root. WASM-based plugins install into the platform plugin
directory (`~/.local/share/codegg/plugins/` on Linux,
`~/Library/Application Support/codegg/plugins/` on macOS,
`%LOCALAPPDATA%\codegg\plugins\` on Windows). See
`architecture/plugin.md` for the canonical install paths and
`/plugin-install` for the in-TUI installer.
