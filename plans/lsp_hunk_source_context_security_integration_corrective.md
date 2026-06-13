# LSP Hunk Source Context Security Integration Corrective Plan

## Purpose

The first review-integration pass added useful building blocks around `hunkSourceContext`: a deterministic routing policy, a compact agent-facing formatter, security workflow option plumbing, and new structured evidence types.

However, the current security workflow integration does not yet execute real hunk semantic collection. It emits policy-decision metadata as `HunkNavigation` evidence and allows that metadata to participate in finding eligibility. This creates a false-evidence risk and conflicts with the documented claim that actual hunk semantic evidence is being collected.

This corrective pass should restore conservative evidence semantics, introduce a real executor boundary for `hunkSourceContext`, wire an explicit enable path, and make documentation match the implemented architecture.

## Current State

As of `a6ea6287462d128ea4ee64cfbf8678635e5d6a26`:

- `HunkSourceContextPolicy` and `decide_hunk_source_context()` exist and are well-tested.
- `format_hunk_source_context_summary()` exists and produces bounded, deterministic text.
- `SecurityReviewWorkflowOptions` includes `enable_hunk_source_context`, defaulting to `false`.
- `SecurityEvidenceKind::HunkNavigation` exists.
- `ChangedHunk::to_hunk_descriptor()` exists.
- `run_security_review_workflow()` conditionally calls `collect_hunk_source_context_all_files()` when hunk context is enabled.
- `collect_hunk_source_context_for_file()` currently does not execute `HunkSourceNavigationCollector` or `hunkSourceContext`; it emits a synthetic `HunkNavigation` evidence item stating that policy selected the file.
- `collect_hunk_source_context_all_files()` therefore merges routing decisions, not semantic evidence.
- `is_finding_eligible()` allows `HunkNavigation + ChangedHunk` and `HunkNavigation + Preflight` to satisfy the evidence gate.
- The normal `/security-review` command has no flag that sets `enable_hunk_source_context`.
- The formatter is implemented but not meaningfully used in the workflow output path.
- Documentation overstates the current integration by claiming that actual hunk semantic evidence is collected.

## Critical Correctness Requirement

A routing decision is not security evidence.

The workflow must never convert:

```text
policy says hunkSourceContext would be useful
```

into:

```text
semantic hunk evidence supports a finding
```

Only evidence derived from an actual `HunkSourceNavigationResponse` may use the `HunkNavigation` evidence kind.

## Non-Goals

Do not add multi-file semantic collection beyond the existing per-file loop.

Do not add richer security-specific hunk scoring in this pass.

Do not change core `hunkSourceContext` parsing/navigation semantics.

Do not make hunk collection mandatory.

Do not require a live LSP server in unit tests.

Do not remove the existing `securityContext` enrichment path.

## Phase 1 — Remove Policy Decisions From Security Evidence

Problem:

`collect_hunk_source_context_for_file()` currently emits a synthetic `StructuredSecurityEvidence` item with `kind = HunkNavigation` when policy returns `Use`, even though no semantic collection occurred.

Implementation:

- Remove policy-only `HunkNavigation` evidence generation.
- On `HunkSourceContextDecision::Use`, if no executor is available, return:
  - empty evidence;
  - no semantic summary;
  - a note such as:

```text
hunkSourceContext recommended for <file>, but no executor is available; continuing without semantic hunk evidence
```

- On `Skip`, continue returning empty evidence and a clear skip note.
- Keep policy decisions available for tracing/debugging, but never pass them into finding synthesis as evidence.

Acceptance criteria:

- A policy `Use` decision without execution produces zero `HunkNavigation` evidence.
- A policy `Skip` decision produces zero evidence.
- Security findings cannot be created from routing metadata.

## Phase 2 — Introduce a `HunkSourceContextExecutor` Boundary

Mirror the existing `SecurityContextExecutor` pattern.

Suggested trait:

```rust
#[async_trait::async_trait]
pub trait HunkSourceContextExecutor: Send + Sync {
    async fn execute_hunk_source_context(
        &self,
        request: egglsp::hunk_context::HunkSourceNavigationRequest,
    ) -> Result<egglsp::hunk_context::HunkSourceNavigationResponse, String>;
}
```

Possible implementation:

```rust
pub struct LspHunkSourceContextExecutor {
    tool: Arc<crate::tool::lsp::LspTool>,
}
```

The implementation may either:

- invoke the existing `hunkSourceContext` tool operation and deserialize its `results`; or
- construct/use the existing `HunkSourceNavigationCollector` directly through a stable service boundary.

Recommendation:

- Prefer invoking the existing tool operation if that avoids duplicating request validation, path/root checks, and result serialization semantics.
- Prefer a direct collector adapter only if the tool call path would force unnecessary JSON round-trips or awkward tool context requirements.

Acceptance criteria:

- Security workflow code depends on a trait, not directly on `LspTool` internals.
- A fixture executor can return static `HunkSourceNavigationResponse` objects in tests.
- Runtime execution remains fail-open.

## Phase 3 — Make Per-File Collection Execute Real Hunk Context

Refactor:

```rust
collect_hunk_source_context_for_file(...)
```

into an executor-backed function.

Suggested signature:

```rust
pub async fn collect_hunk_source_context_for_file<E: HunkSourceContextExecutor + ?Sized>(
    hunks: &[ChangedHunk],
    patch: &str,
    file_path: &Path,
    policy: &HunkSourceContextPolicy,
    executor: Option<&E>,
) -> (
    Vec<StructuredSecurityEvidence>,
    Option<String>,
    Vec<String>,
)
```

Flow:

1. Evaluate policy.
2. On `Skip`, return empty evidence plus note.
3. On `Use` with no executor, return empty evidence plus unavailable note.
4. Convert `ChangedHunk` values to `HunkDescriptor` values using `to_hunk_descriptor()`.
5. Build `HunkSourceNavigationRequest`:
   - file path;
   - internal pre-parsed hunks;
   - `patch = None` to avoid reparsing synthetic patch text, or use the real per-file patch if available;
   - include flags from policy;
   - conservative caps.
6. Execute through `HunkSourceContextExecutor`.
7. Convert the real response with `evidence_from_hunk_source_context()`.
8. Format the response with `format_hunk_source_context_summary()`.
9. Return evidence, summary, and notes.

Important:

- Do not use synthetic header-only patch text as the semantic input if pre-parsed hunks are already available.
- Preserve original per-file patch text where practical for policy size/hunk checks.

Acceptance criteria:

- `HunkNavigation` evidence is emitted only from a real `HunkSourceNavigationResponse`.
- Diagnostic evidence is emitted only from response diagnostics.
- Formatter output comes from the real response.
- Executor errors return empty evidence and a note.

## Phase 4 — Refactor All-Files Collection Around the Executor

Update:

```rust
collect_hunk_source_context_all_files(...)
```

Suggested signature:

```rust
pub async fn collect_hunk_source_context_all_files<E: HunkSourceContextExecutor + ?Sized>(
    hunks: &[ChangedHunk],
    policy: &HunkSourceContextPolicy,
    executor: Option<&E>,
) -> (
    Vec<StructuredSecurityEvidence>,
    Vec<String>,
    Vec<String>,
)
```

Behavior:

- Group hunks by file.
- Build one request per file.
- Execute one hunk-context request per file.
- Continue after per-file failures.
- Aggregate:
  - actual evidence;
  - formatted summaries;
  - skip/unavailable/error notes.

Add global caps:

- maximum files processed;
- maximum total requests;
- optional timeout per request.

These may reuse or mirror existing LSP enrichment limits.

Acceptance criteria:

- One failed file does not block other files.
- No policy-only evidence is aggregated.
- Summaries correspond one-to-one with successful semantic responses.

## Phase 5 — Restore Conservative Finding Eligibility

Problem:

`HunkNavigation` currently counts as an independent evidence dimension even when it may only represent routing metadata.

After Phase 1, only semantic evidence should use this kind, but the gate should still be reviewed conservatively.

Recommended rules:

- `HunkNavigation` may support a finding only when combined with one of:
  - `RiskMarker`;
  - failing `Preflight` evidence;
  - `Diagnostic` evidence;
  - `CodeReasoning`;
  - `CallPath`.
- `ChangedHunk + HunkNavigation` alone should not automatically produce a finding unless the hunk navigation evidence is semantically strong and explicitly classified.

Preferred immediate patch:

Remove:

```rust
(has_hunk_nav && has_changed_hunk)
```

Keep or reconsider:

```rust
has_marker && has_hunk_nav
has_preflight_fail && has_hunk_nav
```

Potential stronger model:

Split evidence kinds:

```rust
HunkSymbolContext
HunkDefinitionContext
HunkReferenceContext
```

But do not expand enums unnecessarily in this corrective pass unless it materially improves the gate.

Acceptance criteria:

- Changed code plus enclosing-symbol metadata alone cannot generate a security finding.
- Actual diagnostics plus changed-hunk evidence can still support synthesis under existing conservative rules.
- Marker-only semantics remain unchanged.

## Phase 6 — Add Explicit Command/API Enablement

Problem:

`enable_hunk_source_context` is not exposed by `SecurityReviewCommandArgs`.

Add an explicit flag, for example:

```text
--hunk-context
```

Potential paired disable flag if later default-enabled:

```text
--no-hunk-context
```

Implementation:

- Add `hunk_context: bool` to `SecurityReviewCommandArgs`.
- Parse `--hunk-context`.
- Set `options.enable_hunk_source_context = args.hunk_context`.
- Keep default false for now.
- Update help/docs.

Acceptance criteria:

- Users can intentionally enable the integration.
- Default command behavior remains unchanged.
- JSON and human-rendered output include notes when hunk context is enabled but unavailable.

## Phase 7 — Wire Executor Through Command and Background Paths

The command/background paths already accept an optional `Arc<LspTool>` for `securityContext` enrichment.

Extend this plumbing so the same runtime LSP tool can back both executor traits.

Possible design:

```rust
let security_executor = lsp_tool.clone().map(LspSecurityContextExecutor::new);
let hunk_executor = lsp_tool.map(LspHunkSourceContextExecutor::new);
```

Update workflow entry points to accept both optional executors, or introduce a small runtime bundle:

```rust
pub struct SecurityReviewLspExecutors<'a> {
    pub security_context: Option<&'a dyn SecurityContextExecutor>,
    pub hunk_source_context: Option<&'a dyn HunkSourceContextExecutor>,
}
```

Recommendation:

- Prefer a bundle if function signatures are becoming unwieldy.

Acceptance criteria:

- Local TUI/background mode can execute real hunk context when enabled.
- Remote/socket mode without an LSP tool remains fail-open with a clear note.
- Existing `securityContext` enrichment behavior is preserved.

## Phase 8 — Use the Formatter in the Actual Output Path

Problem:

`format_hunk_source_context_summary()` currently exists primarily as tested infrastructure.

Use successful summaries in one of these places:

- append compact summaries to `SecurityReviewOutput.notes`;
- attach them to relevant `SecurityReviewPrompt.evidence` entries;
- add a dedicated optional output field if the output schema can change additively.

Recommendation:

- Add concise per-file summaries to notes for the first pass.
- Avoid inserting large summaries into every prompt.
- Keep output bounded.

Acceptance criteria:

- Successful hunk semantic collection is visible in output.
- Freshness and truncation metadata survive into the human-readable summary.
- Failed/skipped collection produces notes, not fabricated summaries.

## Phase 9 — Correct Diagnostic Line Semantics in Security Evidence

`FileDiagnostic.line` is 0-indexed internally, while hunk DTO ranges and security output are generally 1-indexed.

Review `evidence_from_hunk_source_context()`:

```rust
line: Some(diag.line)
```

If security evidence expects user-facing 1-indexed lines, convert with `diag.line + 1`.

Acceptance criteria:

- Diagnostic evidence line numbers match hunk/new-side line conventions.
- Grouping/window logic does not suffer an off-by-one mismatch.
- Add direct tests.

## Phase 10 — Documentation Correction

Update documentation to describe one architecture consistently.

Recommended wording:

- The security review workflow may deterministically execute hunk semantic collection when `--hunk-context` / option is enabled and an executor is available.
- Policy decisions themselves are routing metadata and never security evidence.
- Real `HunkNavigation` evidence comes only from `HunkSourceNavigationResponse`.
- The workflow is fail-open.
- Multi-file diffs are processed one file at a time by the workflow, while the underlying tool remains single-file per request.

Remove or revise contradictory claims such as:

- "recommendation-based: the model invokes the tool" if the security workflow executes it deterministically;
- "collects actual evidence" before the executor path exists.

Files:

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `AGENTS.md`
- `README.md` if the flag is user-facing

Acceptance criteria:

- Docs match implementation exactly.
- No policy-only path is described as semantic collection.

## Phase 11 — Tests

Add focused tests without live LSP servers.

### Evidence safety tests

- policy `Use` without executor emits no evidence;
- policy `Skip` emits no evidence;
- `ChangedHunk + policy decision` cannot produce a finding;
- `ChangedHunk + HunkNavigation` alone is not finding-eligible after the gate correction;
- `RiskMarker + real HunkNavigation` behavior matches the chosen conservative rule.

### Executor tests

Use a fixture `HunkSourceContextExecutor`:

- success returns real response and evidence;
- failure returns empty evidence plus note;
- stale diagnostics remain marked in formatted summary;
- truncation survives formatting;
- multiple files continue after one executor failure.

### Command tests

- `--hunk-context` sets `enable_hunk_source_context`;
- default leaves it disabled;
- enabled without executor produces unavailable note;
- enabled with fixture executor injects actual evidence.

### Line indexing tests

- internal diagnostic line 9 becomes security evidence line 10 if evidence is 1-indexed.

Acceptance criteria:

- Tests fail against the current policy-only evidence implementation.
- No live LSP server is required.
- Existing security workflow tests continue to pass.

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

If full workspace tests or Clippy are skipped, record why in the implementation summary.

## Review Checklist

Before considering this corrective pass complete:

- Policy decisions are never emitted as `HunkNavigation` evidence.
- Real hunk evidence requires a successful executor response.
- Finding eligibility remains conservative.
- The workflow can explicitly enable hunk context.
- Local runtime can provide an executor.
- Missing executor and per-file failures are fail-open.
- Formatter output is used in the actual workflow output.
- Diagnostic line indexing is correct.
- Documentation describes deterministic execution, not recommendation-only behavior.

## Expected Follow-Up

After this corrective pass:

1. Evaluate whether hunk semantic evidence improves security review precision in real workloads.
2. Add richer security-specific hunk enrichment only if evidence quality is good.
3. Add multi-file request orchestration limits/timeouts if repository-wide diffs are common.
4. Consider review/edit-planning integration outside the security workflow using the same executor and formatter abstractions.
