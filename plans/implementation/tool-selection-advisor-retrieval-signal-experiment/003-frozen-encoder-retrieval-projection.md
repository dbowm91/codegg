# Tool-Selection Advisor Retrieval-Signal Experiment M003 — Frozen-Encoder Retrieval Projection

Status: blocked/conditional on M002

Repository baseline: `1093ad0e3285e8ee66684e8a7f3401a200c0596e`

Hard dependencies:

- M001 positive inferability/preregistration;
- M002 valid negative result (deterministic Signal V2 did not clear the gates).

Source roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m003--frozen-encoder-learned-retrieval-projection`

Primary class: model experiment.

## 1. Objective

Train the smallest useful retrieval-specific alignment layer on top of the existing frozen MiniLM embeddings.

Do not fine-tune the transformer in this milestone.

## 2. Architecture

Implement only the M001-preregistered projection families.

Preferred candidates:

- shared linear 384→128;
- asymmetric query/descriptor linear 384→128;
- asymmetric 2-layer MLP 384→128→128.

Normalize projected embeddings before cosine/dot scoring as preregistered.

Trainable parameters must remain <=500k.

Artifact records:

- projection architecture;
- dimensions;
- parameter count;
- MiniLM manifest/tokenizer/weights hashes;
- Retrieval Signal V2 schema hash;
- training partition fingerprint;
- objective/version;
- calibration/temperature;
- weights hash.

## 3. Training data

Optimizer input:

- clean historical train partition only;
- M001-approved inferable relevance labels;
- deterministic train-only unknown/renamed variants if preregistered.

No dev/test/v2/v3 examples in optimization.

Each positive pair is:

```text
RetrievalQueryV2(context) <-> RetrievalDescriptorV2(relevant tool)
```

## 4. Loss

Use the M001-frozen objective.

Expected shape:

- contrastive/in-batch softmax;
- graded positive weighting;
- explicit hard-negative term;
- optional margin for same-family distractors.

No-tool cases may train a query-norm/no-match auxiliary only if preregistered; otherwise omit from projection optimization and leave abstention to the ranker.

Do not conflate retrieval confidence with downstream abstention.

## 5. Hard-negative mining

Mine on train only using frozen pre-M003 scorers:

- current BM25;
- deterministic Signal V2 semantic;
- same-family candidates.

Freeze mined negative identities before optimizer runs so training is reproducible.

Never mine negatives from v3 or future v4.

## 6. Dev selection

Evaluate every preregistered arm on:

- 64/128/256-tool dev universes;
- K16/24/32;
- unknown/renamed dev slice;
- tool-family slices;
- primary versus secondary relevance;
- persistent-miss cases;
- no-tool candidate retrieval diagnostics;
- latency/RSS.

Primary selection gates:

- 64 recall >=0.99;
- 128 recall >=0.98;
- 256 recall >=0.95;
- K<=32;
- zero authority violations.

If several clear, choose smallest projection/lowest K/lowest latency.

## 7. Generalization guards

A projection is ineligible if:

- unknown/renamed MRR or recall regresses >0.02 versus deterministic Signal V2;
- any declared tool-family slice regresses >0.03 without compensating aggregate gate necessity;
- candidate identity/order changes score after canonical remapping;
- performance depends on canonical tool-name memorization rather than description signal.

Add a name-masking diagnostic:

- replace canonical tool names with stable synthetic identities while keeping descriptions;
- the projection must retain meaningful retrieval above lexical baseline.

This is diagnostic/gating as frozen in M001.

## 8. V3 handling

After selecting one artifact on dev, v3 may be evaluated once diagnostically.

No training/grid/gate/K change may follow.

## 9. Resource contract

Because MiniLM is already loaded for the downstream ranker, report:

- projection artifact bytes;
- incremental RSS;
- incremental query projection latency;
- incremental descriptor projection/cache build latency.

Do not count the shared MiniLM weights twice when describing incremental deployment cost.

## 10. Stop conditions

Close negatively if:

- no preregistered arm clears retrieval gates;
- gains disappear under unknown/name-masked diagnostics;
- the only successful variant requires transformer fine-tuning;
- projection latency materially breaks the existing turn-side envelope.

Transformer fine-tuning would require a new plan.

## 11. Verification

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor
scripts/verify.sh quick
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## 12. Acceptance

Positive M003 closure freezes one retrieval-projection artifact and makes M004 ready. Negative closure stops this experiment before v4.
