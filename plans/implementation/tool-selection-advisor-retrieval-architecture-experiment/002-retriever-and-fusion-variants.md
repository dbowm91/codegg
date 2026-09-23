# Tool-Selection Advisor Retrieval-Architecture Experiment M002 — Retriever and Fusion Variants

Status: ready for handoff

Repository baseline: `e483c2d6`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md#m002--retriever-and-fusion-variants`

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`

Applicable ADRs:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: infrastructure

## 1. Objective

Implement exactly the preregistered M001 variant space behind the existing authority boundary so the M003 sweep can measure it: fusion tuning (alpha-weighted union, RRF_K grid, max fusion), query/descriptor alignment (pooling ablation, advisor-side lexical signal), and two-stage re-rank with the frozen M003 span-packed ranker. No selection, no threshold logic, no gate evaluation.

## 2. Why this milestone is ready

Hard dependency closed: M001 (`plans/closure/tool-selection-advisor-retrieval-architecture-experiment/001-status.md`) froze protocol `r001-preregistered-retrieval-architecture-v1`, the variant/universe/K grids, gates, budgets, sweep command, checkpoint binding, and fixture fingerprint `e631cd2f…`. Interface dependencies stable: `sequence_retrieval` modes and frontier types, `sequence_ranking` frozen-artifact loading, `operating_point` universe expansion, and the normative reference fusion vectors in `retrieval_architecture` that retriever output must match exactly.

## 3. Current implementation evidence

`src/tool_advisor/retrieval_architecture.rs`: `FUSION_VARIANTS`, `FUSION_ALPHAS = [0.0, 0.25, 0.5, 0.75, 1.0]`, `RRF_KS = [30.0, 60.0, 120.0]`, `POOLING_VARIANTS = ["mean", "cls"]`, `RERANK_CANDIDATE_NS = [48, 64]`, normative `fuse_weighted_union` / `fuse_rrf_with_k` / `fuse_max` with exact vector tests. `src/tool_advisor/sequence_retrieval.rs`: `HybridRetriever` with BM25 fallback, descriptor-embedding cache hygiene, name-tiebreak determinism. The M003 span-packed ranker loads via `sequence_ranking::load_artifact`; its weights and calibration are frozen inputs, never retrained.

## 4. Invariants that must not regress

- ADR-0009 authority monotonicity; `#![deny(unsafe_code)]` in the lib.
- M001 preregistration constants, M004 protocol constants, and the frozen M003 artifact byte-identical.
- Historical corpora immutable; no v3/v4 case read.
- Catalog BM25 and the resolved surface untouched: the advisor-side lexical signal is a new local ordering over deferred descriptors, not a change to discovery ranking.
- Default-off/local-only posture; no telemetry, download, network, or runtime discovery-path change.

## 5. Scope

### In scope

- Alpha-weighted union, RRF_K-parameterized RRF, and max fusion wired into retrieval, matching the M001 reference vectors exactly.
- Pooling ablation (mean vs cls) on query and descriptor sides.
- Advisor-side lexical ordering over the eligible deferred set.
- Two-stage re-rank: union top-N (preregistered N) through the frozen span-packed ranker down to top-K, with per-case ranker-cost accounting and permutation-invariance checks.
- Unit tests for every variant; encoder-dependent tests skip cleanly when reference assets are absent.

### Explicitly out of scope

- Frontier measurement, gate evaluation, operating-point selection (M003).
- Any variant outside the M001 grids (requires roadmap amendment, not silent addition).
- Ranker retraining, calibration changes, promotion logic, v4 holdout.
- Larger-K measurement beyond constructing the parameterized paths M003 will sweep.

## 6. Required production changes

### Core/domain

Extend the retrieval layer (new types or `HybridRetriever` modes at the implementer's discretion, confined to the experiment module) with the preregistered variant axes. Fusion output must equal the M001 reference functions on identical inputs. Pooling variants reuse the existing encoder asset interface. The re-rank stage consumes only the union top-N names and the frozen ranker; ranker scores never leak into the descriptor cache.

### Storage and migrations

None. No migration, no artifact change, no asset writes.

### Protocol and DTOs

Variant selection is a typed parameter struct using only M001-preregistered values; unknown variant names fail closed. No protocol version change.

### Runtime and concurrency

Synchronous paths only; no new threads. Encoder failure anywhere falls back to BM25 without becoming an availability gate.

### Frontend or operator surface

None.

### Security and authorization

Deferred-only input throughout; cache holds descriptor embeddings only, never user context; denied/hidden tools never enter any universe. Re-rank revalidates the deferred universe before scoring.

### Documentation and static guards

Rustdoc on each variant axis mapping it to its preregistration constant. A test (not just review) asserts every `FUSION_VARIANTS` / `FUSION_ALPHAS` / `RRF_KS` / `POOLING_VARIANTS` / `RERANK_CANDIDATE_NS` value is constructible — an unhandled grid value fails the build-time contract by failing the test.

## 7. Ordered work packages

### Work package A — Fusion variants

Intent: make the alpha grid adjudicate dual-signal vs weight-sensitive misses.

Required changes: parameterized weighted union, RRF_K, and max fusion behind the retriever interface.

Acceptance evidence: property tests asserting retriever fusion output equals `fuse_weighted_union` / `fuse_rrf_with_k` / `fuse_max` on shared synthetic inputs across the full grids; alpha 0.5 reproduces M004 equal-weight ordering.

### Work package B — Query/descriptor alignment

Intent: test whether the semantic ceiling is a pooling or descriptor-normalization artifact.

Required changes: mean/cls pooling on both sides; advisor-side lexical ordering over deferred descriptors reusing catalog BM25 machinery without altering it.

Acceptance evidence: determinism tests (permuted input, identical output); pooling variants produce stable orderings on synthetic embeddings; lexical ordering matches `bm25_ordering` from M001 on shared fixtures.

### Work package C — Two-stage re-rank

Intent: test whether the frozen order-robust ranker recovers what coarse retrieval misses, with honest cost accounting.

Required changes: union top-N to ranker top-K path with per-case forward/latency counting; permutation-invariance check (permuted candidate set, identical promoted set within the M003 numeric contract).

Acceptance evidence: re-rank recall on a small synthetic universe exceeds the coarse input recall or the cost accounting proves why not; invariance test passes; ranker artifact hash asserted unchanged.

## 8. Failure, cancellation, restart, and contention semantics

Unknown variant parameters fail closed at construction. Encoder or ranker asset absence fails closed to BM25 (retrieval) with an explicit skip in tests, never a panic in production paths. No persistent state; nothing to resume.

## 9. Compatibility and migration

Additive experiment code only. The M004 retrieval paths and their tests are unmodified; any shared helper change must keep all M004-behavior tests green.

## 10. Required tests

### Focused unit tests

- Fusion equivalence with M001 reference vectors across full grids (property tests, no encoder).
- Grid-exhaustiveness: every preregistered value constructible; unknown values rejected.
- Determinism under permutation for every variant; name-tiebreak stability.
- Fallback: encoder failure yields BM25 ordering.
- Universe/K identity on new variant points; missing/duplicate fail closed.
- Authority: non-deferred names rejected at every new entry point.

### Integration tests

- Re-rank path on a small synthetic universe with cost accounting; ranker hash unchanged.
- Lexical ordering equivalence with M001 `bm25_ordering` on dev fixtures (fast, no encoder).

### Restart and recovery tests

- None (no persistent state).

### Contention and cancellation tests

- None (no concurrent or cancellable path added).

### Security and negative tests

- Descriptor cache never contains user context (assert on cache keys after mixed use).
- Denied/hidden tool fixtures never enter variant universes.

### Migration and compatibility tests

- None (no migration).

## 11. Required verification commands

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_architecture::
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::sequence_retrieval::
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::operating_point:: --skip m004_operating_point_sweep
cargo fmt --all -- --check
git diff --check
```

Encoder-dependent tests must skip cleanly (with an explicit message) when `target/tool-advisor/reference-assets/` is absent; they must never fail for missing local assets. Do not run the M004 ignored sweep. Do not use `--all-features` for workspace sweeps.

## 12. Documentation updates

- Rustdoc mapping each variant axis to its preregistration constant.
- No user-facing documentation change.

## 13. Acceptance criteria

- Every preregistered variant value is constructible, deterministic, authority-safe, and unit-tested; fusion outputs match M001 reference vectors exactly.
- No selection, gating, or threshold logic exists; M003 has nothing left to design, only to run.
- M003 is dependency-ready with no open design question.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- A variant needs an option outside the M001 grids (request roadmap amendment).
- The frozen ranker or encoder assets require modification or re-download.
- Any v3/v4 case would be needed for validation.
- Wiring a variant requires changing catalog BM25, the resolved surface, or M004 behavior.
- Scope would expand into training, calibration, promotion, or the runtime discovery path.

## 15. Closure evidence required

- Implementation commit SHA; requirement-to-evidence matrix against sections 5/7/13.
- Variant-by-variant test outcomes, including fusion-equivalence vectors and skip records for absent assets.
- Commands from section 11 with results, labeled local vs CI.
- Invariant, failure/recovery, migration, and security reviews per the closure template.
- Registry updates (M002 closed, M003 ready) recorded in the closure record.

## 16. Handoff notes

- The reference fusion functions in `retrieval_architecture.rs` are normative: reimplementing different math instead of calling them is a defect.
- Encoder workloads are allowed here (unlike M001) but must stay small and skippable; the one long sweep belongs to M003.
- New tests default to `current_thread` unless real concurrency is involved.
- Preserve unrelated user changes.
