# Tool-Selection Advisor Late-Interaction Retriever M003 — Top-Layer Late-Interaction Fine-Tuning

Status: blocked/conditional on M002 disposition B

Repository baseline: `c4cc6c5b3c9154428b8560a60aa0129624435dfe`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-late-interaction-retriever-experiment-roadmap.md#m003--top-layer-late-interaction-fine-tuning`

Hard dependency:

- M002 closure disposition B with a frozen research initialization artifact and clean generalization.

Primary class: model experiment.

## 1. Objective

Test whether minimal encoder adaptation can close the remaining retrieval gap while preserving unknown/renamed and description-based generalization.

Only the top one or two MiniLM layers may train.

Full-model fine-tuning is a non-goal and requires a new workstream if later justified.

## 2. Training scope

Trainable variables:

- M002 selected token projection;
- either top 1 MiniLM transformer layer or top 2 layers.

Frozen:

- embeddings;
- all lower transformer layers;
- tokenizer;
- Signal V2 text/schema;
- downstream span-packed ranker.

Use the existing differentiable BERT path qualified by the sequence-encoder fine-tuning corrective.

## 3. Preregistration

Before any optimizer run freeze:

- top-layer count: {1, 2};
- learning-rate grid;
- epoch grid;
- seeds;
- temperature;
- hard-negative count;
- batch size;
- gradient clipping;
- optimizer;
- checkpoint selection rule;
- generalization gates;
- resource/wall-clock bound.

Target a bounded grid. A suggested upper bound is 24 optimizer arms; if implementation requires more, the prereg must justify it before measurement.

## 4. Training target

Use the same train-only corrected inferable pairs and frozen hard-negative construction as M002.

Do not:

- add dev-derived negatives;
- reintroduce implicit-future labels;
- use v3/v4;
- distill from an external LLM;
- train on ToolRet/Tool-REX.

## 5. Loss

Use the same token-level MaxSim contrastive objective as M002 so architecture attribution remains clean.

A changed loss requires preregistration before training and must be justified as necessary for differentiable encoder training, not selected from dev outcomes.

## 6. Dev qualification

Evaluate every arm on:

- 64/128/256 x K16/24/32;
- unknown/renamed;
- canonical-name masked;
- family holdouts;
- paraphrase/no-exact-verb;
- M001/M002 residual misses;
- latency/RSS/training time.

Primary retrieval gates:

- u64 >=0.99;
- u128 >=0.98;
- u256 >=0.95;
- K<=32;
- zero authority violations.

Generalization gates are the stricter of M001/M002 frozen guards.

## 7. Anti-memorization conditions

No arm qualifies if:

- unknown/renamed recall regresses >0.02 versus M002 research artifact;
- canonical-name masking falls >0.02 below lexical baseline;
- name masking changes a qualifying result into a failed 256-tool recall result;
- any family loses >0.03 recall;
- training produces strong train gain without corresponding dev inferable gain.

Report train-vs-dev gap explicitly.

## 8. Disposition

### A — qualify

Freeze one artifact only if all quality/generalization/resource gates pass. M004 becomes ready.

### D — negative

If no arm qualifies, close this late-interaction architecture line negative. M004/M005 remain blocked.

There is no M003 "near-gate" escalation. Full-model training, larger encoders, external training corpora, or ANN systems require separate planning.

## 9. Resources

Record:

- trainable parameter count;
- artifact bytes;
- peak training RSS;
- wall-clock per arm and full grid;
- inference RSS delta;
- descriptor-cache bytes;
- query encode p50/p95;
- MaxSim p50/p95;
- total retrieval p50/p95.

Retain M001 inference hard limits unless the M003 preregistration explicitly tightens them.

## 10. Hosted/default-feature posture

Training remains feature-gated and local/operator-invoked.

Default CodeGG builds must not:

- link Candle training machinery unless already required by the existing feature graph;
- load weights;
- create token caches;
- change tool disclosure.

Routine hosted CI still validates the default workspace; focused feature-gated tests carry model semantics.

## 11. Verification

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

## 12. Acceptance

M003 closes A or D.

Only A produces an artifact eligible for M004.
