# Context Ledger Hardening Follow-Up

## Purpose

Harden the first-pass context ledger/artifact projection implementation so it is operationally reliable in normal agent runs. The prior pass added the right architecture: `src/context/`, in-memory artifact storage, tool-output projection, live `ContextLedgerState`, `[context]` config, and agent-loop projection wiring. This follow-up should correct the remaining integration and safety gaps before moving on to broader cache-aware `ContextBlock`/stable-prefix work.

The goal of this pass is not to redesign compaction. The goal is to make artifact-backed projection usable, recoverable, correctly gated by config, and safe under realistic tool-output contents.

## Current repo state

Relevant files from the first pass:

- `src/context/artifact.rs`
  - Defines `ArtifactKind`, `ContextArtifact`, `ContextArtifactStore`, `InMemoryArtifactStore`, `build_handle()`, `estimate_tokens()`, and `compute_content_hash()`.
  - Current store is in-memory only.
  - Current handle construction is plain string formatting.
- `src/context/projection.rs`
  - Defines `ProjectionConfig`, `ToolOutputProjection`, and `project_tool_output()`.
  - Extracts touched files, commands, test results, and unresolved errors.
- `src/context/read_tool.rs`
  - Defines `ContextReadTool`, but it does not appear to be registered in the normal session tool registry path.
  - Current session isolation checks `handle.contains(&self.session_id)`, which is too loose.
  - Current byte slicing can panic on non-UTF-8 character boundaries.
- `src/agent/context_frame.rs`
  - Defines `ContextLedgerState` and merges ledger fields into `ContextFrame`.
- `src/agent/loop.rs`
  - Owns `context_ledger`, `artifact_store`, `projection_config`, and `turn_index`.
  - Stores projected tool results in artifacts and appends projected `Message::Tool` content.
  - Ignores artifact store write errors.
  - `turn_index` may not be incremented meaningfully.
- `crates/codegg-config/src/schema.rs`
  - Defines `[context]` config with `artifact_store`, `project_tool_outputs`, `max_success_tokens`, `max_failure_tokens`, and `lossless_debug`.
  - Current projection config only consumes `project_tool_outputs`, `max_success_tokens`, and `max_failure_tokens`.

## Non-goals

Do not implement the full cache-aware `ContextBlock` builder in this pass.

Do not implement embeddings/vector recall.

Do not add a large suite of new model-visible tools. One recoverability tool, `context_read`, is enough.

Do not add cross-session artifact access.

Do not store unredacted outputs unless existing codegg storage already deliberately does so. The artifact store should keep the same redacted content that would otherwise enter the model transcript.

Do not make SQLite persistence mandatory if it risks destabilizing the pass. In-memory is acceptable for this follow-up if session-local behavior is explicit and correct, but the API should be ready for persistent backing.

## Phase 1: Add a real `ContextHandle` parser

Replace ad hoc `ctx://` handling with a typed parser.

Suggested type in `src/context/artifact.rs` or a new `src/context/handle.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextHandle {
    pub kind: ContextHandleKind,
    pub session_id: String,
    pub turn_index: usize,
    pub tool_call_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextHandleKind {
    Tool,
}
```

Required functions:

```rust
impl ContextHandle {
    pub fn parse(input: &str) -> Result<Self, ContextHandleError>;
    pub fn build_tool(session_id: &str, turn_index: usize, tool_call_id: &str) -> Result<String, ContextHandleError>;
    pub fn same_session(&self, session_id: &str) -> bool;
}
```

Rules:

- Only accept `ctx://tool/{session_id}/{turn_index}/{tool_call_id}` for now.
- Require exactly the expected number of path segments.
- Reject empty session ids and tool-call ids.
- Reject `/`, control characters, and whitespace in unescaped segments.
- Either percent-encode segments on build or reject unsafe segment characters consistently.
- Parse `turn_index` as `usize`; reject invalid numbers.
- Keep `build_handle()` as a compatibility wrapper if needed, but have it call the typed builder or clearly document that it is fallible via a new function.

Tests:

- Valid handle parses into exact fields.
- Invalid schemes are rejected.
- Missing segments are rejected.
- Extra segments are rejected.
- Empty session/tool-call segment is rejected.
- Session id substring attacks fail exact matching, e.g. current session `s1` must not match `ctx://tool/not-s1/0/c1`.
- Slashes/control chars/whitespace in segments are rejected or encoded.

## Phase 2: Make `context_read` safe and exact

Update `src/context/read_tool.rs` to use `ContextHandle::parse()`.

Requirements:

- Replace `handle.contains(&self.session_id)` with exact parsed session comparison.
- Return a clear permission error when parsed session differs from current session.
- Return a clear format error when parsing fails.
- Keep the tool read-only.
- Keep response compact but explicit.

Fix UTF-8 slicing:

Current code slices `&content[offset..end]`, which can panic if `offset` or `end` is not a character boundary. Replace with a safe helper.

Acceptable approaches:

1. Treat `offset` and `max_bytes` as byte offsets, but clamp start/end to valid UTF-8 boundaries.
2. Rename semantics internally to character offset while preserving the public `offset` parameter name for now.

Prefer byte offsets with boundary clamping because the schema already says bytes.

Suggested helper:

```rust
fn clamp_to_char_boundary(s: &str, mut idx: usize) -> usize {
    idx = idx.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}
```

For end boundaries, clamp down or up consistently; clamping down is simpler and safe.

Tests:

- Non-ASCII content with offset/max_bytes splitting a multibyte character does not panic.
- Offset beyond content still returns the existing “fully consumed” style response.
- Cross-session denial uses exact parsed session, not substring matching.
- Malformed `ctx://` handles produce format errors before store lookup.
- Truncation hint still reports a usable continuation offset.

## Phase 3: Register `context_read` in the session-aware tool path

The first pass added `ContextReadTool` but the normal `ToolRegistry::with_options()` path still registers the standard tool set without it. The model needs a way to expand artifact handles. Wire `context_read` into the session-aware registry or agent loop.

Preferred design:

Extend `ToolRegistryOptions` with optional context artifacts:

```rust
pub struct ToolRegistryOptions {
    // existing fields...
    pub context_artifact_store: Option<Arc<dyn crate::context::ContextArtifactStore>>,
    pub context_session_id: Option<String>,
    pub context_read_enabled: bool,
}
```

Then in `ToolRegistry::with_options()`:

```rust
if options.context_read_enabled {
    if let (Some(store), Some(session_id)) = (options.context_artifact_store.clone(), options.context_session_id.clone()) {
        registry.register(crate::context::ContextReadTool::new(store, session_id));
    }
}
```

Alternative design:

If modifying `ToolRegistryOptions` is too invasive, add an explicit method:

```rust
impl ToolRegistry {
    pub fn register_context_read(
        &mut self,
        store: Arc<dyn crate::context::ContextArtifactStore>,
        session_id: String,
    ) { ... }
}
```

and call it from `AgentLoop::new()` or from the session setup path after `session_id` is assigned.

Important session-id concern:

`AgentLoop::new()` currently initializes `session_id` as empty and the actual session id may be set later. Do not register `context_read` with an empty session id. Find the point where the loop receives/sets its session id and register/update the context tool there. If no clean hook exists, add one.

Acceptance for this phase:

- `context_read` appears in model-facing tool definitions for normal sessions when `[context].artifact_store != false` and `[context].project_tool_outputs != false` or recoverability is otherwise enabled.
- It does not appear when context artifact projection is fully disabled.
- It shares the same artifact store used by `AgentLoop` for tool-result capture.
- Tests prove a projected handle from a stored artifact can be expanded through the registered tool.

## Phase 4: Honor `[context]` config semantics

Current config has fields that are not fully honored. Fix semantics clearly.

Suggested behavior:

```toml
[context]
artifact_store = true          # default true
project_tool_outputs = true    # default true
max_success_tokens = 800
max_failure_tokens = 2000
lossless_debug = false
```

Semantics:

- `artifact_store = false`
  - Do not store artifacts.
  - Do not emit `ctx://` handles unless a store is available.
  - If projection is still enabled, projection text should not imply recoverability by handle.
- `project_tool_outputs = false`
  - Do not compress/project model-facing tool results.
  - Preserve current full redacted tool output behavior.
  - Optional: still store artifacts if `artifact_store = true`, but this is lower priority.
- `lossless_debug = true`
  - Bypass projection and append the full redacted output to the model transcript.
  - If artifact store is enabled, still store the artifact for diagnostics/replay.
  - This should behave similarly to a lossless/debug policy.

Implementation options:

- Expand `ProjectionConfig` to include `artifact_store_enabled` and `lossless_debug`, or add a separate resolved context config struct owned by `AgentLoop`.
- Prefer a separate `ResolvedContextConfig` if it keeps semantics clean.

Tests:

- `project_tool_outputs = false` leaves model-facing `Message::Tool` content equal to full redacted output.
- `lossless_debug = true` leaves model-facing output full-length but still stores artifact when enabled.
- `artifact_store = false` does not create handles or store artifacts.
- Defaults preserve current intended behavior: artifact store on, projection on, success/failure budgets 800/2000.

## Phase 5: Fix turn indexing and handle uniqueness semantics

The first pass added `turn_index: AtomicUsize`, but handles currently appear to load it without clear incrementation. Make turn numbering deterministic and meaningful.

Preferred approach:

- Use `self.state.turn_count` as the turn index when capturing tool artifacts, because it already increments each agent turn.
- Remove the separate `turn_index` atomic if it is not needed.

Alternative:

- Increment `turn_index` exactly once when a new provider turn begins, before tool calls are handled.

Requirements:

- Multiple tool results in the same model turn share the same turn index but differ by tool-call id.
- Later turns use increasing turn indices.
- Bootstrap tool artifacts and normal tool artifacts follow the same indexing semantics.

Tests:

- Two artifacts in the same turn have same turn index and distinct handles.
- Artifacts from later turns have higher turn index.
- No handle collision occurs when multiple tool calls occur in a single turn.

## Phase 6: Do not silently emit unrecoverable handles

Artifact store writes are currently ignored with `let _ = self.artifact_store.put(artifact).await;`. Harden this.

Behavior:

- If artifact storage succeeds, projection may include the handle and, once `context_read` is registered, an expansion hint.
- If artifact storage fails:
  - Log a warning with tool name, tool-call id, and error.
  - Do not emit a handle that cannot be recovered.
  - Fall back to either full redacted output or projected text without handle, depending on config.

Because the current in-memory store almost never fails, unit tests can use a failing mock store.

Tests:

- Failing store causes no `ctx://` handle to appear in model-facing text.
- Failing store logs or surfaces a diagnostic in a test-observable way if the project has tracing capture helpers.
- Message/tool-call pairing remains valid regardless of store failure.

## Phase 7: Improve model-facing projection affordance

Make projected output explicitly recoverable.

Current header is compact but underspecified:

```text
[ctx://tool/...] via bash
```

Preferred header:

```text
[tool output captured]
Tool: bash
Handle: ctx://tool/<session>/<turn>/<tool_call_id>
Full output: use context_read with this handle.
```

Keep it compact. Do not over-explain. The point is to make models reliably discover the expansion path.

When `context_read` is not registered or artifact storage is disabled, omit the “Full output” line.

Tests:

- Projected output includes `context_read` affordance when recoverable.
- Projected output omits handle/affordance when artifact storage is disabled or storage failed.
- Failure projection still prioritizes useful error lines.

## Phase 8: Light projection quality cleanup

Do not over-engineer projection yet, but fix the roughest issues.

Suggested improvements:

- Deduplicate high-priority failure lines, matching medium-priority behavior.
- Cap high-priority and medium-priority line counts separately.
- Tighten `extract_test_results()` so generic `failed`, `passed`, and `running ` do not over-capture unrelated output. Prefer patterns such as:
  - `test result:`
  - `running \d+ tests?`
  - `\d+ passed`
  - `\d+ failed`
  - `FAILED ` for pytest-style failures
  - `failures:`
- Include command string in model-facing projection for bash when available.
- Make read/diff/webfetch projections distinguish artifact kind if cheap.

Tests:

- Repeated identical error lines appear once or within a small cap.
- Generic prose containing `failed` is not always classified as a test result.
- `cargo test` and pytest output still preserve important failure summaries.
- Bash command appears in projection when available.

## Phase 9: Documentation updates

Update `architecture/context-ledger.md` and any relevant config docs.

Document accurately:

- Artifacts are currently session-local/in-memory unless SQLite persistence is implemented in this pass.
- `context_read` is the recovery path for full redacted outputs.
- `[context].artifact_store`, `[context].project_tool_outputs`, and `[context].lossless_debug` behavior.
- Projection is lossy only in model-facing text; raw redacted output remains recoverable when artifact storage succeeds.

If persistence remains a follow-up, explicitly say so.

## Phase 10: Test and validation checklist

Run the relevant suite:

```bash
cargo fmt
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features context
cargo test --workspace --all-features
```

If full workspace tests are too slow or currently flaky, document exactly which tests were run and any known unrelated failures.

Required new/updated tests:

- `ContextHandle` parser/building.
- Exact session isolation in `context_read`.
- Safe non-ASCII slicing in `context_read`.
- `context_read` registration in session-aware tool registry or agent setup.
- Config gating for `artifact_store`, `project_tool_outputs`, and `lossless_debug`.
- Store failure fallback with no unrecoverable handles.
- Turn-index semantics.
- Projection affordance and light projection dedupe.

## Acceptance criteria

This pass is complete when:

1. `context_read` is actually available to normal model sessions when artifact-backed projection is enabled.
2. `context_read` uses exact parsed session matching and cannot be bypassed by substring handles.
3. `context_read` cannot panic on non-ASCII output slicing.
4. `[context].artifact_store`, `[context].project_tool_outputs`, and `[context].lossless_debug` behave as documented.
5. Turn indices in `ctx://` handles are meaningful and deterministic.
6. Store failures do not produce unrecoverable model-facing handles.
7. Projected tool outputs include a compact, reliable expansion affordance when recoverable.
8. High-priority projection lines are deduped/capped enough to avoid repeated-log explosions.
9. All new behavior has targeted tests.
10. Formatting, clippy, and tests pass or failures are documented with evidence that they are unrelated.

## Suggested stopping point

Stop after the hardening above. Do not begin the broader cache-aware `ContextBlock` rewrite in this pass.

The next architectural pass should start only after this one is stable. That later pass should introduce stable-prefix context packing, cacheability/volatility metadata, provider `cached_tokens` feedback, stronger tool-definition hashing, and phase-scoped dynamic tool palettes.
