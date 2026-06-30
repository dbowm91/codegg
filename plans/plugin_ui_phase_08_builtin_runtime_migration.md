# Phase 8 Plan: Builtin Runtime Migration

## Objective

Move Codegg’s built-in plugin handlers onto the unified runtime/capability model. Built-ins should register as `runtime = builtin` plugins with explicit capabilities, use the same registry indexes as external plugins, and dispatch through the same service/runtime path where possible.

The goal is to eliminate the remaining parallel architecture where built-ins are native Rust hook handlers outside the capability model.

## Current State

The repo has native built-in plugin modules under `src/plugin/builtin/` for integrations such as Codex, Copilot, GitLab, and Poe. These built-ins currently behave like special-case hook providers. Phase 5 began adapting them to the new manifest/registry world, but the builtin execution path is still not a first-class runtime equivalent to process/WASM.

After Phase 6 and Phase 7, the expected runtime lineup is:

- `ProcessRuntime` for local process-backed commands;
- `WasmRuntime` for sandboxed WASM plugins;
- `BuiltinRuntime` for first-party native Rust handlers.

This phase adds the third runtime cleanly.

## Files to Add

### `src/plugin/runtime/builtin.rs`

Add a builtin runtime implementation.

Recommended shape:

```rust
pub struct BuiltinRuntime {
    handlers: Arc<BuiltinHandlerRegistry>,
}

pub struct BuiltinHandlerRegistry {
    // Map handler id -> handler implementation.
}
```

The runtime should support at least hook invocation and command invocation shape, even if command-capable builtins are initially sparse.

Recommended trait implementation:

```rust
#[async_trait]
impl PluginRuntime for BuiltinRuntime {
    async fn invoke(&self, invocation: PluginInvocation) -> Result<PluginResponse, RuntimeError>;
}
```

If built-in hooks still use `HookContext` internally, add adapter functions rather than leaking hook-only types into the runtime trait.

## Files to Modify

### `src/plugin/builtin/mod.rs`

Refactor built-in registration around manifest/capability declarations.

Recommended additions:

```rust
pub fn builtin_plugin_manifests() -> Vec<(String, PluginManifest)>;
pub fn builtin_runtime_registry() -> BuiltinHandlerRegistry;
```

Each built-in should declare:

- plugin id, e.g. `builtin:copilot`;
- runtime: `PluginRuntimeSpec::Builtin { handler: "copilot" }`;
- capabilities: hook/command/status/event as appropriate;
- trust: `PluginTrustClass::Builtin`;
- enabled by default unless config says otherwise.

### `src/plugin/builtin/*.rs`

Update each built-in module to expose a capability manifest and handler entrypoint.

Recommended pattern:

```rust
pub const PLUGIN_ID: &str = "builtin:copilot";
pub const HANDLER_ID: &str = "copilot";

pub fn manifest() -> PluginManifest { ... }
pub async fn invoke(invocation: PluginInvocation) -> Result<PluginResponse, RuntimeError> { ... }
```

If existing built-ins are hook-only, keep the actual auth/provider logic intact and wrap it.

### `src/plugin/service.rs`

Ensure `PluginService` initializes or accepts a `BuiltinRuntime` instance and dispatches `PluginRuntimeSpec::Builtin` through it.

The service should no longer branch directly to `crate::plugin::builtin::builtin_hook_handler()` except as a transitional compatibility shim.

### `src/plugin/registry.rs`

No major structural changes should be required if Phase 5/6 were done correctly. Add tests proving built-in plugins register as normal plugins.

### `src/plugin/mod.rs`

Export any new builtin runtime types only as needed.

## Registration Flow

Preferred registration at startup:

1. Build a `PluginRegistry`.
2. Register built-in plugin manifests using `PluginInfo` with `trust = Builtin`.
3. Construct `PluginService` with the registry and runtime dispatcher.
4. Load external manifests later.

Built-ins should not be represented only by ad-hoc static maps. Static maps may still back the handler registry, but plugin discovery/listing should use the same registry path as external plugins.

## Builtin Capability Mapping

Map existing built-ins conservatively:

- Copilot: auth/provider-related hook capabilities if that is what it currently supports.
- Codex: auth/provider or provider integration hooks as appropriate.
- GitLab: auth/provider/event hooks if currently present.
- Poe: auth/provider integration hooks as appropriate.

Do not invent user-facing commands unless there is already handler behavior for them. The point is model unification, not scope expansion.

## Invocation Adapters

Add helpers for hook-style builtins:

```rust
fn hook_invocation_to_context(invocation: &PluginInvocation) -> Result<HookContext, RuntimeError>;
fn hook_result_to_plugin_response(result: HookResult) -> PluginResponse;
```

The reverse mapping should preserve:

- transformed output in `data`;
- blocked/error state in diagnostics and `ok`;
- no UI effects unless the builtin intentionally emits them.

## Tests

Add tests covering:

- `builtin_plugin_manifests()` returns expected built-ins;
- built-in manifests have `runtime = builtin`;
- built-in manifests declare explicit capabilities;
- registering built-ins populates registry command/hook indexes;
- disabling a builtin excludes its capabilities from queries;
- invoking a known builtin through `PluginService` reaches `BuiltinRuntime`;
- unknown builtin handler returns a structured runtime error;
- hook adapter preserves transformed output and error/blocking semantics.

Existing tests for `register_builtins()` should be updated rather than deleted.

## Documentation Updates

Update `docs/PLUGINS.md` and `architecture/plugin.md`:

- built-ins are first-party plugins using `runtime = builtin`;
- built-ins share enable/disable, registry listing, trust labels, diagnostics, and capabilities with external plugins;
- built-ins have full trust and are not sandboxed;
- external process/WASM plugin behavior remains separate.

Update `.opencode/skills/plugin/SKILL.md` if it documents the older built-in flow.

## Acceptance Criteria

- Built-ins register through the unified plugin registry.
- Built-ins declare runtime and capabilities through `PluginManifest`.
- Builtin invocation uses a `BuiltinRuntime` implementation or equivalent runtime dispatcher, not a separate service branch.
- Built-in enable/disable state affects capability queries.
- Existing built-in behavior is preserved.
- Tests cover registry, dispatch, unknown handler, and hook adapter behavior.

## Non-Goals

- Do not add new built-in plugin features unless needed for migration.
- Do not add plugin management UX yet.
- Do not expose process/WASM permissions to built-ins; built-ins are first-party trust class.
- Do not wire new lifecycle hook locations beyond what is needed to preserve current behavior.

## Risks and Mitigations

### Risk: Builtin logic gets over-abstracted

Mitigation: keep builtin handler internals simple. Only the registration and invocation boundary needs unification.

### Risk: Existing auth/provider behavior regresses

Mitigation: add targeted tests around each built-in’s current hook behavior before moving the dispatch path.

### Risk: Static maps remain as hidden parallel registry

Mitigation: allow static maps only for handler lookup. Plugin listing/capabilities/enabled state must come from `PluginRegistry`.

## Handoff Notes for Phase 9

Once built-ins, process, and WASM all use the same runtime/capability model, Phase 9 can safely wire lifecycle hooks into core/daemon/agent/tool paths without caring which runtime backs each plugin.
