# Tool-Selection Advisor Retrieval-Architecture Experiment M003 — Extended Frontier and Operating-Point Selection

Status: ready for handoff

Repository baseline: `cde81715`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md#m003--extended-frontier-and-operating-point-selection`

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`

Applicable ADRs:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: infrastructure

## 1. Objective

Run the preregistered R001 sweep once on dev and adjudicate it: measure every M002 variant path over the universe/K grid, select the smallest/cheapest point clearing 64>=0.99, 128>=0.98, 256>=0.95 with zero violations (K<=32 primary; bounded larger K only with all budgets met), then re-attach the unchanged M004 promotion separator at the frozen point. Positive close freezes at most one operating point and registers a separate fresh v4 qualification plan; negative close freezes nothing and unblocks nothing.

## 2. Why this milestone is ready

Hard dependency closed: M002 (`plans/closure/tool-selection-advisor-retrieval-architecture-experiment/002-status.md`, implementation `cde81715`) landed every preregistered variant behind the authority boundary with unit tests and two live asset-backed tests. Interface dependencies stable: M001 preregistration (`r001-preregistered-retrieval-architecture-v1`, fixture `e631cd2f…`, sweep command), the frozen span-packed ranker, and the M004 promotion separator reused unchanged.

## 3. Current implementation evidence

`src/tool_advisor/retrieval_architecture.rs`: `VariantSelection::new` (fail-closed grid validation) with canonical `point_mode()` names (`r001-<fusion>-a<alpha>-k<rrf_k>-p<pooling>-n<rerank_n>`, injective over the grid); `select_coarse_ordering` dispatching to the M001 normative fusion functions; `VariantRetriever<S: SemanticScorer>` with pooling-aware experiment-local embedding cache and BM25 fallback on any encoder failure; `EncoderSemanticScorer` production wrapper; `advisor_lexical_ordering` delegating to catalog BM25; `rerank_pool` with name-sorted chunking, per-chunk cost accounting, and deferred-universe revalidation; `FrozenRankerScorer` bound to the span-packed architecture. Live tests prove alpha 1.0/0.0 reproduce M004 semantic/BM25 orderings and the frozen ranker reranks with counted forwards.

## 4. Invariants that must not regress

- ADR-0009 authority monotonicity; `#![deny(unsafe_code)]` in the lib.
- M001 preregistration constants and M004 protocol constants untouched; the frozen M003 span-packed artifact byte-identical (loader verifies the head hash; manifest pins architecture and dev partition `b804b7d8…`).
- Historical corpora immutable; no v3/v4 case read for tuning or selection. v3 may be read diagnostically only after the point is frozen, with no parameter changes following.
- Catalog BM25 and the resolved surface untouched; default-off/local-only posture; no telemetry, download, network, or runtime discovery-path change.

## 5. Scope

### In scope

- One ignored-test sweep (`PREREG_SWEEP_COMMAND`) measuring the variant grid on dev: fusion settings (5 weighted-union alphas + 3 RRF_K + max = 9) × 2 poolings × 3 universes × primary Ks, plus bounded extended Ks (48/64, only with all budgets met) and two-stage re-rank arms (pools 48/64 through the frozen ranker).
- Fingerprint-bound per-universe checkpoint/resume (`checkpoint_path`/`checkpoint_accepts`, M004 pattern); fixture fingerprint recomputed live and refused on mismatch.
- Latency/cost evidence per point (retrieval p95/max, re-rank per-case cost, cold-load, wall-clock) checked against `verify_budgets`.
- Operating-point selection: smallest/cheapest clearing point, zero violations; promotion separator re-attached unchanged at the frozen point.
- Sweep receipt set under `target/tool-advisor/retrieval-architecture/`.

### Explicitly out of scope

- Any variant outside the M001 grids (requires roadmap amendment).
- Ranker retraining, calibration changes, gate relaxation, K-range changes.
- v4 holdout creation or qualification (separate plan, only on positive close).
- v3 tuning of any kind.

## 6. Required production changes

### Core/domain

Sweep harness in the experiment module reusing `VariantRetriever<EncoderSemanticScorer>`, `rerank_pool`/`FrozenRankerScorer`, `expand_universe`, `select_retrieval_point`, `extended_selection_allowed`, and the M004 promotion separator without modification. Fusion/pooling/rerank code is final; only measurement, checkpointing, and selection logic are added.

### Storage and migrations

None. Receipts are files under the preregistered output dir, never storage migrations.

### Protocol and DTOs

Sweep receipts carry protocol `r001-preregistered-retrieval-architecture-v1`, schema version, preregistration fingerprint, fixture fingerprint, and implementation commit SHA. No protocol version change.

### Runtime and concurrency

Serial ignored test (`--test-threads=1` convention); synchronous paths only; no new threads. Encoder load cached or budgeted inside `BUDGET_SWEEP_WALLCLOCK_S` (M004 precedent ~65 min with first-load outliers).

### Frontend or operator surface

None.

### Security and authorization

Deferred-only universes end to end; re-rank revalidation already in `rerank_pool`; denied/hidden tools never enter. Gate: zero authority violations.

### Documentation and static guards

Receipt schema documented in code; selection rationale (why this point, what was rejected and why) recorded in the closure tables.

## 7. Ordered work packages

### Work package A — Sweep harness with checkpoint/resume

Intent: run the full grid once, resumably, inside the wall-clock budget.

Required changes: ignored sweep test exactly matching `PREREG_SWEEP_COMMAND`; per-universe fingerprint-bound checkpoints; live fixture-fingerprint verification before measuring; budget evidence collection per point.

Acceptance evidence: interrupted run resumes without recomputation (checkpoint test on synthetic fingerprints exists in M001/M002; here demonstrated by receipt); full run completes within `BUDGET_SWEEP_WALLCLOCK_S` or closes negatively on resources.

### Work package B — Frontier measurement and adjudication

Intent: decide whether any variant clears the gates that stopped M004.

Required changes: measure all grid points; adjudicate dual-signal vs weight-sensitive misses from the alpha sweep; record per-tool ranks/margins for the residual misses at the best point.

Acceptance evidence: complete frontier tables per universe (recall at each K, violations, latency); explicit verdict per variant family.

### Work package C — Selection and promotion re-attachment

Intent: freeze at most one operating point and prove the promotion path still works on it.

Required changes: smallest/cheapest clearing-point selection (K<=32 primary; extended K only via `extended_selection_allowed`); unchanged M004 promotion separator run at the frozen point.

Acceptance evidence: frozen point receipt (mode name, universe, K, recall, budgets, fingerprints) or a plan-literal negative (best recalls, violation counts, resource accounting); promotion evidence attached to the frozen point only.

## 8. Failure, cancellation, restart, and contention semantics

Fixture/preregistration/budget/identity mismatches fail closed with explicit errors. SIGTERM or duplicate launch resumes from fingerprints, never recomputes silently. No concurrent sweep paths.

## 9. Compatibility and migration

Additive sweep code only. M002/M004 paths and tests unmodified and green.

## 10. Required tests

### Focused unit tests

- Checkpoint accept/refuse logic on synthetic fingerprints (extends M001 coverage to the sweep resume path).
- Selection rule: smallest/cheapest clearing point wins; ties broken deterministically; no clearing point selects nothing.
- `extended_selection_allowed` integration with measured budget evidence shapes.

### Integration tests

- The ignored sweep itself (long, serial, checkpointed); skips cleanly with an explicit message when reference assets are absent.
- Promotion separator regression tests reused unchanged.

### Restart and recovery tests

- Kill/resume: partial checkpoints resume to identical final receipts (or documented equivalence bounds).

### Contention and cancellation tests

- None (no concurrent path).

### Security and negative tests

- Authority-violation fixtures fail closed through the sweep path; zero violations recorded.
- Budget-exceeded points fail closed, never selected.

### Migration and compatibility tests

- None (no migration).

## 11. Required verification commands

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_architecture::
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::sequence_retrieval::
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::operating_point:: --skip m004_operating_point_sweep
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_architecture::tests::r001_extended_frontier_sweep --ignored --nocapture
cargo fmt --all -- --check
git diff --check
```

The ignored sweep runs once for closure evidence; it must skip cleanly (explicit message) when `target/tool-advisor/reference-assets/` is absent and never fail for missing local assets. Do not run the M004 ignored sweep. Do not use `--all-features` for workspace sweeps.

## 12. Documentation updates

- Receipt schema rustdoc; selection rationale in closure tables.
- No user-facing documentation change.

## 13. Acceptance criteria

- Complete frontier tables per universe with violations and latency; every grid value measured or its absence explained by a preregistered fail-closed rule.
- At most one frozen operating point with full receipt, or a plan-literal negative with best recalls and resource accounting.
- Positive close registers a separate fresh v4 qualification plan (new number in this workstream, not order-invariance M005); negative close unblocks nothing.
- M003 closes the workstream either way per the roadmap completion definition.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- The sweep needs a variant, K, budget, or gate outside preregistration.
- The frozen ranker or encoder assets require modification or re-download.
- Any v3/v4 case would be needed before the freeze.
- Wiring the sweep requires changing catalog BM25, the resolved surface, M002/M004 behavior, or the promotion separator.
- Scope would expand into training, calibration, or the runtime discovery path.

## 15. Closure evidence required

- Implementation commit SHA; requirement-to-evidence matrix against sections 5/7/13.
- Frontier tables, selection rationale, promotion evidence, resource accounting.
- Commands from section 11 with results, labeled local vs CI.
- Invariant, failure/recovery, migration, and security reviews per the closure template.
- Registry updates (M003 closed; v4 plan registered if positive) recorded in the closure record.

## 16. Handoff notes

- The grid is large (9 fusion settings × 2 poolings × 3 universes × 3+ primary Ks, plus extended and re-rank arms): share one encoder load across the whole sweep and reuse the warm descriptor cache across K values of the same universe/pooling to stay inside the wall-clock budget.
- Re-rank chunks by name-sorted order inside `rerank_pool`; chunk-local scores merge by global sort — the sweep must not re-sort pools by coarse score before chunking, or permutation-invariance breaks.
- The ranker manifest caps at 16 candidates per forward while pools are 48/64: chunk counts and forwards in every `RerankReport` are the honest cost; do not truncate pools silently to 16.
- New tests default to `current_thread` unless real concurrency is involved.
- Preserve unrelated user changes.
