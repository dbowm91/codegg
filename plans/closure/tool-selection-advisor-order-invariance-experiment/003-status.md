# Tool-Selection Advisor Order-Invariance Experiment M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-order-invariance-experiment/003-balanced-training-and-dev-selection.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md#m003--balanced-training-and-dev-selection`

Repository baseline reviewed: `c96fbdfa75996187f725289890ccaaf5b140d180`

Implementation commits or pull requests:

- `c96fbdfa` — order-invariance M003: balanced training and dev selection

## 1. Executive finding

M003 closes positively. All four predeclared arms trained on the
477-view M001 balanced train set with the corrected graded objective
(listwise graded relevance + grade-weighted binary relevance +
separate abstention BCE, consistency default 0). The
shared-marker **span-pooled** packed ranker is selected: it passes
all eight §8 dev gates, including permutation top-1 consistency
0.951 ≥ 0.95, dev MRR 0.6255 within 0.01 of the frozen linear
baseline, hard-slice gains, and finite usable candidate-relevance
calibration. The v1 control scores the highest dev MRR (0.6927) but
fails consistency (0.380) exactly as the order-shortcut hypothesis
predicts; the batched pairwise reference is perfectly equivariant
(1.000, drift 0.0) but misses two quality gates by ~0.002. The
post-selection v3 diagnostic (non-gating) shows no positional
collapse: v3 MRR 0.6254 / R1 0.4647 vs BM25 0.5115 / 0.3118, against
the old packed artifact's v3 R1 = 0.0. M004 is unblocked.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Train on clean train + M001 views; dev selection on clean dev + dev suite; test/v2/v3 forbidden for selection | `run_selection_sweep`: views from `partition.train_cases` only; dev metrics on canonical dev; v3 runs after selection with `non_gating: true` | pass | View fingerprint `f51a386e…`; canonical fps `c67df3ca…`/`b804b7d8…` |
| Graded listwise + binary relevance + separate abstention + optional consistency objectives | `loss_for_case` with `ObjectiveWeights`; `graded_target_distribution`, `binary_relevance_targets`; pure unit tests | pass | Manifests record `graded-listwise-binary-v1`; legacy single-target path retired by design (§3 correction) |
| Multi-tool semantics (no single-target collapse) | `graded_targets_preserve_multiple_relevant`, `binary_targets_mark_positives_with_grade_weights`; multi-tool nDCG gate per arm | pass | Equal grades share equal mass |
| Frozen start, staged unfreezing only if warranted | All arms `head-only`; differentiable LayerNorm path untouched | pass | No unfreeze needed for selection; M004 owns operating point, not capacity |
| Four arms incl. v1 control with no selection eligibility on history | Sweep arms v1-control/shared/span/pairwise; v1 fails consistency gate | pass | No arm removed; no failures (`failures: []`) |
| Dev metrics incl. stratified, permutation, calibration, resource signals | `DevSelectionMetrics` (MRR/R1/R3/R5/nDCG, no-tool F1, binary Brier/ECE/NLL, position buckets, 16×8 permutation evidence, slice MRR/R1) | pass | Permutation sample predeclared (16 cases × 8) and recorded |
| §8 gates incl. 0.95 consistency, position buckets, linear-relative quality, calibration | `check_selection_gates` + `selection_gates_require_all_plan_thresholds` unit test; per-arm verdicts in receipt | pass | Baseline `frozen-linear` (dev MRR 0.6331) from `target/tool-advisor-smoke/model.json` |
| Predeclared finite sweep; all attempts recorded | `m003_sweep_config` in code + `sweep_fingerprint 011daa5d…`; 4/4 arms reported, 0 failures | pass | Conditional consistency follow-up predeclared but not triggered (no arm failed ONLY consistency) |
| V3 diagnostic after selection, no feedback | `v3_diagnostic` computed after `selected_arm`; no training/threshold/architecture change follows | pass | v3 cannot qualify (plan-literal) |
| Artifact contract (§11) | Manifests carry arch/contract fingerprints, objective version, dual calibration, dev metrics, head/encoder hashes; compact receipt `assets/tool-advisor/order-invariance-m003-selection.json` | pass | Full report local at `target/…/m003-arms/m003-selection.json` |

## 3. Production implementation evidence

`src/tool_advisor/sequence_ranking.rs`: `ObjectiveWeights`,
`TrainingViewMode`, corrected `loss_for_case`,
`consistency_partner_case` + `aligned_partner_logits`,
`CandidateCalibrationFit` + temperature/bias grid fit,
`dev_relevance_observations`, `binary_calibration_report`,
`RankerAsAdvisor`, `DevSelectionMetrics`,
`check_selection_gates`, `SelectionSweepConfig`,
`run_selection_sweep` with failure-tolerant arms, post-selection v3
diagnostic, `TRAINING_OBJECTIVE_GRADED_V1`. `train()` consumes
canonical or balanced views, caches consistency partners for
head-only, and persists dual calibration. No retrieval, promotion,
authority, or default-surface change.

## 4. Verification executed

### Commands run

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::sequence_ranking::tests::m003_selection_sweep --ignored --nocapture
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

### Results

- Full `tool_advisor` lib suite: 129 passed, 0 failed, 3 ignored
  (M001 full diagnostic, M002 cost probe, M003 sweep).
- M003 sweep (ignored, ~2.7 h incl. contention): ok. 4/4 arms
  trained, 0 failures. Selected `span-packed`
  (`target/tool-advisor/order-invariance/m003-arms/span-packed.json`).
  Gate table (baseline frozen-linear dev MRR 0.6331):
  - v1-control: MRR 0.6927, consistency 0.380 → ineligible
    (consistency, multi-nDCG).
  - shared-packed: MRR 0.6022, consistency 0.419 → ineligible
    (consistency, aggregate MRR).
  - **span-packed: MRR 0.6255, consistency 0.951, no-tool F1 0.857,
    multi-nDCG 0.8658, hard-neg MRR 0.7865, ctx MRR 0.4767 →
    eligible (8/8).**
  - batched-pairwise: MRR 0.6215, consistency 1.000, drift 0.000 →
    ineligible (aggregate MRR by 0.0016, multi-nDCG by 0.016).
  - v3 diagnostic (selected, non-gating): MRR 0.6254 / R1 0.4647 vs
    BM25 0.5115 / 0.3118 over 170 cases.
- Workspace sweep: 245 ok batches, 0 failed, EXIT=0. Full clippy:
  clean, EXIT=0. `verify.sh quick`: passed. `cargo fmt --all
  --check` and `git diff --check`: clean.
- Two post-sweep lint fixes (`redundant_field_names`,
  `redundant_as_str`) are behavior-neutral and covered by the same
  gates above; the sweep inputs are unaffected.

## 5. Invariant review

- Train/dev/test/v2/v3 corpora immutable: sweep reads assets only;
  `git status` shows no asset modifications.
- v3 never selects: sweep code orders selection before the v3 block;
  report marks `non_gating: true`.
- Views inherit (never recompute) source splits; partners are
  train-derived with `::consistency` ids.
- Advisor authority/runtime untouched; abstention and relevance stay
  separate heads/fields.

## 6. Failure and recovery review

- Per-arm failures record into `failures` without aborting the
  sweep; zero-arm success fails closed.
- Empty/invalid sweep configs, empty dev observations, and
  zero-active-term objectives fail closed.
- Partner name loss fails closed (no silent misalignment).
- Artifact loading still hash-guards head weights.

## 7. Migration and compatibility review

Additive: config/manifest/report structs gain serde-defaulted
fields; old configs parse (canonical views, corrected objective by
design); old artifacts load. No storage/protocol/config migration.

## 8. Security review

No authorization, secret, network, or privilege surface touched.
`#![deny(unsafe_code)]` holds. Truncation/selection never consume
labels. Training is local-only with no telemetry.

## 9. Documentation and operations

- Implementation plan `003-...md` status moves to implemented.
- Compact selection receipt committed at
  `assets/tool-advisor/order-invariance-m003-selection.json`
  (13 KB; arm metrics, gates, hashes, v3 diagnostic).
- Sweep command recorded in the ignored test rustdoc.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Three selection margins are thin: consistency 0.951 (+0.001), MRR margin +0.0024, multi-nDCG margin +0.0018 | Selection is plan-literal positive, but v4 must confirm rather than trust the margin | M005 fresh-v4 qualification gates are independent; any v4 order/quality miss closes the experiment negatively |
| low | Shared-marker (non-span) arm still fails consistency (0.419): marker identity removal alone does not remove absolute-position leakage | Confirms span pooling carries the invariance in packed family | None; M004 consumes the span artifact |
| low | Pairwise reference misses quality gates despite perfect invariance | Equivariance alone does not guarantee relevance quality head-only | None; recorded as architecture evidence |

No high/critical findings. No stop condition triggered (an arm
clears consistency with usable quality; cost inside envelope).

## 11. Roadmap disposition

Milestone closed positively and next dependency may proceed: M004
becomes ready. M005 remains blocked on positive M004 closure. Live
primary-model M004 remains blocked pending a future M005
disposition A.

## 12. Registry updates

- `plans/registry.md`: order-invariance roadmap row M003 active ->
  M003 closed (positive selection), M004 ready; M003 plan row active
  -> closed; M004 plan row blocked -> ready; execution-order gate
  paragraph updated; this closure added under recently closed work.
- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md`:
  M003 ready -> closed; M004 blocked on M003 -> ready.
- `plans/implementation/.../003-...md`: active -> implemented.
- `plans/implementation/.../004-...md`: blocked on M003 -> ready for handoff.
