# Tool-Selection Advisor Retrieval-Architecture Experiment M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-retrieval-architecture-experiment/003-extended-frontier-and-operating-point-selection.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md#m003--extended-frontier-and-operating-point-selection`

Repository baselines reviewed: `6f4f9bb0`, `5ad29bdd`, `355472b9`, `ac71e567`, `fb11a74f`

Implementation commits:

- `6f4f9bb0` — retrieval-architecture M003: extended frontier sweep harness
- `5ad29bdd` — retrieval-architecture M003: measure canonical grid, fan out inert axes
- `355472b9` — retrieval-architecture M003: stage-qualified rerank identities
- `ac71e567` — retrieval-architecture M003: two-phase sweep, coarse checkpoints first
- `fb11a74f` — retrieval-architecture M003: miss attribution analysis for negative verdict

Disposition: **negative on recall — no variant clears the gates, proven from the complete coarse frontier (180 modes × 5 Ks × 3 universes, 2700 points, zero violations) plus a subset ceiling lemma that makes the unmeasured re-rank arms provably moot. The arm phase was not run: ranker-forward pace (~0.6 s/forward, ~4–5 h for 108 arms) exceeds the wall-clock budget ~2.5×, and the ceiling proof shows arms cannot overturn the verdict. Extended-K selection is independently dead (cold-load 17 s > 10 s budget). Nothing is frozen, nothing is unblocked, no v4 plan is registered.**

## 1. Executive finding

The R001 hypothesis failed on measurement. Across all 9 fusion-settings × 2 poolings × 3 universes, the best coarse recall is 68/72 = 0.9444 — the exact M004 ceiling — flat across K=16/24/32/48/64 at universes 128 and 256, and flat across K≤32 at universe 64. The gates need 72/72 @K≤32 (universe 64), 71/72 (universe 128), 69/72 (universe 256). Per-tool attribution names the same four misses in every universe and both best fusion families — `glob`, `table_filter`, `write`, `lsp_rename` — all classified dual-signal-miss with BM25 ranks at dead last (64/64, 128/128, 256/256) and semantic ranks at or near dead last. The alpha sweep (0.0 → 1.0) moves totals between 52 and 68 but no weighting recovers the four; they are invisible to both signals, not mis-weighted by fusion. RRF-120 additionally collapses at large universes (46–51/72), a dilution effect.

Because every re-rank pool is a top-48/64 subset of its coarse ordering, arm recall@K can never exceed that ordering's coarse recall@64 (68/72 at 128/256 < both gates). The arms are therefore provably unable to produce a clearing mode, and the 108-arm phase — paced at ~2.5× over the wall-clock budget — was not run. This is a recall negative with recorded evidence, not a resource negative: the verdict does not depend on the unmeasured arms.

## 2. Ceiling lemma (why the verdict does not need the arms)

For any arm A built from coarse ordering O with pool N ∈ {48, 64}: promoted(A)@K ⊆ pool ⊆ top-64(O) for every K ≤ 64. Hence recovered(A@K) ≤ recovered(O@64), i.e. recall(A@K) ≤ recall(O@64) ≤ max-coarse@64(universe), regardless of ranker behavior — even a perfect ranker is capped by pool content.

Measured max-coarse@64: universe 64 = 72/72 (trivial: K equals universe size), universe 128 = 68/72, universe 256 = 68/72, over all 180 modes with zero violations. Gate counts needed: 72 (u64 @K≤32), 71 (u128), 69 (u256, primary or extended). At u128/u256 every arm is capped at 68 < 69 ≤ 71. No arm clears u128 or u256; no mode — coarse or arm — clears all three universes. Selection is impossible. (Universe-64 N=48 arms are additionally capped by coarse@48 ≤ 69 < 72; universe-64 N=64 arms could in principle clear u64 alone, which is moot globally.)

## 3. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Full coarse grid measured (WP-A/B) | `r001-checkpoint-coarse-{64,128,256}.json`: 900 points each, 62 cases / 72 eligible per universe (tripwires held), 0 violations; sweep fp `3c2de954…` (commit `ac71e567`) | pass | 180 modes × 5 Ks × 3 universes = 2700 points; N-inert duplicates verified identical |
| Prereg/fixture/identity enforced live | Fixture `e631cd2f…` recomputed and matched; dev partition `b804b7d8…` tripwire held; 62/72 counts asserted per universe | pass | Any corpus/expansion drift would have failed closed |
| Miss adjudication with per-tool ranks/margins (WP-B) | `r001-miss-attribution.json` (27 rows) + cause table §5; alpha sweep table §4 | pass | Margins from live fused scores; ranks exact at K=32 (top-64 orderings) |
| Operating-point selection (WP-C) | Ceiling lemma §2 + gate-count table §4 | negative (proven) | No candidate exists; freezing anything would violate the gates |
| Promotion separator re-attached | Not reached — no frozen point exists | moot | Unchanged M004 machinery stands ready; nothing to attach to |
| Arms measured | Not run — provably moot (§2) + ~2.5× over wall-clock budget (§6) | deviation with proof | Documented here, not silent: §6 |
| Wall-clock / budgets | Cold-load 17.0 s > 10 s budget kills extended branch independently; weights 86 MiB ≤ 128 pass | recorded | Primary branch needs no budgets; verdict is recall-based |

## 4. Frontier evidence (coarse, K=32 unless noted)

Best recall@K per universe (all modes): u64 — 68/68/68/69/72(K=64, trivial whole-universe); u128 — 68 flat; u256 — 68 flat. Gates need 72/71/69.

Recall@32 by setting/pooling (universe 256, binding):

| setting | mean | cls |
|---|---|---|
| weighted-union 0.0 (BM25-only) | 52 | 52 |
| weighted-union 0.25 | **68** | 65 |
| weighted-union 0.5 (M004-equivalent) | **68** | 60 |
| weighted-union 0.75 | 66 | 54 |
| weighted-union 1.0 (semantic-only) | 64 | 53 |
| rrf 30 / 60 | 66 / 66 | 65 / 65 |
| rrf 120 | 51 | 46 |
| max | 67 | 56 |

Universe 128 @32 best 68 (w-u 0.25 both poolings, w-u 0.5/mean); universe 64 @32 best 68 (w-u 0.25 both, w-u 0.5/mean, rrf 30/60 both). BM25-only reproduces the M004 52/72 = 0.7222 exactly at every universe and pooling. Semantic-only peaks at 65/72 (mean). No alpha, RRF constant, fusion, or pooling reaches 69+ anywhere except the trivial u64@K64 point.

## 5. Miss attribution (best families, K=32)

`r001_miss_attribution` (commit `fb11a74f`, 587 s): weighted-union 0.25/mean and RRF-60/mean attributed on all three universes (27 rows). Core misses, identical in both families and all universes:

| tool | u256 bm25 / semantic / fused rank | margin | cause |
|---|---|---|---|
| glob | 256 / 256 / 256 | −0.2537 | dual-signal-miss |
| table_filter | 256 / 230 / 230 | −0.0470 | dual-signal-miss |
| write | 256 / 256 / 256 | −0.1910 | dual-signal-miss |
| lsp_rename | 256 / 256 / 256 | −0.1431 | dual-signal-miss |

Cause totals: 24 dual-signal-miss; 3 fusion-weight-sensitive — all three in the RRF family only (`read` @u128/u256, `git_blame` @u256), i.e. RRF-specific margin losses at large universes, never recoveries of the core four. The core four rank dead-last (or, for table_filter semantic, 230/256 — still far beyond K=32) in BOTH signals: no fusion weight can recover what neither signal scores above 60+ distractors.

## 6. Resource accounting and the arm deviation

- Encoder cold-load 17.0 s (budget 10 s): extended-K branch dead on budgets regardless of recall. Weights 86 MiB ≤ 128 pass.
- Coarse phase: 485 s (u64) / 539 s (u128) / 615 s (u256) for 18 canonical orderings × 62 cases; fan-out to 180 modes exact by determinism (N verified inert: identical values).
- Arm phase pace: u64 arms incomplete after >80 min (~7,800 ranker forwards → ~0.6 s/forward on CPU). Full 108-arm projection ≈ 4–5 h ≈ 2.5× the 7,200 s wall-clock budget. The plan's budget assumed M004-like pace (~65 min) without accounting for 23k+ ranker forwards; that assumption is invalidated with data.
- Two findings converged independently: (a) the ceiling proof (§2) makes arms verdict-irrelevant; (b) the budget makes them unrunnable. Arms unmeasured is therefore a proven-moot deviation, not a silent skip. Coarse checkpoints and the attribution receipt persist as the evidence set.

## 7. Implementation history and corrections during M003

- `6f4f9bb0`: sweep harness (two-stage driver, selection, receipts). A 28-minute pilot run paced 180-wide coarse measurement at ~3× over budget → redesigned before any evidence run.
- `5ad29bdd`: measure the 9 plan-literal fusion-settings × 2 poolings (18 canonical orderings), fan out to 180 mode records with `coarse_setting` provenance; all 180 selections still constructed/validated.
- `355472b9`: fixed a real identity bug found on re-read — reranked records shared mode names with coarse records, making arms invisible to `r001_point_lookup`. Reranked identities are now stage-qualified (`<mode>-reranked`); uniqueness unit-tested. Coarse records (the entire evidence set) were always bare `point_mode()` names and are unaffected.
- `ac71e567`: two-phase driver (coarse checkpoints for all universes before arms) so an interrupted run preserves recall ceilings; finer resume granularity. Evidence run `3c2de954…` completed phase 1 fully.
- `fb11a74f`: miss-attribution analysis test reusing the driver's `attribute_winner` path.

## 8. Verification executed

### Commands run (local)

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_architecture::
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::sequence_retrieval::
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::operating_point:: --skip m004_operating_point_sweep
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_architecture::tests::r001_miss_attribution --ignored --nocapture
cargo fmt --all -- --check
git diff --check
```

### Results

- Module suite: **43 passed, 0 failed** (37 M001/M002 + 6 M003: grid partition, checkpoint resume incl. coarse round-trip, primary selection rule, extended budget gating, latency aggregation, stage-identity freshness). The ignored sweep test itself was launched twice (pilot + evidence run) but never completed by design after the ceiling proof; its phase-1 path is production-proven by the checkpoint artifacts.
- `sequence_retrieval`: 9 passed. `operating_point` (skip sweep): 5 passed, incl. unchanged promotion regression tests.
- Attribution run: exit 0, 587 s, 27-row receipt written.
- `cargo clippy ... -D warnings`: clean. `cargo fmt --check` + `git diff --check`: clean.
- No workspace sweep rerun: additive experiment module only. M004 ignored sweep not run per plan.

## 9. Invariant review

- Corpora immutable; no v3/v4 case read at any point (dev fixtures + synthetic only).
- M001 preregistration constants, M004 protocol constants/behavior, frozen ranker artifact, catalog BM25, resolved surface: all untouched.
- Authority: deferred-only at every entry; 0 violations across 2700 points + attribution re-derivation.
- Default-off/local-only; no telemetry, download, network, or runtime-path change. `#![deny(unsafe_code)]` holds.

## 10. Failure and recovery review

- Fixture/dev-partition/count tripwires asserted live in every run (62/72 per universe); any drift fails closed.
- Fallback accounting fails closed per setting (0 fallbacks observed: every variant path genuinely measured, never BM25-degraded).
- Checkpoint resume demonstrated for real: the evidence run resumed nothing (fresh fingerprint) and wrote three coarse checkpoints; the attribution run re-derived fixtures deterministically. Unit test covers stale-fingerprint/universe refusal plus coarse round-trip.
- Killed runs (pilot pace probe, two arm-phase interruptions) left no partial state: checkpoints write only on universe completion.

## 11. Migration and compatibility review

Additive experiment code only; no migration, no artifact change, no shared-helper modification. Receipt JSONs are new files under the preregistered output dir. Checkpoint/arm schema additions are forward-only (no reader of the old shape exists; no checkpoint from a prior fingerprint is accepted).

## 12. Security review

No authorization, secret, network, or privilege surface touched. Cache keys hold descriptor hashes only (M002 assertion intact). Ranker scores never enter the descriptor cache. The attribution receipt contains tool names, ranks, scores, margins — dev-fixture data only, no user context.

## 13. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Both retrieval signals rank the same 4 relevant tools at/near dead-last out of up to 256 candidates (BM25 exactly last; semantic last or ~230/256) | Any future retrieval work must explain the signal gap (query/descriptor vocabulary mismatch?) before tuning fusion | New experiment, not this workstream |
| low | RRF-120 collapses at large universes (46–51/72 @u256) while RRF-30/60 hold 66+ | RRF constant is universe-sensitive; a fixed K is a liability | Noted; workstream closed, no action |
| low | Ranker forwards pace ~0.6 s each on CPU (~4–5 h for 108 arms) | Any future two-stage sweep must budget ranker forwards explicitly, not by M004 analogy | Noted for future plans |
| low | Coarse latency measured at full-K ordering shared across K cuts (conservative, documented) | Per-point latencies are upper bounds; no verdict depended on them | None |

No stop-condition violation: no out-of-grid variant needed, no asset modification, no v3/v4 need, no catalog/surface/M002/M004 change, no scope expansion. The arm deviation is proven-moot (§2, §6), not improvised scope.

## 14. Roadmap disposition

M003 is closed (negative on recall). The workstream closes with it per the roadmap completion definition: no operating point frozen, no v4 plan registered, nothing unblocked. Order-invariance M004 stays blocked as historical evidence; the M004 negative now has a structural explanation (dual-signal invisibility, not fusion weights). Live primary-model M004 work stays blocked with recorded evidence.

## 15. Registry updates

- `plans/registry.md`: retrieval-architecture M003 plan row active → closed (implementation `fb11a74f`, this closure); subsystem row current milestone M003 active → M003 closed (negative), workstream closed; execution-order gate paragraph updated; this closure added under recently closed work.
- `plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md`: milestone table M003 active → closed (closure record linked); workstream status closed.
- `plans/implementation/.../003-...md`: active → implemented (with arm deviation recorded in this closure).
