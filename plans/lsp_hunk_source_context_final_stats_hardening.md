# LSP Hunk Source Context Final Statistics Hardening Plan

## Purpose

The hunk-source-context integration is now operational and suitable for real-workload evaluation. The remaining issues after `dbb2d8d4a1a1f735b456b73a4fa4a0f6094d85f5` are narrow:

1. `evidence_items_emitted` is cumulatively overcounted across multiple files.
2. `files_considered` is initialized from the file cap rather than incremented as files are actually processed.
3. The forwarding regression test covers the trait boundary but not the production `LspHunkSourceContextExecutor` adapter specifically.

This pass should fix those semantics without expanding the architecture.

## Current State

The current code already provides:

- request-specific per-hunk cap clamping;
- typed internal hunk execution;
- `allowed_root`-relative path resolution;
- deterministic file ordering;
- policy skips that do not consume request budget;
- structured success/failure/timeout statistics;
- receipt fields derived from real attempts and successes;
- fail-open behavior;
- extensive policy, timeout, ordering, and budgeting tests.

Known defects:

### Cumulative evidence count

The aggregation path currently does:

```rust
all_evidence.extend(result.evidence);
stats.evidence_items_emitted += all_evidence.len();
```

This repeatedly adds prior files' evidence.

Example:

- file A emits 2 items;
- file B emits 3 items;
- file C emits 1 item.

Current reported count:

```text
2 + 5 + 6 = 13
```

Correct count:

```text
2 + 3 + 1 = 6
```

### Ambiguous `files_considered`

The current initialization:

```rust
files_considered: grouped.len().min(max_files)
```

means "files inside the file cap," not necessarily files whose policy was actually evaluated before an earlier request-cap break.

### Production adapter test gap

The recording executor test proves request construction through the `HunkSourceContextExecutor` trait boundary, but does not instantiate the real `LspHunkSourceContextExecutor` adapter.

## Non-Goals

Do not change public `hunkSourceContext` JSON.

Do not add new receipt fields unless strictly necessary.

Do not alter finding eligibility.

Do not add new LSP operations.

Do not change policy defaults.

Do not add live language-server test dependencies.

Do not begin per-hunk targeted semantic enrichment in this pass.

## Phase 1 — Fix `evidence_items_emitted`

Preferred implementation:

```rust
all_evidence.extend(result.evidence);
```

After the loop:

```rust
stats.evidence_items_emitted = all_evidence.len();
```

This makes the aggregate authoritative and avoids incremental accounting drift.

Alternative:

```rust
stats.evidence_items_emitted += result.evidence.len();
all_evidence.extend(result.evidence);
```

Recommendation: use the post-loop assignment.

Acceptance criteria:

- One file with 2 items reports 2.
- Files with 2, 3, and 1 items report 6.
- Zero evidence reports 0.
- `stats.evidence_items_emitted == result.evidence.len()` for every collection result.

## Phase 2 — Define `files_considered` Precisely

Preferred semantic definition:

> Number of files for which the workflow evaluated hunk-context policy.

Implementation:

- Initialize `files_considered` to `0`.
- Increment immediately after the file-cap check and before policy evaluation:

```rust
stats.files_considered += 1;
```

- Do not increment for files beyond `max_files`.
- If the request cap breaks the loop before evaluating another file, that file is not considered.

This yields:

- file-cap skipped files: not considered;
- policy-skipped files: considered and counted in `files_policy_skipped`;
- eligible files blocked by request cap before policy evaluation: not considered;
- eligible files evaluated before request-cap break: considered.

Alternative semantic:

If the intended meaning is "files inside the configured file cap," rename the field to:

```rust
files_within_cap
```

Recommendation: keep `files_considered` and make it reflect actual policy evaluation.

Acceptance criteria:

- Five files, `max_files = 3` => considered at most 3.
- Eight files, `max_requests = 1`, first file eligible => considered 2 only if the second file's policy is evaluated before detecting the request cap; otherwise considered 1. Pick one control-flow order and test it explicitly.
- Policy-skipped files increment both `files_considered` and `files_policy_skipped`.
- `files_policy_skipped <= files_considered` always holds.

## Phase 3 — Clarify Request-Cap Control Flow

To make `files_considered` deterministic, choose one of these loop orders.

### Option A — Check request cap before policy evaluation

```rust
if attempted_requests >= max_requests {
    note and break;
}
stats.files_considered += 1;
let decision = decide_hunk_source_context(...);
```

Pros:

- no unnecessary policy work;
- `files_considered` tracks only files that could still execute.

Cons:

- skipped files after the request budget is exhausted are not counted as policy-skipped.

### Option B — Evaluate policy before request cap

```rust
stats.files_considered += 1;
let decision = decide_hunk_source_context(...);
if decision is Use && attempted_requests >= max_requests {
    note and break;
}
```

Pros:

- policy-skip statistics remain complete within the file cap;
- request budget is consumed only for actual calls.

Cons:

- one additional policy evaluation may occur after request budget is exhausted.

Recommendation: use Option B. Policy evaluation is cheap, and it keeps skip statistics meaningful.

Acceptance criteria:

- The chosen order is documented in a short code comment.
- Tests assert the exact considered/skip counts around request-cap boundaries.

## Phase 4 — Add Aggregate Statistics Invariants

Add assertions in tests for these invariants:

```rust
stats.requests_succeeded
    + stats.requests_failed
    + stats.requests_timed_out
    == stats.requests_attempted
```

provided no future cancellation category exists.

Also assert:

```rust
stats.evidence_items_emitted == result.evidence.len()
stats.files_policy_skipped <= stats.files_considered
stats.requests_attempted <= max_requests
stats.files_considered <= max_files
```

If successful requests can later be cancelled separately, update the invariant accordingly.

Acceptance criteria:

- Invariants are covered in success, failure, timeout, and mixed-result tests.
- A future accounting regression fails tests immediately.

## Phase 5 — Strengthen the Production Adapter Test Seam

Goal:

Prove that `LspHunkSourceContextExecutor` forwards the request unchanged into the typed target.

Preferred implementation:

Introduce a small internal trait in `src/security/lsp_executor.rs`:

```rust
#[async_trait::async_trait]
trait TypedHunkSourceContextTarget: Send + Sync {
    async fn execute_hunk_source_context_typed_target(
        &self,
        request: HunkSourceNavigationRequest,
    ) -> Result<HunkSourceNavigationResponse, String>;
}
```

Implement for `LspTool`:

```rust
#[async_trait::async_trait]
impl TypedHunkSourceContextTarget for LspTool {
    async fn execute_hunk_source_context_typed_target(
        &self,
        request: HunkSourceNavigationRequest,
    ) -> Result<HunkSourceNavigationResponse, String> {
        self.execute_hunk_source_context_typed(request).await
    }
}
```

Then let the production executor store:

```rust
Arc<dyn TypedHunkSourceContextTarget>
```

while retaining:

```rust
pub fn new(tool: Arc<LspTool>) -> Self
```

Add a `#[cfg(test)]` constructor for a recording target.

Test assertions:

- exact file path forwarded;
- all hunk descriptors forwarded unchanged;
- `patch: None` remains `None`;
- intent preserved;
- include flags preserved;
- requested caps preserved;
- returned response propagated unchanged;
- target error propagated unchanged.

Smaller acceptable alternative:

Use a generic private executor type parameterized over the typed target, with a public type alias for production.

Acceptance criteria:

- The test invokes `LspHunkSourceContextExecutor::execute_hunk_source_context()`.
- The production adapter path, not only a fixture workflow executor, is under test.
- No live LSP server is required.

## Phase 6 — Add Focused Regression Tests

Required tests:

### Evidence counting

- `evidence_items_emitted_matches_total_evidence_len`
- `evidence_items_emitted_does_not_double_count_multiple_files`
- `zero_evidence_reports_zero_items_emitted`

### File counting

- `files_considered_increments_per_policy_evaluation`
- `policy_skipped_file_counts_as_considered`
- `files_beyond_file_cap_are_not_considered`
- `request_cap_boundary_has_documented_considered_count`

### Invariants

- `request_outcome_counts_sum_to_attempted`
- `policy_skipped_never_exceeds_considered`
- `attempted_never_exceeds_request_cap`

### Production adapter

- `lsp_hunk_executor_forwards_exact_typed_request`
- `lsp_hunk_executor_propagates_target_response`
- `lsp_hunk_executor_propagates_target_error`

Acceptance criteria:

- Tests fail against the current cumulative evidence-count implementation.
- No test launches a language server.

## Phase 7 — Documentation and Comments

Update only semantics affected by this pass.

Files:

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `AGENTS.md`, only if execution-stat definitions are listed there
- inline docs on `HunkSourceContextExecutionStats`

Document:

```text
files_considered = number of files whose hunk-context policy was evaluated
evidence_items_emitted = final aggregate evidence vector length
requests_attempted = actual executor calls
```

Acceptance criteria:

- Field definitions are unambiguous.
- Documentation matches control-flow order.

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

Before this pass is complete:

- `evidence_items_emitted` equals final evidence length.
- Evidence is not cumulatively double-counted.
- `files_considered` has one documented meaning.
- File/request/skip statistics obey tested invariants.
- The real `LspHunkSourceContextExecutor` forwarding seam is under test.
- No live LSP dependency is added.
- No broader architecture changes are introduced.

## Expected Follow-Up

After this pass, stop hardening this subsystem and move to instrumented real-repository evaluation. The implementation should be considered stable unless workload data reveals a concrete problem.
