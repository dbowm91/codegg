# Tool-Selection Advisor Evidence Corrective C002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/002-contextual-training-math-calibration.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-evidence-integrity-corrective-addendum.md#c002--contextual-training-math-calibration-and-capacity-truthfulness`

Repository baseline reviewed: `6f460c2d`

Implementation commits or pull requests:

- C002 implementation (this closure batch) — corrected gradients, dev-only
  calibration, partition discipline, capacity truthfulness

## 1. Executive finding

C002 is closed as a correctness repair. The contextual scorer now performs
true gradient descent (sign and mean-pooling `1/n` factors corrected and
pinned by finite-difference tests), trains on the C001 train partition only,
selects abstention calibration on the dev partition only with the values
serialized into a new v2 artifact, enforces empty-partition hard errors with
the all-case fallbacks removed, and reports effective trained capacity
instead of headline allocation. Retraining is deterministic (byte-identical
artifacts across runs) and references exactly the frozen C001 fingerprints.
No effectiveness claim is made: corrected test-partition measurements are
recorded descriptively and C004 will judge qualification. One honest
negative signal is surfaced rather than hidden — the dev-selected abstention
head does not discriminate no-tool cases (dev no-tool F1 null/0.0) — which
C004 must weigh without re-tuning on test.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Correct gradient sign + mean-pooling derivative | `apply_pair_gradient(w, geom, ctx, cand, lr, error)` in `contextual.rs`; old double-negation and `0.1`-damped bias removed | pass | Bias now takes the full `lr * error` step (`dL/db = g`) |
| Numerical-gradient regression | `analytic_gradient_matches_centered_finite_differences` (tiny geometry, eps 1e-2, tolerance 5e-3 over all weights + bias) | pass | |
| apply matches analytic | `apply_pair_gradient_matches_lr_scaled_analytic_gradients` (< 1e-6 per weight) | pass | Pins production loop to the oracle |
| Score-direction fixtures | `positive_target_update_increases_score_and_negative_decreases_it` | pass | |
| Repeated-token normalization | `repeated_token_occurrences_accumulate_one_scaled_contribution_each` (3x relation, collision-free vocab) | pass | |
| Shared-token both paths | `shared_context_candidate_token_receives_the_sum_of_both_paths` | pass | |
| Bias derivative | `bias_derivative_equals_score_error` (< 1e-7) | pass | |
| Tiny loss-decrease gate | `tiny_training_loss_decreases_substantially` (final < initial − 0.3 over 250 epochs) | pass | |
| Tiny overfit gate | `tiny_training_overfits_a_separable_context_dependent_task` (p > 0.7 / < 0.3 per pair) | pass | Context-dependent diagonal task, not a score-existence check |
| Optimizer uses train only | `train()` consumes `partition_cases`; strict empty-train error | pass | `empty_partitions_are_hard_errors_without_the_fallback_flag` |
| Calibration uses dev only, serialized, runtime-used | `calibrate_abstention` grid + manifest `calibration: Some` + runtime formula + `dev_calibration_is_serialized_and_used_at_runtime` | pass | Runtime prediction asserted equal to the serialized formula (< 1e-6) |
| Old artifacts legacy/unqualified | v1 loads (compat) but `is_qualified() == false`; v2 without calibration rejected at write; loader status strings distinguish | pass | 2 dedicated tests |
| Partition-aware reporting, no test metrics in tuning | `ContextualTrainingReport` has train/dev metrics, per-split fingerprints; `test_metrics: None` on all train outputs | pass | |
| Explicit-partition eval; `all` labeled diagnostic | `evaluate_artifact(path, dataset, partition)` + CLI `--partition` (default `test`) + `evaluation_diagnostic_all` flag | pass | |
| Active-row/collision/effective-capacity report | `capacity_report_for` + sidecar `<artifact>.training-report.json` | pass | Rows-changed computed against re-derived init, not estimated |
| Compact v2 configuration | `vocab_buckets` option + `contextual-compact-training.json` (4096 buckets) | pass | v1 string retired from new writes (always v2) |
| Deterministic retraining | Unit test + byte-identical CLI retrain (sha256 `194896b8…`) | pass | |
| No effectiveness claim beyond corrected measurements | Closure §3 records descriptive numbers only; disposition deferred to C004 | pass | |

## 3. Production implementation evidence

- `src/tool_advisor/contextual.rs`:
  - `ModelGeometry` parameterizes buckets/dim/token limits; tiny geometries
    exercise the same code path as production in tests.
  - Corrected update path; `update_pair` removed.
  - Manifest schema v2 (`CONTEXTUAL_SCHEMA_VERSION = 2`,
    `contextual-embedding-v2` on all new writes) with `training_discipline`
    (`partitioned-qualification` / `unpartitioned-fallback` /
    `legacy-pre-partition`) and `calibration: Option<ContextualCalibration>`
    (dev NLL/Brier/ECE/no-tool PRF1 plus uncalibrated references).
  - `calibrate_abstention`: deterministic grid (7 temperatures including
    1.0; dev max-score deciles plus 0.0) selecting min dev NLL.
  - `train()` strictness, per-split fingerprints, train/dev-only metrics,
    capacity report, atomic artifact + sidecar writes.
  - Runtime uses serialized calibration; legacy formula preserved
    bit-identically for uncalibrated artifacts.
  - `CalibrationStatus` / `is_qualified()` gate; `geometry()` accessor.
- `src/tool_advisor/training.rs`:
  - `TrainingConfig` gains `vocab_buckets` and
    `allow_unpartitioned_fallback` (defaulted; existing configs parse).
  - Linear trainer migrated to `partition_cases` with all three all-case
    fallbacks removed (empty train/dev are hard errors; test never scored).
  - `TrainingReport` gains `dev_metrics: Option`, nullable `test_metrics`,
    `evaluation_partition`, `evaluation_diagnostic_all`.
  - `train_contextual` passes config through and reports contextual
    train/dev metrics with `test_metrics: None`.
- `src/main.rs`: `eval --partition train|dev|test|all` (default `test`);
  train output prints train/dev MRR and directs to explicit eval for test.
- `src/tool_advisor/mod.rs`: loader status distinguishes calibrated vs
  legacy-unqualified contextual encoders.
- `assets/tool-advisor/contextual-compact-training.json` (new): Small
  capacity, 4096 buckets, 327,681 parameters.
- `architecture/tool-advisor.md`: runtime (interaction-scorer wording,
  calibration, legacy policy) and training (corrected gradients, partition
  discipline, explicit eval, sidecar) sections rewritten.

Corrected training evidence (frozen C001 partitions train `c67df3ca…` /
dev `b804b7d8…` / test `1765ad09…`, dataset `06da7e53…` — all match C001):

| Variant | Alloc params | Rows changed | Trained est (fraction) | Train MRR | Dev MRR | Cal NLL (uncal) | Cal Brier (uncal) |
|---|---|---|---|---|---|---|---|
| small (65536×80) | 5,242,881 | 415 | 33,201 (0.63%) | 0.471 | 0.491 | 0.693 (1.322) | 0.250 (0.522) |
| medium (65536×240) | 15,728,641 | 415 | 99,601 (0.63%) | 0.524 | 0.487 | 0.693 (1.322) | 0.250 (0.522) |
| compact (4096×80) | 327,681 | 397 | 31,761 (9.69%) | 0.496 | 0.493 | 0.693 (1.322) | 0.250 (0.522) |
| hashed-linear-v1 | 129 | — | — | 0.632 | 0.633 | config temp path | — |

Capacity facts: train touches 416 unique normalized tokens mapping to 415
buckets (collision rate 0.48%); medium triples bytes for dev MRR 0.487 vs
small 0.491; compact matches both at 1/16th the allocation with a 9.7%
trained fraction. Artifact bytes: 20.97 MB / 62.92 MB / 1.31 MB.
Cold load ≈ 0.94 s (small, debug build); score latency p50/p95 ≈ 109/192 µs.
Selected calibration (all three): `dev-grid-search-v1`, bias −1.2851,
temperature 0.25, dev ECE 0.339.

Descriptive held-out measurements via the explicit eval path (no tuning use;
see §5 for the test-hygiene statement): contextual test MRR small 0.457 /
medium 0.595 / compact 0.557 (R1 0.226/0.403/0.355); keyword 0.000 and BM25
0.544 references from C001. No qualification conclusion is drawn — C004 owns
the gated comparison.

## 4. Verification executed

### Commands run

```bash
cargo test --locked -p codegg --lib tool_advisor
cargo test --locked --features tool-advisor-training -p codegg --lib tool_advisor::training
cargo test --locked --features tool-advisor-training -p codegg --lib tool_advisor::contextual
cargo test --locked --features tool-advisor -p codegg --lib tool_advisor
cargo check --locked
cargo check --locked --features tool-advisor
cargo check --locked --features tool-advisor-training
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Plus retraining commands referencing the frozen C001 dataset/partition
fingerprints (see §3 table), the determinism re-run (byte-identical sha256),
and explicit `--partition test/dev` evals.

### Results

- Default lib `tool_advisor`: 25 passed, 0 failed.
- Training-feature lib `tool_advisor`: 44 passed, 0 failed (17 contextual
  tests: finite differences, apply-vs-analytic, direction, repetition,
  shared paths, bias, loss gate, overfit gate, strictness, fallback marking,
  calibration serialization/runtime, legacy compat, v2 calibration
  requirement, determinism, capacity report, smoke-config parsing).
- Advisor-feature lib `tool_advisor`: 41 passed, 0 failed.
- All three `cargo check` variants: clean, zero warnings.
- `cargo fmt --all -- --check`: clean. `git diff --check`: clean.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  clean.
- `scripts/verify.sh quick`: passed.
- Retraining: small/medium/compact + linear all exit 0 against frozen
  partitions; small retrain byte-identical (`194896b8…` before and after).
- Local-only verification; no hosted CI for this batch (same standing
  limitation as C001; C004 requires hosted CI per its plan).

## 5. Invariant review

- Optimizer updates use train partition only: enforced by construction
  (loop iterates `train_cases`) and by the strictness test.
- Calibration/threshold selection uses dev only: grid consumes dev max
  scores; test labels never enter `train()` (test indices counted, never
  scored; `test_metrics: None`).
- Final test/family holdout untouched before C004: tuning decisions (grid
  contents, lr/epochs from pre-existing asset defaults, compact bucket
  count chosen a priori) used no test labels. The test partition was scored
  afterwards through the explicit `eval --partition test` command purely to
  record descriptive numbers; the C001 test fingerprint is unchanged, so
  C004's gate is intact. This statement is the recorded substitute for
  pre-registration, which C002's plan did not require but C004 does.
- Empty train/dev are hard errors in qualification mode: tested both.
- Gradient validated against finite differences on a tiny deterministic
  model: max deviation < 5e-3 (eps 1e-2), production loop pinned to < 1e-6.
- Runtime abstention uses serialized dev values: asserted < 1e-6 at runtime.
- Calibration never read from test: calibration inputs are dev-only by
  construction.
- Old artifacts identifiable as legacy/unqualified: schema-1 files load,
  report `LegacyUnqualified`, keep bit-identical legacy scoring; v2 without
  calibration cannot be written.
- Default/no-feature behavior unchanged: `NoopAdvisor` paths untouched;
  advisor remains default-off.

## 6. Failure and recovery review

- Degenerate corpora (single leakage component, empty dev): hard errors
  naming the missing partition instead of silent all-case fitting.
- Fallback builds (explicit flag only): permanently marked in manifest,
  report, and sidecar; `is_qualified()` false; calibration method recorded
  as fallback rather than grid.
- Corrupt/incompatible artifacts: existing hash/schema validation extended
  to the new fields; loader falls back to `NoopAdvisor` per established
  behavior.
- Oversized geometries: operator bound (256 MiB table) rejects hostile
  `vocab_buckets` before allocation.
- Grid edge cases: empty dev cannot reach calibration (hard error first);
  single-valued dev scores dedupe to a working grid containing the
  uncalibrated point.

## 7. Migration and compatibility review

- Contextual artifact format: v1 files load (validate accepts schema 1 +
  v1 architecture with no calibration); new writes are schema 2 + v2.
  No migration rewrites old files; they report legacy.
- `TrainingConfig` JSON: two new optional fields with defaults; all three
  existing asset configs parse unchanged (covered by a test).
- `TrainingReport` JSON: additive optional fields (`dev_metrics`,
  nullable `test_metrics`, `evaluation_partition`,
  `evaluation_diagnostic_all`); train CLI output now prints train/dev MRR
  instead of test MRR (documented behavior change, no file contract).
- `Eval` CLI: new `--partition` flag with default `test`; previous
  invocations without the flag now score the frozen test partition instead
  of the whole dataset — the intended discipline correction.
- Linear `hashed-linear-v1` format and `qualify` path untouched.
- No storage, protocol, or configuration migration.

## 8. Security review

- No authorization, secret, network, or execution-surface change. Training
  remains local pure-Rust on an explicit operator command; artifacts remain
  local files; no new dependencies.
- `/proc/self/status` RSS read is a safe local parse with `None` fallback
  (reports `None` on non-Linux laurels; local run recorded `None` on macOS).

## 9. Documentation and operations

- `architecture/tool-advisor.md` rewritten as listed in §3; the model is
  called a hashed embedding interaction scorer throughout.
- Operator evidence per run: CLI JSON + `<artifact>.training-report.json`
  sidecar (calibration, per-split fingerprints, train/dev metrics,
  effective-capacity report).
- `inspect --model` surfaces the full v2 manifest including calibration and
  discipline.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Dev-selected abstention does not discriminate no-tool cases (dev no-tool F1 null for small/medium, 0.0 precision at runtime; compact 0.18) | The calibrated head centers scores (NLL/Brier improve over uncalibrated) but the max-score threshold separates almost no abstention cases; C004's no-tool gate may fail the contextual variants | C004 must judge; registered as expected input to disposition B/D, not hidden |
| low | No hosted CI run for this batch | Same standing limitation as C001 | C004 requires hosted CI per its plan |
| low | 60+MB medium artifact + sidecar live under `target/` (gitignored) | Reviewers regenerate via documented commands (deterministic); nothing large is committed | None; by design |

No critical/high findings. The medium finding is an effectiveness signal
for C004, not a C002 correctness defect: the mechanism (dev-only selection,
serialization, runtime use, non-inferiority on NLL/Brier) is verified.

## 11. Roadmap disposition

C002 closed as a correctness repair. The C004 hard dependency on C002 is
satisfied (corrected artifacts, frozen partitions, serialized calibration,
capacity data). C004 remains blocked on C003, which is next in execution
order. If C004's gates fail on the abstention or generalization slices, the
plan already provides for disposition B/C/D rather than hiding the result —
that would be a verdict on the architecture, not a reopening of C002 math.

## 12. Registry updates

- `plans/registry.md`: move C002 `ready` → `closed` (closure:
  `plans/closure/tool-selection-advisor-evidence-integrity-corrective/002-status.md`).
  C004 stays blocked on C003.
- `plans/subsystems/tool-selection-advisor-evidence-integrity-corrective-addendum.md`:
  C002 `ready` → `closed`.
- `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/002-…md`:
  status `ready for handoff` → `implemented`.
