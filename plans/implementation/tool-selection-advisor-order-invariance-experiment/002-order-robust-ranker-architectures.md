# Tool-Selection Advisor Order-Invariance Experiment M002 — Order-Robust Ranker Architectures

Status: active

Repository baseline: `79045a4c683151e06d1f998b080f41c7c8e84567`

Hard dependency:

- M001 candidate-order diagnostics/permutation contract — positive closure.

Source roadmap:

- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md#m002--order-robust-ranker-architectures`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: experimental infrastructure.

## 1. Objective

Implement ranker variants whose semantics are not tied to candidate-list ordinal identity and prove their permutation behavior before training selection.

## 2. Required variants

### A. Current distinct-marker packed control

Retain `sequence-encoder-packed-marker-v1` only as a negative/control arm.

No new qualification can select it merely because historical dev metrics are high.

### B. Shared-marker packed

Use the same marker token identity before every candidate descriptor.

Requirements:

- one reserved existing vocabulary token, e.g. `[unused0]`;
- no candidate index encoded by marker token choice;
- marker positions still recorded for candidate identity mapping;
- explicit artifact architecture id distinct from v1.

This removes marker-token ordinal leakage but not absolute-position information.

### C. Shared-marker descriptor-span pooled packed

For every candidate, record descriptor token span and build candidate representation from that span, preferably attention-mask-aware mean pooling, instead of marker hidden state alone.

Requirements:

- shared marker identity;
- descriptor span start/end in `PackedEncoding`;
- no candidate-index-specific learned embedding added outside BERT;
- deterministic dropped-candidate accounting.

### D. Batched pairwise cross-encoder

Build one context+descriptor pair per candidate:

```text
[CLS] AdvisorContextV2 [SEP] descriptor [SEP]
```

Pad rows into one batch and run one encoder batch forward where supported.

Properties:

- candidate score depends on context+descriptor, not list position;
- permutation changes batch row order only;
- mapped-back per-candidate scores should be invariant within floating tolerance;
- preserve one separate context representation or explicit context head for abstention.

If Candle implementation constraints require multiple forwards, keep semantics identical and report the cost; do not weaken the invariance contract.

## 3. Architecture property tests

Before any training:

- permute identical candidate sets;
- map outputs back by candidate identity;
- batched pairwise must show exact/numerically bounded score invariance;
- shared-marker variants report their residual score drift caused by absolute positions;
- distinct-marker control should remain available to demonstrate the old failure mode.

Required test cases:

- 2 candidates;
- 5 candidates;
- 16 candidates;
- duplicate-like descriptions with distinct names;
- unknown synthetic identity;
- long descriptors near token budget;
- candidate truncation/dropped descriptors.

## 4. Token-budget semantics

Packed variants must not silently change which candidate is scoreable as a function of arbitrary order without reporting it.

For each packed architecture:

- record descriptor token lengths;
- report dropped candidates;
- add a permutation test around the max token budget;
- if permutation can cause different candidates to be dropped, either:
  - reject the packed architecture for selection; or
  - introduce a deterministic order-independent pre-budget truncation policy based on descriptor identity/length before encoding.

Do not make relevance labels influence truncation.

## 5. Artifact versioning

New architecture manifests record:

- architecture id;
- marker strategy;
- candidate representation strategy;
- batching strategy;
- pooling;
- max candidates/tokens;
- encoder/tokenizer/source hashes;
- permutation-contract version;
- later training objective version.

Historical v1 artifacts continue to load unchanged.

## 6. Runtime isolation

All variants remain under experimental advisor features. No default build or production/live surface changes.

## 7. Resource probe

Measure untrained/inference-only architecture cost on representative 4/8/16-candidate cases:

- forwards;
- total tokens;
- wall latency p50/p95;
- RSS delta;
- batch tensor size.

Do not select architecture on resources alone; M003 owns quality selection.

## 8. Regression tests

At minimum:

- shared marker really uses one token id for every candidate;
- descriptor spans map back to correct candidate;
- batched pairwise mapped scores invariant under random permutations;
- no candidate identity lost across padding;
- packed truncation policy is deterministic/order-independent or variant fails closed for selection;
- historical packed v1 remains loadable.

## 9. Stop conditions

Block M003 if:

- no architecture can satisfy the M001 permutation contract;
- pairwise batching is numerically/correctness unstable;
- packed variants cannot prevent order-dependent candidate loss under the token budget;
- implementation would require a non-Rust sidecar/service.

## 10. Verification

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor::sequence_encoder
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor::sequence_ranking
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

## 11. Acceptance

M002 closes when at least one order-robust candidate architecture is implemented, artifact-versioned, and passes architecture-level permutation/truncation correctness. M003 then becomes ready.
