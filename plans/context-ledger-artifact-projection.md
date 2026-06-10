# Context Ledger + Artifact Projection First Pass

## Purpose

Implement the first practical slice of cache-aware context reduction in codegg. The goal is not maximum token compression. The goal is to reduce volatile, repeated, low-value context while preserving recoverability, debugging fidelity, provider prompt-cache stability, and agent reliability.

This pass should focus on one high-leverage seam: tool results currently flow directly into the model transcript as full redacted strings. Replace that with artifact-backed storage plus compact model-facing projections, and start populating the existing `ContextFrame` fields from real tool activity.

## Current repo context

Relevant existing code:

- `src/agent/loop.rs`
  - Main request/tool/result loop.
  - Adds assistant messages and tool results to `request.messages`.
  - Calls `compact_if_needed()` before provider calls and again after tool results.
  - Records provider usage including `cached_tokens`.
  - Uses `ToolRegistry::execute_capture()` and `StructuredToolResult` for provenance-capable tool execution.
- `src/agent/compaction.rs`
  - Has `ContextTracker`, legacy compaction, hybrid compaction, `CompactionPolicy`, `EvidenceRef`, `ProgrammaticCompactionState`, and semantic checkpoint support.
- `src/agent/context_frame.rs`
  - Has the correct high-level state shape: goal, task, constraints, decisions, touched files, commands, test results, unresolved errors, security findings, next steps.
  - Current live population is incomplete; `build_context_frame()` mostly fills todo/security fields.
- `src/tool/mod.rs`
  - Tool trait supports `execute_structured()`.
  - Tool registry supports provenance, elapsed time, `defer_loading()`, and `expose_in_definitions()`.
- `src/tool/catalog.rs`
  - Already supports deferred tool discovery and BM25 search.
- `crates/codegg-config/src/schema.rs`
  - Existing compaction config is mature enough for this first pass. Avoid broad schema churn unless needed.

## Design target

Introduce this flow:

```text
raw tool result
  -> artifact capture
  -> command/tool-aware projection
  -> context ledger update
  -> compact Message::Tool inserted into request.messages
  -> full result recoverable by handle
```

The model should usually see a compact result like:

```text
[tool result captured: ctx://tool/<session>/<turn>/<tool_call_id>]
Tool: bash
Status: failed
Command: cargo test
Summary: 3 tests failed in codegg-context; first failure is test_context_projection_preserves_failure_header.
Key output:
- error[E0425]: cannot find function `project_tool_output` in this scope
- failures: context_projection_tests::preserves_failure_header
Full output: use context_read with handle ctx://tool/<session>/<turn>/<tool_call_id>
```

The full redacted output must remain available for later expansion. Do not discard raw information during this pass.

## Non-goals for this pass

Do not attempt full Magic Context parity.

Do not implement vector search, semantic recall, or background historian workers yet.

Do not redesign all compaction around `ContextBlock` yet. This pass may prepare types for it, but should not require a full context builder rewrite.

Do not add many model-visible tools. Prefer one compact context/artifact read tool if needed.

Do not make lossy projections irreversible.

Do not regress provider message validity. Preserve assistant tool-call/result pairing invariants.

## Phase 1: Add artifact storage primitives

Add a small internal artifact module. Prefer `src/context/` or `src/agent/context_ledger/` depending on current crate organization. If the repo already has a better storage abstraction, use it. Keep the first pass simple.

Suggested types:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextArtifact {
    pub handle: String,
    pub session_id: String,
    pub turn_index: usize,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub kind: ArtifactKind,
    pub created_at_ms: i64,
    pub content_hash: String,
    pub redacted_content: String,
    pub raw_bytes_len: usize,
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    ToolResult,
    CommandOutput,
    ReadResult,
    Diff,
    TestOutput,
    WebFetch,
    Image,
}
```

Storage options:

1. Minimal first pass: in-memory per `AgentLoop` map plus optional persistence later.
2. Better first pass: SQLite table using the existing session database if available.

Prefer SQLite if it can be done without large migration risk. If using SQLite, add a migration in the existing session schema system. Suggested table:

```sql
CREATE TABLE IF NOT EXISTS context_artifacts (
    handle TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    turn_index INTEGER NOT NULL,
    tool_call_id TEXT,
    tool_name TEXT,
    kind TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    redacted_content TEXT NOT NULL,
    raw_bytes_len INTEGER NOT NULL,
    estimated_tokens INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_context_artifacts_session_created
ON context_artifacts(session_id, created_at_ms);
```

If this is too invasive, implement the API with in-memory backing first and leave a clear TODO for persistent backing. The public interface should not care which backing is used.

Required API shape:

```rust
#[async_trait]
pub trait ContextArtifactStore: Send + Sync {
    async fn put(&self, artifact: ContextArtifact) -> Result<(), AppError>;
    async fn get(&self, handle: &str) -> Result<Option<ContextArtifact>, AppError>;
    async fn list_recent(&self, session_id: &str, limit: usize) -> Result<Vec<ContextArtifact>, AppError>;
}
```

Use `ctx://tool/{session_id}/{turn_index}/{tool_call_id}` handles for tool results. Sanitize or encode parts if needed.

## Phase 2: Add tool-output projection

Add a projector that transforms full tool output into a compact model-facing string plus metadata for the ledger.

Suggested module: `src/context/projection.rs` or `src/agent/context_ledger/projection.rs`.

Suggested types:

```rust
pub struct ToolOutputProjection {
    pub model_text: String,
    pub summary: String,
    pub status: ProjectionStatus,
    pub detected_kind: ArtifactKind,
    pub touched_files: Vec<String>,
    pub commands_run: Vec<String>,
    pub test_results: Vec<String>,
    pub unresolved_errors: Vec<String>,
}

pub enum ProjectionStatus {
    Success,
    Failure,
    Unknown,
}
```

Projection rules for the first pass:

- Always include the handle.
- Always include tool name.
- For bash, include command if available from tool call arguments.
- Preserve failure lines more aggressively than success lines.
- Preserve compiler/test failure headers.
- Preserve final summaries such as `test result: FAILED`, `failures:`, `error:`, `warning:`, `panicked at`, `Traceback`, `AssertionError`, `FAILED`, `E   `, and nonzero exit hints.
- For successful verbose commands, collapse aggressively.
- For `read`, include path and byte/token estimate; do not echo huge file contents if artifact-backed capture is enabled.
- For `diff`/`git diff`, include file list and hunk count/line counts if cheaply detectable.
- For `webfetch`, include title/status/URL if available, but avoid repeating huge body content.

Avoid brittle overfitting. A simple line scorer is acceptable:

- High priority lines: errors, failures, panics, stack trace starts, failed tests, compiler diagnostics, paths with line numbers.
- Medium priority lines: warnings, summary lines, changed file names.
- Low priority lines: progress bars, download logs, repeated build noise, dependency compilation spam.

Keep projection deterministic and unit-testable.

Initial default budgets:

- Failed command projection: about 1500-2500 tokens.
- Successful command projection: about 300-800 tokens.
- Read result projection: path + size + first/last small snippet only if the result is large.
- Lossless debug mode should bypass projection and inject full output as today.

Do not rely on exact tokenizer APIs for this first pass; use existing `eggcontext` token estimates through `ContextTracker`/shared estimator where available.

## Phase 3: Wire projection into `AgentLoop`

Find the two sites where tool results are appended as `Message::Tool` in `src/agent/loop.rs`. The important normal path currently redacts local paths and pushes full content into `request.messages` after tool execution.

Replace the normal path with:

1. Redact local paths as today.
2. Store the full redacted output as a `ContextArtifact`.
3. Project the output into compact `model_text`.
4. Append `Message::Tool { tool_call_id, content: model_text }`.
5. Add the projected message to `context_tracker`.
6. Update ledger/context-frame state from projection metadata.

Maintain existing bus events carefully:

- The TUI should still be able to display useful tool output.
- Do not unexpectedly hide all detail from the user. The user-facing event may keep richer output than the model-facing projection, or it may show the projection plus a handle depending on current UI expectations.
- Avoid emitting raw unredacted output anywhere new.

Add an `AgentLoop` field for the artifact store and ledger state. If the store needs a pool, initialize it in `AgentLoop::new()` from the existing optional `SqlitePool`. Fall back to in-memory if no pool exists.

## Phase 4: Make `ContextFrame` live

Add a small ledger state object owned by `AgentLoop`, for example:

```rust
#[derive(Debug, Default)]
pub struct ContextLedgerState {
    pub touched_files: IndexSet<String>,
    pub commands_run: Vec<String>,
    pub test_results: Vec<String>,
    pub unresolved_errors: Vec<String>,
    pub artifact_handles: Vec<String>,
}
```

Use `IndexSet` if already available; otherwise use `Vec` with dedupe helper.

Update this ledger after each projected tool result.

Modify `build_context_frame()` so it includes ledger-derived fields instead of returning empty vectors for touched files, commands, test results, and unresolved errors.

Keep the frame compact:

- Limit touched files to a reasonable number, e.g. 20 most recent/deduped.
- Limit commands to 10 recent.
- Limit test results/errors to high-signal summaries.
- Do not inject all artifact handles into the frame; include handles only when directly relevant or in projection text.

## Phase 5: Add a compact context-read tool

Add one model-visible tool only if needed for recoverability. Preferred name: `context_read`.

Purpose:

- Given a `ctx://...` handle, return the full redacted artifact content or a bounded slice.

Schema should be compact:

```json
{
  "type": "object",
  "properties": {
    "handle": { "type": "string" },
    "offset": { "type": "integer", "default": 0 },
    "max_bytes": { "type": "integer", "default": 20000 }
  },
  "required": ["handle"]
}
```

Behavior:

- Validate handle format.
- Require same-session access unless there is an intentional cross-session mode.
- Return a clear error if the artifact is missing.
- Respect `max_bytes` with a truncation hint.
- Never return unredacted raw content.

If store plumbing makes a tool difficult in this pass, skip the model-visible tool but ensure the plan leaves projection text saying the full output is stored for future expansion. However, recoverability is much stronger if this tool lands in the first pass.

## Phase 6: Minimal config

Avoid a large schema change. Add at most one config section if necessary.

Suggested first-pass config:

```toml
[context]
artifact_store = true
project_tool_outputs = true
max_success_tokens = 800
max_failure_tokens = 2000
lossless_debug = false
```

If adding a new top-level config is too much for this pass, initially gate with existing compaction config:

- Enable projection when `compaction.enabled != false` and `compaction.preserve_evidence != false`.
- Disable projection under `CompactionPolicy::LosslessDebug`.

Prefer explicit config if straightforward. Update both root config schema and `crates/codegg-config/src/schema.rs` if the project maintains mirrored schema types.

## Phase 7: Tests

Add focused unit tests. Do not depend on live providers.

Required projection tests:

- Successful short output is passed through or lightly wrapped.
- Successful verbose output is summarized and includes handle.
- Failed Rust compiler output preserves `error[E...]`, file/line snippets, and final failure summary.
- Failed pytest output preserves failing test name and assertion/error header.
- Repeated/noisy lines are collapsed.
- Projection never exceeds configured approximate token budget by a large margin.

Required artifact tests:

- Put/get roundtrip by handle.
- Missing handle returns a clean error/None.
- Same-session access check for `context_read` if implemented.
- Truncated context_read response includes a continuation/truncation hint.

Required agent-loop tests, if existing test harness allows:

- Tool result inserted into `request.messages` is projected, not full giant output.
- Full output is stored and recoverable.
- `ContextFrame` includes touched files/commands/test failure summary after tool results.
- `LosslessDebug` or equivalent mode bypasses projection.

Existing compaction invariant tests should continue passing. Pay special attention to assistant tool-call/tool-result pairing.

## Phase 8: Documentation

Update architecture docs briefly:

- Explain that codegg now stores large tool outputs as context artifacts.
- Explain that the model sees compact projections with `ctx://` handles.
- Explain that this is designed for cache-aware context reduction and does not discard raw redacted output.

Add a short config example if a new `[context]` section is introduced.

## Acceptance criteria

This pass is complete when:

1. Large tool outputs are no longer blindly inserted in full into the model transcript under the default/balanced policy.
2. Full redacted output is stored behind a durable or session-local handle.
3. Model-facing tool results include enough diagnostic detail to continue debugging without immediate expansion.
4. The agent can recover full output by handle, preferably through `context_read`.
5. `ContextFrame` is populated with real touched files, commands, test results, and unresolved errors from tool activity.
6. Existing compaction invariants remain intact.
7. Unit tests cover projection, artifact storage, and frame updates.
8. `cargo fmt`, `cargo clippy`, and relevant tests pass.

## Suggested implementation order

1. Add artifact and projection modules with unit tests.
2. Add in-memory artifact store first.
3. Wire projection into the normal tool-result insertion path in `AgentLoop`.
4. Add ledger state and populate `ContextFrame` from it.
5. Add `context_read` if store access can be plumbed cleanly.
6. Add SQLite persistence if not already done; otherwise leave it for the next pass.
7. Add config gating and docs.
8. Run formatting, clippy, and tests.

## Edge cases to watch

- Provider message contracts require every assistant tool call to have a matching tool result. Do not drop `Message::Tool`; only replace its content.
- Some models may need enough direct failure output to avoid repeatedly expanding handles. Be conservative on failures.
- Avoid changing user-visible TUI behavior too abruptly. The TUI can show richer output than the model sees.
- Do not store unredacted content in the artifact store unless the rest of the session storage already permits it. Prefer storing the same redacted content that would have entered the model transcript.
- Avoid cache churn from injecting changing, verbose ledger text every turn. Keep the frame compact and only inject through existing compaction/control paths.
- Handle tool outputs that are not UTF-8-ish gracefully. Store lossy UTF-8 if current tool APIs are string-based.
- Avoid adding many specialized tools. One `context_read` tool is enough for this pass.

## Follow-up pass after this lands

After this first pass is stable, the next pass should introduce a real cache-aware `ContextBlock` builder:

- stable prefix blocks,
- volatile tail blocks,
- cacheability/volatility metadata,
- stronger tool-definition hash invalidation,
- provider usage feedback using `cached_tokens`,
- dynamic phase-scoped tool palettes,
- deferred compaction that avoids breaking cached prefixes.

Do not start that broader rewrite until artifact-backed projection and live context-frame updates are working and tested.
