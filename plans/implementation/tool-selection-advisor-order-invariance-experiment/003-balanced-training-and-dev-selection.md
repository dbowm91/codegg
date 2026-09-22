# Tool-Selection Advisor Order-Invariance Experiment M003 — Balanced Training and Dev Selection

Status: active

Repository baseline: `722d8e7c874f5ccdb134c8359077004c3f27cd86`

Hard dependencies:

- M001 permutation/data contract;
- M002 order-robust architecture implementation.

Source roadmap:

- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md#m003--balanced-training-and-dev-selection`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: model experiment.

## 1. Objective

Train and select one order-robust sequence ranker using only clean train/dev data plus M001 permutation views.

The historical test, v2, and v3 corpora are forbidden for architecture/hyperparameter/threshold selection.

## 2. Training inputs

Optimizer input:

- historical clean train partition only;
- deterministic balanced permutation views from M001.

Development selection:

- historical clean dev partition;
- M001 dev permutation suite;
- optional additional **dev-only** semantic cases created before any experiment run, with provenance/leakage groups and no overlap with future v4.

Do not alter the canonical 256-case corpus.

## 3. Objective correction

Replace the single-target-only ranking objective with an objective that preserves graded relevance.

At minimum compare:

### Listwise graded relevance

Convert relevance grades 1..3 into a normalized target distribution and optimize candidate softmax cross entropy/KL.

No-tool cases omit listwise relevance loss.

### Binary candidate relevance

For every candidate, train a binary relevance logit/probability:

- positive when relevance >0;
- optionally weight by relevance grade;
- negatives include hard distractors.

This signal later supports promotion calibration.

### Abstention

Separate BCE against `case.none`.

Do not derive candidate relevance confidence from the abstention head.

### Permutation consistency

For architectures that are not mathematically order-equivariant, optionally add a consistency term:

- score the same source case under two deterministic permutations;
- map scores back by candidate identity;
- penalize divergence in normalized relevance distributions.

Record the exact coefficient and loss definition.

## 4. Multi-tool semantics

Do not collapse equal/graded relevant candidates to one `target_index`.

Training/evaluation must support:

- multiple relevant tools;
- relevance grades;
- preferred order as an evaluation signal where appropriate.

Add a regression proving two equally relevant candidates can both receive high relevance probability.

## 5. Training stages

Start with frozen MiniLM + trainable heads.

Then, only if dev quality/invariance warrants:

- top-1 layer unfreeze;
- top-2 layer unfreeze.

Full fine-tune remains optional and must be justified by a positive dev delta versus cost.

Use the already-qualified differentiable LayerNorm path.

## 6. Architecture arms

Evaluate at minimum:

- historical distinct-marker v1 control (no new selection eligibility unless order-invariance gates pass, which is not expected);
- shared-marker packed;
- span-pooled shared-marker packed;
- batched pairwise cross-encoder.

An arm may be removed after a documented correctness/resource failure.

## 7. Dev metrics

Report standard metrics plus order robustness:

- MRR;
- Recall@1/3/5;
- nDCG@5;
- no-tool P/R/F1;
- candidate binary relevance Brier/ECE/NLL;
- target-position-stratified MRR/R1;
- permutation top-1 identity consistency;
- mean/max score drift;
- worst permutation MRR;
- permutation regret;
- multi-tool recall/nDCG;
- unknown-tool dev slice where available;
- latency/RSS/forwards.

## 8. Dev selection gates

A model is eligible for M004 only if all are met:

- permutation top-1 identity consistency >=0.95;
- no target-position bucket with >=10 cases is >0.05 absolute R1 below aggregate R1;
- aggregate dev MRR >= frozen linear dev MRR -0.01;
- improves MRR or R1 >=0.02 over linear on at least one context-sensitive/hard-negative dev slice;
- no-tool F1 >= best nontrivial dev baseline -0.02;
- multi-tool nDCG not worse than linear by >0.02;
- candidate-relevance calibration is finite and usable for threshold selection;
- no authority/runtime invariant change.

For mathematically pairwise order-equivariant architecture, the permutation consistency gate should be effectively 1.0 and score drift within numeric tolerance.

## 9. Hyperparameter discipline

Predeclare the finite sweep before evaluating dev:

- learning rates;
- epochs;
- objective weights;
- fine-tune stages;
- architecture variants.

Do not expand the sweep after inspecting v3.

Record all attempted configurations, including failures.

## 10. V3 diagnostic

Only **after** selecting the winning artifact on dev:

- run the selected artifact once on v3 for diagnostic comparison;
- do not modify training, threshold, architecture, or M004 choices based on v3;
- report whether positional collapse disappeared.

V3 cannot qualify the model.

## 11. Artifact contract

Selected artifact records:

- model/architecture version;
- training source fingerprints;
- permutation augmentation version/seed;
- objective weights;
- fine-tune stage;
- candidate relevance calibration parameters;
- abstention calibration parameters;
- dev selection metrics;
- head/encoder hashes;
- v3 diagnostic explicitly marked post-selection/non-gating.

## 12. Stop conditions

Close negatively if:

- no arm clears permutation consistency;
- quality gain disappears once position is balanced;
- no-tool behavior remains unusable;
- only the old positional shortcut control appears strong;
- runtime cost is clearly outside the existing deployment envelope.

## 13. Verification

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor
cargo test --workspace --locked -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

## 14. Acceptance

M003 closes positively when one selected artifact satisfies dev quality, no-tool, multi-tool, calibration, and permutation-invariance gates without using test/v2/v3 for selection. Positive closure makes M004 ready.
