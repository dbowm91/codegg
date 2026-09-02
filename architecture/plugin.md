# Plugin Module

## Purpose

Extends agent capabilities via a multi-runtime plugin system: built-in
native-Rust auth hooks, local process commands, and sandboxed WASM
modules.  Provides a capability-based registry, policy-gated lifecycle
hooks, and a management UX for listing, enabling, disabling, diagnosing,
installing, and removing plugins.

## Where It Lives

| Path | Role |
|------|------|
| `src/plugin/mod.rs` | Public re-exports, `create_default_plugin_service()` |
| `src/plugin/activation.rs` | Durable global/workspace activation records and immutable resolved views |
| `src/plugin/registry.rs` | `PluginRegistry`, `PluginInfo`, capability indexing |
| `src/plugin/service.rs` | `PluginService` — hook dispatch and command invocation |
| `src/plugin/manifest.rs` | `PluginManifest`, `PluginCapability`, `PluginRuntimeSpec` |
| `src/plugin/hooks.rs` | `HookType`, `HookContext`, `HookResult` |
| `src/plugin/install.rs` | `install_from_path`, `uninstall`, path validation |
| `src/plugin/management.rs` | `PluginManager`, `PluginManagementView`, doctor |
| `src/plugin/management_ui.rs` | `UiNode`-based plugin list/info/doctor renderers |
| `src/plugin/policy.rs` | `PluginPolicy` (composite), sub-policies |
| `src/plugin/permission.rs` | `PolicyDecision`, `check_*_allowed` functions |
| `src/plugin/lifecycle.rs` | `LifecycleHooks`, typed I/O contracts |
| `src/plugin/runtime/mod.rs` | `PluginRuntime` trait, `RuntimeError`, `RuntimeLimits` |
| `src/plugin/runtime/process.rs` | `ProcessRuntime` implementation |
| `src/plugin/runtime/wasm.rs` | `WasmRuntime` implementation (`plugins` feature) |
| `src/plugin/runtime/wasm_cache.rs` | `WasmModuleCache` — mtime-keyed compiled module cache |
| `src/plugin/runtime/builtin.rs` | `BuiltinRuntime`, `BuiltinHandlerRegistry` |
| `src/plugin/builtin/` | Native auth hook handlers (copilot, gitlab, codex, poe) |
| `src/plugin/loader.rs` | Legacy WASM shim (delegates to `WasmRuntime`) |
| `src/plugin/event_bus.rs` | `PluginEventBus`, subscription management |
| `src/plugin/tui.rs` | `TuiPluginRegistry` (legacy, retained for compat) |
| `src/plugin/marketplace.rs` | Marketplace integration |
| `src/plugin/api.rs` | `API_VERSION`, `ApiVersion`, provider/tool types |
| `crates/codegg-protocol/src/plugin.rs` | Wire types: `PluginInvocation`, `PluginResponse` |
| `crates/codegg-protocol/src/ui.rs` | `UiNode`, `UiEffect`, `UiLimits` |

## How It Works

### Runtime Abstraction

All three runtimes implement the same trait:

```rust
// src/plugin/runtime/mod.rs
#[async_trait]
pub trait PluginRuntime: Send + Sync {
    async fn invoke(
        &self, invocation: PluginInvocation
    ) -> Result<PluginResponse, RuntimeError>;
}
```

| Runtime | Feature Gate | Notes |
|---------|-------------|-------|
| `BuiltinRuntime` | always | Hook-only; no command dispatch |
| `ProcessRuntime` | always | Spawns child process, parses stdout |
| `WasmRuntime` | `plugins` | Wasmtime sandbox with fuel budgets |

Without the `plugins` feature, `WasmRuntime::invoke` returns
`RuntimeError::Unsupported` and WASM plugins silently return passthrough
results.

### Capability-Based Registry

`PluginRegistry` (`registry.rs:144`) indexes five capability types
extracted from manifests at registration time:

- **Commands** — slash-command names + aliases (global uniqueness enforced)
- **Hooks** — `HookType` + priority (sorted ascending)
- **Panels** — auto-namespaced with `{plugin_id}:{raw_id}`
- **Status widgets** — auto-namespaced similarly
- **Event subscriptions** — pattern-matched by event type

All queries filter against an `enabled_plugin_ids()` snapshot acquired
under a single read guard, eliminating lock-contention false negatives.

The registry is the installed-plugin/capability index; it is not the durable
activation authority. `PluginActivationStore` writes user-scoped
`plugin-activation.json` atomically beneath the daemon runtime root. A
`Global` record supplies the default, while a `Workspace(id)` record overrides
it for that stable workspace identity. Missing records preserve the legacy
default-active behavior for non-builtin plugins. Builtin plugins use an
explicit builtin policy so auth/provider compatibility is not changed by
third-party activation records.

`PluginService::for_workspace()` resolves the store against the installed
registry and pins a `ResolvedPluginActivationSet`. Turn/runtime construction
uses this context-bound service, so project A and project B may see different
activation states while sharing one registry and runtime implementation. A
running turn retains its pinned view; later turns resolve later records.

### Hook Dispatch Pipeline

```
Caller (AgentLoop, LifecycleHooks, etc.)
  -> PluginService::dispatch_hook(ctx)
      -> registry.hooks_for(hook_type) → sorted by priority
      -> for each hook:
           1. Policy check (lifecycle, trust, auth-hook trust)
           2. execute_hook_with_timeout()
              builtin:* → BuiltinRuntime or fallback handler lookup
              else     → WasmRuntime (plugins feature) or passthrough
           3. If blocked or error → short-circuit
           4. Output becomes next hook's input
      -> Final HookResult with accumulated effects
```

**Outer timeout** (service.rs:31): 5 seconds, configurable via
`with_hook_timeout()`. **Inner timeout** for WASM: 30 seconds
(`WASM_HOOK_TIMEOUT` in loader.rs).

### Policy System

`PluginPolicy` (`policy.rs:222`) is a composite of five sub-policies,
all defaulting to conservative:

| Sub-Policy | Default | Controls |
|------------|---------|----------|
| `PluginLifecyclePolicy` | observation=on, mutating/blocking=off | Hook type gating |
| `PluginUiPolicy` | dialog/toast=on, panel/status=off | UI effect surfaces |
| `PluginPermissionPolicy` | secrets=deny, env=deny, auth-high-trust=yes | Secret/env/trust |
| `PluginInstallPolicy` | traversal=reject, outside-dir=refuse | Install safety |
| `PluginRuntimePolicy` | undeclared=deny, unknown-surfaces=deny | Capability enforcement |

`PolicyDecision` variants: `Allow`, `Deny(reason)`, `Degrade(reason)`.

### Management UX

`PluginManager` (`management.rs:201`) wraps `PluginService` and provides:

| Method | Description |
|--------|-------------|
| `list()` | All plugins as `PluginManagementView` |
| `info(selector)` | Single plugin by id/name/prefix |
| `enable(selector)` | Persist a global activation override |
| `disable(selector)` | Persist a global deactivation override |
| `enable_for_workspace(selector, id)` | Persist a workspace activation override |
| `disable_for_workspace(selector, id)` | Persist a workspace deactivation override |
| `install_from_path(path)` | Copy + register + live registry |
| `uninstall(selector)` | Validate + unregister + rm |
| `doctor(selector)` | Read-only diagnostic checks |

**Selector resolution** (registry.rs:281): exact id → exact name →
unique prefix on id → unique prefix on name → error on ambiguous/none.

## Key Types & APIs

### Core Types

```rust
// src/plugin/manifest.rs:46
pub enum PluginRuntimeSpec {
    Builtin { handler: String },
    Process { command, args, timeout_ms },
    Wasm { module, timeout_ms, memory_max_mb, fuel_per_call },
}

// src/plugin/manifest.rs:75
pub enum PluginCapability {
    Command(PluginCommandSpec),
    Hook(PluginHookSpec),
    Panel(PluginPanelContribution),
    StatusWidget(PluginStatusContribution),
    EventSubscription(PluginEventSubscriptionSpec),
}

// src/plugin/registry.rs:12
pub struct PluginInfo {
    pub id, pub manifest, pub enabled,
    pub trust, pub diagnostics, pub source,
}

// src/plugin/registry.rs:28
pub struct PluginSourceMetadata { pub install_path, pub original_source_path, pub installed_by }

// src/plugin/registry.rs:42
pub enum PluginInstallKind { Builtin, LocalPath, RegistryLoaded, Unknown }
```

### Hook Types

```rust
// src/plugin/hooks.rs:4
pub enum HookType {
    Auth, Provider, ToolDefinition, ToolExecuteBefore,
    ToolExecuteAfter, ChatParams, ChatHeaders, Event, Config,
    ShellEnv, TextComplete, SessionCompacting, MessagesTransform,
}

// src/plugin/hooks.rs:61
pub struct HookContext { pub hook_type, pub input }

// src/plugin/hooks.rs:92
pub struct HookResult { pub output, pub blocked, pub error, pub effects }
```

### Service & Error

```rust
// src/plugin/service.rs:20
pub struct PluginService {
    registry, hook_timeout, builtin_runtime, policy,
    activation_store, pinned_activation,
}

// src/plugin/service.rs:530
pub enum PluginError { CommandNotFound, PluginNotFound, PluginDisabled, Registry, Runtime }
```

### Permission Checks

```rust
// src/plugin/permission.rs:9
pub enum PolicyDecision { Allow, Deny(String), Degrade(String) }

pub fn check_invocation_allowed(...) -> PolicyDecision;
pub fn check_ui_effect_allowed(...) -> PolicyDecision;
pub fn check_lifecycle_hook_allowed(...) -> PolicyDecision;
pub fn check_secret_access_allowed(...) -> PolicyDecision;
```

## Configuration Surface

Plugins are loaded from the platform-specific plugins directory:

```
~/.local/share/codegg/plugins/          (Linux)
~/Library/Application Support/codegg/plugins/  (macOS)
%LOCALAPPDATA%\codegg\plugins\          (Windows)
```

Resolved via `dirs::data_local_dir()` in `install.rs:33`.

**Manifest format** (`manifest.toml`):

```toml
name = "my-plugin"
version = "1.0.0"
api_version = 1

[runtime]
kind = "process"
command = "python3"
args = ["plugin.py"]

[[capabilities]]
type = "hook"
hook_type = "tool.execute.before"

[[capabilities]]
type = "command"
name = "my-cmd"
description = "Run my command"
```

**Feature flag**: `plugins` enables WASM support (`Cargo.toml`).
Controlled via `cargo build --features plugins`.

## Invariants & Gotchas

- **BuiltinRuntime is hook-only**: Command invocations against builtin
  plugins return `PluginError::Runtime`. No command handler registry.
- **Installation and activation are separate**: enable/disable writes the
  daemon-owned activation store. The default management methods write a
  global override; context-aware callers may write a workspace override.
  Re-registration reloads durable state, and stale install identities remain
  inactive with diagnostics.
- **Global command uniqueness**: Two plugins cannot register the same
  normalized command name (leading `/` stripped, lowercased). Second
  registration fails with `DuplicateCommand`.
- **Alias collision**: Command aliases participate in duplicate detection
  against both names and other aliases.
- **Panel/status auto-namespacing**: IDs are prefixed with `{plugin_id}:`
  at registration time; already-prefixed IDs are not double-prefixed.
- **Fuel accounting**: Failed/timed-out WASM invocations return full
  reserved fuel (no burn). Successful calls return unused fuel; the
  budget decreases by consumed amount only.
- **Install source path policy**: `validate_local_install_source`
  rejects `..` lexically before canonicalizing. `validate_install_source`
  rejects `ParentDir`, `RootDir`, `Prefix` before canonicalizing.
  Archive entries use strict `validate_relative_install_path`.
- **Uninstall validation**: `validate_uninstall_target` canonicalizes
  the target and checks it is under the plugins dir.
- **`PluginResponse` is protocol-owned**: The local `PluginResponse` in
  `service.rs` is removed. `codegg_protocol::plugin::PluginResponse`
  (with `effects: Vec<UiEffect>`) is the canonical type, re-exported
  from `plugin/mod.rs:21`.
- **`HookResult.effects`**: The local `HookResult` in `hooks.rs:92`
  carries `effects: Vec<UiEffect>`. The legacy `api::hooks::HookResult`
  in `api.rs:86` is a separate type without effects.

## Testing

```bash
# Single crate (includes plugin tests)
cargo test -p codegg

# Plugin-specific integration (install, manifest, registry)
cargo test -p codegg -- plugin

# SDKs
cargo test --manifest-path examples/plugins/sdk-rust/Cargo.toml
PYTHONPATH=examples/plugins/sdk-python \
  python3 -m unittest discover examples/plugins/sdk-python/tests -v

# Validation script
./scripts/validate_plugin_ui.sh
```

## Related Docs

- [hooks.md](hooks.md) — external hooks system
- [agent.md](agent.md) — AgentLoop integration with plugins
- [tool.md](tool.md) — tool execution hooks
- [provider.md](provider.md) — provider middleware hooks
- `crates/codegg-protocol/src/plugin.rs` — wire types
- `crates/codegg-protocol/src/ui.rs` — UI node and effect types
