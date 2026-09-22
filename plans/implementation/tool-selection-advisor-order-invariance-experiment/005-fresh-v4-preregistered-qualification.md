# Tool-Selection Advisor Order-Invariance Experiment M005 — Fresh V4 Preregistered Qualification

Status: blocked on M004

Repository baseline: `198524aa4ff8656928c86cf36168892532f2e29c`

Hard dependencies:

- M001-M003 positive closure;
- M004 positive retrieval/promotion operating-point closure.

Source roadmap:

- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md#m005--fresh-v4-preregistered-qualification`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: qualification/closure.

## 1. Objective

Perform one final offline qualification of the selected order-robust artifact and frozen retrieval/promotion operating point against a fresh v4 semantic holdout.

V4 must be unseen by training, dev selection, v3 diagnostics, threshold selection, and retrieval-K selection.

## 2. Fresh v4 holdout

Create a new repository-owned holdout after M004 closes.

Minimum:

- >=192 cases;
- >=150 semantic/leakage groups;
- >=32 no-tool;
- >=32 hard-negative;
- >=32 unknown/renamed;
- >=32 multi-tool;
- >=32 AdvisorContextV2 long-session;
- >=16 true counterfactual pairs;
- >=20 cases each plugin/MCP, LSP, research/search, structured/data;
- ordinary filesystem/git/shell/code-navigation coverage;
- balanced relevant-candidate presentation positions across 0..N-1.

The holdout must explicitly test order invariance:

- at least 50 source scenarios have >=3 deterministic candidate permutations;
- permutations belong to one semantic/leakage group;
- correct candidate identity is unchanged after permutation;
- no single target position dominates.

## 3. Leakage

Require zero exact/normalized/template/explicit-family/copied-context overlap with:

- historical train/dev/test;
- v2;
- v3;
- any M003 dev-only semantic augmentation.

V4 candidate unknown identities must be novel where that slice requires novelty.

## 4. Separate preregistration commit

Commit A freezes:

- v4 fingerprints/manifest;
- selected model and encoder/tokenizer hashes;
- training/permutation-contract fingerprint;
- candidate relevance + abstention calibration;
- promotion threshold;
- retrieval mode/K;
- all quality/order/retrieval/promotion/resource gates;
- exact release command;
- target hardware.

Hosted CI must pass before final evaluation when available.

No self-referential SHA inside the protocol hash; pass Commit A SHA separately.

## 5. Quality gates

Minimum final gates:

- aggregate MRR >= frozen linear MRR -0.01;
- aggregate Recall@1 >= linear Recall@1 -0.02;
- >=0.02 MRR or R1 gain on at least one contextual/hard-negative slice;
- no required semantic slice regresses >0.02 MRR versus linear;
- no-tool F1 >= best nontrivial baseline -0.02;
- multi-tool nDCG >= linear -0.02;
- unknown/renamed MRR >= linear -0.02;
- each family slice >= linear -0.02.

## 6. Order-invariance gates

For the predeclared v4 permutation scenarios:

- top-1 identity consistency >=0.95;
- pairwise/batched exact-order-equivariant model target: >=0.99;
- max target-position R1 spread <=0.05;
- permutation worst-case MRR >= canonical-order MRR -0.02;
- mapped candidate score drift within architecture-specific preregistered tolerance.

A model that performs well only when the relevant tool appears early fails qualification.

## 7. Counterfactual gates

Report pair accuracy over true counterfactual pairs.

Minimum:

- pair accuracy >=0.70;
- neither side may systematically collapse to one candidate position.

## 8. Retrieval gates

At the frozen M004 operating point:

- 64-tool recall >=0.98;
- 128-tool recall >=0.95;
- 256-tool recall >=0.92;
- zero authority violations.

Missing/duplicate frontier point is correctness failure E.

## 9. Promotion gates

At the frozen candidate-relevance calibration/threshold:

- relevant-tool promotion recall >=0.50;
- no-tool promotion rate <=0.10;
- irrelevant-tool promotion rate <=0.15;
- max promotions <=2;
- schema p95 <=16 KiB;
- promotion identity consistency under permutation >=0.95;
- zero authority violations.

## 10. Calibration

Report independently:

### Abstention
- Brier/ECE/NLL;
- no-tool P/R/F1.

### Candidate relevance
- Brier/ECE/NLL;
- positive/negative precision-recall;
- promotion threshold provenance.

Do not collapse these into one calibration field.

## 11. Resources

Preserve previous gates unless M004 froze stricter ones:

- process-cold load p95 <=10 s;
- total qualification <=600 s;
- encoder weights <=128 MiB.

Report:

- binary delta;
- RSS;
- rank/retrieval p50/p95/max;
- forwards/case;
- cache behavior;
- K scaling;
- permutation-suite incremental cost.

## 12. One-run discipline

After Commit A and green CI:

1. clean tree;
2. validate all hashes;
3. build exact release binary;
4. run v4 qualification once;
5. commit machine result and closure without changing inputs.

A failed execution with no usable model result may be retried only after recording the failure.

## 13. Disposition

- **A — qualify for live M004:** all correctness/order/quality/retrieval/promotion/calibration/resource gates pass.
- **B — quality/order gain but deployment cost too high:** all non-resource gates pass; resource fails.
- **C — retrieval/promoter useful but ranker not qualified:** retrieval/promotion/safety pass; ranker quality/order fails.
- **D — no useful quality/order gain:** valid evidence but ranker fails quality/order requirements.
- **E — correctness/framework/evidence failure:** protocol/leakage/authority/permutation-fixture correctness fails.

Only A makes the existing live-primary-model M004 dependency-ready. Original provider/operator/trajectory prerequisites remain separate.

## 14. Historical evidence handling

Historical v3 remains a diagnostic comparison only. Do not rewrite its D disposition.

The closure should explain whether the new model resolved the specific v3 positional failure mechanism.

## 15. Acceptance

M005 closes with one machine-readable v4 result and explicit disposition. Positive A is required before any live small-model trajectory study can resume.
