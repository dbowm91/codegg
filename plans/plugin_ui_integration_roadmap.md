# Plugin UI and Runtime Integration Roadmap

## Purpose

Codegg already has three relevant foundations that should be unified rather than bypassed:

1. A legacy `src/plugin` module with feature-gated Wasmtime support, manifest/registry/service concepts, built-in plugin handlers, and installer scaffolding.
2. A modern `codegg-protocol` crate that is now the appropriate thin waist for multi-frontend, daemon, socket, stdio, GUI, web, CLI, and automation clients.
3. A TUI that has been partially modularized into state domains, component rendering, async command dispatch, command registry, and remote TUI snapshots.

The integration goal is not merely to revive the old plugin module. The goal is to make plugins declare capabilities and emit portable UI intent. Codegg core should enforce capability policy and execute plugins through runtime adapters. Frontends should render protocol-level UI descriptions through their own native presentation layer.

The long-term target is:

- WASM is the primary sandboxed plugin runtime.
- Process/stdout plugins are supported for hackable one-off local commands.
- Built-in Rust plugins use the same registry and capability model.
- PyO3/Python bindings remain a later SDK/runtime convenience, not the first trust boundary.
- Plugin UI is declarative and frontend-neutral, not ratatui-specific.

## Current Architectural Seams

### Protocol thin waist

`crates/codegg-protocol` already exposes core, DTO, frame, and TUI protocol modules. This is the correct home for shared plugin and UI DTOs. Plugin UI types must not depend on ratatui, crossterm, root `App`, `TuiCommand`, or current fixed dialog enums.

### TUI rendering and state

`src/tui` already has:

- app state domains;
- a command registry;
- async spawn-and-complete patterns;
- TUI command dispatch;
- dialogs, status bar, sidebar, toast manager, messages widget, and component rendering;
- remote TUI channels and snapshots.

The correct frontend integration is to add `PluginUiState` and a renderer adapter that lowers `codegg_protocol::ui::UiNode` to ratatui widgets.

### Dynamic command loading

`src/command/mod.rs` and `src/tui/command.rs` already load project-local commands from config and `command/` / `commands/` markdown files. This is the fastest path to one-off script commands. The existing template command behavior must remain unchanged when no runtime is declared.

### Legacy plugin module

`src/plugin` should not be deleted immediately. It should be migrated behind a runtime-neutral abstraction. The old manifest/registry/hook concepts are useful, but the public model should become capability + runtime, not "plugin equals WASM hook handler."

## Design Principles

1. Protocol first. Shared plugin invocation, plugin response, and UI descriptions belong in `codegg-protocol`.
2. Runtime-neutral. Command routing, UI rendering, and lifecycle hooks should not know whether a plugin is WASM, process, or builtin.
3. WASM for serious hooks. Lifecycle hooks that can mutate messages, tools, providers, auth, shell env, or compaction should default to WASM or builtin handlers.
4. Process scripts for local hackability. Process plugins are local executable code. They are useful, but they are not sandboxed unless a future OS-level sandbox is added.
5. UI intent, not terminal drawing. Plugins should emit semantic UI nodes/effects: dialog, panel, status item, table, markdown, key-value list, toast, chat block. Frontends decide presentation.
6. Keep command dispatch non-blocking. All plugin execution must follow the existing spawn-and-complete pattern.
7. Backward-compatible command loading. Current template commands must continue to work exactly as they do today.
8. Small first schema. Avoid building a comprehensive UI framework before the first command plugin works.

## Phase Sequence

### Phase 1: Protocol thin waist for plugin UI and invocation

Add `crates/codegg-protocol/src/ui.rs` and `crates/codegg-protocol/src/plugin.rs`. Define frontend-neutral serializable DTOs for `UiNode`, `UiEffect`, `PluginInvocation`, `PluginResponse`, command specs, runtime specs, capability specs, permission sets, and manifest DTOs.

This phase creates the schema all runtimes and frontends will share. It should be pure protocol work with serde tests and no TUI/root crate coupling.

### Phase 2: TUI renderer adapter for portable UI nodes

Add a TUI-side renderer that lowers protocol `UiNode` values into ratatui widgets. Add `PluginUiState` under TUI state to hold plugin dialogs, panels, status items, and last command effects.

This phase should not execute plugins yet. It should prove that a hardcoded or test-provided `UiEffect` can be applied to TUI state and rendered safely.

### Phase 3: Generic TUI command plumbing for plugin effects

Add generic TUI command variants for plugin command start, plugin command completion, and plugin UI effects. Add `src/tui/commands/plugins.rs` and route it from `runtime/command_dispatch.rs`.

This phase creates the non-blocking path from slash command invocation to plugin response application.

### Phase 4: Extend dynamic slash commands into command plugins

Extend the existing dynamic command system to support runtime-backed commands while preserving template commands. Support `runtime: process` first. Pass `PluginInvocation` JSON to stdin, capture stdout/stderr with timeout and byte caps, parse structured JSON if possible, otherwise render stdout as plain text.

This phase delivers the `/quota`-style use case.

### Phase 5: Plugin manifest and registry redesign

Refactor plugin manifests and registry around capabilities and runtimes. A plugin declares runtime kind separately from command, hook, panel, status widget, tool card, or event capabilities. The registry should answer capability queries without caring about runtime implementation details.

This phase prepares installed plugins and future WASM lifecycle hooks.

### Phase 6: Runtime abstraction and process runtime

Introduce `PluginRuntime` and implement `ProcessRuntime`. The runtime should be independently testable and enforce timeout, cwd, env allowlist, stdout/stderr caps, and exit-code normalization.

Process plugins must be labeled as local executable code, not sandboxed plugins.

### Phase 7: WASM runtime modernization

Refactor the existing Wasmtime loader into `WasmRuntime`. Keep feature gating, module limits, fuel/memory limits, timeouts, and caching. Change invocation to the new `PluginInvocation` / `PluginResponse` envelope.

### Phase 8: Builtin runtime migration

Move current built-in plugin handlers into the unified runtime/capability registry as `runtime = builtin`. Builtins should use the same enable/disable, listing, diagnostics, and capability filtering as external plugins.

### Phase 9: Core lifecycle hook integration

Wire plugin hooks into provider/auth resolution, tool definition generation, tool pre/post hooks, chat params/headers, message transforms, compaction, shell env, and event subscriptions. These should live in core/daemon/agent/tool paths, not only TUI.

Start with observation and post-action hooks before allowing blocking/mutating hooks.

### Phase 10: Frontend-neutral plugin UI events

Extend core or TUI protocol events to carry `UiEffect` payloads so plugin UI can work over socket/stdio/remote frontend paths. Prefer core events for session-scoped plugin UI that should be visible to all subscribed clients.

### Phase 11: Corrective hardening

Close correctness gaps in the plugin UI/runtime integration before
expanding into plugin management UX, SDKs, or broader lifecycle-hook
coverage. See `plans/plugin_ui_corrective_hardening_pass.md` for full
scope. The four target fixes are:

1. WASM fuel accounting returns unused fuel (not consumed fuel)
2. `BuiltinRuntime` strictly rejects unsupported invocation types and
   unknown hook type strings (no Auth fallback)
3. `EmitChat` effects render visibly in the TUI (toast / info dialog)
4. `PluginRegistry` capability queries filter against a snapshot of
   enabled plugin ids; no more `try_read()` fallbacks

### Phase 12: Plugin management UX

Add `/plugins`, `/plugin-install`, `/plugin-enable`, `/plugin-disable`, `/plugin-info`, `/plugin-remove`, and `/plugin-doctor`. Render plugin management through the same `UiNode` renderer.

### Phase 12: Security and policy hardening

Add trust classes and capability enforcement: builtin, local_process, sandboxed_wasm, and later trusted_embedded. Enforce capability gates for network, filesystem, secrets, shell env, message access, tool interception, and UI contribution surfaces.

### Phase 13: SDKs and examples

Create examples first: stdout-only process command, structured JSON dialog command, WASM command returning a table, WASM status widget, and a lifecycle hook. Then add a Rust/WASM SDK and later a pure Python subprocess SDK. Defer PyO3 until the protocol and ergonomics prove stable.

### Phase 14: TUI component modularization follow-through

Gradually refactor first-party informational surfaces to use the shared `UiNode` renderer where it reduces duplication: info dialogs, stats output, task lists, usage/cost/context, shell-show summaries, and plugin management views.

### Phase 15: Multi-frontend readiness

Extend client capability negotiation for plugin UI support: dialogs, panels, status items, tables, forms, diff views, streaming logs, and command palette contributions. Unsupported UI effects should degrade deterministically to chat or markdown.

## Recommended Initial Build Order

1. Phase 1 protocol schema.
2. Phase 2 TUI renderer/state adapter.
3. Phase 3 command/effect plumbing.
4. Phase 4 process-backed command plugins.
5. Phase 5 registry redesign.
6. Phase 7 WASM modernization once the command/response envelope has proven itself.

This order gives Codegg immediately useful hackable commands while avoiding a premature lifecycle-hook ABI. It also validates the frontend-neutral UI model before plugin authors rely on it.

## Cross-Cutting Testing Requirements

Each phase should include focused unit tests. By Phase 4, add integration tests that exercise:

- existing template commands still loading unchanged;
- process command stdout fallback;
- structured JSON `PluginResponse` parsing;
- timeout handling;
- oversized output handling;
- invalid JSON fallback/error behavior;
- TUI application of a dialog/table response;
- remote/frontend-safe serialization of UI payloads.

By Phase 7, add tests for WASM feature gating, module size limits, timeout/fuel enforcement, and runtime-disabled fallback.

## Documentation Requirements

Add or update docs as the implementation lands:

- `docs/PLUGINS.md` should describe the new runtime/capability model.
- `architecture/plugin.md` should be updated after Phase 5 to stop describing WASM as the only plugin model.
- `architecture/tui.md` should document `PluginUiState` and portable UI rendering after Phase 2.
- Example command plugin manifests should live under `examples/` or `docs/examples/` once Phase 4 lands.

## Non-Goals for the First Five Phases

- Do not add PyO3.
- Do not expose ratatui `Component` to plugins.
- Do not support arbitrary plugin draw callbacks.
- Do not implement lifecycle hooks before command plugins are working.
- Do not require every frontend to support every UI node.
- Do not remove legacy plugin files until the new registry/runtime model can represent their behavior.

## Definition of Done for the First Milestone

The first milestone is complete when a developer can add a project-local command such as `/quota` that runs a Python or shell script, emits either plain stdout or structured JSON, and Codegg renders it as chat or a typed dialog without blocking the TUI. The protocol types should be frontend-neutral, and existing template slash commands should continue to work unchanged.
