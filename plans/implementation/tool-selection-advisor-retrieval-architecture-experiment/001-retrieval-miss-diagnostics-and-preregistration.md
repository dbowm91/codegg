# Tool-Selection Advisor Retrieval-Architecture Experiment M001 — Retrieval-Miss Diagnostics and Preregistration

Status: implemented (evidence gathered; positive result — see `plans/closure/tool-selection-advisor-retrieval-architecture-experiment/001-status.md`)

Repository baseline: `ccd548515f59f082c64a128c3b8ab3079ddf9f88`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md#m001--retrieval-miss-diagnostics-and-preregistration`

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`

Applicable ADRs:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: infrastructure

## 1. Objective

Attribute the four systematic M004 retrieval misses on dev and freeze the selection contract (protocol name/version, variant grid, universe/K grid, gates, budgets, exact sweep command, checkpoint/resume and fail-closed rules) so M002/M003 have zero unpreregistered degrees of freedom.

## 2. Why this milestone is ready

Hard dependencies closed: order-invariance M001-M004 (M004 negative closure with full frontier tables at `plans/closure/tool-selection-advisor-order-invariance-experiment/004-status.md`), M003 positive selection with receipt `assets/tool-advisor/order-invariance-m003-selection.json`. Interface dependencies stable: `sequence_retrieval` modes and `RetrievalFrontierPoint` universe/K identity, `operating_point` expansion and gate enforcement, ADR-0009 authority boundary. No hard dependency on any future holdout.

## 3. Current implementation evidence

`src/tool_advisor/sequence_retrieval.rs`: `RetrievalMode` (Bm25/Semantic/Rrf/NormalizedUnion), `RRF_K = 60.0`, equally weighted min-max `fuse_normalized`, cosine over MiniLM mean-pooled query/descriptor embeddings, BM25 fallback, name-tiebreak determinism. `src/tool_advisor/operating_point.rs`: `OPERATING_POINT_PROTOCOL = m004-preregistered-operating-point-v1`, `FRONTIER_UNIVERSES = [64, 128, 256]`, `FRONTIER_KS = [16, 24, 32]`, `expand_universe` preserving every labeled relevant tool, fail-closed `select_retrieval_operating_point`. M004 frontier facts (dev, 62 cases / 72 relevant per universe): BM25 52/72 flat; semantic 64-65/72; normalized-union 68/72 ceiling from K16; RRF the only K-sensitive mode but capping at 68/72. Local run receipts are not in git; the closure-record tables are the authoritative committed evidence.

## 4. Invariants that must not regress

- ADR-0009 authority monotonicity; `#![deny(unsafe_code)]` in the lib.
- Historical train/dev/test, v2, v3 corpora immutable; no v3/v4 case read for tuning or diagnosis.
- M004 protocol constants and the frozen M003 artifact untouched.
- Default-off/local-only posture; no telemetry, download, network, or runtime discovery-path change.

## 5. Scope

### In scope

- Miss-attribution diagnostics over dev-only fixtures plus the committed M004 closure tables.
- Preregistration constants block for the M003 sweep: protocol name/version, variant grid, universe/K grid, gates, budgets, exact sweep command, checkpoint/resume binding, fail-closed rules.
- Fixture fingerprinting (M003 artifact + dev corpus + expansion code) without asset modification.

### Explicitly out of scope

- Any encoder sweep or new long-running measurement (M001 runs no encoder workload).
- Variant implementation (M002) and frontier selection (M003).
- Gate relaxation, K-range changes beyond preregistering the M003 grid, promotion logic, v4 holdout, ranker changes.
- Catalog BM25 or resolved-surface changes.

## 6. Required production changes

### Core/domain

Add an experiment-local diagnostics module (e.g. under `src/tool_advisor/`) that, given per-case ranked lists, attributes each missed relevant tool with its BM25 rank, semantic rank, fused rank, and fusion score margin to the K-boundary, plus a shared-vs-independent cause classification. Reuse `expand_universe` and frontier point types; do not duplicate them.

### Storage and migrations

None. No migration, no artifact change, no asset writes.

### Protocol and DTOs

New protocol constants for the upcoming sweep (suggested name `r001-preregistered-retrieval-architecture-v1` with its own schema version) plus preregistered grids/gates/budgets as typed constants. Self-referential SHA is forbidden inside the protocol hash; the implementation commit SHA is passed separately at sweep time.

### Runtime and concurrency

No runtime change. Diagnostics run synchronously on small synthetic fixtures in unit tests; no threads, no caching of user context.

### Frontend or operator surface

None.

### Security and authorization

Diagnostics consume only deferred descriptors already admitted to the fixture universe. No authority logic touched.

### Documentation and static guards

Rustdoc on the attribution function and the preregistration constants stating the frozen grids, gates, budgets, and the exact M003 sweep command.

## 7. Ordered work packages

### Work package A — Miss attribution

Intent: determine whether the four union misses share one cause (e.g. query/descriptor vocabulary gap, pooling choice, fusion weighting) or are independent, using BM25-only fast runs and the committed M004 tables without any new encoder sweep.

Required changes: attribution function plus cause classification; fast BM25-rank reproduction on dev fixtures to confirm the 52/72 ceiling mechanics.

Acceptance evidence: unit tests on synthetic ranked lists covering shared-cause vs independent-cause classification, boundary-margin math, and empty-relevant handling; a committed-code path that reproduces the BM25 flat curve.

### Work package B — Preregistration contract

Intent: leave M002/M003 with no undeclared degrees of freedom.

Required changes: typed constants for protocol name/version, variant grid (fusion alphas, RRF_K values, pooling/normalization variants, two-stage N, extended K values), universe grid, gates (64>=0.99, 128>=0.98, 256>=0.95, zero violations; K<=32 primary, extended K conditional on budgets), budgets (retrieval p50/p95/max per K, re-rank cost, cold-load p95, weights, total wall-clock), exact sweep command, checkpoint path pattern bound to sweep fingerprint, fail-closed rules (missing/duplicate point, violation, budget breach all error).

Acceptance evidence: serialization round-trip test over the preregistration; a test asserting the extended-K branch cannot satisfy selection unless all budgets pass; rustdoc recording the exact sweep command.

### Work package C — Fixture freeze

Intent: bind the future sweep to exact inputs.

Required changes: fingerprint derivation over the M003 selection receipt, the dev corpus partition, and the universe-expansion code version; validation that expansion preserves every labeled relevant tool (reuse existing validation).

Acceptance evidence: unit test that any fixture byte change alters the fingerprint and that the sweep refuses a mismatched fingerprint.

## 8. Failure, cancellation, restart, and contention semantics

Diagnostics are pure functions over in-memory ranked lists; no partial-failure state exists. Preregistration mismatches fail closed with explicit errors. The M003 sweep (not this milestone) inherits the M004 per-universe checkpoint/resume pattern: resume on fingerprint match, recompute never silently, duplicate launch resumes rather than forking.

## 9. Compatibility and migration

Additive experiment module and constants only. No migration, no rollback concern, no legacy-path removal.

## 10. Required tests

### Focused unit tests

- Attribution math: ranks, K-boundary margins, shared-vs-independent classification, ties, empty relevant sets.
- Fusion-reference vectors for each preregistered variant option (alpha grids, RRF_K values, max-fusion) on synthetic scores.
- Preregistration serialization round-trip and self-SHA exclusion.
- Fingerprint mismatch refusal; universe/K identity mismatch refusal.

### Integration tests

- BM25-only dev diagnostic reproducing the flat 52/72 curve shape (fast, no encoder, no new assertions on semantic values beyond the committed tables).
- Expansion-preservation check reusing existing validation on a small synthetic corpus.

### Restart and recovery tests

- Checkpoint-path binding test: same fingerprint resumes, different fingerprint refuses (unit-level, no long run).

### Contention and cancellation tests

- None (no concurrent or cancellable path added).

### Security and negative tests

- Non-deferred candidates never enter attribution universes; authority-violation input fails closed.

### Migration and compatibility tests

- None (no migration).

## 11. Required verification commands

```bash
cargo test --locked -p codegg --lib -- tool_advisor::retrieval_architecture::
cargo test --locked -p codegg --lib -- tool_advisor::operating_point:: --skip m004_operating_point_sweep
cargo fmt --all -- --check
git diff --check
```

Do not run the M004 ignored sweep or any encoder workload in this milestone. Do not use `--all-features` for workspace sweeps.

## 12. Documentation updates

- Rustdoc on attribution and preregistration items (grids, gates, budgets, sweep command).
- No user-facing documentation change.

## 13. Acceptance criteria

- The four systematic misses are attributed (per-tool BM25/semantic/fused ranks and margins) with a shared-vs-independent cause verdict recorded for the closure.
- Every M002/M003 degree of freedom (variants, universes, Ks, gates, budgets, sweep command, checkpoint binding, fail-closed rules) is a typed preregistered constant.
- Fixture fingerprinting binds the future sweep to exact inputs and refuses mismatches.
- M002 is dependency-ready with no open design question.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- Attribution suggests the variant space in the roadmap is insufficient (request roadmap amendment instead of silently widening scope).
- The dev corpus or M003 receipt cannot be fingerprinted deterministically.
- Any v3/v4 case would be needed for diagnosis.
- A hard dependency closure record is missing or contradictory.
- Scope would expand into ranker training, promotion redesign, catalog BM25, or the runtime discovery path.

## 15. Closure evidence required

- Implementation commit SHA; requirement-to-evidence matrix against sections 5/7/13.
- Attribution results (per-miss ranks/margins, cause verdict) plus unit-test outcomes.
- Preregistration constants with the exact M003 sweep command and budget table.
- Commands from section 11 with results, labeled local vs CI.
- Invariant, failure/recovery, migration, and security reviews per the closure template.
- Registry updates (M001 closed, M002 ready) recorded in the closure record.

## 16. Handoff notes

- Long-encoder precedent: M004 took ~65 min with first-embedding-load outliers up to ~175 s; this milestone deliberately performs no encoder work, so ordinary fast-test resources apply.
- New tests default to `current_thread` unless real concurrency is involved.
- Preserve unrelated user changes; plans-only files in this change are the roadmap, this plan, and `plans/registry.md`.
