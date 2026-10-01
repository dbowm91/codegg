# Tool-Selection Advisor Late-Interaction Retriever M002 — Token Projection Training

Status: blocked/conditional on M001 disposition B — TERMINAL: M001 closed
with disposition D (no architectural signal), so M002 never opens. No
training occurs in this workstream.

Repository baseline: `c4cc6c5b3c9154428b8560a60aa0129624435dfe`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-late-interaction-retriever-experiment-roadmap.md#m002--learned-token-projection`

Hard dependency:

- M001 closure disposition B — useful frozen late-interaction signal but retrieval gates not yet cleared.

Primary class: model experiment.

## 1. Objective

Learn a small projection on **token vectors before MaxSim** while keeping MiniLM frozen.

This is not a retry of retrieval-signal M003. That experiment projected already-pooled 384-d vectors. M002 preserves token multiplicity through relevance scoring.

## 2. Allowed architectures

Only linear token projections:

- shared 384→128 projection;
- asymmetric query 384→128 + descriptor 384→128 projections.

No MLP arm. The prior pooled 2-layer MLP collapse is sufficient evidence not to widen this experiment without a separate preregistration.

Each projected token is L2-normalized before MaxSim.

Maximum trainable parameters: 100,000 unless exact bias accounting requires a trivially larger bound; the prereg receipt must record the exact count.

## 3. Preregistration

Before optimizer runs, freeze in a separate commit:

- architecture arms;
- initialization;
- learning rates;
- epochs;
- temperatures;
- seeds;
- hard-negative counts;
- batch size;
- grade weights;
- optimizer/weight decay;
- early-stop rule if any;
- generalization gates;
- selection tie-break.

The grid must be finite and small. Do not expand it after dev results.

## 4. Training data

Use only:

- historical clean train partition;
- corrected inferable retrieval relevance view;
- current-step/explicit-next-step positives;
- M001-frozen query/descriptor text and token caps.

Exclude implicit-future labels from retrieval optimization.

No dev/test/v2/v3/future-v4 examples in optimization.

## 5. Objective

Use a token-level late-interaction contrastive objective.

For each query:

- score positive descriptors with projected MaxSim;
- use in-batch negatives;
- add preregistered hard negatives;
- weight positives by original relevance grade where multiple inferable positives exist.

Hard negatives are frozen before training and may come from:

- M001 exact-MaxSim nearest wrong candidates;
- Signal V2 field-weighted lexical nearest wrong candidates;
- same-family distractors.

No no-tool abstention objective here; downstream ranker owns abstention.

## 6. Dev evaluation

For every preregistered arm:

- universes 64/128/256;
- K16/24/32;
- corrected inferable target;
- unknown/renamed;
- canonical-name masking;
- family slices;
- paraphrase/no-exact-verb slice;
- resource evidence.

Primary qualification gates remain:

- u64 >=0.99;
- u128 >=0.98;
- u256 >=0.95;
- K<=32;
- zero authority violations.

## 7. Generalization guards

An arm is ineligible if any are true:

- unknown/renamed recall >0.02 below M001 frozen MaxSim baseline;
- canonical-name-masked recall >0.02 below the matched lexical baseline;
- any required family recall >0.03 below M001;
- the arm gains only on canonical names while losing description-only signal;
- candidate order changes mapped scores beyond f32 tolerance.

## 8. Disposition

### A — qualifies retrieval

Freeze the smallest qualifying projection artifact and make M004 ready.

Selection order among qualifying arms:

1. smaller trainable parameter count;
2. smaller K;
3. lower p95 retrieval latency;
4. lower cache bytes;
5. deterministic architecture/grid-order tiebreak.

### B — near-gate, generalization clean

M003 may become ready only if:

- best arm reaches >=52/53 at u64 and u128;
- reaches >=51/53 at u256;
- all generalization guards pass;
- at least one M001 residual miss improves without a family regression;
- resources stay inside M001 bounds.

Freeze that arm only as a **research initialization artifact**, explicitly not production-eligible.

### D — negative

If neither A nor B, close the workstream negative. Do not fine-tune MiniLM.

## 9. Artifact

If A or B:

- projection architecture/dims;
- weights;
- exact MiniLM hashes;
- M001 contract fingerprint;
- corrected relevance fingerprint;
- train split fingerprint;
- hard-negative fingerprint;
- optimizer/grid point;
- parameter count;
- weights SHA-256;
- disposition.

## 10. OOD diagnostic

If licensing/provenance is clean and no network/runtime dependency is introduced, a small external tool-retrieval diagnostic derived from ToolRet may be run **after dev selection**.

It is non-gating and cannot change the selected arm.

Do not import external data into training in this workstream.

## 11. Verification

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

## 12. Acceptance

M002 closes with A/B/D and a complete finite-grid receipt.

A opens M004. B alone may open M003. D terminates the architecture line.
