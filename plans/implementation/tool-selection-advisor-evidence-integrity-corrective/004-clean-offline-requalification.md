# Tool-Selection Advisor Evidence Corrective C004 — Clean Offline Requalification and Live-M004 Gate

Status: ready for handoff (unblocked by C001+C002+C003 closures)

Repository baseline: `71460c0cb1421f33a62d57123ac562c8a7c4bf1c`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-evidence-integrity-corrective-addendum.md#c004--clean-offline-requalification-and-live-m004-gate`

Predecessor plans/closures:

- `plans/closure/tool-selection-advisor-post-closure-corrective/001-status.md`
- `plans/closure/tool-selection-advisor-post-closure-corrective/002-status.md`
- `plans/closure/tool-selection-advisor-post-closure-corrective/003-status.md`
- `plans/implementation/tool-selection-advisor-post-closure-corrective/004-small-model-trajectory-qualification.md`

Hard dependencies: C001 + C002 + C003 accepted closure.

Primary class: evidence/closure corrective.

## 1. Objective

Re-establish trustworthy offline evidence after correcting split leakage, training math/calibration, and candidate shortlisting.

C004 is the only gate allowed to mark the existing live-primary-model M004 plan ready. It must not inherit the predecessor 0.939/0.946 MRR values as qualification evidence.

## 2. Pre-registration

Before running final evaluation, freeze and record:

- C001 dataset and partition fingerprints;
- true family-holdout definitions;
- unknown-tool transformation rules;
- model artifact hashes/configs;
- calibration values selected on dev;
- BM25/keyword/hashed-linear baselines;
- contextual small/medium/compact variants selected for final comparison;
- promotion threshold/margin/max candidates/schema budget;
- primary metrics and pass/fail gates below.

Do not tune after inspecting final-test/family-holdout results.

## 3. Evaluation slices

Run at least:

1. frozen final test;
2. contextual counterfactual pairs;
3. hard negatives;
4. no-tool/abstention;
5. unknown/renamed tool descriptors;
6. each true tool-family holdout;
7. large-catalog deferred candidate-recall fixture;
8. aggregate ranking.

Report each separately; aggregate score cannot hide a failed contextual/unknown/no-tool slice.

## 4. Models/modes

Compare:

- keyword;
- BM25;
- `hashed-linear-v1`;
- corrected contextual small;
- corrected contextual medium;
- compact contextual configuration if C002 produced one.

For disclosure mechanism, compare:

- no advisor;
- reactive rerank;
- proactive pre-turn promote using the corrected C003 shortlist.

No live external LLM calls belong in C004.

## 5. Metrics

Ranking:

- Recall@1/3/5;
- MRR;
- nDCG;
- candidate coverage before neural scorer.

Contextual generalization:

- counterfactual pair accuracy;
- renamed/unknown-tool Recall@K/MRR;
- family-holdout metrics.

Abstention/calibration:

- no-tool precision/recall/F1;
- Brier score;
- ECE;
- NLL;
- false-promotion rate at configured threshold.

Resource:

- allocated vs touched parameters;
- artifact bytes;
- cold load;
- peak RSS;
- p50/p95 score latency at realistic candidate counts;
- preselector latency;
- prompt/schema bytes added by promotion.

## 6. Qualification gates

A contextual artifact may receive a positive offline disposition only if all of the following hold on frozen data:

- zero C001 leakage violations;
- preselector candidate recall >= 0.98 for labeled relevant deferred candidates on the large-catalog fixture;
- zero authority-negative promotion violations;
- aggregate final-test MRR is not worse than `hashed-linear-v1` by more than 0.01;
- contextual model improves MRR or Recall@1 by at least 0.02 on at least one predeclared context-sensitive slice (counterfactual, unknown-tool, or true family holdout) without a >=0.02 regression on the other context-sensitive slices;
- no-tool F1 is not worse than the best non-contextual baseline by more than 0.02;
- calibrated Brier/ECE improve over or are non-inferior to the uncalibrated contextual scorer;
- runtime footprint/latency remains within the local target declared by closure.

If exact statistical repetition is cheap, include bootstrap/repeated-seed uncertainty. Do not overstate differences smaller than run variance.

## 7. Architecture disposition

C004 must explicitly choose one:

### A — Positive qualification

The corrected contextual architecture demonstrates useful generalization per the gates. Record the selected capacity/configuration and mark the existing live M004 plan ready for operator-configured primary-model A/B testing.

### B — Mechanically correct but no useful gain

Keep contextual model as research/observe baseline. Do not spend live-provider budget merely to validate plumbing. Existing M004 remains blocked/superseded pending a new model-architecture experiment.

### C — Resource-inefficient

If a smaller/compact configuration matches quality, select it and document why raw 5M/15M physical parameter count is not useful. Existing M004 may proceed only with the selected efficient configuration.

### D — Correctness failure

Register a narrower corrective; do not unblock live M004.

Negative evidence is an acceptable C004 closure result.

## 8. Live-M004 registry transition

Only positive disposition A or C may change:

```text
Tool-selection advisor post-closure corrective M004
blocked -> ready
```

The registry update must cite C004 closure evidence and the exact selected artifact/configuration.

Disposition B or D must leave live M004 blocked or explicitly supersede it. Historical M001-M003 closures remain unchanged.

## 9. Required tooling

Extend existing `tool-advisor bench/eval/qualify` rather than building a separate benchmark framework.

The final report should be machine-readable JSON plus a concise human-readable closure summary. Include partition/artifact hashes in every qualification result.

## 10. Verification

Before final evaluation:

```bash
cargo test --workspace --locked
cargo test --locked --features tool-advisor
cargo test --locked --features tool-advisor-training
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Final qualification commands must explicitly name frozen partitions/artifacts.

Hosted CI for the closing commit should be recorded when available. If CI is unavailable, closure states that limitation rather than implying hosted verification.

## 11. Acceptance criteria

C004 closes when clean offline evidence is complete, reproducible, partition-fingerprinted, and yields one explicit architecture disposition. It is not required to be positive.

The existing live M004 plan is ready only if C004 explicitly records a positive disposition.

## 12. Stop conditions

Stop if final test/family holdout is used to tune thresholds/model choice, if prior leaked metrics are mixed into the new result, or if the implementation attempts to substitute live LLM trajectories for unresolved offline correctness.

## 13. Closure evidence

- frozen partition/artifact/config hashes;
- full per-slice metric table;
- calibration table;
- candidate-recall report;
- resource/effective-capacity report;
- authority-negative result;
- selected architecture disposition;
- exact registry transition;
- exact local/hosted verification output.
