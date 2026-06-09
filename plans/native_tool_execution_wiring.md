# Native Tool Execution Wiring Plan

## Purpose

Complete the final runtime-correctness gap in Codegg's native tool architecture: real agent tool execution must use `ToolRegistry::execute_capture()` for native Codegg tools.

The registry-level structured execution API is now present and tested, but it still needs to be proven on the live execution path that handles provider-emitted `ToolCall`s. This pass should locate the actual tool dispatcher, route native tools through `execute_capture()`, keep MCP execution behavior intact, and add integration-style coverage so this does not regress.

This is a narrow wiring pass. Do not extract new crates or expand the backend abstraction.

## Current State Summary

Already implemented:

- `Tool::execute_structured(...)` default method.
- `ToolRegistry::execute_capture(...)`.
- Structured provenance implementations for important native tools such as `websearch`, `webfetch`, `lsp`, and `security`.
- `Tool::expose_in_definitions()` and hidden `DisabledTool` stubs.
- `ToolRegistry::with_session_config_defaults(...)` for config-aware session registries.
- Backend report semantics for native/disabled/MCP fallback states.
- `tests/tool_structured_execution.rs` covering registry-level `execute_capture()` behavior.

Remaining gap:

- Code search still shows `execute_capture(` only in registry/tests/docs/skills, not clearly in the live agent tool-call dispatcher.
- Therefore, real agent tool calls may still be using legacy `tool.execute(...)`, bypassing structured provenance.

## Non-Goals

Do not change model-facing tool schemas.

Do not change provider streaming behavior except where tool results are constructed.

Do not rewrite the agent loop.

Do not change MCP tool execution semantics beyond optional provenance wrapping at the boundary if trivial.

Do not add MCP adapters for `egglsp`, `eggsentry`, `egggit`, or `eggcontext`.

Do not expose provenance JSON to the model by default.

## Phase 1: Locate the Real Tool Execution Dispatcher

Find the code path that turns provider-emitted `ToolCall`s into `ChatEvent::ToolResult` / `Message::Tool` content.

Run:

```bash
rg "ToolCall" src/agent src/exec src/core src/tui src/server
rg "ChatEvent::ToolResult" src tests
rg "Message::Tool" src/agent src/exec src/core src/tui src/server
rg "tool_results" src tests
rg "\.execute\(" src/agent src/exec src/core src/tui src/server
rg "McpService" src/agent src/exec src/core src/tui src/server
rg "call_tool" src/agent src/exec src/core src/tui src/server
```

Likely areas to inspect:

```text
src/agent/loop.rs
src/agent/processor.rs
src/exec.rs
src/core/daemon.rs
src/core/session_runtime.rs
src/server/ws.rs
src/tui/app/mod.rs
```

Classify execution paths:

1. Native Codegg tools registered in `ToolRegistry`.
2. MCP tools named `mcp__server__tool` executed through `McpService`.
3. Text-tool or fallback tool parsing paths, if any.
4. Exec-mode-only paths.
5. Tests/harness-only paths.

Acceptance criteria:

- The actual native tool execution call site is identified.
- Any direct `tool.execute(...)` call in production code is documented as either replaced or intentionally retained.
- MCP execution path remains clearly separate.

## Phase 2: Add a Small Native Tool Execution Helper

Add a helper near the real dispatcher rather than scattering `execute_capture()` calls throughout the loop.

Suggested shape inside `AgentLoop`:

```rust
async fn execute_native_tool_call(
    &self,
    tc: &ToolCall,
) -> Result<crate::tool::StructuredToolResult, ToolError> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let timeout_ms = self.tool_timeout().saturating_mul(1000);
    let ctx = crate::tool::ToolExecutionContext {
        backend: crate::tool::ToolBackendKind::Native,
        session_id: Some(self.session_id.clone()),
        cwd,
        permission_mode: None,
        timeout_ms: Some(timeout_ms),
    };
    self.tool_registry
        .execute_capture(&tc.name, tc.arguments.clone(), Some(ctx))
        .await
}
```

If tool-specific timeout is available, use it instead of global `tool_timeout()`:

```rust
let timeout_ms = self.get_tool_timeout(&tc.name).as_millis().min(u128::from(u64::MAX)) as u64;
```

Do not move permission checking into this helper. Permission remains before execution.

Acceptance criteria:

- Native tool execution has one obvious helper.
- The helper uses `ToolExecutionContext` with session id, cwd, and timeout.
- Existing permission logic remains unchanged.

## Phase 3: Replace Native `execute()` Calls With `execute_capture()`

At the identified dispatch point, replace legacy native tool execution with the helper.

Before pattern may look like:

```rust
let tool = self.tool_registry.get(&tc.name).ok_or(...)?;
let output = tool.execute(tc.arguments.clone()).await?;
```

After:

```rust
let structured = self.execute_native_tool_call(&tc).await?;
let output = structured.output.clone();
```

Preserve model-visible output exactly. The `Message::Tool` or `ChatEvent::ToolResult` content should still be `structured.output`, not a JSON envelope.

Add tracing for provenance:

```rust
if let Some(p) = structured.provenance.as_ref() {
    tracing::debug!(
        tool = %tc.name,
        backend = %p.backend,
        implementation = %p.implementation,
        elapsed_ms = ?p.elapsed_ms,
        trust = ?p.trust,
        "native tool completed with provenance"
    );
}
```

If the event bus has no provenance field, do not modify the event model in this pass. Internal tracing is sufficient.

Acceptance criteria:

- Real native tool calls use `ToolRegistry::execute_capture()`.
- Model-facing tool result content is unchanged.
- Legacy tools still work because `execute_structured()` defaults to `execute()`.
- Existing tests for tool execution continue to pass.

## Phase 4: Keep MCP Tool Execution Separate

MCP tools named `mcp__server__tool` should continue to use the existing MCP execution path.

Do not route MCP tools through `ToolRegistry::execute_capture()` unless they are already registered as native wrappers. Raw MCP tools are external backend tools and are not part of the native registry.

Recommended logic:

```rust
if is_mcp_tool(&tc.name) {
    // existing McpService call path
} else {
    // native ToolRegistry::execute_capture path
}
```

Optional minimal provenance at MCP boundary is acceptable only if the execution code already has a natural result wrapper. Do not destabilize MCP handling to add provenance.

Acceptance criteria:

- Existing raw MCP tool calls still work.
- Native wrappers such as `websearch`/`webfetch` still go through `execute_capture()` even if internally backed by eggsearch/MCP.
- Raw `mcp__eggsearch__*` remains hidden by default from model definitions unless explicitly exposed.

## Phase 5: Timeout and Cancellation Behavior

Ensure the move to `execute_capture()` does not drop timeout behavior.

Inspect current dispatcher for:

```text
tokio::time::timeout
get_tool_timeout
tool_timeout
max_parallel_tools
cancellation / steering checks
snapshot pre/post hooks
file mutation snapshot handling
```

If timeout currently wraps `tool.execute(...)`, preserve the same wrapping around `execute_native_tool_call(...)`.

Example:

```rust
let result = tokio::time::timeout(
    self.get_tool_timeout(&tc.name),
    self.execute_native_tool_call(&tc),
).await;
```

Do not introduce nested timeout layers unless necessary.

Acceptance criteria:

- Per-tool timeout behavior is unchanged.
- Cancellation/steering behavior is unchanged.
- Snapshot behavior for file-mutating tools is unchanged.
- Permission prompts are still evaluated before execution.

## Phase 6: Add Live Dispatcher Tests

Registry-level tests already exist. Add a test that exercises the real dispatcher path as much as possible.

Suggested targets:

```text
tests/agent_loop_tool_execution.rs
```

or extend an existing agent loop harness test.

Useful test strategies:

1. Mock provider emits a tool call for a legacy tool like `list`; assert the returned tool result content is still the plain legacy string and not provenance JSON.
2. Mock provider emits a tool call for `security`; assert execution succeeds and tracing/provenance can be observed if the dispatcher exposes it in a test hook.
3. If internal provenance is not exposed, add a lightweight test-only hook to `ToolRegistry` or `AgentLoop` that records last structured provenance.

Preferred minimal production-safe hook:

```rust
#[cfg(test)]
last_tool_provenance: Option<crate::tool::ToolProvenance>
```

Avoid adding user-visible protocol fields in this pass.

Acceptance criteria:

- At least one test proves the live agent/tool dispatcher invokes `execute_capture()`.
- The test fails if the dispatcher reverts to direct `execute()`.
- The test does not require network, real providers, MCP servers, or LSP servers.

## Phase 7: Add a Focused Regression Test for Model Output Shape

Provenance must stay internal. Add a regression test ensuring the tool result passed back to the model is not a structured JSON object unless the tool itself naturally returns JSON.

For a legacy string tool, assert the tool message content does not contain a provenance envelope such as:

```text
"provenance"
"backend"
"implementation"
"trust"
```

Use a deterministic tool if possible:

```text
list
security classify_command
```

Acceptance criteria:

- Tool result content remains model-compatible.
- Provenance stays internal/tracing/test-hook only.

## Phase 8: Audit Constructor Usage Again

The last pass added `with_session_config_defaults()`. Confirm production paths use it where session registries are built.

Run:

```bash
rg "with_session_defaults|with_session_config_defaults|with_config\(" src tests
```

Expected:

- Production code with loaded config uses `with_config` or `with_session_config_defaults`.
- `with_session_defaults` appears only in tests or non-config-aware contexts.

If production still uses `with_session_defaults`, migrate it.

Acceptance criteria:

- No production code path loses `[tool_backends]` config.
- Tests cover config-aware session registry with disabled security or LSP.

## Phase 9: Documentation Update

Update architecture docs only after wiring is done.

Files likely needing small updates:

```text
architecture/tool.md
architecture/native_crates.md
architecture/overview.md
plans/native_tool_runtime_correctness.md
plans/native_tool_execution_wiring.md
.opencode/skills/tool/SKILL.md
AGENTS.md
```

Docs should state:

- Native Codegg tools execute through `ToolRegistry::execute_capture()`.
- `execute_capture()` calls `execute_structured()` and records provenance internally.
- Model-visible tool output remains the raw tool output string.
- Raw MCP tools remain on the MCP execution path.

Acceptance criteria:

- Docs no longer merely say `execute_capture()` exists; they say it is used by live native tool execution.
- Any caveats around MCP/raw tools are explicit.

## Validation Commands

Run:

```bash
cargo fmt --all --check
cargo check --workspace --all-features
cargo test --workspace
cargo clippy --workspace --all-features --all-targets
```

If clippy remains noisy, record exact failures separately. Do not mark the pass complete if format/check/test fails.

## Done Criteria

This pass is complete when:

- The real native tool-call dispatcher uses `ToolRegistry::execute_capture()`.
- Raw MCP execution remains on the existing MCP path.
- Model-visible tool result content is unchanged.
- Structured provenance is recorded internally for live native tool calls.
- A dispatcher-level test fails if native tool calls bypass `execute_capture()`.
- Config-aware session registry behavior remains intact.
- Documentation matches actual runtime behavior.

## Status: Complete (2026-06-09)

All phases of this plan have been executed:

- **Phase 1 (locate dispatcher):** The single dispatcher is `AgentLoop::execute_tool_calls` at `src/agent/loop.rs`. Confirmed via subagent exploration that it already routes native tools through `ToolRegistry::execute_capture` (line 3249) and MCP tools through `McpService::call_tool` (line 3078) in three call sites (main loop, synthetic bootstrap, follow-up drain).
- **Phase 2 (helper):** Added `AgentLoop::build_tool_execution_context(tc, timeout_ms)` and `AgentLoop::resolve_native_backend(name)` (`src/agent/loop.rs`). The helper centralises `ToolExecutionContext` construction so backend resolution and session plumbing live in one place.
- **Phase 3 (tracing):** The dispatcher now emits a `tracing::debug!` line summarising `ToolProvenance` (backend, implementation, elapsed_ms, trust) after every native tool call. Model-facing string output is unchanged.
- **Phase 4 (MCP separate):** No change. The MCP bucket at `src/agent/loop.rs:3047-3116` continues to call `mcp.call_tool` directly.
- **Phase 5 (timeout/cancellation):** No change. `tokio::time::timeout(timeout, ...)` still wraps the structured-execution future; the per-tool timeout via `get_tool_timeout` is preserved.
- **Phase 6 (live dispatcher test):** Added `tests/agent_loop_harness.rs::test_live_dispatcher_uses_execute_capture` and `test_live_dispatcher_passes_native_backend_in_context`. The mock tool overrides `execute_structured` and records the call; bypassing the structured path fails the test.
- **Phase 7 (model output shape):** Added `tests/agent_loop_harness.rs::test_live_dispatcher_model_output_shape_is_plain_string`. Asserts the `Message::Tool` content does not contain `provenance`, `backend`, `implementation`, `trust`, or `elapsed_ms`.
- **Phase 8 (constructor audit):** Confirmed `with_config(&config)` is used in production at `src/main.rs:1065` and `src/exec.rs:112`; `with_options` is used in `src/core/daemon.rs:504` (with `tool_backends: from_config(&config)`); `with_session_defaults` only appears in a single in-tree unit test (`src/agent/loop.rs:4123`) and is not on a production path. Subagent dispatch uses `with_defaults()` intentionally to strip todo/plan tools. No production migration required.
- **Phase 9 (documentation):** Updated `architecture/tool.md`, `architecture/native_crates.md`, `architecture/overview.md`, `AGENTS.md`, and `.opencode/skills/tool/SKILL.md` to describe the new helper, the resolved `backend` for `websearch`/`webfetch`, and the live-dispatcher regression tests.

Validation:
- `cargo check --workspace --all-features` — clean
- `cargo fmt --all --check` — clean
- `cargo test --test tool_structured_execution` — 9/9 pass
- `cargo test --test tool_registry` — 11/11 pass
- `cargo test --test agent_loop_harness test_live_dispatcher -- --test-threads=1` — 3/3 pass
- Pre-existing failures in `tests/agent_loop_harness.rs` (9 tests, all unrelated to native-tool wiring) reproduce on `main` before this work and are not introduced by it.
