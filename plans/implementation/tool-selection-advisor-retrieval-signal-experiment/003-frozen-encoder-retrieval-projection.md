# Tool-Selection Advisor Retrieval-Signal Experiment M003 — Frozen-Encoder Retrieval Projection

Status: active

Repository baseline: `99af426f`

Hard dependencies:

- M001 positive current-step inferability/preregistration re-audit, recorded by M006 (`plans/closure/tool-selection-advisor-retrieval-signal-experiment/006-status.md`); the original blocked M001 closure remains historical evidence.
- M002 valid negative result (deterministic Signal V2 did not clear the gates), formally closed at `plans/closure/tool-selection-advisor-retrieval-signal-experiment/002-completion-status.md`.
- Positive M011 train-partition inferability audit and optimizer-input freeze, corrected and closed by M012: `plans/closure/tool-selection-advisor-retrieval-signal-experiment/012-status.md`; receipt `assets/tool-advisor/retrieval-signal-m003-train-audit.json`. M006 re-audited only the M001 dev gate-critical miss set, not M003's optimizer labels.

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

## 10a. Frozen realization choices before the sweep

The M001 receipt freezes the model grid and loss weights, while this plan
leaves a few deterministic realization details open. Before looking at any
M003 development result, freeze these details in
`projection_contract()` in `src/tool_advisor/retrieval_signal_m003.rs`; its
SHA-256 is copied to the receipt.

- Encode `RetrievalQueryV2::flat_text` and `RetrievalDescriptorV2::flat_text`
  with the pinned, frozen MiniLM and mean pooling; L2-normalize projected
  vectors before cosine scoring.
- Initialize projection weights with the preregistered seed using a
  deterministic xorshift uniform distribution bounded by `sqrt(6/fan_in)`;
  initialize biases to zero. Use AdamW with zero weight decay, train-partition
  order, seeded Fisher-Yates order per epoch, and exactly five epochs.
- Use in-batch multi-positive log-softmax. Rows sharing the same relevant tool
  identity are positives; grade 3 has weight 1.0 and grade 2 has weight 0.5.
- Mine seven distinct hard-negative identities from the union of candidate
  identities in the filtered historical train partition. Combine current
  catalog BM25 rank and frozen MiniLM cosine rank by rank sum, reserving the
  best same-family candidate when one exists. Freeze the identity list and its
  hash before fitting.
- The hard-negative term is the mean `relu(0.2 - positive_cosine +
  negative_cosine)` at weight 0.5. Apply the same hinge to selected same-family
  negatives at weight 0.1. No-tool rows remain outside projection training.
- A candidate is eligible only if the frozen recall gates and zero-authority
  requirement pass, unknown/renamed MRR is within 0.02 of the frozen-embedding
  baseline, family recall is within 0.03, and name-masked recall exceeds the
  description-only lexical baseline. Choose the lowest parameter count, then
  lowest passing K, then lowest measured projection latency.

These are implementation details of the declared loss/grid, not new search
dimensions. The contract hash and all 81 outcomes must be committed before any
v3 diagnostic is opened.

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
