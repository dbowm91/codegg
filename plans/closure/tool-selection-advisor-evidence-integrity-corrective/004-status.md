# Tool-Selection Advisor Evidence Corrective C004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/004-clean-offline-requalification.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-evidence-integrity-corrective-addendum.md#c004--clean-offline-requalification-and-live-m004-gate`

Repository baseline reviewed: `28084cf8`

Implementation commits or pull requests:

- C004 implementation (this closure batch) — pre-registered requalification
  harness, full offline evidence, disposition B

## 1. Executive finding

C004 is closed with an explicit **disposition B — mechanically correct but
no useful gain**, which the plan accepts as a valid closure result. Clean
offline evidence is complete, reproducible, and partition-fingerprinted:
the corrected contextual architecture (small/medium/compact) was compared
against keyword, BM25, and `hashed-linear-v1` on the frozen test partition,
counterfactual/unknown/hard-negative/no-tool slices, four true
family-excluded retraining holdouts, a 64-tool candidate-recall fixture,
and promotion/authority/resource measurements. The contextual variants fail
the pre-registered gates decisively (aggregate MRR 0.46–0.60 vs linear
0.71; no context-sensitive gain without regression; no-tool F1 far below
baselines; dev calibration worse than uncalibrated on test abstention;
preselector recall 0.83 vs 0.98). The contextual scorer is therefore demoted
to a research/observe baseline. **The live M004 plan remains blocked**; no
live-provider budget is spent. Correctness properties all hold (zero
leakage, zero authority violations, deterministic retraining, qualified
artifacts), so this is a verdict on the architecture, not a new
correctness finding — no narrower corrective is registered.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Pre-registration before final eval | `assets/tool-advisor/c004-requalification.json` (dataset/partition fps, holdouts, artifact hashes, calibration expectations, exclusion configs, promotion procedure, gates, resource targets) | pass | Aggregate test MRRs seen descriptively in C002 are disclosed; no config/threshold/calibration choice changed afterwards (git history); slice gates evaluated fresh |
| Frozen final-test slice | 62 test cases, fp `1765ad09…`, scored by all models | pass | |
| Counterfactual slice | 24 pair members (12 pairs), pair accuracy reported | pass | |
| Hard-negative slice | 16 test cases | pass | |
| No-tool/abstention slice | 9 none cases; P/R/F1 + Brier/ECE/NLL per model | pass | |
| Unknown/renamed slice | 62 transformed test cases via `unknown_tool_holdout` | pass | |
| True family holdouts | plugin/lsp/research/structured with in-run exclusion retraining (4 contextual-small + 4 linear variants, manifests verified) | pass | Standard artifacts shown on holdouts for reference only |
| Large-catalog recall fixture | 62 labeled test cases × 64-tool universe, budget 16; recall 0.8286 | pass | Measurement valid; gate fails |
| Aggregate ranking | Full per-model/slice metric table (§3) | pass | |
| Model comparison set | keyword, BM25, linear, ctx-small/medium/compact + 8 exclusion variants | pass | |
| Disclosure comparison | No-advisor baseline (no promotions), promote simulation at dev-selected threshold 0.15, rerank equivalence noted (identical offline order) | pass | Contextual promote recall 0.0 (scores below threshold); linear 0.244 at 0.231 FPR |
| No live calls | Harness is pure-Rust offline; no provider/network code path | pass | |
| Machine-readable + human-readable report | `requalify --json` verdict + concise text mode | pass | |
| Partition/artifact hashes in every result | Report carries dataset/split fps, per-model artifact sha256, exclusion lists | pass | |
| Hosted CI | Not available; stated as limitation | partial | Local: workspace suite + features + verify.sh quick all green |

## 3. Production implementation evidence

- `src/tool_advisor/requalify.rs` (new, ~1800 lines with tests): frozen
  protocol runner reusing `evaluate`, C001 partitions, C002 artifacts,
  C003 preselection/promotion. Tollgates verify dataset/split fingerprints,
  artifact hashes, exclusion lists, and calibration values before any test
  label is touched. Includes in-run exclusion retraining, dev-only shared
  threshold selection (grid maximizing mean dev promotion-F1, ties to the
  higher threshold), promotion simulation mirroring the pre-turn
  threshold/budget loop, the authority probe, the 64-tool recall fixture,
  resource measurement (bytes, cold load, score latency, sidecar
  effective-capacity), per-variant gate bundles, and the A/B/C/D
  disposition with the M004 transition directive. Seven unit tests pin the
  metric helpers and all four disposition mappings.
- `src/tool_advisor/contextual.rs` + `training.rs` + `mod.rs`: additive
  `exclude_tool_families` support (config → filtered train/dev on frozen
  splits → manifest record → report record) for both trainers, with an
  exclusion test on the real corpus (test fp unchanged `1765ad09…`).
- `src/main.rs`: `tool-advisor requalify --prereg … [--json]`.
- `assets/tool-advisor/c004-requalification.json` (new): the frozen
  pre-registration.
- `architecture/tool-advisor.md`: C004 disposition-B section; the model is
  documented as a research baseline and M004 as still blocked.

Per-slice metric table (test partition fp `1765ad09…`; MRR / R1 / no-tool
F1 / abstention Brier):

| Model | test | counterfactual | unknown | hard-neg | no-tool | holdout-P/L/R/S (MRR) |
|---|---|---|---|---|---|---|
| keyword | 0.000 / 0.000 / 0.254 | 0.000 / 0.000 | 0.000 / 0.000 | 0.000 | 0.000 / F1 1.000 / Br 0.000 | 0.000 ×4 |
| BM25 | 0.638 / 0.484 / — | 0.792 / 0.708 | 0.575 / 0.387 | 0.412 | 0.000 / F1 — / Br 1.000 | 0.660 / 0.609 / 0.590 / 0.646 |
| hashed-linear-v1 | 0.707 / 0.629 / 0.625 | 0.649 / 0.542 | 0.710 / 0.629 | 0.839 | 0.000 / F1 0.714 / Br 0.226 | 0.747 / 0.763 / 0.680 / 0.792 |
| ctx-small | 0.457 / 0.226 / 0.182 | 0.515 / 0.333 | 0.500 / 0.306 | 0.365 | 0.000 / F1 0.200 / Br 0.250 | 0.521 / 0.501 / 0.487 / 0.512 |
| ctx-medium | 0.595 / 0.403 / — | 0.684 / 0.542 | 0.543 / 0.306 | 0.417 | 0.000 / F1 — / Br 0.250 | 0.606 / 0.526 / 0.571 / 0.653 |
| ctx-compact | 0.557 / 0.355 / — | 0.670 / 0.500 | 0.564 / 0.403 | 0.370 | 0.000 / F1 — / Br 0.250 | 0.495 / 0.510 / 0.354 / 0.639 |
| small-excl-X | 0.457 (all X) | — | — | — | F1 0.200 / Br 0.250 | 0.521 / 0.501 / 0.487 / 0.512 |
| linear-excl-X | 0.706–0.726 | — | — | — | F1 0.615–0.714 | 0.718 / 0.731 / 0.680 / 0.750 |

(F1 — = undefined: model never abstains on the slice. Pair accuracy:
counterfactual 0.00 small / 0.33 medium+linear / 0.25 compact / 0.42 BM25.)

Calibration table (no-tool slice; calibrated vs same-rank uncalibrated):

| Variant | Cal bias/temp | Cal Brier/ECE/NLL | Uncal Brier/ECE | Dev NLL cal/uncal |
|---|---|---|---|---|
| small | −1.2851145 / 0.25 | 0.250 / 0.500 / 0.693 | 0.047 / 0.217 | 0.693 / 1.322 |
| medium | −1.2851146 / 0.25 | 0.250 / 0.500 / 0.693 | 0.047 / 0.217 | 0.693 / 1.322 |
| compact | −1.2851154 / 0.25 | 0.250 / 0.500 / 0.693 | 0.047 / 0.217 | 0.693 / 1.322 |

Dev-grid calibration improves dev NLL/Brier yet transfers worse than the
uncalibrated form on test abstention — small-sample dev score-distribution
overfit, recorded as a finding, not a mechanism defect.

Candidate-recall report: 70 relevant deferred tools over 62 labeled test
cases in 64-tool universes at budget 16 → 58 shortlisted, recall 0.8286
(12 misses, e.g. `plugin-158:write`, `filesystem-010:lint`,
`research-126:docs_lookup`), mean preselect 0.55 ms, max 1 ms. Genuine
lexical crowding in realistic catalogs, not a fixture artifact.

Resource/effective-capacity report: small 20.97 MB / 415 rows / 33,201
trained (0.63%), cold 928 ms, p50/p95 109/199 µs; medium 62.92 MB / 415
rows / 99,601 (0.63%), cold 2804 ms, p95 431 µs; compact 1.31 MB / 397
rows / 31,761 (9.69%), cold 58 ms, p95 197 µs; linear 129 params. All
within declared targets; preselect far below provider latency.

Authority-negative result: 0 violations (denied tool absent from eligible
set and unpromotable under adversarial top-score prediction).

Promotion report (shared dev-selected threshold 0.15): contextual
test promotion recall 0.000 at 0.000 false-promotion rate (scores below the
operating point — the pre-turn promote path would never fire for these
artifacts); linear recall 0.244 at 0.231 FPR, 0.000 no-tool promotion rate,
mean 151 added schema bytes (max 183, budget 16384).

Gate verdicts: leakage-zero pass; preselector-recall FAIL (0.8286 <
0.98); authority-zero pass; variant-small FAIL (MRR −0.250 vs linear, slice
regressions everywhere, F1 0.2 vs 1.0, cal worse than uncal);
variant-medium FAIL (+0.035 counterfactual gain but −0.167 unknown
regression, F1 null, cal worse); variant-compact FAIL (+0.021 counterfactual
MRR with −0.042 R1 on the same slice plus −0.146 unknown, F1 null, cal
worse); footprint-preselect-latency pass.

Selected architecture disposition: **B — mechanically correct but no
useful gain**. Contextual artifacts remain research/observe baselines; no
live-provider budget is spent; M004 remains blocked pending a new
model-architecture experiment.

Implementation commits: this closure batch (C004 harness + evidence).

## 4. Verification executed

### Commands run

```bash
cargo test --workspace --locked -- --test-threads=1
cargo test --locked --features tool-advisor -p codegg --lib
cargo test --locked --features tool-advisor-training -p codegg --lib
cargo test --locked -p codegg --lib tool_advisor
cargo test --locked -p codegg --lib agent::request_preparation
cargo test --locked -p codegg --lib agent::tool_surface
cargo test --locked -p codegg --lib tool::tool_search
cargo test --locked -p codegg --test tool_surface_minimization
cargo check --locked
cargo check --locked --features tool-advisor
cargo check --locked --features tool-advisor-training
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Final qualification commands (explicit frozen partitions/artifacts):

```bash
codegg tool-advisor requalify --prereg assets/tool-advisor/c004-requalification.json --json
codegg tool-advisor eval --model target/tool-advisor/contextual-small.bin --dataset assets/tool-advisor/corpus.jsonl --partition test --json
```

### Results

- `cargo test --workspace --locked`: 245 result lines, all ok, 0 failed.
- Feature-wide lib suites: tool-advisor 4884 passed; tool-advisor-training
  4894 passed; 0 failed.
- Focused suites: tool_advisor (default 30, training 57 incl. 7 new
  requalify verdict tests), request_preparation 16, tool_surface 5,
  tool_search 4, tool_surface_minimization 13 — all green.
- `cargo check` (3 variants): clean. `cargo fmt --check`: clean.
  `cargo clippy --all-targets --all-features -D warnings`: clean.
- `scripts/verify.sh quick`: passed. `git diff --check`: clean.
- Requalification protocol executed twice end to end (second run after
  final build): identical fingerprints, identical gate verdicts, identical
  disposition B — reproducibility confirmed empirically, not just by
  determinism argument.
- Local-only verification; hosted CI unavailable (recorded limitation per
  the plan's allowance).

## 5. Invariant review

- Pre-registration frozen before final eval: prereg file checked in with
  this batch; tollgates enforce it at runtime (any fingerprint/hash/
  calibration mismatch aborts before test labels are read).
- No tuning after test inspection: threshold selection is dev-only by
  construction; exclusion retraining consumes train/dev indices only
  (manifests verified); no config, grid, gate, or target changed after any
  test number was observed. The C002 descriptive test MRRs are disclosed in
  §2 with the no-change attestation.
- Test/family-holdout labels never trained on: main artifacts trained on
  C001 train only (C002 evidence); holdout measurements use exclusion
  retraining whose manifests prove the exclusion (hashes differ from
  standard artifacts, qualified flags hold).
- Prior leaked metrics never mixed in: the predecessor 0.939/0.946 MRR
  values appear nowhere in the new results; every number above is
  recomputed on frozen partitions.
- No live LLM trajectories substituted: the harness links no provider
  code; M004's live study is untouched and still blocked.
- Opt-in/default-off policy unchanged: no configuration or default changed.

## 6. Failure and recovery review

- Fingerprint/hash/calibration mismatch: hard abort before any measurement.
- Empty slice or missing artifact: hard error naming the gap (no silent
  zero-filling; `pair_accuracy` returns 1.0 only on a genuinely empty pair
  set, which cannot occur on this corpus).
- `all`-partition diagnostics: the harness never uses `all`; eval CLI
  flags it when an operator does.
- Exclusion training producing fallback/unqualified output: hard error.
- Second full execution reproduced the verdict bit-for-bit on gates and
  disposition (timings naturally vary).

## 7. Migration and compatibility review

- Additive only: new `requalify` module (feature-gated), new `Requalify`
  CLI subcommand, additive exclusion fields (defaulted) on configs,
  manifests, and reports, one new asset JSON. No existing command,
  artifact format reader, or report field changed meaning.
- `TrainingConfig`/`ContextualTrainingConfig` JSON remain backward
  compatible (all new fields defaulted; existing asset configs parse in
  tests).
- Artifacts from C002 load and verify unchanged (hashes in §3 match C002).

## 8. Security review

- No authorization, secret, network, or execution-surface change. The
  harness reads local fixtures/artifacts and writes local reports under
  `target/`; no telemetry, no provider calls, no user content involved.
- No new dependencies.

## 9. Documentation and operations

- `architecture/tool-advisor.md`: C004 disposition-B section added;
  contextual scorer documented as a research baseline, M004 as blocked.
- Operator reproduction: retrain (deterministic; C002 commands) then
  `codegg tool-advisor requalify --prereg
  assets/tool-advisor/c004-requalification.json --json`. The human-readable
  mode prints gates and the M004 transition directive.
- Exclusion-run outputs live under `target/tool-advisor/c004-run/`
  (gitignored by design); their hashes and manifests are recorded in the
  verdict JSON and §3.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Contextual variants show no useful gain (see §3 tables) | Live M004 must not proceed; architecture needs a new experiment | None in this workstream — disposition B is the closure |
| medium | Dev-grid abstention calibration transfers worse than uncalibrated on test | Any future architecture must calibrate on more abstention data or learn abstention explicitly | Future experiment scope, not a C004 defect |
| medium | Preselector recall 0.83 < 0.98 on 64-tool universes | Deferred-first shortlisting drops ~17% of relevant tools in large catalogs | Future preselector/rerank work; C003 mechanism stands |
| low | No hosted CI run | Standing limitation across C001–C004 | Recorded; local evidence is complete |
| low | Compact counterfactual slice both gains (MRR +0.021) and regresses (R1 −0.042) | No verdict impact (unknown-slice regression disqualifies independently under any reading) | Documented to prevent re-litigation |

No critical/high findings. No correctness failure → no narrower corrective
(D expressly not taken).

## 11. Roadmap disposition

C004 closed with disposition B. The evidence-integrity corrective
workstream is complete: C001 (clean partitions), C002 (correct math and
calibration), C003 (deferred-first shortlisting) all closed; C004 closed
with a negative qualification verdict as the plan allows. The existing live
M004 plan **remains blocked** (now pending a new model-architecture
experiment rather than evidence repair). No downstream plan lists C004 as a
hard dependency besides M004, so nothing else is unblocked; the unblock
audit finds no other registered plan gated on this workstream.

## 12. Registry updates

- `plans/registry.md`: move C004 `ready` → `closed` (closure:
  `plans/closure/tool-selection-advisor-evidence-integrity-corrective/004-status.md`,
  disposition B); keep M004 `blocked` with the blocker refined to cite the
  C004-B verdict (positive disposition absent; new architecture experiment
  required).
- `plans/subsystems/tool-selection-advisor-evidence-integrity-corrective-addendum.md`:
  C004 `ready` → `closed (disposition B)`; workstream status → `closed`.
- `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/004-…md`:
  status `ready for handoff` → `implemented`.
- No other registry rows change: the audit confirms no registered future
  plan besides M004 depends on C004, and M004's other prerequisites
  (operator-configured live calls, trajectory suite, resources) were never
  satisfied.
