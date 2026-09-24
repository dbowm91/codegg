# Tool-Selection Advisor Retrieval-Signal Experiment M005 — Fresh V4 Preregistered Qualification

Status: blocked on M004

Repository baseline: `1093ad0e3285e8ee66684e8a7f3401a200c0596e`

Hard dependency:

- M004 positive frozen operating point.

Source roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m005--fresh-v4-qualification`

Primary class: final qualification/closure.

## 1. Objective

Run one final preregistered release-mode qualification of the complete new retrieval signal + frozen span-packed ranker + promotion stack against a fresh v4 holdout.

This plan supersedes no historical closure. The old order-invariance M005 remains blocked historical planning; this is the only fresh qualification path for this experiment.

## 2. Fresh v4 holdout

Construct only after M004 freezes.

Minimum:

- >=192 cases;
- >=150 semantic/leakage groups;
- >=32 no-tool;
- >=32 hard-negative;
- >=32 actual unknown/renamed;
- >=32 multi-tool;
- >=32 AdvisorContextV2 long-session;
- >=16 true counterfactual pairs;
- >=20 each plugin/MCP, LSP, research/search, structured/data;
- ordinary filesystem/git/shell/code-navigation coverage;
- balanced relevant-candidate positions.

Additionally include retrieval-signal stressors:

- paraphrase without exact descriptor verbs;
- schema-cue cases where operation is inferable from parameter concepts;
- identifier-heavy queries;
- candidates without parameter schema;
- unknown tools with informative descriptions;
- multi-tool cases where each positive is explicitly inferable from current state.

## 3. Label sufficiency requirement

Every relevant v4 label must include a local rationale showing which allowed `AdvisorContextV2` field makes the tool inferable.

A case with a hidden/implicit future workflow dependency is invalid.

Semantic validator fails closed on missing rationale/support.

## 4. Leakage

Zero overlap with:

- historical train/dev/test;
- v2;
- v3;
- M003 train-generated variants;
- any M001/M002/M003 dev-only augmentation.

Require zero exact/normalized/template/explicit-family/copied-context overlap.

## 5. Separate preregistration commit

Commit A freezes:

- v4 fingerprints/manifest;
- selected Retrieval Signal V2/projection hash;
- MiniLM hashes;
- span-packed ranker hash;
- retrieval mode/K;
- abstention and candidate-relevance calibration;
- promotion threshold;
- all quality/order/retrieval/promotion/resource gates;
- exact release command/hardware.

Hosted CI must pass before model evaluation.

## 6. Retrieval gates

On fresh v4 expanded fixtures:

- 64-tool recall >=0.98;
- 128-tool recall >=0.95;
- 256-tool recall >=0.92;
- zero authority violations.

Report separately:

- direct/highest-grade relevance;
- secondary relevance;
- unknown/renamed;
- schema-present/schema-absent;
- paraphrase stress slice.

## 7. End-to-end ranker quality

At frozen retrieval point:

- aggregate MRR >= frozen linear baseline -0.01;
- aggregate Recall@1 >= linear -0.02;
- >=0.02 MRR or R1 gain on at least one contextual/hard-negative slice;
- no required slice regresses >0.02 MRR;
- no-tool F1 >= best nontrivial baseline -0.02;
- multi-tool nDCG >= linear -0.02;
- unknown/renamed MRR >= linear -0.02.

## 8. Order robustness

Preserve the order-invariance contract:

- top-1 identity consistency >=0.95;
- max target-position R1 spread <=0.05;
- worst-permutation MRR >= canonical MRR -0.02.

Retrieval candidate ordering itself must not create an ordinal shortcut in the downstream ranker.

## 9. Promotion gates

At frozen M004 threshold:

- relevant promotion recall >=0.50;
- no-tool promotion rate <=0.10;
- irrelevant promotion rate <=0.15;
- max promotions <=2;
- schema p95 <=16 KiB;
- authority violations =0.

## 10. Resource gates

Release mode:

- process-cold p95 <=10 s;
- total qualification <=600 s unless preregistration records a justified bounded suite split;
- shared encoder weights <=128 MiB;
- report projection bytes separately.

Report p50/p95/max retrieval/rank/total latency and RSS.

## 11. One-run discipline

After preregistration + green CI:

1. verify clean tree and all hashes;
2. validate v4 semantic/leakage contract;
3. build exact release binary;
4. run final qualification once;
5. commit result and closure without tuning anything.

Retry only execution failures with no usable result, and record the reason.

## 12. Disposition

- **A — qualify for live trajectory:** all correctness/retrieval/ranker/order/promotion/resource gates pass.
- **B — model quality passes, deployment cost fails.**
- **C — retrieval useful but downstream advisor not qualified.**
- **D — no useful generalizing gain.**
- **E — correctness/evidence/framework failure.**

Only A may make the existing live-primary-model trajectory study dependency-ready, subject to its original provider/operator prerequisites.

## 13. Verification

```bash
cargo test --workspace --locked -- --test-threads=1
cargo test --locked --features tool-advisor-encoder-training -- --test-threads=1
scripts/verify.sh quick
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## 14. Acceptance

M005 closes with one machine-readable fresh-v4 result and explicit A/B/C/D/E disposition. Historical v1/v2/v3 results remain unchanged.
