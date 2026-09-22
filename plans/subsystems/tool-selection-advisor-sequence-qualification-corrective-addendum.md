# Tool-Selection Advisor Sequence Qualification — Post-Closure Corrective Addendum

Status: active

Repository planning baseline: `c5fa1a0850d88744984a2aba445a1e4ba2b9958f`

Controlling architecture:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Predecessor work:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md` — closed with M005 disposition D.
- `plans/closure/tool-selection-advisor-sequence-encoder-experiment/006-status.md` — historical M005 qualification closure; remains immutable.
- `assets/tool-advisor/sequence-qualification-result.json` — historical M005 machine-readable result.
- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/006-clean-offline-sequence-encoder-qualification.md` — original qualification requirements.

Normative process:

- `plans/003-planning-process.md#7-corrective-passes`

## 1. Purpose

The sequence-encoder workstream produced a credible model-quality signal but the final M005 qualification harness did not implement the complete evidence contract required by its own plan.

The selected frozen MiniLM packed-marker ranker reached:

- test MRR 0.8306 versus 0.7070 for `hashed-linear-v1`;
- Recall@1 0.8065 versus 0.6290;
- perfect relevant-tool recovery on the deterministic 64- and 128-tool retrieval fixtures.

M005 still closed conservatively because its contextual-slice gate was unavailable and a 10-second cold-load budget failed.

A post-closure audit found that the negative qualification is not sufficient evidence to reject the architecture:

1. `contextual_artifact` was preregistered as `null`, which guaranteed the implemented contextual gate would fail.
2. The implemented `contextual-slice-gain` gate compared aggregate MRR against the historical contextual-model arm; it did not evaluate the predeclared contextual slices required by the M005 plan.
3. Unknown/renamed-tool, family-holdout, no-tool-baseline, calibrated-vs-uncalibrated, proactive-promotion, and long-session/context-v2 gates were not emitted by the final harness.
4. The resource report omitted release-binary delta, RSS, and p50/p95 latency evidence required by the plan.
5. The 16.7-second cold-load result came from the development-profile qualification command, so it is not a reliable deployment-performance gate.
6. The harness collapsed every failure into disposition D even though the roadmap defines B for "quality gain but deployment cost too high".
7. The original final test has now been inspected. A later positive qualification should not rely exclusively on that already-observed test set.

This corrective repairs only the qualification/evidence boundary. It does not reopen model training, retrieval architecture, authority rules, or the historical M005 closure.

## 2. Invariants

- The selected M003 model artifact is frozen. Corrective work MUST NOT retrain, change its weights, change its pooling strategy, change its architecture, or select a different model based on qualification data.
- Existing C001 train/dev/test and family-holdout data remains immutable historical evidence.
- Historical M005 preregistration, result artifact, and closure record remain unchanged.
- Advisor use remains optional/default-off.
- No learned sequence model is wired into live primary-model behavior by C001.
- `ResolvedToolSurface` remains the authority source; qualification cannot widen the candidate universe.
- Threshold/calibration selection uses existing train/dev evidence only; neither the historical exposed test set nor the new final holdout may tune thresholds.
- Remote provider/teacher calls are out of scope.
- No model download path is added to normal CodeGG.

## 3. Corrective dependency graph

```text
historical M005 closure (immutable)
             |
             v
C001 qualification harness + fresh holdout
             |
             v
C002 separately preregistered release qualification
             |
          positive A
             v
existing live-primary-model M004
```

- C001 is closed positively with `plans/closure/tool-selection-advisor-sequence-qualification-corrective/001-status.md`.
- C002 is ready; its separate preregistration freeze and final-run discipline remain mandatory.
- Existing live-primary-model M004 remains blocked unless C002 records disposition A.

## 4. Milestones

### C001 — Qualification harness completeness and fresh holdout

Plan:

- `plans/implementation/tool-selection-advisor-sequence-qualification-corrective/001-qualification-harness-and-fresh-holdout.md`

Status: closed.

Repair the qualification semantics, implement the omitted evidence slices/gates, establish release-mode resource measurement, and freeze a new evaluation-only holdout that has zero content/template leakage into prior train/dev/test data. C001 MUST NOT run the new holdout through the selected model.

### C002 — Preregistered release-mode requalification

Plan:

- `plans/implementation/tool-selection-advisor-sequence-qualification-corrective/002-preregistered-release-qualification.md`

Status: ready.

Freeze all artifacts/gates/configuration in a separate commit, wait for hosted CI when available, then run exactly one final qualification on the fresh holdout plus historical diagnostic slices. Compute a truthful A/B/C/D/E disposition. Only A can unblock live M004.

## 5. Exit conditions

The corrective closes when:

- all originally required quality/safety/calibration/promotion/resource evidence is emitted;
- final metrics are generated by a reproducible release-mode qualification path;
- the selected model artifact is provably unchanged from historical M005;
- the fresh holdout passes zero-leakage checks and was not used for tuning;
- one explicit disposition is recorded;
- live M004 is unblocked only for disposition A.

A result of B, C, D, or E is valid closure and leaves live M004 blocked.
