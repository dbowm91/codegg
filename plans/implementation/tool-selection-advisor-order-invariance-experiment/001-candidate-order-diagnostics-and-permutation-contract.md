# Tool-Selection Advisor Order-Invariance Experiment M001 — Candidate-Order Diagnostics and Permutation Contract

Status: active

Repository baseline: `198524aa4ff8656928c86cf36168892532f2e29c`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md#m001--candidate-order-diagnostics-and-permutation-contract`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: evidence/infrastructure.

## 1. Objective

Make candidate-order shortcut learning measurable and create one deterministic permutation contract that later architecture/training milestones consume.

Do not train a new model in M001.

## 2. Current evidence to reproduce

Repository-local diagnostics must reproduce at least:

- historical corpus total 256;
- historical non-no-tool 207;
- highest-relevance target index 0: 201;
- target index 1: 6;
- no target beyond index 1;
- v3 non-no-tool 146;
- v3 target index 2: 133;
- v3 target index 3: 13;
- v3 target index 0/1: 0.

The diagnostic must derive these values from repository assets rather than hard-coding them.

Also report target-position distribution separately for train/dev/test partitions.

## 3. Permutation metric contract

Add reusable evaluation helpers that, for one model/case:

1. generate N deterministic candidate permutations;
2. score every permutation;
3. map scores/ranks back to canonical candidate identity;
4. compare predictions independent of presentation order.

Default N: 20 where factorial/candidate count permits; otherwise enumerate all unique permutations up to the bound.

Report:

- top-1 identity consistency;
- top-k set consistency;
- mean/max per-candidate score drift;
- Kendall/Spearman rank agreement when defined;
- MRR range across permutations;
- Recall@1 range across permutations;
- performance grouped by true target position;
- worst-case permutation regret versus best permutation.

For exact-order-equivariant architectures, score drift tolerance should be <=1e-5 or a documented deterministic floating-point tolerance.

## 4. Current-artifact diagnostic

Run the frozen v3-selected packed artifact against:

- historical dev partition permutation suite;
- historical test partition as diagnostic only;
- observed v3 as diagnostic only.

Do not alter the artifact.

Capture enough evidence to determine whether the v3 collapse is explained by candidate-order sensitivity.

This diagnostic is not a qualification and does not unlock any downstream live work.

## 5. Deterministic training permutation views

Create an in-memory or generated-local **training view**, not a replacement corpus.

For every clean train-partition source case:

- keep source `case_id`/semantic/leakage ownership traceable;
- generate deterministic permutations from a seed recorded in the training config;
- preserve relevance/preferred labels by candidate name;
- permute no-tool cases too;
- do not move augmented views into dev/test;
- do not assign a new leakage group to each permutation.

For non-no-tool cases, the aggregate augmented train view must place the highest-relevance candidate approximately uniformly across every valid candidate position.

Target maximum absolute position-frequency deviation:

- <=5 percentage points when enough cases exist;
- otherwise report exact combinatorial limitation.

For multi-tool equal-top relevance, balance each top-relevant candidate across positions rather than always choosing the first/last matching label.

## 6. Dev permutation suite

Create a dev-only permutation suite derived from the clean dev partition.

It is evaluation-only:

- no optimizer updates;
- no threshold tuning from individual permutation outcomes beyond selecting an architecture/training configuration using aggregate dev invariance metrics;
- no test/v3 examples included.

The suite fingerprint and seed must be recorded.

## 7. Training-source integrity

Permutation views must not change:

- context text;
- candidate descriptor text;
- label values;
- none/no-tool status;
- semantic group;
- leakage group;
- train/dev/test assignment.

A round-trip property test reconstructs the canonical source case after unpermuting.

## 8. Static diagnostics

Add a machine-readable order-bias report with:

- source dataset fingerprint;
- partition fingerprints;
- target-index histograms;
- candidate-count histogram;
- no-tool order distribution;
- multi-tool top-label position distribution;
- current-artifact permutation metrics where available.

Suggested output:

- `target/tool-advisor/order-invariance/m001-order-diagnostics.json`

Do not commit model outputs unless planning/closure conventions require a compact receipt; large generated artifacts remain under target.

## 9. Regression tests

At minimum:

- candidate permutation preserves candidate-name/descriptor pairs;
- labels survive round-trip permutation;
- no-tool cases remain no-tool under all permutations;
- train/dev/test ownership never changes;
- target-position balancing meets declared tolerance;
- permutation seed is deterministic;
- canonical mapped-back predictions compare by candidate identity, not index;
- current historical position histogram reproduces known values;
- v3 diagnostic histogram reproduces known values.

## 10. Non-goals

Do not:

- modify historical corpus files;
- retrain MiniLM;
- change packed marker implementation;
- change retrieval;
- change promotion threshold;
- use v3 to select future hyperparameters.

## 11. Verification

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor
cargo test --workspace --locked -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

## 12. Acceptance

M001 closes when positional-bias evidence is reproducible, permutation metrics are reusable, and deterministic train/dev permutation contracts are frozen with leakage/label invariants proven. M002 then becomes ready.
