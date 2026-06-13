# LSP Hunk Source Context Executor Wiring Cleanup Plan

> **Status: COMPLETED** — Commit `8251f01` on `main`.

## Purpose

The corrective security integration pass at `62499809277b3d2edec6ce267804cb5c2d2216b0` fixed the dangerous evidence semantics: routing decisions no longer become `HunkNavigation` evidence, the finding gate is conservative again, the workflow has a real executor trait, `--hunk-context` exists, formatter output is used, and diagnostic line indexing is corrected.

One concrete adapter mismatch still prevents the real runtime path from reliably producing hunk semantic evidence. The security workflow sends pre-parsed `HunkDescriptor` values with `patch: None`, but `LspHunkSourceContextExecutor` serializes that request through the model-facing `LspTool::execute()` path. `LspInput` does not accept a `hunks` field, so the tool drops the descriptors and reconstructs a request with `hunks: vec![]` and `patch: None`.

This pass should introduce a typed internal execution path, preserve real policy inputs, make file selection deterministic, and clean up programmatic executor plumbing.

## Current State

Verified current behavior:

- `HunkSourceContextExecutor` is defined in `src/security/workflow/context.rs`.
- `LspHunkSourceContextExecutor` wraps `Arc<LspTool>`.
- The security workflow converts `ChangedHunk` values into pre-parsed `HunkDescriptor` values.
- Security workflow requests use:

```rust
HunkSourceNavigationRequest {
    hunks: descriptors,
    patch: None,
    ...
}
```

- `LspHunkSourceContextExecutor` serializes the full typed request to JSON and calls `LspTool::execute()`.
- `LspInput` does not define `hunks`, so Serde ignores that field.
- The model-facing `hunkSourceContext` branch reconstructs:

```rust
HunkSourceNavigationRequest {
    hunks: vec![],
    patch: parsed.patch.clone(),
    ...
}
```

- The resulting collector request has neither hunks nor patch and fails open.
- `collect_hunk_source_context_all_files()` uses synthetic hunk headers for policy evaluation, so patch byte-size policy does not reflect actual changed content.
- Per-file groups are stored in a `HashMap`, and the eight-file cap is applied in nondeterministic iteration order.
- The background/TUI path can provide both executors, but `run_security_review_command_with_executor()` cannot provide a hunk executor.

## Non-Goals

Do not expose pre-parsed hunks in the model-facing tool schema solely for this internal workflow.

Do not change public `hunkSourceContext` JSON output.

Do not add richer security hunk scoring.

Do not add automatic patch application.

Do not require live LSP servers in unit tests.

Do not redesign the security workflow broadly.

## Phase 1 — Add a Typed Internal `LspTool` Hunk Execution Method

Introduce a typed method on `LspTool` that accepts the internal DTO directly.

Suggested API:

```rust
impl LspTool {
    pub async fn execute_hunk_source_context_typed(
        &self,
        request: egglsp::hunk_context::HunkSourceNavigationRequest,
    ) -> Result<egglsp::hunk_context::HunkSourceNavigationResponse, String>;
}
```

Responsibilities:

- validate `request.file_path` against `allowed_root` using the same path rules as the tool operation;
- construct `SemanticContextCollector` and `HunkSourceNavigator` once;
- call `HunkSourceNavigationCollector::collect(request)`;
- return the typed response directly;
- preserve internal pre-parsed `hunks`;
- preserve request-specific intent and per-section caps;
- avoid JSON serialization/deserialization.

Implementation guidance:

- Extract shared collector construction into a private helper if needed:

```rust
fn build_hunk_source_navigation_collector(
    &self,
    radius: u32,
    max_symbols: usize,
    max_diagnostics: usize,
    max_references: usize,
) -> HunkSourceNavigationCollector;
```

- Keep path validation centralized. Do not duplicate subtly different root checks in the typed method and tool branch.

Acceptance criteria:

- A typed request containing `hunks` and `patch: None` reaches `HunkSourceNavigationCollector` unchanged.
- Internal request fields are not lost through JSON adaptation.
- The method remains read-only.

## Phase 2 — Make the Model-Facing Tool Branch Reuse the Typed Method

Refactor the `"hunkSourceContext"` match branch in `Tool::execute()`.

Current tool-facing behavior should remain patch-only. Build a typed request from `LspInput`, then call the typed method:

```rust
let request = HunkSourceNavigationRequest {
    file_path: file_path_str.clone(),
    hunks: Vec::new(),
    patch: parsed.patch.clone(),
    intent: "navigation".to_string(),
    ...
};

let response = self.execute_hunk_source_context_typed(request).await?;
```

Rules:

- Do not add `hunks` to the public schema in this pass.
- Preserve current defaults for radius, max hunks, definitions, references, and hierarchy flags.
- Preserve `LspToolOutput` wrapping and serialization.

Acceptance criteria:

- Public behavior remains backward compatible.
- Both model-facing patch input and internal pre-parsed hunk input use the same typed execution implementation.
- Collector construction is not duplicated.

## Phase 3 — Update `LspHunkSourceContextExecutor`

Replace the JSON round-trip with a direct typed call:

```rust
#[async_trait::async_trait]
impl HunkSourceContextExecutor for LspHunkSourceContextExecutor {
    async fn execute_hunk_source_context(
        &self,
        request: HunkSourceNavigationRequest,
    ) -> Result<HunkSourceNavigationResponse, String> {
        self.tool.execute_hunk_source_context_typed(request).await
    }
}
```

Acceptance criteria:

- Pre-parsed hunk descriptors are preserved.
- The executor no longer depends on `LspInput` accepting internal DTO fields.
- No JSON parse/extract failure is possible in this adapter.

## Phase 4 — Add Concrete Adapter Regression Coverage

Add tests that specifically catch the current mismatch.

Preferred tests:

- `typed_hunk_execution_preserves_preparsed_hunks`
- `lsp_hunk_executor_does_not_drop_internal_hunks`
- `model_facing_patch_path_reuses_typed_execution`

Testing strategies without a live LSP server:

Option A, preferred:

- factor collector invocation behind a small internal trait or closure and use a fixture collector in unit tests;
- assert the exact `HunkSourceNavigationRequest` received by the collector.

Option B:

- test a private request-normalization/helper function that proves the typed executor path does not convert through `LspInput`.

Option C:

- use a temp repository and a fixture semantic layer if existing test infrastructure can do so without launching a language server.

Acceptance criteria:

- Tests fail against the current JSON-round-trip implementation.
- No live language server is required.

## Phase 5 — Preserve Real Per-File Patch Data for Policy Evaluation

Problem:

`collect_hunk_source_context_all_files()` reconstructs only synthetic hunk headers, so `max_patch_bytes` measures a tiny synthetic string rather than actual changed content.

Preferred design:

- Preserve the real per-file patch during `discover_targets_from_diff()` or return an additional per-file diff structure.

Suggested DTO:

```rust
pub struct ChangedFileDiff {
    pub file_path: PathBuf,
    pub patch: String,
    pub hunks: Vec<ChangedHunk>,
}
```

Then use:

```rust
collect_hunk_source_context_all_files(
    files: &[ChangedFileDiff],
    policy: &HunkSourceContextPolicy,
    executor: Option<&dyn HunkSourceContextExecutor>,
)
```

Smaller alternative:

- pass a `HashMap<PathBuf, String>` of real per-file patches alongside `parsed_hunks`.

Rules:

- Policy size checks should use real patch bytes.
- Execution may still use pre-parsed hunks and `patch: None`.
- Avoid reparsing patches solely to recover hunk descriptors.

Acceptance criteria:

- Oversized changed content is skipped according to `max_patch_bytes`.
- A small header with a large body cannot bypass the policy.
- Existing target discovery behavior remains unchanged.

## Phase 6 — Make Per-File Processing Order Deterministic

Problem:

The workflow groups hunks in a `HashMap` and applies the eight-file cap in iteration order.

Implementation:

- materialize groups into a vector;
- sort by normalized file path before applying the cap;
- optionally prioritize files with stronger existing signals in a later pass, but use lexical ordering now.

Suggested pattern:

```rust
let mut grouped: Vec<(PathBuf, Vec<ChangedHunk>)> = ...;
grouped.sort_by(|a, b| a.0.cmp(&b.0));
for (index, (file_path, hunks)) in grouped.into_iter().enumerate() {
    if index >= max_files { ... }
}
```

Acceptance criteria:

- The same repository state selects the same files across runs.
- Cap notes report exact skipped counts.
- Tests cover more than eight files and stable selection.

## Phase 7 — Add Explicit Request and Timeout Caps

The current path caps files at eight but does not enforce a dedicated hunk-context timeout.

Add options or reuse explicit workflow options:

```rust
pub max_hunk_context_files: usize,
pub max_hunk_context_requests: usize,
pub hunk_context_timeout_ms: u64,
```

Defaults can mirror existing LSP enrichment limits:

- max files/requests: 8;
- timeout: 2500 ms.

Wrap each executor call with `tokio::time::timeout` at the workflow layer, not inside the executor trait.

Acceptance criteria:

- One slow file does not stall the whole review indefinitely.
- Timeout produces a fail-open note.
- Request count and file count are bounded explicitly.

## Phase 8 — Clean Up Programmatic Executor Plumbing

Problem:

`run_security_review_command_with_executor()` accepts only a `SecurityContextExecutor` and always supplies `None` for hunk execution.

Introduce an executor bundle:

```rust
pub struct SecurityReviewExecutors<'a> {
    pub security_context: Option<&'a dyn SecurityContextExecutor>,
    pub hunk_source_context: Option<&'a dyn HunkSourceContextExecutor>,
}
```

Add a primary API:

```rust
pub async fn run_security_review_command_with_executors(
    root: &Path,
    args: &SecurityReviewCommandArgs,
    executors: SecurityReviewExecutors<'_>,
) -> Result<String, String>;
```

Compatibility:

- keep `run_security_review_command_with_executor()` as a wrapper for the existing security-only API;
- keep `run_security_review_command()` as the no-executor wrapper;
- make background/TUI composition use the bundle.

Acceptance criteria:

- Programmatic callers can enable real hunk context.
- Existing public functions remain source-compatible where practical.
- Executor plumbing is not duplicated across command/background paths.

## Phase 9 — Clarify Availability and Receipt Semantics

The current receipt tracks generic `lsp_available` and `enriched`, where `enriched` refers to `securityContext` enrichment.

Decide whether hunk execution needs explicit receipt metadata:

```rust
pub hunk_context_requested: bool,
pub hunk_context_available: bool,
pub hunk_context_executed: bool,
```

Recommendation:

- Add fields only if the receipt/result panel needs to distinguish these states.
- At minimum, ensure output notes clearly distinguish:
  - requested but no executor;
  - executor called but failed;
  - executor succeeded with zero evidence;
  - executor succeeded with evidence.

Acceptance criteria:

- The UI does not imply hunk enrichment succeeded merely because LSP is available.
- State descriptions remain accurate.

## Phase 10 — Documentation Precision

Update documentation after implementation.

Use precise language:

- `--hunk-context` causes deterministic invocation of best-effort LSP hunk collection.
- LSP results remain server-dependent and fail-open.
- The internal security workflow uses pre-parsed hunk descriptors through a typed API.
- The model-facing tool remains patch-only.
- Per-file selection order is deterministic and bounded.
- Patch byte-size policy uses actual per-file patch data.

Files:

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `AGENTS.md`
- `README.md`

Acceptance criteria:

- No documentation claims that LSP results themselves are deterministic.
- No docs imply internal hunk DTOs are exposed to the model-facing schema.

## Phase 11 — Tests

Add focused tests.

### Typed execution tests

- pre-parsed hunks survive typed execution;
- public patch-only branch calls the same typed implementation;
- concrete executor no longer serializes through `LspInput`.

### Policy tests

- real patch bytes trigger oversized-patch skip;
- synthetic header size is not used;
- file ordering is lexical/stable before cap;
- more-than-eight files always select the same first eight.

### Timeout/fail-open tests

- fixture executor timeout produces note and no evidence;
- one file failure does not block later files;
- success with zero evidence is distinguishable from failure.

### API tests

- executor bundle passes both executors;
- legacy security-only wrapper still works;
- `--hunk-context` with programmatic hunk executor produces actual evidence.

Acceptance criteria:

- Tests fail against the current concrete adapter mismatch.
- No live LSP server is required.

## Suggested Verification Commands

Run:

```bash
cargo fmt --all
cargo test --lib lsp
cargo test --lib security
cargo test -p egglsp
```

Then, if feasible:

```bash
cargo test --all --workspace
cargo clippy --all-targets --all-features -- -D warnings
```

If full workspace tests or Clippy are skipped, record why.

## Review Checklist

Before considering this pass complete:

- Internal pre-parsed hunks no longer pass through `LspInput`.
- `LspHunkSourceContextExecutor` calls a typed internal method.
- Model-facing patch input remains backward compatible.
- Policy evaluates actual per-file patch bytes.
- File selection before caps is deterministic.
- Hunk execution has explicit request/time bounds.
- Programmatic APIs can provide both executor types.
- Tests cover the concrete adapter path.
- Documentation distinguishes deterministic invocation from best-effort LSP results.

## Expected Follow-Up

After this cleanup, the security hunk integration should be operational rather than merely safe. The next step should be real-workload evaluation: measure whether hunk semantic evidence improves finding precision, prompt quality, and reviewer usefulness before adding richer security-specific hunk scoring or broader review/edit-planning integration.
