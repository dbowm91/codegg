# Tool-Selection Advisor Sequence-Encoder Experiment M003 — Sequence-Encoder Ranking Experiment

Status: blocked on M001A

Repository baseline: `8487967d9cc2605c398d3c608e353c8871289a7d`

Hard dependencies:

- positive M001A reference-checkpoint materialization/qualification;
- M002 AdvisorContextV2 (closed).

Source roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#m003--true-sequence-encoder-ranking-experiment`

Primary class: experimental capability.

## Objective

Test whether a real pretrained bidirectional sequence encoder can outperform the linear and hashed-embedding baselines on contextual/unknown-tool selection while remaining small enough for local CodeGG use.

## Model candidates

Use only the M001A-qualified real pretrained local asset(s). Generated tiny fixtures are wiring tests and MUST NOT be used as M003 model candidates. Target at least two capacity points when available:

- approximately 14–15M TinyBERT-class;
- approximately 22M MiniLM/XtremeDistil-class.

A smaller 5–10M student may be added only after one pretrained reference demonstrates signal; do not manufacture parameter count with unused embedding rows.

## Ranking variants

### Variant A — pairwise cross-encoder

Input per candidate:

```text
[CLS] AdvisorContextV2 [SEP] canonical name + description + category + disclosure [SEP]
```

Head:

- scalar relevance logit;
- explicit no-tool/abstention logit from pooled task state;
- optional relevance-grade ordinal/listwise projection.

### Variant B — packed marker ranker

One bounded sequence contains context plus several descriptors. Use existing tokenizer marker IDs when possible rather than mutating tokenizer vocabulary.

Score hidden state at each marker. Derive:

- per-candidate relevance;
- explicit abstention logit;
- optional confidence features: top score, margin, entropy.

The packed layout must record exact token-budget allocation and dropped-candidate behavior.

## Training stages

For each viable architecture/capacity:

1. encoder frozen, train ranking/abstention head only;
2. unfreeze top 1–2 encoder layers if stage 1 has dev signal;
3. full fine-tune only if justified by dev results and overfitting controls.

Use deterministic seeds and early stopping on dev metrics only.

## Objectives

Prefer one clearly documented objective per run rather than combining many hidden terms.

Recommended starting point:

- graded candidate relevance converted to normalized soft targets;
- listwise cross-entropy over candidates;
- binary no-tool/abstention BCE;
- optional pairwise margin term only if listwise alone cannot separate hard negatives.

Teacher soft probabilities may be consumed if already present, but M003 must not make remote teacher generation a prerequisite.

## Data discipline

- consume C001 train/dev partitions;
- never inspect frozen test/family-holdout labels during model selection;
- training-only augmentation, if introduced, must have independent provenance and leakage signatures;
- no automatic remote data generation;
- report exact counts by source/provenance.

## Artifact contract

New artifacts must include:

- architecture id;
- framework/version;
- encoder config hash;
- tokenizer hash/version;
- local source-weight hash and license/provenance;
- fine-tuning stage;
- context/candidate schema versions;
- max sequence/candidate counts;
- training/dev partition fingerprints;
- calibration parameters;
- final weight hash.

Old contextual artifacts remain loadable research baselines but are never silently reinterpreted.

## Offline development metrics

During M003 use train/dev only:

- ranking MRR/Recall@1/3;
- counterfactual-pair accuracy;
- unknown-name dev slice if available without touching frozen test transforms;
- no-tool precision/recall/F1;
- Brier/ECE/NLL;
- loss curves;
- overfit gap;
- load/forward latency;
- artifact/RSS.

Compare pairwise versus packed on both quality and candidate-count scaling.

## Stop rules

Do not proceed to M004 with a model that:

- cannot beat `hashed-linear-v1` on at least one predeclared contextual dev slice;
- collapses no-tool detection;
- requires non-Rust runtime services;
- cannot load from explicit local assets;
- violates reasonable local memory/latency bounds.

A negative M003 result is valid and blocks downstream work.

## Verification

- numerical/logit sanity tests;
- frozen/head-only/top-layer training tests;
- artifact roundtrip/hash rejection;
- tokenizer determinism;
- default feature isolation;
- CPU inference tests;
- selected accelerated-backend parity test;
- full repository verification.

## Acceptance

M003 closes with a selected experimental encoder/ranking variant **or** an explicit negative result. M004 proceeds only if at least one variant shows credible dev signal and acceptable local resource behavior.
