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
| `src/plugin/contributions.rs` | Passive contribution resolution (assets + MCP declarations) |
| `src/plugin/package.rs` | Portable package detection and passive translation (parse-only) |
| `src/plugin/playwright.rs` | PATH-only Playwright bundle detection (never installs, never probes) |
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
| `src/plugin/tui.rs` | `TuiPluginRegistry` — **deprecated** pre-Phase-5 TUI extension types; new UI goes through manifest capabilities + protocol `UiEffect` |
| `src/plugin/marketplace.rs` | Bounded extension catalog discovery and explicit host-mediated install |
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

`src/plugin/runtime/wasm.rs` is compiled **only** under the `plugins`
feature (`#[cfg(feature = "plugins")] pub mod wasm;` in `runtime/mod.rs:3-4`),
and every item in `runtime/wasm_cache.rs` is gated the same way, so without
that feature there is no `WasmRuntime` type and no cache entries at all.
Each caller then degrades on its own terms:

| Path | Without `plugins` |
|------|-------------------|
| Command (`service.rs`) | `PluginResponse { ok: false, data: {"status": "wasm_runtime_disabled"} }` plus an `Info` diagnostic |
| Tool (`service.rs`) | `Err(PluginError::Runtime("WASM runtime requires the 'plugins' feature"))` |
| Hook (`service.rs`) | passthrough — `HookResult::ok(ctx.input)` |

`RuntimeError::Unsupported` is therefore *not* the no-feature signal; it is
raised **inside** the feature when a WASM module lacks the expected export.

`PluginRuntimeSpec::Passive` is a manifest-only marker, not a fourth
runtime: there are exactly three `impl PluginRuntime for` sites
(`runtime/builtin.rs:87`, `runtime/process.rs:91`, `runtime/wasm.rs:115`).
Command invocation against a passive plugin returns `PluginError::Runtime`
(`service.rs:331`, "passive plugin has no executable command runtime"); the
parallel tool path refuses at `service.rs:484`.

### Capability-Based Registry

`PluginRegistry` (`registry.rs:173`) indexes six capability types
extracted from manifests at registration time:

- **Commands** — slash-command names + aliases (global uniqueness enforced)
- **Tools** — model-facing tool names contributed by the plugin
- **Hooks** — `HookType` + priority (sorted ascending)
- **Panels** — auto-namespaced with `{plugin_id}:{raw_id}`
- **Status widgets** — auto-namespaced similarly
- **Event subscriptions** — pattern-matched by event type

The six match `PluginCapability` (`manifest.rs:79`) exactly: `Command`,
`Tool`, `Hook`, `Panel`, `StatusWidget`, `EventSubscription`.

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

**Outer timeout** (`service.rs:41`, `hook_timeout`): 5 seconds, configurable
via `with_hook_timeout()`. **Inner timeout** for WASM: 30 seconds, set
inline at `loader.rs:151` as `RuntimeLimits { timeout_ms: 30_000 }` (and
`Some(30_000)` in the `WasmRuntimeSpec` at `loader.rs:142`). There is no
`WASM_HOOK_TIMEOUT` constant.

Process commands use a separate budget: `runtime::DEFAULT_TIMEOUT_MS`
(`runtime/mod.rs:12`) is 5 000 ms and is the fallback when the manifest
declares no `timeout_ms`; `ProcessRuntime::invoke` resolves
`spec.timeout_ms` first and the runtime limit second, and exceeding it
yields `RuntimeError::Timeout { timeout_ms }`.

### Policy System

`PluginPolicy` (`policy.rs:223`) is a composite of five sub-policies,
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

`PluginManager` (`management.rs:275`) wraps `PluginService` and provides:

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
| `remove(selector)` | Remove from the installed set |
| `doctor(selector)` | Read-only diagnostic checks |
| `doctor_all()` | Read-only diagnostics for every installed plugin |

**Selector resolution** (registry.rs:324): exact id → exact name →
unique prefix on id → unique prefix on name → error on ambiguous/none.

## Key Types & APIs

### Core Types

```rust
// src/plugin/manifest.rs:49
pub enum PluginRuntimeSpec {
    Passive,
    Builtin { handler: String },
    Process { command, args, timeout_ms },
    Wasm { module, timeout_ms, memory_max_mb, fuel_per_call },
}

// src/plugin/manifest.rs:79
pub enum PluginCapability {
    Command(PluginCommandSpec),
    Tool(PluginToolSpec),
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

// src/plugin/registry.rs:45
pub enum PluginInstallKind { Builtin, LocalPath, RegistryLoaded, Unknown }
```

### Hook Types

```rust
// src/plugin/hooks.rs:6
pub enum HookType {
    Auth, Provider, ToolDefinition, ToolExecuteBefore,
    ToolExecuteAfter, ChatParams, ChatHeaders, Event, Config,
    ShellEnv, TextComplete, SessionCompacting, MessagesTransform,
}

// src/plugin/hooks.rs:62
pub struct HookContext { pub hook_type, pub input }

// src/plugin/hooks.rs:93
pub struct HookResult { pub output, pub blocked, pub error, pub effects }
```

`HookType` has exactly 13 variants. Its wire spelling is dotted
(`"tool.execute.before"`, `"chat.params"`, `"session.compacting"`, …) via
`HookType::as_str` / `HookType::parse`; the `strum` `Display`/`EnumString`
derive is snake_case and is **not** the wire form.

### Service & Error

```rust
// src/plugin/service.rs:24
pub struct PluginService {
    registry, hook_timeout, builtin_runtime, policy,
    activation_store, pinned_activation, workspace_root,
}

// src/plugin/service.rs:814
pub enum PluginError { CommandNotFound, PluginNotFound, PluginDisabled, Registry, Runtime }
```

`builtin_runtime` and `policy` are both `Option<…>` — an absent policy means
"no policy installed", not "allow everything"; the tool path clears
`response.effects` outright when no policy is present.

`workspace_root` is the authoritative root threaded in from the daemon's
`ExecutionContext`; it is the fallback for the process CWD when unset
(tests and standalone use) and must not be confused with the process-global
CWD.

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

Resolved by `install::plugins_dir()` (`install.rs:33`) via
`dirs::data_local_dir()` (`install.rs:34`).

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
- **Passive contributions**: Optional manifest `contributions` entries package
  bounded skills, CodeGG-compatible agents, instruction fragments, and MCP
  declarations. They are resolved from the immutable workspace activation
  view; discovery never invokes a plugin runtime. Asset inputs flow through
  `ProjectAssetSnapshotBuilder`, MCP inputs through `McpService`, and each
  identity is namespaced with `plugin:<plugin-name>:`. MCP contribution types
  accept `local`/`stdio` for the existing local stdio transport and
  `remote`/`http` for the existing remote HTTP transport; aliases are
  canonicalized before `McpService` handles the connection.
- **Origin and inspection**: MCP servers retain configured/plugin origin and
  reconciliation removes only the owning plugin's servers. Management info
  and doctor expose counts and validation diagnostics without bodies or
  secrets. Project-native assets retain precedence over plugin defaults.
- **`PluginResponse` is protocol-owned**: The local `PluginResponse` in
  `service.rs` is removed. `codegg_protocol::plugin::PluginResponse`
  (with `effects: Vec<UiEffect>`) is the canonical type, re-exported
  from `plugin/mod.rs:25`.
- **`HookResult` is single-definition**: `src/plugin/hooks.rs:93` is the only
  `HookResult` in the tree; it carries `effects: Vec<UiEffect>`.
  `api::hooks` (`api.rs:39`) **re-exports** the canonical
  `HookContext`/`HookResult`/`HookType` instead of keeping a divergent copy
  — there is no separate legacy `api::hooks::HookResult` any more, so a
  second definition would be a regression.
- **Plugin tool output is host-bounded**: a plugin tool's `data` is capped at
  `min(max_output_bytes, 256 KiB)`; over the cap it is replaced with
  `{"status": "output_truncated", "text": …}` and a `Warning` diagnostic is
  appended.

## Testing

```bash
# Single crate (includes plugin tests)
cargo test -p codegg

# Plugin-specific integration (install, manifest, registry)
cargo test -p codegg -- plugin

# Plugin integration members of the `agent_loop` test family
cargo test --test agent_loop -- plugin
cargo test --test agent_loop -- plugin_contributions

# WASM plugin support is behind a feature
cargo test -p codegg --features plugins -- plugin

# SDKs
cargo test --manifest-path examples/plugins/sdk-rust/Cargo.toml
PYTHONPATH=examples/plugins/sdk-python \
  python3 -m unittest discover examples/plugins/sdk-python/tests -v

# Validation script
./scripts/validate_plugin_ui.sh
```

The SDKs are **separate** cargo/python projects and are not part of the
workspace, so `cargo test --workspace` does not run them.

## Related Docs

- [hooks.md](hooks.md) — external hooks system
- [agent.md](agent.md) — AgentLoop integration with plugins
- [tool.md](tool.md) — tool execution hooks
- [provider.md](provider.md) — provider middleware hooks
- `crates/codegg-protocol/src/plugin.rs` — wire types
- `crates/codegg-protocol/src/ui.rs` — UI node and effect types

## Source Verification

Verified 2026-10-06 against `src/plugin/` (22 modules).
- Corrected the capability count **five → six** and added the missing
  **Tools** bullet. `PluginCapability` (`manifest.rs:79`) has exactly 6
  variants — `Command`, `Tool`, `Hook`, `Panel`, `StatusWidget`,
  `EventSubscription` — so the previous list was missing `Tool` outright.
- Corrected 4 stale line refs: `PluginRegistry` `registry.rs:144`→`:173`,
  `PluginPolicy` `policy.rs:222`→`:223`, `PluginManager`
  `management.rs:256`→`:275`, selector resolution `registry.rs:281`→`:324`.
- Corrected the hook-timeout refs. The 5s outer timeout is
  `service.rs:41` (`hook_timeout`), not `service.rs:31` (a struct field).
  There is **no `WASM_HOOK_TIMEOUT` constant**; the 30s inner WASM timeout is
  set inline at `loader.rs:151` (`RuntimeLimits { timeout_ms: 30_000 }`) and
  `loader.rs:142` (`Some(30_000)` in the `WasmRuntimeSpec`).
- Confirmed correct as written: `PluginPolicy` really is a composite of five
  sub-policies (`policy.rs:224`-`:228`: `lifecycle`, `ui`, `permissions`,
  `install`, `runtime`).

Verified 2026-10-23 against `src/plugin/` (20 modules + `builtin/` +
`runtime/`), `crates/codegg-protocol/src/plugin.rs`, `tests/agent_loop/`, and
`examples/plugins/`.
- **Removed a dead type reference.** The doc claimed "the legacy
  `api::hooks::HookResult` in `api.rs:86` is a separate type without
  effects". That type no longer exists: `api::hooks` (`api.rs:39`) now
  **re-exports** the canonical `crate::plugin::hooks::{HookContext,
  HookResult, HookType}` — the module comment says the divergent copies were
  removed on purpose. `src/plugin/hooks.rs:93` is the only `HookResult` in the
  tree.
- **Corrected a wrong feature-gate claim.** The doc said "without the
  `plugins` feature, `WasmRuntime::invoke` returns `RuntimeError::Unsupported`".
  `runtime/wasm.rs` is not compiled at all without the feature
  (`#[cfg(feature = "plugins")] pub mod wasm;` in `runtime/mod.rs`), so there
  is no such type to call. `RuntimeError::Unsupported` is raised *inside* the
  feature when a module lacks the expected export. Replaced with the three
  real no-feature behaviours (command → `ok:false` +
  `"wasm_runtime_disabled"`; tool → `PluginError::Runtime`; hook →
  `HookResult::ok(ctx.input)` passthrough).
- **Corrected a stale line ref**: the passive-plugin command refusal is at
  `service.rs:331`, not `service.rs:314`; the parallel tool refusal is at
  `service.rs:484`.
- **Corrected 5 stale line refs**: `PluginInstallKind` `registry.rs:42`→`:45`,
  `HookType` `hooks.rs:4`→`:6`, `HookContext` `hooks.rs:61`→`:62`,
  `HookResult` `hooks.rs:92`→`:93`, `PluginError` `service.rs:625`→`:814`
  (off by 189).
- **Added the missing modules** to "Where It Lives": `contributions.rs`
  (passive contribution resolution), `package.rs` (parse-only package
  detection), `playwright.rs` (PATH-only bundle detection). Corrected the
  `tui.rs` row — the module is explicitly **deprecated** pre-Phase-5 TUI
  extension types, not a live registry.
- **Added depth**: the 13-variant `HookType` wire spelling (`as_str`/`parse`
  are dotted; the `strum` derive is snake_case and is *not* the wire form),
  the `PluginService::workspace_root` field and its `ExecutionContext`
  provenance, the optional `builtin_runtime`/`policy` fields and the
  "no policy ⇒ effects cleared" rule, the `remove`/`doctor_all` manager
  methods, the `min(max_output_bytes, 256 KiB)` plugin-tool output bound, and
  the `agent_loop` test-family members that cover plugins.
- **Re-confirmed as accurate**: the three `impl PluginRuntime for` sites
  (`runtime/builtin.rs:87`, `runtime/process.rs:91`, `runtime/wasm.rs:115`),
  the 5s outer hook timeout (`service.rs:41`, `with_hook_timeout` at
  `service.rs:142`), the 30s inner WASM timeout (`loader.rs:142`/`:151`),
  the five `PluginPolicy` sub-policies and their defaults, the `Allow`/
  `Deny`/`Degrade` decision set, the `normalize_command_name` uniqueness and
  alias-collision rules, the `{plugin_id}:` panel/status namespacing, the
  fuel reserve/return accounting, the install-path validation order, the
  `plugin-activation.json` path (`src/core/instance.rs:187`) and its atomic
  temp-file+rename write, `PluginResponse` ownership, and the six
  `PluginCapability` variants.
- **Two further line refs corrected on a second pass** (same date):
  the `PluginResponse` re-export is `plugin/mod.rs:25`, not `:21`; and the
  `wasm_cache` module is declared unconditionally in `runtime/mod.rs` but
  **every item in it** is `#[cfg(feature = "plugins")]`-gated — the prose now
  says that rather than claiming the module is not compiled.
