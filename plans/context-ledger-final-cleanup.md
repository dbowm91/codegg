# Context Ledger Final Cleanup Plan

## Purpose

Close the remaining correctness gaps in the context ledger/artifact projection path before starting the broader cache-aware `ContextBlock` and stable-prefix context builder work.

The repo is now in good shape after the hardening pass. `context_read` is session-aware and registered, the agent loop and registry share an artifact store, `ContextHandle` parsing exists, model-facing projections have recovery affordances, config fields are mostly honored, and UTF-8 slicing is safe. This pass should be small and surgical.

## Current state summary

Relevant files:

- `src/context/handle.rs`
  - New typed parser: `ContextHandle::parse()`, `ContextHandle::build_tool()`, `same_session()`, and `clamp_to_char_boundary()`.
- `src/context/artifact.rs`
  - Still has `build_handle()` as raw string formatting.
  - Defines `ContextArtifactStore` and `InMemoryArtifactStore`.
- `src/context/read_tool.rs`
  - Uses `ContextHandle::parse()` and exact session matching.
  - Uses UTF-8-safe boundary clamping.
- `src/context/projection.rs`
  - Has `artifact_store_enabled` and `lossless_debug` in `ProjectionConfig`.
  - Emits `context_read` affordance when a handle is provided.
- `src/tool/factory.rs`
  - Builds a shared in-memory artifact store.
  - Registers `context_read` when `artifact_store && project_tool_outputs`.
- `src/tool/mod.rs`
  - `ToolRegistryOptions` supports context artifact store/session id/read enabling.
- `src/agent/turn_runtime.rs`, `src/agent/agent_loop_factory.rs`, `src/agent/runtime_factory.rs`
  - Thread the shared artifact store into the `AgentLoop`.
- `src/agent/loop.rs`
  - Uses `self.state.turn_count` for handles.
  - Stores artifacts and suppresses handles on store failure or disabled artifact-store config.

## Non-goals

Do not implement SQLite artifact persistence in this pass unless it is trivial and fully tested.

Do not start the cache-aware `ContextBlock` rewrite.

Do not introduce vector search or semantic recall.

Do not change the public UX beyond making existing context ledger behavior correct and predictable.

Do not add additional model-visible tools beyond `context_read`.

## Phase 1: Make `build_handle()` use the typed builder

`src/context/artifact.rs` still has:

```rust
pub fn build_handle(session_id: &str, turn_index: usize, tool_call_id: &str) -> String {
    format!("ctx://tool/{session_id}/{turn_index}/{tool_call_id}")
}
```

This bypasses the new typed validation in `ContextHandle::build_tool()`.

Update it so handle construction cannot generate invalid/unreadable handles.

Preferred API:

```rust
pub fn build_handle_checked(
    session_id: &str,
    turn_index: usize,
    tool_call_id: &str,
) -> Result<String, ContextHandleError> {
    ContextHandle::build_tool(session_id, turn_index, tool_call_id)
}
```

Then either:

1. Replace callers with `build_handle_checked()` and handle errors explicitly; or
2. Keep legacy `build_handle()` as a thin compatibility wrapper that falls back to a safe escaped/sanitized representation.

Prefer option 1 in agent-loop capture paths. If handle building fails, do not emit a handle and do not attempt artifact storage under an invalid key. Fall back to model-facing projection without a handle or full redacted output depending on config.

Requirements:

- Do not `unwrap()` handle creation in the agent loop.
- If handle creation fails, log a warning with session id, turn index, tool-call id, and error.
- Do not expose malformed `ctx://` handles to the model.
- Ensure all tests expecting `build_handle()` raw formatting are updated to checked behavior.

Tests:

- Valid session/tool ids produce the expected handle.
- Session id with whitespace fails checked builder.
- Tool-call id with `/` fails checked builder.
- Agent-loop helper or extraction unit test proves malformed handle creation falls back without a handle.

## Phase 2: Make `artifact_store = false` actually skip storage

Currently the normal tool-result path stores the artifact and then suppresses the effective handle when `artifact_store_enabled` is false. That prevents model-facing recovery handles but does not literally disable storage.

Fix the semantics:

- If `self.projection_config.artifact_store_enabled == false`, skip `artifact_store.put(...)` entirely.
- Pass an empty handle to `project_tool_output()`.
- Do not record an artifact handle in `ContextLedgerState`.
- Keep projection behavior independent: `project_tool_outputs = true` may still compress the model-facing output even without a recovery handle.

Expected behavior matrix:

| artifact_store | project_tool_outputs | lossless_debug | model-facing content | store artifact | handle shown |
| --- | --- | --- | --- | --- | --- |
| true | true | false | projected | yes | yes |
| false | true | false | projected | no | no |
| true | false | false | full redacted | optional yes | no affordance unless deliberately kept |
| true | true | true | full redacted | yes | optional handle acceptable, but avoid implying projection |
| false | false | false | full redacted | no | no |

For simplicity, in this pass use this rule:

- Show handles only when `artifact_store = true` and storage succeeded.
- Register `context_read` when `artifact_store = true`, even if projection is disabled.

Tests:

- `artifact_store = false` leaves the in-memory store empty after a tool result.
- `artifact_store = false` projection output contains no `ctx://` and no `context_read` affordance.
- Projection can still occur without artifact storage.

## Phase 3: Log artifact store failures

The current code suppresses handles on store failure, which is good, but it should not degrade silently.

Change artifact storage to preserve the error:

```rust
let store_result = self.artifact_store.put(artifact).await;
let store_ok = match store_result {
    Ok(()) => true,
    Err(err) => {
        tracing::warn!(
            tool_call_id = %id,
            tool_name = %tool_name_str,
            session_id = %self.session_id,
            error = %err,
            "failed to store context artifact; omitting recovery handle"
        );
        false
    }
};
```

Make sure tool name is available before storing, so the log is useful.

Tests:

- Add a failing mock `ContextArtifactStore` in tests.
- Verify the model-facing projection has no handle when store fails.
- If there is an existing tracing capture helper, assert a warning is emitted. If not, skip log assertion and at least test no handle is emitted.

## Phase 4: Align `context_read` registration with artifact storage, not projection

`src/tool/factory.rs` currently enables `context_read` only when both artifact storage and projection are enabled.

For final cleanup, register `context_read` whenever `artifact_store = true`, regardless of `project_tool_outputs`.

Rationale:

- `lossless_debug = true` may still store artifacts for diagnostics.
- `project_tool_outputs = false` may still benefit from explicit artifact recovery/debug tooling.
- `context_read` is read-only and session-scoped, so exposing it when storage is active is reasonable.

Implementation:

```rust
let context_read_enabled = artifact_store_enabled;
```

If there is concern about tool-schema overhead, add a separate future config field later, such as `context_read_tool = true`. Do not add it in this pass unless needed.

Tests:

- `artifact_store = true`, `project_tool_outputs = false` still registers `context_read`.
- `artifact_store = false` does not register `context_read`.
- Default config registers `context_read`.

## Phase 5: Ensure bootstrap tool path matches normal path

The previous implementation had separate logic for synthetic/bootstrap tool calls and normal tool calls. Confirm both paths now follow the same semantics:

- checked handle building,
- artifact-store config gating,
- store failure fallback,
- no unrecoverable handles,
- ledger update without empty/fake handles,
- projection/full-output behavior matching config.

If there is duplicated logic, extract a helper method on `AgentLoop`, for example:

```rust
async fn build_tool_result_message(
    &mut self,
    id: &str,
    tool_name: &str,
    tool_args: Option<&str>,
    raw_content: &str,
) -> Message
```

The helper should:

1. Redact local paths.
2. Build a checked handle if artifact storage is enabled.
3. Store artifact if handle creation succeeds.
4. Choose the effective handle only when storage succeeds.
5. Project or pass through based on config.
6. Update the context ledger.
7. Return `Message::Tool`.

This reduces future drift between bootstrap and normal paths.

Tests:

- Bootstrap path and normal path produce consistent projected output under default config.
- Bootstrap path respects `artifact_store = false`.
- Bootstrap path does not emit handles if storage fails.

## Phase 6: Clean up `ContextLedgerState` empty-handle behavior

`ContextLedgerState::record_projection()` currently receives a handle string. Ensure it does not store empty handles.

If not already handled, update it:

```rust
if !handle.is_empty() && !self.artifact_handles.contains(&handle.to_string()) {
    self.artifact_handles.push(handle.to_string());
}
```

Tests:

- Recording a projection with `""` does not add an artifact handle.
- Recording a projection with a valid handle deduplicates correctly.

## Phase 7: Update docs to match exact behavior

Update `architecture/context-ledger.md` and any config docs touched by the prior passes.

Document:

- Artifacts are currently in-memory/session-local.
- `artifact_store = false` means no artifacts are stored and no handles are shown.
- `context_read` is registered when artifact storage is enabled.
- `project_tool_outputs = false` disables model-facing projection but does not necessarily disable artifact capture if artifact storage is enabled.
- `lossless_debug = true` keeps full redacted output in the model transcript.
- Handles are built and parsed through `ContextHandle`; malformed segments are rejected.

Keep docs concise. Do not overstate durability until SQLite persistence lands.

## Phase 8: Validation

Run:

```bash
cargo fmt
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features context
cargo test --workspace --all-features
```

If full workspace tests are too expensive or there are unrelated failures, document exactly what was run and which failures are unrelated.

At minimum, targeted tests must cover:

- checked handle creation,
- invalid segment fallback,
- `artifact_store = false` skips storage,
- store failure emits no handle,
- context_read registration when artifact store is enabled regardless of projection,
- no context_read when artifact store is disabled,
- bootstrap and normal tool-result path consistency,
- no empty artifact handle stored in ledger.

## Acceptance criteria

This pass is complete when:

1. No code path generates raw `ctx://` handles by string formatting without validation.
2. Invalid handle inputs never produce model-facing handles.
3. `[context].artifact_store = false` actually prevents artifact storage.
4. Store failures are logged and do not produce unrecoverable handles.
5. `context_read` registration follows artifact storage availability, not projection availability.
6. Bootstrap and normal tool-result paths share the same artifact/projection semantics.
7. `ContextLedgerState` does not retain empty handles.
8. Docs accurately describe current session-local artifact behavior.
9. Targeted tests cover the edge cases above.
10. Formatting, clippy, and tests pass or unrelated failures are documented.

## Suggested stopping point

Stop after these cleanup items. Once this is complete, the context ledger/artifact projection layer should be stable enough for the next architectural step: a cache-aware context packer with stable-prefix `ContextBlock`s, volatility/cacheability metadata, provider `cached_tokens` feedback, and dynamic phase-scoped tool palettes.
