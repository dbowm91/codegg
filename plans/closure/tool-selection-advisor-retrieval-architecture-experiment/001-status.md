# Tool-Selection Advisor Retrieval-Architecture Experiment M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-retrieval-architecture-experiment/001-retrieval-miss-diagnostics-and-preregistration.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md#m001--retrieval-miss-diagnostics-and-preregistration`

Repository baseline reviewed: `e483c2d6`

Implementation commits or pull requests:

- `e483c2d6` — retrieval-architecture M001: miss diagnostics and preregistration

Disposition: **positive — misses attributed to a shared systematic scoring cause; the M002/M003 selection contract is frozen with zero unpreregistered degrees of freedom; M002 unblocked.**

## 1. Executive finding

M001 closes positively. The BM25 diagnostic reproduces the M004 ceiling exactly on dev (62 cases): 52/72 recovered at every universe (64/128/256) and every K (16/24/32), with K-invariant miss sets — a systematic lexical gap affecting the same 20 tools, not a cutoff edge. Combined with the committed M004 tables (semantic ~0.89 flat, normalized-union 0.9444 ceiling from K16 with no K-scaling), the verdict is a shared systematic scoring cause for the misses; whether the four union misses are strictly dual-signal or partly fusion-weight sensitive is adjudicated by the preregistered alpha grid (0.0–1.0, which spans BM25-only through semantic-only), so both hypotheses are covered with no roadmap amendment needed. The selection contract is frozen as typed constants: protocol `r001-preregistered-retrieval-architecture-v1`, variant/universe/K grids, M004-unchanged gates, budgets, exact M003 sweep command, fingerprint-bound checkpointing, and fixture fingerprint `e631cd2f…`. No encoder workload ran. M002 is dependency-ready.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Miss attribution with BM25/semantic/fused ranks, K-boundary margins, shared-vs-independent verdict | `attribute_misses` + `MissCause`/`MissAttribution`; BM25-real diagnostic + committed M004 tables | pass | 20 lexical-gap tools reproduced with K-invariant sets; union-4 verdict shared-systematic, subclass adjudicated by alpha grid |
| Preregistration of every M002/M003 degree of freedom | `preregistration()`, `PREREG_*`/`FUSION_*`/`RRF_KS`/`POOLING_VARIANTS`/`RERANK_CANDIDATE_NS` constants, `PREREG_SWEEP_COMMAND` | pass | Grids, gates, budgets, seed, checkpoint pattern, fail-closed rules all typed constants |
| Fixture fingerprint binding without asset modification | `fixture_fingerprint[_for]` + `EXPECTED_FIXTURE_FINGERPRINT` + `dev_partition_tripwire` | pass | Live fp `e631cd2f…` hardcoded as tripwire; dev partition reproduces `b804b7d8…` |
| No encoder sweep or long measurement | All new tests run in ~5 s; no encoder/asset access in M001 code | pass | Semantic per-tool ranks deliberately deferred to the M003 sweep |
| No gate relaxation, no K-range change, no promotion/ranker/catalog change | Gates identical to M004; `expand_universe` and frontier types reused, not duplicated | pass | M004 constants and M003 artifact untouched |
| M002 dependency-ready | `002-retriever-and-fusion-variants.md` registered as ready | pass | Normative reference fusion vectors with exact tests |

## 3. Production implementation evidence

`src/tool_advisor/retrieval_architecture.rs` (gated behind `tool-advisor-encoder-training`, which implies `encoder-experiment`; wired in `src/tool_advisor/mod.rs`): preregistration constants and `Preregistration` struct with self-SHA exclusion; `attribute_misses` pure diagnostic; normative `fuse_weighted_union` / `fuse_rrf_with_k` / `fuse_max` (alpha 0.5 and RRF_K 60.0 reproduce M004 orderings); `verify_budgets` + `extended_selection_allowed` (extended K requires gates AND budgets); `checkpoint_path` + `checkpoint_accepts`; `eligible_deferred_names` + `verify_universe`; `bm25_ordering` + `bm25_recall_at_k` mirroring the `sequence_retrieval` BM25 path without K truncation. No retrieval, ranking, authority, storage, protocol, or default-surface change beyond the experiment module.

## 4. Verification executed

### Commands run (local; with `--features tool-advisor-encoder-training`, see deviation note)

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_architecture::
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::operating_point:: --skip m004_operating_point_sweep
cargo clippy --locked --features tool-advisor-encoder-training -p codegg --lib -- -D warnings
cargo fmt --all -- --check
git diff --check
```

### Results

- New module: **22 passed, 0 failed** — attribution (7: dual-signal ranks/margin, recovered/empty, weight-sensitive, margin-miss, lexical-gap, insufficient-evidence, tie determinism), fusion vectors (4: weighted incl. alpha endpoints, RRF hand-computed, max, determinism), preregistration round-trip + self-SHA exclusion + seed-mutation sensitivity (1), fixture fingerprint byte-sensitivity + live determinism + dev tripwire (3), budget breach + extended-K gating (2), checkpoint binding (1), universe/K identity mismatch refusal (1), non-deferred exclusion + outsider fail-closed (1), synthetic-corpus expansion preservation (1), BM25 dev diagnostic reproducing 52/72 flat with K-invariant miss sets (1).
- `operating_point` (skip sweep): 5 passed, 0 failed — no M004-behavior regression.
- `cargo clippy ... -D warnings`: clean. `cargo fmt --check` and `git diff --check`: clean.
- No workspace sweep rerun: additive experiment module only; clippy + fmt + focused suites cover the change surface.
- Deviation from plan §11: the plan text omits `--features tool-advisor-encoder-training`, but the module is feature-gated (same as `operating_point`/`sequence_retrieval`), so the flag is required for the tests to exist; without it the filters match zero tests. The M002 plan records the corrected commands. No M004 ignored sweep or encoder workload ran.

## 5. Invariant review

- Train/dev/test/v2/v3 corpora immutable: diagnostics read the builtin corpus and the committed M003 receipt only; dev tripwire `b804b7d8…` confirms zero drift.
- v3 never touched: no v3/v4 case read for tuning or diagnosis.
- M004 protocol constants and M003 artifact untouched; new protocol has its own name/version.
- Authority boundary untouched: deferred-only universes, outsider orderings fail closed (tested).
- Default-off/local-only posture unchanged; no telemetry, download, or runtime path change.

## 6. Failure and recovery review

- Fixture/preregistration/budget/identity mismatches all fail closed with explicit errors (unit-tested each).
- Attribution is a pure function over in-memory orderings; no partial-failure state.
- M003 inherits the M004 per-universe checkpoint/resume pattern: fingerprint+universe match resumes, anything else recomputes (binding logic unit-tested, no long run here).

## 7. Migration and compatibility review

Additive experiment module and constants only; no storage/protocol/config migration, no artifact format change, no rollback concern.

## 8. Security review

No authorization, secret, network, or privilege surface touched. `#![deny(unsafe_code)]` holds. Cache-hygiene rule (descriptor embeddings only, never user context) is documented for M002 with a mandated test; M001 introduces no cache.

## 9. Documentation and operations

- Implementation plan `001-...md` status moves to implemented (evidence gathered, positive result).
- Rustdoc on attribution, preregistration, budgets, and sweep command; exact M003 sweep command frozen in `PREREG_SWEEP_COMMAND`.
- Fixture fingerprint `e631cd2f3398d8a1a690fafa7fa7eff5e18454b1dfe64596d57026fd5ed184a7` recorded here and hardcoded as `EXPECTED_FIXTURE_FINGERPRINT`.
- No user-facing documentation change.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Semantic per-tool ranks not measured in M001 (by design, no encoder workload) | Exact dual-signal vs weight-sensitive subclass for the 4 union misses awaits measurement | M003 alpha sweep adjudicates; both hypotheses covered by the frozen grid — no amendment needed |
| low | First-embedding-load outliers (up to ~175 s in M004) dominate sweep wall-clock | M003 cost risk | M003 must cache or budget encoder load within `BUDGET_SWEEP_WALLCLOCK_S` (already preregistered) |
| low | Plan §11 omitted the `--features` flag the gated module requires | Zero-test false pass if copied literally | Corrected in this record; M002 plan carries the working commands |

No stop-condition violation: attribution supports the roadmap variant space, fixtures fingerprinted deterministically, no v3/v4 need arose.

## 11. Roadmap disposition

M001 is closed (positive). M002 moves to ready for handoff with no open design question; M003 remains not started behind M002. Nothing else is unblocked; order-invariance M005 stays blocked as historical evidence.

## 12. Registry updates

- `plans/registry.md`: retrieval-architecture M001 plan row active → closed (implementation `e483c2d6`, this closure); new M002 row as ready; subsystem row current milestone M001 active → M001 closed, M002 ready; execution-order gate paragraph updated; this closure added under recently closed work.
- `plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md`: milestone table M001 active → closed (closure record linked); M002 not started → ready for handoff (plan linked).
- `plans/implementation/.../001-...md`: active → implemented (evidence gathered; positive result).
- `plans/implementation/.../002-...md`: newly registered as ready for handoff.
