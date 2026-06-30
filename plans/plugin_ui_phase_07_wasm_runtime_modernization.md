# Phase 7 Plan: WASM Runtime Modernization

## Objective

Refactor the legacy Wasmtime plugin loader into the unified runtime architecture introduced in Phase 6. WASM plugins should consume the same `PluginInvocation` envelope and produce the same protocol `PluginResponse` envelope as process and builtin runtimes.

This phase should preserve the valuable parts of the old WASM implementation: feature gating, module size limits, timeout enforcement, memory/fuel constraints, and module caching. It should remove the old assumption that a plugin is only a hook executor with bespoke hook-specific JSON.

## Current State

The repo still has a legacy `src/plugin/loader.rs` with Wasmtime execution behind the `plugins` feature. It has useful primitives, but it predates the new plugin model:

- it is hook-centric;
- it returns `HookResult`, not protocol `PluginResponse`;
- it relies on old hook function naming;
- it is not a `PluginRuntime` implementation;
- the old fuel accounting should be reviewed carefully before reuse.

Phase 6 should already have added:

- `src/plugin/runtime/mod.rs`;
- `PluginRuntime` trait;
- `ProcessRuntime` implementation;
- canonical use of `codegg_protocol::plugin::PluginResponse`.

This phase adds `WasmRuntime` to that abstraction.

## Files to Add

### `src/plugin/runtime/wasm.rs`

Add a WASM runtime implementation behind the existing `plugins` feature.

Recommended shape:

```rust
#[cfg(feature = "plugins")]
pub struct WasmRuntime {
    spec: WasmRuntimeSpec,
    limits: RuntimeLimits,
    cache: Arc<WasmModuleCache>,
}

#[cfg(feature = "plugins")]
pub struct WasmRuntimeSpec {
    pub module_path: PathBuf,
    pub timeout_ms: Option<u64>,
    pub memory_max_mb: Option<u64>,
    pub fuel_per_call: Option<u64>,
    pub entrypoint: Option<String>,
}
```

The runtime should implement:

```rust
#[async_trait]
impl PluginRuntime for WasmRuntime {
    async fn invoke(&self, invocation: PluginInvocation) -> Result<PluginResponse, RuntimeError>;
}
```

When `plugins` is disabled, expose a small stub type or dispatcher error that reports the runtime is unavailable without requiring Wasmtime dependencies.

### `src/plugin/runtime/wasm_cache.rs` or internal module

If the current loader has reusable module caching, move it into a dedicated cache type. Keep it internal to the WASM runtime unless other runtimes need it.

Recommended responsibilities:

- keyed by canonical module path plus file metadata;
- compile module once;
- invalidate on modified timestamp/size change;
- expose cache hit/miss diagnostics.

## Files to Modify

### `src/plugin/runtime/mod.rs`

Export the WASM runtime conditionally.

Add runtime dispatch support for `PluginRuntimeSpec::Wasm` where appropriate.

### `src/plugin/service.rs`

Update command and hook invocation paths to dispatch WASM plugins through `WasmRuntime` when runtime is `Wasm`.

For this phase, support command invocation first. Hook support can be bridged through a compatibility adapter if needed:

- command capability invokes generic entrypoint;
- hook capability invokes the same runtime with `PluginCapabilityInvocation::Hook`.

### `src/plugin/loader.rs`

Either:

1. reduce this file to a compatibility wrapper around `runtime::wasm`; or
2. move the Wasmtime logic into `runtime::wasm` and leave old public functions as shims.

Do not keep two independent WASM execution implementations.

### `src/plugin/hooks.rs`

Keep hook enums and `HookContext` for compatibility, but add conversion helpers into `PluginInvocation` and back from `PluginResponse` to `HookResult`.

Recommended:

```rust
impl HookContext {
    pub fn into_plugin_invocation(self, plugin_id: String, invocation_id: String) -> PluginInvocation { ... }
}

impl HookResult {
    pub fn from_plugin_response(response: PluginResponse, fallback_input: serde_json::Value) -> Self { ... }
}
```

This prevents old hook paths from shaping the WASM ABI.

## WASM ABI

Keep the first modern ABI JSON-based and memory-first. Do not move to WIT/component model in this phase.

Recommended exported function contract:

- `alloc(len: i32) -> i32`
- `dealloc(ptr: i32, len: i32)` optional for first pass
- `codegg_plugin_invoke(ptr: i32, len: i32) -> i64`

The invocation bytes are UTF-8 JSON for `PluginInvocation`.

Return layout recommendation:

- high 32 bits: response pointer
- low 32 bits: response length

or, if retaining the legacy layout, document it exactly and add tests. Avoid ambiguous pointer-plus-length memory conventions.

Returned bytes are UTF-8 JSON for `PluginResponse`.

Legacy hook-specific exports such as `hook_auth`, `hook_tool_execute_before`, etc. can be supported through an adapter only when `codegg_plugin_invoke` is absent. The modern ABI should be preferred.

## Runtime Limits

Carry forward and verify:

- maximum WASM file size;
- per-call timeout;
- memory max;
- fuel per call;
- maximum response bytes;
- trap-to-error conversion;
- malformed JSON handling;
- missing export handling.

Fuel accounting should be corrected, not blindly copied. Validate these invariants:

- a fresh plugin receives the configured per-call fuel;
- remaining fuel is not confused with consumed fuel;
- budget exhaustion means too much fuel was consumed, not that the current tracked value is zero;
- timeout and fuel errors are distinct diagnostics.

## Invocation Semantics

WASM runtime receives a `PluginInvocation` with:

- protocol version;
- invocation id;
- plugin id;
- capability invocation type;
- args;
- input JSON;
- context.

WASM returns a full `PluginResponse` with effects/data/diagnostics.

For hook compatibility:

- `response.ok == false` should map to a hook error unless the response data explicitly says to block;
- if response data contains transformed payload, use it as hook output;
- otherwise preserve the current input as hook output;
- diagnostics should be logged.

## Tests

### Unit tests without real Wasmtime

Where possible, test conversion and dispatch logic without the `plugins` feature:

- `PluginRuntimeSpec::Wasm` returns a clear unavailable error when feature is disabled;
- hook context conversion creates correct `PluginInvocation`;
- `PluginResponse` maps to `HookResult` correctly;
- malformed/empty responses are handled.

### Feature-gated integration tests

Under `#[cfg(feature = "plugins")]`, add at least one tiny WASM fixture or test module.

Preferred fixture behavior:

- accepts `PluginInvocation` JSON;
- returns `PluginResponse { ok: true, effects: [], data: {"seen": ...}, diagnostics: [] }`;
- one variant returns invalid JSON;
- one variant traps or times out if feasible.

Test cases:

- valid command invocation;
- valid hook invocation through adapter;
- missing export error;
- invalid response JSON error;
- oversized module rejected;
- oversized response rejected;
- timeout enforced;
- fuel/memory limits enforced if reliable in CI;
- module cache hit/miss increments or observable reuse.

## Documentation Updates

Update `docs/PLUGINS.md` and `architecture/plugin.md`:

- canonical runtime/capability model;
- modern WASM ABI;
- JSON invocation/response envelope;
- legacy hook export compatibility if retained;
- feature flag behavior;
- limits and diagnostics;
- explicit note that process plugins are local executable code, while WASM is the sandboxed path.

Correct any stale snippets using `[runtime] type = "wasm"` if the code expects `[runtime] kind = "wasm"`.

## Acceptance Criteria

- `WasmRuntime` implements the same runtime interface as `ProcessRuntime`.
- WASM command invocation uses `PluginInvocation` and `PluginResponse`.
- Legacy hook dispatch either uses the new runtime through adapters or has clear compatibility shims.
- The old loader no longer represents a separate architectural path.
- `plugins` feature compiles when enabled and disabled.
- Fuel/timeout/memory behavior is tested or explicitly documented where not testable.
- Docs describe the modern ABI and remove stale manifest-runtime terminology.

## Non-Goals

- Do not add WIT/component model yet.
- Do not add PyO3.
- Do not implement plugin marketplace/install UX.
- Do not wire every lifecycle hook into the core path unless Phase 9 has begun.
- Do not expose arbitrary UI drawing callbacks from WASM.

## Risks and Mitigations

### Risk: Existing WASM loader bugs get preserved

Mitigation: treat old loader code as reference material. Re-test fuel accounting, memory layout, timeout behavior, and response parsing explicitly.

### Risk: ABI churn before examples exist

Mitigation: keep ABI simple: one JSON invocation function and one JSON response. Add examples in Phase 13 after runtime stabilizes.

### Risk: Feature-gated code rots

Mitigation: add at least one `cargo check --features plugins` or feature-gated test target to the documented validation command set.

## Handoff Notes for Phase 8

After WASM supports the unified runtime interface, built-in plugins should move onto the same runtime/capability model. Builtins should not remain a separate hook-only path.
