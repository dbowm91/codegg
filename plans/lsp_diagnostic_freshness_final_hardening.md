# LSP Diagnostic Freshness Final Hardening Plan

## Purpose

The diagnostic freshness hardening pass after `afd4a0490828472f3b0124e7bc0952571f2e64e1` closed the major semantic issues: `generated_at_ms` became `age_ms`, URI conversion is URL-aware, save-with-text participates in freshness tracking, legacy per-file diagnostics mostly derive from snapshots, and security context now carries clearer stale/unavailable evidence notes.

This final hardening pass should close the remaining consistency issues before moving to semantic-context DTO consolidation. Keep this pass small and correctness-focused.

## Current State

Verified current behavior:

- `LspDiagnosticSnapshot` now exposes `age_ms`, documented as elapsed time since diagnostics were received.
- Normal fresh snapshots set `age_ms` from `entry.received_at.elapsed()`.
- File URI conversion uses `url::Url::parse(...).to_file_path()` with string fallback.
- `save_file(uri, Some(text))` updates `last_content_change_at` and can make diagnostics `PossiblyStale`.
- `DiagnosticsCollector::get_diagnostics_for_file()` now uses `get_diagnostic_snapshot_for_key()` instead of raw diagnostics.
- Public `diagnostics` output exposes `freshness`, `source`, `age_ms`, `usable_evidence`, and `diagnostics_may_still_be_warming`.
- `semanticContext` and `securityContext` include `diagnostic_evidence` with `age_ms`.
- `securityContext` adds distinct notes for `Stale`, `PossiblyStale`, and `Unavailable` diagnostics.
- `capability_snapshot_for_file()` exists and is used by `semanticContext` capability gating.

Remaining concerns:

- Stale snapshots currently set `age_ms: 0`, which makes old cached diagnostics look newly received.
- The public `diagnostics` operation still computes `diagnostics_may_still_be_warming` through `service.diagnostics_may_still_be_warming()` instead of directly from snapshot freshness, while `DiagnosticsCollector::get_diagnostics_for_file()` derives warming from snapshot freshness.
- `get_all_diagnostics()` still returns freshness-blind bulk diagnostics from raw cached vectors.
- The public `capabilities` operation still duplicates snapshot construction instead of using `capability_snapshot_for_file()`.
- Some tests named like snapshot transition tests still exercise raw dispatch/cache behavior rather than `LspClient::diagnostic_snapshot()` transitions directly.

## Non-Goals

Do not make `egglsp::semantic_context::SemanticContextResponse` the runtime source of truth in this pass.

Do not remove existing public JSON fields unless unavoidable.

Do not add new LSP operations.

Do not require live language servers in tests.

Do not change security-review synthesis behavior beyond preserving diagnostic evidence semantics.

Do not refactor unrelated LSP operation plumbing.

## Phase 1 — Fix Stale Snapshot `age_ms`

Problem:

`LspClient::diagnostic_snapshot()` returns stale cached diagnostics with `age_ms: 0` when the cache entry predates `diagnostics_invalidated_at`. Because `age_ms` now means elapsed time since diagnostics were received, stale cached diagnostics should preserve their actual age.

Change:

- In the stale branch, set:

```rust
age_ms: entry.received_at.elapsed().as_millis() as i64,
```

not `0`.

Rules:

- `Unavailable` snapshots may keep `age_ms: 0`, because there is no diagnostic receipt time.
- `Fresh`, `PossiblyStale`, and `Stale` snapshots with cached diagnostics should all report elapsed age from the cache entry.

Acceptance criteria:

- Stale snapshots with cached diagnostics report non-negative age derived from the cached entry.
- Tests distinguish unavailable `age_ms == 0` from stale cached diagnostics with a meaningful age.
- Documentation remains consistent: `age_ms` is always age since diagnostics were received when diagnostics exist.

## Phase 2 — Align Public Diagnostics Warming Semantics With Snapshot Freshness

Problem:

`DiagnosticsCollector::get_diagnostics_for_file()` derives warming from snapshot freshness, but the public `diagnostics` operation still uses `service.diagnostics_may_still_be_warming()`. This can diverge when a snapshot is `PossiblyStale` but a cache entry exists.

Preferred change:

- Add a small helper on `LspDiagnosticSnapshot`:

```rust
impl LspDiagnosticSnapshot {
    pub fn diagnostics_may_still_be_warming(&self) -> bool {
        matches!(self.freshness, LspDiagnosticFreshness::PossiblyStale) && self.diagnostics.is_empty()
    }
}
```

- Use this helper in:
  - `DiagnosticsCollector::get_diagnostics_for_file()`;
  - public `diagnostics` tool output.

Compatibility note:

The existing `LspClient::diagnostics_may_still_be_warming()` may remain for low-level callers, but public/collector output should use snapshot semantics so the read model is consistent.

Acceptance criteria:

- The public `diagnostics` operation and collector compatibility wrapper compute warming the same way.
- A `PossiblyStale` empty snapshot sets warming true.
- A `PossiblyStale` non-empty snapshot does not claim the server has no diagnostics; it can still surface diagnostics as best-effort stale-ish evidence with `freshness: PossiblyStale`.

## Phase 3 — Decide and Document Bulk Diagnostics Freshness Semantics

Problem:

`DiagnosticsCollector::get_all_diagnostics()` still reconstructs `HashMap<String, Vec<FileDiagnostic>>` from raw cached diagnostics. It has no freshness metadata.

Choose one of two options.

Option A, minimal:

- Rename or document this as a legacy freshness-blind bulk view.
- Add doc comments stating callers that need reliability metadata must use per-file snapshots.
- Do not use this method in security review or semantic-context evidence paths.

Option B, better:

- Add a new method:

```rust
pub async fn get_all_diagnostic_snapshots(
    &self,
) -> Result<HashMap<String, LspDiagnosticSnapshot>, LspError>
```

- Preserve `get_all_diagnostics()` as a compatibility wrapper.
- The snapshot method should use client-level snapshot construction per URI.

Recommendation: Option B if implementation is straightforward; otherwise do Option A with explicit docs and a follow-up note.

Acceptance criteria:

- Bulk diagnostics behavior is no longer ambiguous.
- Freshness-sensitive consumers have an obvious API.
- Legacy callers are not broken.

## Phase 4 — Reuse `capability_snapshot_for_file()` in the Public `capabilities` Operation

Problem:

A helper exists for capability snapshot construction, but the public `capabilities` operation still duplicates server/language derivation logic.

Change:

- Update the public `capabilities` operation to call `self.capability_snapshot_for_file(&file).await`.
- If it returns `Some(snapshot)`, serialize it.
- If it returns `None`, return `LspCapabilitySnapshot::default()` as before, unless there is a better explicit unavailable response already supported by the operation.

Acceptance criteria:

- There is one tool-level helper for constructing capability snapshots from file paths.
- `semanticContext`, `securityContext`, and `capabilities` use the same server/language derivation path.
- Existing behavior on missing/uninitialized capabilities remains fail-soft.

## Phase 5 — Add Direct `diagnostic_snapshot()` Transition Tests

Problem:

Some existing tests verify notification ingestion and cache entries, but the most important behavior is the snapshot classification itself.

Add direct tests around `LspClient::diagnostic_snapshot()` or a factored pure helper if constructing `LspClient` is too heavy.

Preferred approach:

- Extract a small pure classification helper if needed:

```rust
fn classify_diagnostic_freshness(
    received_at: Option<Instant>,
    last_content_change_at: Option<Instant>,
    diagnostics_invalidated_at: Option<Instant>,
) -> LspDiagnosticFreshness
```

or a helper that accepts an optional cache entry and returns a snapshot metadata struct.

Test cases:

- no cache entry -> `Unavailable`;
- cache entry and no later content change -> `Fresh`;
- cache entry and later content change -> `PossiblyStale`;
- cache entry older than invalidation -> `Stale`;
- cache entry newer than invalidation -> `Fresh` or `PossiblyStale` depending on content-change state;
- stale cached diagnostics preserve nonzero/elapsed `age_ms`;
- unavailable snapshot has `age_ms == 0`;
- URL-decoded file path is used in snapshot `file_path`.

Avoid timing flakiness:

- Use controlled `Instant` values where possible.
- If direct `Instant` construction is awkward, insert small sleeps only as a last resort and keep assertions loose.

Acceptance criteria:

- Tests exercise snapshot classification, not only dispatch/cache insertion.
- Tests are deterministic and do not require live LSP servers.
- Test names accurately describe the behavior being asserted.

## Phase 6 — Documentation Cleanup

Update only if implementation changes affect docs.

Files:

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `AGENTS.md`, only for verified facts

Docs should state:

- `age_ms` is zero for unavailable snapshots and elapsed diagnostic age for cached diagnostic snapshots, including stale cached snapshots.
- Public `diagnostics_may_still_be_warming` is derived from snapshot semantics.
- Bulk diagnostics are either freshness-blind legacy output or there is a new bulk snapshot API.
- Capability snapshot construction is shared across `capabilities`, `semanticContext`, and `securityContext`.

Acceptance criteria:

- No stale references to `generated_at_ms` remain.
- No docs imply stale diagnostics are newly received when `age_ms` is zero.
- Bulk diagnostics semantics are explicit.

## Suggested Verification Commands

Run:

```bash
cargo fmt --all
cargo test -p egglsp
cargo test --lib lsp
```

Then, if feasible:

```bash
cargo test --all --workspace
```

If the full workspace test is skipped, record the reason in the handoff summary.

## Review Checklist

Before considering this pass complete:

- Stale cached diagnostics report real `age_ms`.
- Public diagnostics and collector diagnostics use the same warming semantics.
- Bulk diagnostics semantics are explicit or snapshot-capable.
- `capabilities` reuses the same capability snapshot helper as context operations.
- Direct snapshot transition tests exist and are accurately named.
- No live LSP server is required by the new tests.
- Public JSON remains backward compatible except for already-completed `generated_at_ms -> age_ms` rename.

## Expected Follow-Up

After this final hardening pass, diagnostic freshness should be stable enough to stop iterating on small correctness fixes. The next meaningful LSP phase should be semantic-context consolidation: make `egglsp::semantic_context::SemanticContextResponse` the internal source of truth and treat the existing tool packets as presentation DTOs.
