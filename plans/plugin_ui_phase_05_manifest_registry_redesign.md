# Phase 5 Plan: Plugin Manifest and Registry Redesign

## Objective

Refactor Codegg’s plugin model around capabilities and runtimes. A plugin should declare what it contributes separately from how it executes.

This phase prepares installed plugins and future WASM lifecycle hooks while preserving the process command path introduced in Phase 4.

## Problem Statement

The current `src/plugin` module is an early WASM-hook-oriented design. It has useful pieces: manifest parsing, registry, service dispatch, installer, built-ins, event bus, and TUI extension scaffolding. But the model is too runtime-specific and too root/TUI-local for the current architecture.

The new model should answer these questions cleanly:

- Which slash commands are available?
- Which plugin owns this command?
- Which runtime should execute it?
- Which hooks are registered for this hook point?
- Which panels/status widgets/event subscriptions exist?
- Which plugins are enabled?
- Which permissions/trust class apply?

It should not require the command system or TUI to know whether the plugin is WASM, process, or builtin.

## Target Model

A plugin has:

- stable id;
- name/version/api version;
- runtime spec;
- trust class;
- permission set;
- capability list;
- enabled state;
- installation/source metadata;
- diagnostics/last error.

A capability is one of:

- command;
- hook;
- panel;
- status widget;
- event subscription;
- later: tool, tool card, settings section.

A runtime is one of:

- builtin;
- process;
- wasm;
- later: trusted embedded/PyO3 if justified.

## Files to Modify

### `src/plugin/manifest.rs`

Refactor or extend the manifest structs to map onto `codegg_protocol::plugin::PluginManifestDto`.

Recommended approach:

- Keep a compatibility parser for the legacy manifest shape if practical.
- Add a new canonical manifest shape using runtime + capabilities.
- Convert both old and new forms into a single internal `PluginManifest` / `PluginInfo` representation.

Example canonical TOML:

```toml
name = "quota"
version = "0.1.0"
api_version = 1

[runtime]
kind = "process"
command = "python3"
args = ["quota.py"]
timeout_ms = 5000

[[capabilities]]
type = "command"
name = "quota"
description = "Show provider quota"
output = ["chat", "dialog"]

[permissions]
network = false
filesystem = "none"
env = ["CODEGG_PROVIDER"]
secrets = []
session_messages = false
tool_interception = false
```

For WASM:

```toml
name = "policy-filter"
version = "0.1.0"
api_version = 1

[runtime]
kind = "wasm"
module = "plugin.wasm"
timeout_ms = 1000
memory_max_mb = 16
fuel_per_call = 1000000

[[capabilities]]
type = "hook"
hook_type = "tool.execute.before"
priority = -10
```

### `src/plugin/registry.rs`

Redesign the registry to index by capability.

Recommended public methods:

```rust
pub fn register(&mut self, plugin: PluginInfo) -> Result<(), PluginRegistryError>;
pub fn unregister(&mut self, plugin_id: &str) -> Option<PluginInfo>;
pub fn get(&self, plugin_id: &str) -> Option<&PluginInfo>;
pub fn list(&self) -> Vec<&PluginInfo>;
pub fn set_enabled(&mut self, plugin_id: &str, enabled: bool) -> Result<(), PluginRegistryError>;
pub fn is_enabled(&self, plugin_id: &str) -> bool;

pub fn command(&self, name: &str) -> Option<PluginCommandRegistration>;
pub fn commands(&self) -> Vec<PluginCommandRegistration>;
pub fn hooks_for(&self, hook_type: &str) -> Vec<PluginHookRegistration>;
pub fn panels(&self) -> Vec<PluginPanelRegistration>;
pub fn status_widgets(&self) -> Vec<PluginStatusRegistration>;
pub fn event_subscribers(&self, event_type: &str) -> Vec<PluginEventRegistration>;
```

Use owned registration structs if lifetime complexity becomes distracting.

### `src/plugin/service.rs`

Reduce service responsibility to registry + runtime dispatch orchestration. Do not make it own TUI-specific behavior.

If the current `PluginService` assumes hooks only, add new methods:

```rust
pub async fn invoke_command(&self, command: &str, invocation: PluginInvocation) -> Result<PluginResponse, PluginError>;
pub async fn dispatch_hook(&self, hook_type: &str, invocation: PluginInvocation) -> Result<PluginResponse, PluginError>;
```

Actual runtime trait extraction can wait for Phase 6, but the service API should anticipate it.

### `src/plugin/tui.rs`

Treat this file as legacy. Do not continue ratatui-facing plugin UI here. Either:

- mark it as deprecated/internal; or
- refactor its route/component concepts into capability declarations that map to protocol `Panel` / `StatusWidget` / `Command` contributions.

Do not expose native TUI components as plugin capability implementation.

### `src/tui/command.rs`

Prepare command registry to optionally include plugin registry commands. If full integration is too large, add a helper seam:

```rust
pub fn append_plugin_commands(commands: &mut Vec<Command>, plugin_registry: &PluginRegistry)
```

Avoid reading installed plugins directly from the TUI command registry constructor if that causes boot ordering issues. The registry can be injected later.

## Internal Types

Recommended internal `PluginInfo` shape:

```rust
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    pub runtime: PluginRuntimeSpec,
    pub trust: PluginTrustClass,
    pub permissions: PluginPermissionSet,
    pub capabilities: Vec<PluginCapability>,
    pub enabled: bool,
    pub source: PluginSource,
    pub diagnostics: Vec<PluginDiagnostic>,
}
```

Recommended trust class:

```rust
pub enum PluginTrustClass {
    Builtin,
    LocalProcess,
    SandboxedWasm,
    TrustedLocal,
}
```

`TrustedLocal` is reserved for future embedded/PyO3-like runtimes and should not be used by default.

## Duplicate and Priority Rules

### Commands

- Command names normalize by trimming leading `/` and lowercasing.
- Built-in/static commands win by default.
- Plugin command duplicate behavior should be explicit. Recommended: reject duplicate plugin command registration unless a config setting allows override.
- Aliases participate in duplicate detection.

### Hooks

- Sort by priority ascending or descending consistently with old hook behavior. Document the rule.
- Disabled plugins are excluded.
- Hook registration should include plugin id and handler name.

### Panels/status widgets

- IDs should be namespaced by plugin id if not already namespaced.
- Duplicate IDs from different plugins should be rejected or auto-namespaced.

## Migration Strategy

1. Add new structs while keeping old structs available if needed.
2. Add conversion from legacy manifest to new internal representation.
3. Update tests to use the new canonical manifest.
4. Keep legacy hook parsing so existing docs/examples do not break immediately.
5. Add deprecation notes to docs after implementation.

## Tests

Add or update tests covering:

- canonical process command manifest parses;
- canonical WASM hook manifest parses;
- legacy manifest shape still parses or fails with a clear error;
- registry registers plugin and lists commands;
- duplicate command names are rejected;
- aliases are checked for duplicates;
- disabled plugin commands/hooks are excluded;
- hook priority ordering is stable;
- panels/status widgets are indexed;
- built-in runtime plugin can be represented;
- trust class is inferred from runtime kind.

## Acceptance Criteria

- New manifest model represents runtime + capabilities separately.
- Registry indexes commands, hooks, panels, status widgets, and event subscriptions.
- Registry enable/disable state affects capability queries.
- Duplicate command handling is deterministic and tested.
- Existing process command work from Phase 4 can map into a plugin command capability.
- Legacy plugin files are not removed prematurely.
- No ratatui dependency is introduced into plugin manifest/registry logic.

## Non-Goals

- Do not implement the final `PluginRuntime` trait in full; that is Phase 6.
- Do not modernize Wasmtime invocation; that is Phase 7.
- Do not wire lifecycle hooks into core paths yet; that is Phase 9.
- Do not add plugin management slash commands; that is Phase 11.
- Do not implement PyO3.

## Risks and Mitigations

### Risk: Breaking old plugin tests

Mitigation: add compatibility conversion or update tests intentionally with clear notes. Do not delete old files until the new model is proven.

### Risk: TUI command registry boot ordering

Mitigation: expose an append/injection seam rather than loading installed plugins deep inside static registry construction.

### Risk: Registry becomes runtime-aware

Mitigation: registry stores runtime specs, but runtime invocation belongs in service/runtime modules. Capability queries should not branch on runtime except for trust/diagnostic labels.

## Handoff Notes for Phase 6

Phase 6 should introduce the runtime trait and move process command execution into `ProcessRuntime`. The registry from this phase should provide enough information for `PluginService` to look up a command capability, fetch its runtime spec, build a `PluginInvocation`, and invoke the appropriate runtime.
