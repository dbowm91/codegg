# LSP Hunk Source Context Operational Hardening Plan

## Purpose

The typed hunk-source-context integration is now operational after `8251f01ac7d60fe863eb040ee870d1d5d3b54037`, with additional regression coverage in `cbd38683c740605e2964c7d7632db7f093a79f96`.

The concrete executor mismatch is fixed: internal pre-parsed hunks now flow directly through `LspTool::execute_hunk_source_context_typed()`, while the model-facing tool remains patch-only.

This final hardening pass should close the remaining operational accuracy issues before shifting from architecture work to real-workload evaluation.

## Current State

Implemented and verified in the current codebase:

- `LspTool::execute_hunk_source_context_typed()` accepts `HunkSourceNavigationRequest` directly.
- `LspHunkSourceContextExecutor` delegates to the typed method without JSON serialization.
- The model-facing `hunkSourceContext` branch reuses the typed method.
- The security workflow preserves real per-file patch text from `egggit::file_diff()` for policy evaluation.
- Per-file processing order is sorted deterministically.
- Hunk-context file, request, and timeout limits exist on `SecurityReviewWorkflowOptions`.
- The programmatic API supports a `SecurityReviewExecutors` bundle.
- Security review receipts expose requested/available/executed hunk-context fields.
- Policy-only routing metadata is not treated as security evidence.
- Finding eligibility remains conservative.

Remaining concerns:

1. Typed hunk execution ignores request-specific per-hunk caps and uses global constants instead.
2. `max_hunk_context_requests` is enforced using the sorted file-loop index, so policy-skipped files consume request budget.
3. `SecurityReviewReceipt::hunk_context_executed` currently means "requested and executor available," not "an executor call was actually attempted or succeeded."
4. Relative typed request paths are resolved against process CWD rather than `LspTool::allowed_root`.
5. Some regression tests assert DTO fields or architectural intent without invoking the real forwarding seam.
6. A workflow comment still overstates deterministic collection; invocation/order are deterministic, but LSP results are best-effort and server-dependent.

## Non-Goals

Do not add richer security-specific hunk scoring.

Do not add new LSP operations.

Do not expose internal pre-parsed hunks in the model-facing schema.

Do not change public `hunkSourceContext` JSON output.

Do not add automatic patch application.

Do not require a live language server in unit tests.

Do not redesign the security review workflow broadly.

## Phase 1 — Honor Request-Specific Per-Hunk Caps

Problem:

`HunkSourceNavigationRequest` carries:

```rust
max_symbols_per_hunk
max_diagnostics_per_hunk
max_references_per_hunk
```

but `LspTool::build_hunk_source_navigation_collector()` currently configures `HunkSourceNavigator` with global constants:

```rust
MAX_CONTEXT_SYMBOLS
MAX_CONTEXT_DIAGNOSTICS
MAX_CONTEXT_REFERENCES
```

The security workflow requests conservative caps of 10, but the typed execution path can return substantially more evidence.

Implementation:

- Refactor collector construction to accept explicit limits:

```rust
fn build_hunk_source_navigation_collector(
    &self,
    radius: u32,
    max_symbols_per_hunk: usize,
    max_diagnostics_per_hunk: usize,
    max_references_per_hunk: usize,
) -> HunkSourceNavigationCollector
```

- In `execute_hunk_source_context_typed()`, derive effective limits from the request and clamp to safe upper bounds:

```rust
let max_symbols = request
    .max_symbols_per_hunk
    .max(1)
    .min(MAX_CONTEXT_SYMBOLS);
let max_diagnostics = request
    .max_diagnostics_per_hunk
    .max(1)
    .min(MAX_CONTEXT_DIAGNOSTICS);
let max_references = request
    .max_references_per_hunk
    .max(1)
    .min(MAX_CONTEXT_REFERENCES);
```

- Preserve request values in the DTO if response metadata needs to report configured limits.
- Decide explicit behavior for zero:
  - recommended: coerce to `1`, matching current `max_hunks` behavior;
  - alternative: allow zero to disable a section, but document and test it.

Recommendation: coerce to `1` for now to avoid ambiguous "requested but silently omitted" behavior.

Acceptance criteria:

- Internal requests capped at 10 return at most 10 symbols, diagnostics, and references per hunk.
- Model-facing requests still use current global defaults unless explicit fields are later exposed.
- Excessive request values are clamped to global safety maxima.
- Truncation flags remain correct after request-specific caps are applied.

## Phase 2 — Track Actual Executor Request Attempts

Problem:

`collect_hunk_source_context_all_files()` currently applies `max_hunk_context_requests` using the sorted file-loop index:

```rust
if i >= max_requests { ... }
```

Policy-skipped files therefore consume request budget even though no executor call occurs.

Implementation:

- Introduce a mutable counter:

```rust
let mut attempted_requests = 0usize;
```

- Evaluate file cap separately from request cap.
- Evaluate routing policy before consuming request budget.
- Increment `attempted_requests` immediately before invoking the executor.
- Stop only when `attempted_requests >= max_requests` and another eligible request would be executed.

Preferred shape:

```rust
for (...) in grouped.iter().take(max_files) {
    let decision = decide_hunk_source_context(...);

    match decision {
        Skip { ... } => { ... }
        Use { ... } => {
            if attempted_requests >= max_requests {
                note request cap;
                break;
            }
            attempted_requests += 1;
            execute...
        }
    }
}
```

To avoid evaluating policy twice, consider splitting:

```rust
collect_hunk_source_context_for_file_with_decision(...)
```

or returning structured execution stats from the per-file helper.

Acceptance criteria:

- Policy-skipped files do not consume request budget.
- Executor-unavailable files should not count as attempted requests unless the metric is explicitly "eligible requests"; choose and document one meaning.
- Recommended metric: count actual executor calls only.
- Tests cover skipped files before an eligible file.

## Phase 3 — Introduce Structured Hunk Execution Statistics

Problem:

The workflow currently returns evidence, summaries, and notes, but not authoritative execution counts. This makes receipt state approximate.

Add a small stats DTO:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HunkSourceContextExecutionStats {
    pub files_considered: usize,
    pub files_policy_skipped: usize,
    pub requests_attempted: usize,
    pub requests_succeeded: usize,
    pub requests_failed: usize,
    pub requests_timed_out: usize,
    pub evidence_items_emitted: usize,
}
```

Update all-files collection to return:

```rust
pub struct HunkSourceContextCollectionResult {
    pub evidence: Vec<StructuredSecurityEvidence>,
    pub summaries: Vec<String>,
    pub notes: Vec<String>,
    pub stats: HunkSourceContextExecutionStats,
}
```

Alternative: append stats as a fourth tuple field, but a named struct is clearer and less brittle.

Acceptance criteria:

- Stats distinguish attempted, succeeded, failed, and timed-out calls.
- Policy skips are counted separately.
- Evidence count is explicit.
- Existing callers migrate cleanly.

## Phase 4 — Make Receipt State Accurate

Problem:

Current background receipt code computes:

```rust
let hunk_context_executed = args.hunk_context && lsp_available;
```

This overreports execution when no eligible hunk exists, policy skips all files, caps prevent calls, or no request is actually made.

Implementation:

Use structured execution stats from Phase 3.

Recommended receipt fields:

```rust
pub hunk_context_requested: bool,
pub hunk_context_available: bool,
pub hunk_context_attempted: bool,
pub hunk_context_succeeded: bool,
pub hunk_context_requests_attempted: usize,
pub hunk_context_requests_succeeded: usize,
```

Compatibility options:

- Rename `hunk_context_executed` to `hunk_context_attempted`; or
- keep `hunk_context_executed` but redefine it strictly as `requests_attempted > 0`.

Recommendation:

- Preserve `hunk_context_executed` for compatibility and define it as `requests_attempted > 0`.
- Add request counts additively if receipt consumers benefit.
- Add `hunk_context_succeeded = requests_succeeded > 0` if UI distinction is useful.

Acceptance criteria:

- Requested + available + all skipped => `executed == false`.
- One timed-out call => `executed == true`, `succeeded == false`.
- One successful zero-evidence response => `executed == true`, `succeeded == true`.
- Receipt semantics are documented.

## Phase 5 — Resolve Relative Paths Against `allowed_root`

Problem:

`execute_hunk_source_context_typed()` resolves relative `request.file_path` values using `std::env::current_dir()`.

Programmatic security reviews may use a repository root different from process CWD, even when `LspTool::allowed_root` is configured correctly.

Implementation:

Replace:

```rust
current_dir().join(&request.file_path)
```

with:

```rust
self.allowed_root.join(&request.file_path)
```

For absolute paths, preserve direct validation.

Refactor into one helper used by both typed and model-facing paths:

```rust
fn resolve_file_from_str(&self, path: &str) -> Result<PathBuf, ToolError>
```

Then have existing `resolve_file(&Option<String>)` delegate to it.

Acceptance criteria:

- Relative internal paths resolve against configured repository root.
- Process CWD does not affect path resolution.
- Absolute paths inside root still work.
- Paths escaping root remain rejected.

## Phase 6 — Strengthen the Concrete Forwarding Tests

Problem:

Some new tests construct a `HunkSourceNavigationRequest` and assert its fields, but do not execute `LspHunkSourceContextExecutor` or verify the request received by the typed target.

Goal:

Add tests that fail if the concrete adapter stops forwarding pre-parsed hunks.

Preferred approach:

Introduce a narrow internal abstraction behind the executor:

```rust
#[async_trait]
pub trait TypedHunkSourceContextTarget: Send + Sync {
    async fn execute_typed(
        &self,
        request: HunkSourceNavigationRequest,
    ) -> Result<HunkSourceNavigationResponse, String>;
}
```

- Implement it for `LspTool`.
- Make `LspHunkSourceContextExecutor` generic over or hold `Arc<dyn TypedHunkSourceContextTarget>` internally.
- Production constructor still accepts `Arc<LspTool>`.
- Tests use a recording fixture target.

Smaller alternative:

- Add a test-only constructor accepting a closure/fixture target.

Required assertions:

- both hunk descriptors arrive unchanged;
- `patch: None` remains `None`;
- intent and include flags survive;
- request-specific limits survive;
- file path survives unchanged before typed path resolution.

Acceptance criteria:

- The test invokes `LspHunkSourceContextExecutor::execute_hunk_source_context()`.
- A regression to JSON serialization or dropping hunks fails the test.
- No live LSP server is required.

## Phase 7 — Test Request-Specific Navigator Limits

Add pure or fixture-based tests proving effective caps.

Suggested tests:

- `typed_hunk_execution_uses_requested_symbol_cap`
- `typed_hunk_execution_uses_requested_diagnostic_cap`
- `typed_hunk_execution_uses_requested_reference_cap`
- `typed_hunk_execution_clamps_caps_to_global_maximum`
- `typed_hunk_execution_coerces_zero_caps_per_policy`

If testing full typed execution is awkward without LSP, factor effective-limit calculation into a pure helper:

```rust
fn effective_hunk_navigation_limits(
    request: &HunkSourceNavigationRequest,
) -> EffectiveHunkNavigationLimits
```

Test that helper and ensure collector construction consumes it.

Acceptance criteria:

- Request-specific caps are not merely stored; they configure the navigator.
- Exact cap and overflow truncation semantics remain correct.

## Phase 8 — Test Actual Request Budgeting

Add tests using fixture executors and mixed file eligibility.

Required cases:

- unsupported file first, eligible file second, max requests 1 => eligible file executes;
- two eligible files, max requests 1 => one executor call;
- policy skips do not increment attempted count;
- timeout increments attempted and timed-out counts;
- executor error increments attempted and failed counts;
- success increments attempted and succeeded counts;
- file cap and request cap remain independently observable.

Acceptance criteria:

- Request cap measures executor calls, not loop position.
- File ordering remains deterministic.

## Phase 9 — Correct Documentation Language

Update stale comments/docs that call collection deterministic.

Use:

- deterministic routing, ordering, and bounded invocation;
- best-effort, server-dependent LSP evidence;
- fail-open execution.

Files:

- `src/security/workflow/report.rs` doc comments;
- `architecture/lsp.md`;
- `.opencode/skills/lsp/SKILL.md`;
- `AGENTS.md`;
- `README.md` if receipt semantics are described there.

Acceptance criteria:

- No docs imply LSP evidence itself is deterministic.
- Receipt field semantics match implementation.
- Request caps are described as actual executor request caps.

## Phase 10 — Verification

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

Before this hardening pass is complete:

- Request-specific per-hunk limits configure the navigator.
- Limits are clamped to safe maxima.
- Request caps count actual executor calls.
- Structured execution stats distinguish skips, attempts, success, failure, and timeout.
- Receipt execution state reflects actual calls.
- Relative typed paths resolve against `allowed_root`.
- Concrete forwarding tests invoke the real executor seam.
- No live LSP server is required for new tests.
- Documentation distinguishes deterministic invocation from best-effort evidence.

## Expected Follow-Up

After this pass, stop iterating on the architecture and evaluate the integration on real repositories. Measure:

- percentage of changed files eligible for hunk context;
- executor success/timeout/failure rates;
- average evidence volume and truncation frequency;
- whether hunk evidence improves security finding precision or only adds noise;
- whether first-hunk-centered semantic collection is adequate for multi-hunk files.

Use those results to decide whether the next investment should be per-hunk targeted enrichment, richer security scoring, or broader review/edit-planning integration.
