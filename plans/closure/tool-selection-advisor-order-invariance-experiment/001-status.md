# Tool-Selection Advisor Order-Invariance Experiment M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-order-invariance-experiment/001-candidate-order-diagnostics-and-permutation-contract.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md#m001--candidate-order-diagnostics-and-permutation-contract`

Repository baseline reviewed: `79045a4c683151e06d1f998b080f41c7c8e84567`

Implementation commits or pull requests:

- `79045a4c` — order-invariance M001: candidate-order diagnostics and permutation contract

## 1. Executive finding

M001 is complete. Candidate-order shortcut learning is now reproducible
from repository assets, one deterministic permutation contract
(`PERMUTATION_CONTRACT_VERSION = 1` in
`src/tool_advisor/order_invariance.rs`) is frozen for M002-M005, and
deterministic train/dev-only permutation views are proven leakage- and
label-invariant. The frozen packed-marker artifact diagnostic shows the
v3 collapse is explained by candidate-order sensitivity (top-1 identity
consistency ~0.40-0.47, per-candidate score drift ~1.7-1.9 logits, MRR
range 0.0-1.0 across presentations). No model was trained, no corpus
was modified, and v3 was used diagnostically only. M002 is unblocked.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Reproduce historical skew from assets, not hard-coding | `historical_position_histogram_reproduces_known_values` derives 256 / 207 / 201@0 / 6@1 / none beyond 1 via `load_cases` + `target_position_histogram` | pass | Per-partition histograms also derived (train 101@0/1@1, dev 50@0/2@1, test 50@0/3@1) |
| Reproduce v3 skew from assets | `v3_diagnostic_histogram_reproduces_known_values` derives 146 / 133@2 / 13@3 / 0@0/1 from `qualification-v3-holdout.jsonl` | pass | |
| Permutation metric contract (N=20 default, mapped-back identity comparison, drift, Kendall/Spearman, MRR/R1 ranges, position grouping, regret) | `generate_permutations`, `evaluate_permutation_robustness`, `summarize_suite` + `mapped_back_predictions_compare_by_identity_not_index` with a position-biased fake advisor | pass | Exact-order-equivariance tolerance documented at call sites (M002 owns the numeric gate) |
| Current-artifact diagnostic on dev / test / v3 | Ignored `full_artifact_permutation_diagnostic` (644 s) + non-ignored 4-case dev-sample test; receipts under `target/tool-advisor/order-invariance/` | pass | Sampling documented in receipt (10 labeled + 3 no-tool per split, 8 perms); artifact unaltered |
| Deterministic balanced training views, uniform target positions (<=5pp) | `balanced_training_views`, `target_position_balancing_meets_declared_tolerance` (deviation ~7e-15pp), 477 views | pass | Uniformity measured per candidate-count stratum; multi-top sources round-robin over equally top-relevant names |
| Dev-only permutation suite with fingerprint + seed | `dev_permutation_suite`, `DEV_SUITE_SEED`, fingerprint `7c410d39…`, 62 cases | pass | Evaluation-only; no optimizer/threshold use beyond aggregate architecture selection |
| Training-source integrity + round-trip | `labels_survive_round_trip_permutation`, `permutation_preserves_candidate_name_descriptor_pairs`, `no_tool_cases_remain_no_tool_under_all_permutations`, `train_dev_test_ownership_never_changes_across_views` | pass | Views inherit source split; never re-partitioned (content-derived ids would rename duplicated components — documented in test) |
| Machine-readable order-bias report | `build_diagnostics_report`, `order_diagnostics_report_is_machine_readable`, `target/tool-advisor/order-invariance/m001-order-diagnostics.json` | pass | Fingerprints match frozen C001 values (`c67df3ca…`/`b804b7d8…`/`1765ad09…`) |
| Non-goals held (no corpus edits, no MiniLM retrain, no marker/retrieval/promotion changes, no v3 selection) | `git status` shows only `src/tool_advisor/order_invariance.rs`, `src/tool_advisor/mod.rs`, planning docs | pass | |

## 3. Production implementation evidence

New module `src/tool_advisor/order_invariance.rs` (~1500 lines incl.
tests), always compiled, no feature gate except the
encoder-training-gated artifact-diagnostic test submodule. Public
contract: `target_index`, `top_relevant_names`, position/count
histograms, `generate_permutations`, `permute_case`/`unpermute_case`,
`placing_permutation`, `kendall_tau`/`spearman_rho`,
`evaluate_permutation_robustness`, `summarize_suite`,
`balanced_training_views`, `augmented_position_deviation_pp`,
`dev_permutation_suite`, `build_diagnostics_report`, seeds
`TRAIN_VIEW_SEED`/`DEV_SUITE_SEED`, `DEFAULT_PERMUTATIONS = 20`.
`src/tool_advisor/mod.rs` adds `pub mod order_invariance;` only.
No runtime, authority, storage, protocol, or default-build change.

## 4. Verification executed

### Commands run

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor::order_invariance
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor::order_invariance::artifact_diagnostic_tests::full_artifact_permutation_diagnostic -- --ignored --nocapture
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

### Results

- Focused order-invariance suite: 11 passed, 0 failed, 1 ignored
  (the ignored item is the full diagnostic below).
- Full `tool_advisor` lib suite with encoder-training: 113 passed,
  0 failed, 1 ignored.
- Full artifact diagnostic (ignored, release of CPU): ok in 644 s.
  Partitions train=132/dev=62/test=62 (of 256); v3=170. Sampled
  10 labeled + 3 no-tool per split, 8 deterministic permutations per
  case: dev top-1 consistency 0.401, drift 1.716, MRR 0.000-1.000,
  regret 0.800; test 0.468/1.832/0.000-1.000/0.750; v3
  0.404/1.857/0.000-1.000/0.800.
- Workspace sweep (`CARGO_BUILD_JOBS=1 ... --test-threads=1`), run
  twice: no `FAILED` and no error lines; final doc-test batches ok.
- `cargo fmt --all -- --check`: clean. `git diff --check`: clean.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean.
- `scripts/verify.sh quick`: passed (all guards + workspace check).

## 5. Invariant review

- Advisor remains optional/local/default-off/advisory-only: no
  runtime or config path touched.
- Historical train/dev/test, v2, v3 corpora immutable: `git status`
  shows no asset modifications; diagnostics read assets only.
- v3 diagnostic-only: v3 outcomes never feed selection; code has no
  v3-gated training or threshold path.
- Permutation never alters authority/candidate identity:
  name/descriptor pairs travel together; relevance keyed by name.
- Training augmentation is local-only: in-memory views, no telemetry.
- Historical closure records untouched.

## 6. Failure and recovery review

- Invalid permutations fail closed (`check_permutation` rejects
  length/duplicate/out-of-range indices).
- Out-of-bounds train/dev indices fail closed.
- Empty/unlabeled inputs fail closed (`target_index` returns `None`;
  empty views yield 0.0 deviation, never NaN).
- Artifact diagnostic skips cleanly when local reference assets are
  absent (CI) instead of failing; local closure carries the numbers.
- Rank ties resolve deterministically (score desc, name asc) so
  metrics are reproducible.

## 7. Migration and compatibility review

No schema, storage, protocol, or config change. New module is
additive; existing `tool_advisor` API unchanged. Machine receipts
live under `target/` (gitignored) and are not committed.

## 8. Security review

No authorization, secret, path, or privilege surface touched. No
network path added. Diagnostic inputs are repository fixtures;
`surface_fingerprint` is empty in permutation scoring so no
cross-case identity leaks into metrics.

## 9. Documentation and operations

- Implementation plan `001-...md` status moves to implemented.
- This closure record is the M001 gate evidence.
- Reproducible diagnostic command recorded in §4 (ignored test) and
  in the test rustdoc.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Full-dev permutation coverage used a documented bounded sample (13 cases x 8 perms per split) for CPU reasons; contract default stays 20 | Diagnostic only; no selection impact | M002+ consumers use the contract default; no action |
| low | Content-derived leakage ids rename a component when an identical input is duplicated inside one partitioned set, so views must inherit (never recompute) source splits | Contract-level subtlety, tested + documented | M003 must consume views via source train indices (planned); no action |

No high/medium/critical findings.

## 11. Roadmap disposition

Milestone closed and next dependency may proceed: M002 becomes ready.
M003-M005 remain blocked per the roadmap dependency graph. Live
primary-model M004 remains blocked (unchanged; only a future M005
disposition A can make it dependency-ready).

## 12. Registry updates

- `plans/registry.md`: order-invariance roadmap row M001 active ->
  M001 closed, M002 ready; M001 plan row active -> closed; M002 plan
  row blocked -> ready; execution-order gate paragraph updated;
  this closure added under recently closed work.
- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md`:
  M001 ready -> closed; M002 blocked on M001 -> ready.
- `plans/implementation/.../001-...md`: active -> implemented.
- `plans/implementation/.../002-...md`: blocked on M001 -> ready for handoff.
