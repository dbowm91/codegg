# Tool-Selection Advisor Retrieval-Architecture Experiment M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-retrieval-architecture-experiment/002-retriever-and-fusion-variants.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md#m002--retriever-and-fusion-variants`

Repository baseline reviewed: `cde81715`

Implementation commits or pull requests:

- `cde81715` — retrieval-architecture M002: retriever and fusion variants

Disposition: **positive — every preregistered variant value is constructible, deterministic, authority-safe, and unit-tested; fusion outputs match the M001 reference vectors exactly; no selection logic exists; M003 is dependency-ready with its handoff plan registered.**

## 1. Executive finding

M002 closes positively. The full preregistered variant space is implemented in `src/tool_advisor/retrieval_architecture.rs` behind the existing authority boundary: `VariantSelection` validates all five grid axes fail-closed with canonical injective `point_mode()` names for M003 frontier points; `select_coarse_ordering` delegates to the M001 normative fusion functions (no reimplemented math); `VariantRetriever` parameterizes pooling over deferred-only descriptors with an experiment-local pooling-aware cache and BM25 fallback on any encoder failure; `advisor_lexical_ordering` reuses catalog BM25 by delegation; `rerank_pool` drives the frozen span-packed ranker through name-sorted chunking with counted forwards and deferred-universe revalidation. Two live asset-backed tests prove the grid endpoints reproduce M004 behavior (alpha 1.0 == semantic ordering, alpha 0.0 == BM25 ordering) and the frozen ranker reranks with honest cost accounting while rejecting wrong architectures. M004 paths and tests are untouched and green.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Fusion variants match M001 reference vectors exactly (WP-A) | `select_coarse_ordering` delegation + `fusion_dispatch_matches_reference_vectors_across_full_grids` (all alphas, all RRF_K, max) + `alpha_half_reproduces_equal_weight_ordering` hand computation | pass | Bogus fusion fails closed at dispatch as well as construction |
| Pooling ablation + advisor lexical signal (WP-B) | `parse_pooling`/`pooling_strategy`, `advisor_lexical_ordering` delegating to `bm25_ordering`, `EncoderSemanticScorer` over the reference asset interface | pass | Lexical equality proven on 4 dev fixtures; catalog BM25 unmodified |
| Two-stage re-rank with frozen ranker + cost accounting (WP-C) | `rerank_pool` + `FrozenRankerScorer` (architecture-bound) + chunk/invariance/revalidation tests + live ranker test | pass | Chunking is explicit and counted (ranker manifest caps at 16/forward vs pools 48/64) |
| Every grid value constructible; unknowns rejected | `variant_grid_is_fully_constructible_and_rejects_unknowns`: all 180 combos, injective mode names, 5 rejection cases incl. near-miss alpha 0.3 | pass | Exact float matching documented (grid values are exact binary fractions) |
| Determinism, fallback, identity, authority | Permutation tests (fusion + retrieval + re-rank), stub query/descriptor failure fallback, `select_retrieval_point` identity on variant mode names, denied/immediate exclusion, outsider rejection | pass | Cache hygiene asserted on keys with adversarial context marker |
| No selection/gating/threshold logic | Code review: no gate evaluation, no operating-point selection, no promotion logic in the module | pass | M003 has measurement only left to design |
| M003 dependency-ready | `003-extended-frontier-and-operating-point-selection.md` registered as ready | pass | Handoff notes carry chunking, budget, and invariance constraints |

## 3. Production implementation evidence

`src/tool_advisor/retrieval_architecture.rs` (+1138 lines, additive only): `VariantSelection`/`parse_pooling`/`point_mode`; `select_coarse_ordering`; `advisor_lexical_ordering`; `SemanticScorer` trait + `EncoderSemanticScorer`; `VariantCacheKey`/`VariantRetriever`/`VariantRetrieval` (experiment-local cache keyed with pooling; descriptor embeddings only); `ChunkRankScorer` trait + `FrozenRankerScorer` (span-packed binding); `RerankReport`/`rerank_pool` (name-sorted chunking, merged global sort, revalidation). `sequence_retrieval.rs`, `sequence_ranking.rs`, `operating_point.rs`, authority, storage, protocol, and default surfaces unchanged. `#![deny(unsafe_code)]` holds.

## 4. Verification executed

### Commands run (local)

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_architecture::
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::sequence_retrieval::
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::operating_point:: --skip m004_operating_point_sweep
cargo clippy --locked --features tool-advisor-encoder-training -p codegg --lib -- -D warnings
cargo fmt --all -- --check
git diff --check
```

### Results

- New module suite: **37 passed, 0 failed** (22 M001 + 15 M002: grid exhaustiveness/injectivity, full-grid fusion delegation, alpha-0.5 hand computation, lexical/dev-fixture equality, fusion determinism, stub end-to-end fusion equivalence with exact scores, query/descriptor fallback, pooling stability + cache separation + surface invalidation, cache-hygiene key scan, variant point identity, chunk counting with hand-scored promotion, permutation invariance, universe revalidation, live M004-endpoint reproduction, live frozen-ranker rerank + wrong-architecture rejection).
- `sequence_retrieval`: 9 passed. `operating_point` (skip sweep): 5 passed. No M004-behavior regression.
- `cargo clippy ... -D warnings`: clean. `cargo fmt --check` + `git diff --check`: clean after one `cargo fmt` pass over the new test code (whitespace/line-width only, re-verified: 37 passed).
- Live tests ran (assets present locally): variant alpha endpoints reproduce M004 semantic/BM25 orderings exactly; frozen ranker loads (head hash verified by the loader), binds architecture + dev partition `b804b7d8…`, promotes 2/3 with counted forwards and zero drops. No skips in this environment; both live tests skip cleanly with explicit messages when assets are absent.
- No workspace sweep rerun: additive experiment module only. M004 ignored sweep not run per plan.

## 5. Invariant review

- Corpora immutable; no v3/v4 case read (synthetic + builtin dev fixtures only).
- M001 preregistration constants, M004 protocol constants, M003 artifact untouched.
- Authority: deferred-only at every new entry point; outsiders fail closed; denied tools excluded even when labeled relevant (tested).
- Default-off/local-only; no telemetry, download, network, or runtime-path change.

## 6. Failure and recovery review

- Unknown variant values, zero K, empty/foreign re-rank pools, encoder failures, and wrong ranker architectures all fail closed with explicit errors (each unit-tested).
- No persistent state added; M003 inherits M004-style checkpoint/resume via the M001-tested binding functions.

## 7. Migration and compatibility review

Additive experiment code only; no migration, no artifact change, no shared-helper modification at all (parity duplicates are documented, not refactors).

## 8. Security review

No authorization, secret, network, or privilege surface touched. Cache keys hold descriptor hashes only (asserted under adversarial context). Ranker scores never enter the descriptor cache by construction (separate types, no shared path).

## 9. Documentation and operations

- Implementation plan `002-...md` status moves to implemented.
- Rustdoc on every variant axis mapping to its preregistration constant; `point_mode` injectivity documented; chunking rationale (16/forward cap vs 48/64 pools) and name-sorted-chunking invariance documented at the function.
- Fixture fingerprint unchanged (`e631cd2f…` live test still passes).
- No user-facing documentation change.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Re-rank chunk scores are chunk-local (ranker scores ≤16 jointly), merged by global sort without cross-chunk calibration | M003 must treat re-rank recall as measured, not calibrated | Documented in code + M003 handoff notes; no action in M002 |
| low | M003 grid is large (~9 fusion settings × 2 poolings × 3 universes × 3+ Ks plus extended/re-rank arms) against the 7200 s wall-clock budget | Sweep cost risk | M003 plan mandates shared encoder load and warm-cache reuse across K; no action in M002 |
| low | `cargo fmt` needed one pass over new test code | None (re-verified clean) | None |

No stop-condition violation: no out-of-grid variant needed, no asset modification, no v3/v4 need, no catalog/surface/M004 change, no scope expansion.

## 11. Roadmap disposition

M002 is closed (positive). M003 moves to ready for handoff with its plan registered and no open design question. Nothing else is unblocked; order-invariance M005 stays blocked as historical evidence.

## 12. Registry updates

- `plans/registry.md`: retrieval-architecture M002 plan row active → closed (implementation `cde81715`, this closure); new M003 row as ready; subsystem row current milestone M002 active → M002 closed, M003 ready; execution-order gate paragraph updated; this closure added under recently closed work.
- `plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md`: milestone table M002 active → closed (closure record linked); M003 not started → ready for handoff (plan linked).
- `plans/implementation/.../002-...md`: active → implemented.
- `plans/implementation/.../003-...md`: newly registered as ready for handoff.
