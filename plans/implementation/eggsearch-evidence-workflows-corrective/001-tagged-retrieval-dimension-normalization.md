# Eggsearch Evidence Workflows C001 — Tagged Retrieval-Dimension Contract Normalization

Status: ready
Primary class: correctness / compatibility corrective
Planning baseline: `agent/eggsearch-evidence-workflow-20261013`, M001 implementation `6be801e3a02349630a153f4141e9a69dce789584` (2026-10-13).
Source: `plans/subsystems/eggsearch-evidence-workflows-post-closure-corrective-addendum.md`.
Original milestone: `plans/implementation/eggsearch-evidence-workflows/001-evidence-fidelity-and-retrieval-state.md`.
Original closure: `plans/closure/eggsearch-evidence-workflows/001-status.md` (**historical closed evidence; do not rewrite**).
Long-term / governance: `plans/000-long-term-specification.md`, `plans/003-planning-process.md`; no ADR expected for a private adapter-contract correction.
Upstream immutable contract: `eggstack/eggsearch` tag `v0.4.1` (release commit `32769d684417567dfb30fc5adea1475cee7f0ed0`), `src/core/retrieval_status.rs`, `architecture/codegg-contract.md` §9. CodeGG's *current* managed pin is `0.3.9`; **this corrective does not update the pin**.

## 1. Objective

Make research retrieval receipts agree with the **actual tagged eggsearch response schema**, preventing search absence, successful completion, partial success, provider failure and unknown coverage from collapsing into a generic status or a false negative finding.

This is a **narrow repair**, not a rewrite of evidence collection or the broader M001 milestone. Retain correct M001 functionality and its historical closure.

## 2. Reproduced static defect / escaping the prior suite

Current `src/research/sources/eggsearch.rs::receipt_from_value` chooses `value.retrieval_state` or `value.retrieval_summary` and then reads scalar `status`, `state`, `complete`. Its explicit failure path otherwise examines only `result.success` and top-level `providers_failed`.

Published eggsearch `v0.4.1` has `ResponseRetrievalSummary { dimensions: Vec<RetrievalDimensionStatus>, has_failures, has_absences, has_truncation, ... }`. Each dimension has authoritative `state`, exact `attempt_outcome`, `truncated`, and `truncation_evidence`, with role, provider and subquery metadata. There is **no schema guarantee** of a scalar `retrieval_summary.status`/`complete`.

Thus a current-shaped `{"retrieval_summary":{"dimensions":[{"state":"completed_no_match", ...}],"has_failures":false,"has_absences":true,...},"groups":[]}` is not recognized as `Complete` by existing code, while a provider failure only under `dimensions` can be missed. The M001 closure tested a synthetic `retrieval_state` scalar and top-level provider failures rather than this contract. Prevent recurrence by replacing those fixtures with real tagged shapes and keeping legacy fixture coverage separately identified.

Important nuance: `ResponseRetrievalSummary.dimensions[*].state` is authoritative **per evidence role / provider / subquery**, not a scalar overall outcome. Do not turn an arbitrary single satisfied dimension into global `Complete`. Respect role coverage and unrelated failed/skipped dimensions. `absence_kind` alone is not authoritative. No live v0.4.1 binary test has been performed in this planning pass.

## 3. Non-goals and invariants

- No new provider engine, eggsearch MCP executable, new `SearchRuntimeContext`, tool schema, extra request/response network hop, persistent cache, new policy, or broader refactor.
- Preserve `SourceRecord.id` UUID, `RetrievalSourceLink`, `ResearchRetrievalReceipt`, `retrieval.json` and old stored run decoding; no migration unless proven unavoidable.
- Keep existing nested `metadata.published_at`, stable_id, provider/group, warning and trust-marker fixes.
- Source text, warning messages, provider labels and provenance are untrusted data; bounded and framed. No upstream field grants permissions, redirects model tool authority, or causes automatic `next_actions`.
- Retain full structured result even if its model-visible diagnostic `output` is truncated; truncated **text-only** fallback remains explicitly incomplete.
- Existing timeout, cancellation and disabled network semantics survive. No claims about completed network searches without explicit upstream evidence.
- Unknown/additive future fields and previously stored receipt objects must not cause a panic, unconditional `Complete` or surprising backward-incompatible deserialization.

## 4. Required contract and state handling

### 4.1 Tagged release schema/fixture gate (before code changes)

Read `eggstack/eggsearch@v0.4.1/src/core/retrieval_status.rs`, the tagged `architecture/codegg-contract.md` §9 and actual tagged response shaping. Record exactly which stable native tool outputs (`research_search` and `security_search`) populate `retrieval_summary`, `retrieval_status` compact, `workflow_coverage`, `providers_failed` and warning arrays. **Use the tool-specific envelope** and avoid assuming a common outer schema. Capture a minimal representative diagnostic JSON fixture via tagged source/serialization tests or a real binary in a one-time smoke; state which. Keep a separate 0.3.9 fixture when available. Do not mistake upstream `main`'s Unreleased changes for v0.4.1 guarantees.

### 4.2 Typed, conservative normalization

Build a small helper (typed partial serde DTO or explicit checked `serde_json::Value` traversal) to:
1. Read `retrieval_summary.dimensions[*].state` when provided, using `attempt_outcome` to resolve legacy missing state only per upstream semantics. Recognize precisely: `satisfied`, `completed_no_match`, `failed`, `skipped_by_policy`, `capability_unavailable`, `interrupted`, `partial`, `not_applicable`. Unknown strings remain **indeterminate**.
2. Observe `has_failures`/`has_truncation`, failed/interrupted/skipped job/dimension counts, `limit_reached_unknown_count`, `workflow_coverage` and confirmed vs unconfirmed `truncation_evidence`; do not infer completion from `has_failures=false` or source count. Treat inconsistent flags/dimensions conservatively and record bounded diagnostic codes.
3. Distinguish completed no-match from failure for **requested roles**. `completed_no_match` is a legitimate successful completed dimension; `not_applicable` is never affirmative absence. `capability_unavailable` and `skipped_by_policy` leave required-role coverage indeterminate. Mixed dimensions are aggregated without order sensitivity or accidental overwriting; CodeGG's existing aggregate receipt enum (`Complete|Partial|Failed|Skipped|Unknown|Truncated`) may be reused if each chosen category remains truthful.
4. Return `Complete` only when the relevant attempted work is positively finished and no required/represented dimensions remain failed, partial, skipped, interrupted or unknown, and no unconfirmed result cap undermines a negative finding. Otherwise use the most conservative representable non-complete outcome (e.g. all-failed/zero sources `Failed`, mixed success/failure `Partial`, policy-only `Skipped`, unknown/capability-only `Unknown`, confirmed truncation `Truncated`) and carry reasons via bounded `warning_codes`/`coverage`.
5. Keep `success=false` authoritative as a failure signal even if response body contains apparently complete dimensions; salvage valid cards as partial evidence and never let their presence erase a tool error.
6. Use `retrieval_status` compact boolean/status hints only when the tagged tool actually emits them and the dimensional diagnostic is absent; never treat compact absence of details as completed no-match. Preserve top-level `providers_failed` as an additional signal, **not** the only failure source.
7. Prefer exact provider IDs from failed dimension statuses (bounded and deduplicated) when top-level `providers_failed` is absent. Preserve concise role/capability/coverage warning codes without leaking raw provider error strings.
8. If no schema-recognized completion evidence exists (older text-only, malformed dimension arrays, missing summary or unfamiliar additive enum), return `Unknown` or an explicit error/truncation/skip receipt as appropriate. Synthetic historical `retrieval_state` fixtures may remain as backwards compatibility tests but MUST NOT be the only successful-path test.

### 4.3 Report, persistence and budget boundary

- Preserve the current adapter→coordinator→`ResearchRunResult` / `ResearchBundle` /`retrieval.json`→report path. Assert the human report explicitly distinguishes true completed empty from all-failed empty, partial with sources and indeterminate empty.
- Audit the coordinator's source-budget truncation/dedup: if dropping sources changes evidence completeness without the receipt tracking it, mark the receipt non-complete with an accurate code. Do not conflate **display truncation** with **source-budget truncation**.
- Prefer no new persisted fields; if unavoidable, make additions serde-defaulted and test reading prior `retrieval.json` files. Do not break any earlier public tool response contract.

## 5. Ordered work packages

**WP1 — Contract census and failing tests.** From immutable v0.4.1 code or bounded offline process smoke, freeze `research_search` and `security_search` response shapes and enumerate exact fields needed. Demonstrate existing M001 converter fails to identify canonical complete-empty and dimension-scoped failure with those fixtures. Record legacy 0.3.9 behavior separately.

**WP2 — Dimension-aware status reducer.** Implement one deterministic reduction path with precise state, attempt outcome and truncation handling. Validate malformed, duplicate and conflicting dimensions without panics; preserve bounded failed providers, roles and warnings. Do not copy or fork upstream provider routing.

**WP3 — End-to-end receipt fidelity.** Wire the reducer through `src/research/sources/eggsearch.rs` and minimum necessary typed helpers into coordinator and report. Add tests for source-budget cap, structured-vs-display truncation, no-card successes, and reading older stored receipts.

**WP4 — Tests / static guard.** Replace synthetic-only success fixtures with canonical tagged shapes; add narrowly targeted guard/test so use of `retrieval_summary.status` or `retrieval_summary.complete` **without inspecting dimensions** cannot recreate the defect. Assert per-role mix is not declared complete solely because one dimension succeeded.

**WP5 — Compatibility and docs.** Explain the upstream dimensional state contract in `architecture/research.md` and correct any misleading data examples; reconcile legacy/0.4.1 behavior in the closure record, not by rewriting historical M001 closure.

## 6. Required negative/positive acceptance matrix

| Scenario from tagged-shaped fixture | Required outcome |
|---|---|
| All requested dimensions `completed_no_match`, successful attempted jobs, zero cards, no unknown limits | `Complete` with zero sources; report permits scoped absence, not a global internet negative |
| `satisfied` for all required dimensions, corresponding cards, no caveats | `Complete` with source associations |
| One `satisfied` and one `failed` dimension; cards exist; `providers_failed` omitted | `Partial`; failed provider/role recorded; no false full coverage |
| All requested dimensions `failed`, no cards; `success=true` at MCP transport level | `Failed`; zero cards not no-match |
| Only `skipped_by_policy` dimensions (or network explicitly disallowed) | `Skipped`/indeterminate, never complete absence |
| Only `capability_unavailable` for a required role | `Unknown` with a capability warning; never `Complete` |
| `interrupted` or `partial`, with/without cards | Incomplete outcome; surviving cards linked; no negative finding |
| Confirmed `truncation_evidence` / `has_truncation` | Explicit incomplete/truncated; do not hide behind `satisfied` |
| `limit_reached_unknown_count > 0` but no confirmed truncation | Incomplete/uncertain; **not** asserted confirmed truncated, **not** `Complete` |
| All `not_applicable` or missing/unknown/malformed states | Unknown/indeterminate for requested evidence; never complete no-match |
| Conflicting `has_failures` flag and success dimensions, or `success=false` plus apparent completed cards | Fail conservatively, retain usable cards as partial; flag conflict |
| Legacy 0.3.9 / text-only / additive future fields | Backward compatible where known; indeterminate absent positive evidence, never panic |
| `StructuredSearchResult.value` full and model-facing `output` truncated | Full value wins; no fake truncation unless actual retrieval/collection truncated |
| Source-budget cap drops retained cards after dedup | Receipt records cap, only retained links, bounded and truthful status |

Every case MUST assert both machine receipt (including flags/providers/source count) and report text; include exact role/provider mixes and deterministic ordering. Ensure at least two fake-MCP end-to-end cases (completed-empty and failed-only-in-dimensions) plus a serialization/old-receipt test.

## 7. Verification commands

Run focused first (adapt only if test names differ, record deviations):

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib research::sources::eggsearch -- --test-threads=1
cargo test -p codegg --lib research:: -- --test-threads=1
cargo test -p codegg --test fake_eggsearch_mcp -- --test-threads=1
cargo test -p codegg --test search_backend_eggsearch -- --test-threads=1
cargo test -p codegg --lib research::store -- --test-threads=1
cargo clippy -p codegg --all-targets --locked -- -D warnings
scripts/verify.sh quick
git diff --check
```

Any hosted CI, all-features or live-process smoke run must be reported separately. Historical M001 closure records a non-workstream missing `reasoning` field in feature-enabled `tests/team_collaboration_m006_trajectory.rs`; do not mutate that unrelated fixture here or claim an unrun matrix passed. Canonical offline tests MUST pass.

## 8. Compatibility, security and concurrency

No planned SQLite or protocol migration; old `retrieval.json` remains readable. Unknown additive enums or malformed arrays produce indeterminate status, never panic. Provider-supplied metadata stays bounded, scrubbed, untrusted and non-executable; maintain `SourceRecord.external_untrusted` policy. No alternate HTTP or retry path; existing task cancellation, shared MCP session lifetime and runtime context remain intact. Status reduction must be local/deterministic, not spawn child tasks, mutate global state, or introduce extra network calls.

## 9. Acceptance and stop conditions

**Accept only if:** tagged-source schema evidence, canonical fixture coverage and every §6 case pass; correct status survives real coordinator/store/report path; zero-result conclusions remain bounded by attempted roles; full structured/truncated display separation holds; no new provider/policy/storage incompatibility appears; focused and canonical quick verification pass.

**Stop and report blocker** if tagged `research_search`/`security_search` responses cannot provide the required role/completeness evidence; if the scalar `ResearchRetrievalReceipt.state` cannot express materially different mandatory coverage states without losing necessary truth; if satisfying the correction forces changing CodeGG external search ownership or inventing success for missing fields. Prefer an explicitly additive backward-compatible receipt detail field only with a migration/old-run-read test.

## 10. Closure evidence required

Create `plans/closure/eggsearch-evidence-workflows-corrective/001-status.md` with:
- implementation commit SHA(s), branch and exact tagged v0.4.1 contract provenance;
- before/after failure reproducer plus a full scenario→receipt/report matrix;
- focused, lint and quick tests (with pass/fail/blocked truth, not blanket claims);
- frozen fixtures and any real pinned-binary smoke result; clearly identify unrun checks;
- authority, trust, cancellation, concurrency and backward-storage checks;
- impact on legacy-retirement M001/M002 and new M004 dependency readiness;
- unresolved findings by severity and recommendation: closed, conditionally closed, or new corrective required.

Do not rewrite `plans/closure/eggsearch-evidence-workflows/001-status.md`; C001 is the strict successor evidence record.
