# Tool-Selection Advisor Sequence-Encoder Experiment M001 — Rust Sequence-Encoder Framework and Local-Asset Spike

Status: implemented

Repository baseline: `8487967d9cc2605c398d3c608e353c8871289a7d`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#m001--rust-sequence-encoder-framework-and-local-asset-spike`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: infrastructure/experiment.

## Objective

Select an experimental pure-Rust BERT-class train/infer stack and prove local asset compatibility before model behavior work begins.

Do not add the chosen framework to the normal CodeGG dependency path yet.

## Candidate stacks

### Candle

Qualify:

- `candle-core`, `candle-nn`, and BERT support from `candle-transformers`;
- safetensors loading;
- WordPiece/tokenizer integration;
- CPU path;
- macOS Accelerate and Metal where available;
- autograd for head-only and partial-unfreeze fine-tuning;
- release binary delta.

Candle currently has direct BERT support and training examples, which lowers implementation distance.

### Burn

Qualify:

- CPU backend;
- Apple Metal backend;
- safetensors import;
- transformer construction/import path;
- autograd for frozen-encoder/head-only and top-layer unfreeze;
- release binary delta.

Burn has attractive unified train/infer and Metal support, but the spike MUST explicitly reproduce or rule out the recent params-only Metal backward NaN class before selecting it for CodeGG's staged fine-tuning.

### Repository-local transformer

Do not build a new tensor/autograd framework in CodeGG merely to avoid a dependency. A custom tiny transformer is considered only if both established Rust frameworks fail the ADR's packaging/runtime gates and a separate plan justifies the ownership burden.

## Reference assets

Use explicit local assets only. The spike should support at least two reference points if licenses/files are available:

- TinyBERT-class 4L/312D, approximately 14.4M parameters;
- MiniLM/XtremeDistil L6/H384, approximately 22M parameters.

The command accepts local paths to:

- config;
- tokenizer vocabulary/config;
- safetensors weights;
- license/provenance metadata.

No implicit Hugging Face Hub or network download is allowed.

## Required probes

For each viable framework/model combination:

1. load local config/tokenizer/safetensors;
2. tokenize a fixed CodeGG task/tool pair deterministically;
3. reproduce deterministic forward output across two runs;
4. run a learned scalar ranking head;
5. run one head-only backward/optimizer step;
6. run one top-transformer-layer-unfrozen backward/optimizer step;
7. verify loss direction on a tiny synthetic separable fixture;
8. save/reload a CodeGG-owned artifact or framework-native checkpoint with explicit manifest;
9. measure CPU load/forward/backward time and RSS;
10. on Apple Silicon, test available accelerated backend and compare gradients/loss against CPU within tolerance;
11. build Linux/macOS default CodeGG without the experiment feature and prove zero new runtime dependency;
12. inspect license/redistribution obligations for framework, tokenizer, and reference assets.

## Feature boundary

Prefer experimental features such as:

- `tool-advisor-encoder-experiment`;
- `tool-advisor-encoder-training`.

They must not be implied by default `tool-advisor` until M005 qualifies a production candidate.

## Selection criteria

Select the stack with the best combination of:

- correct autograd under staged freezing;
- local pretrained-asset compatibility;
- CPU portability;
- Apple Silicon training viability;
- small/isolated dependency graph;
- binary-size impact;
- deterministic artifact handling;
- maintainability.

Performance alone cannot override gradient correctness or ADR locality.

If neither framework qualifies, close M001 with a negative decision and block M003.

## Deliverable

Add/update `architecture/tool-advisor-framework-spike.md` with a new dated sequence-encoder section that records measured results and explicitly supersedes the old "selected" status for experimental purposes without erasing history.

## Verification

At minimum:

```bash
cargo check --locked
cargo check --locked --features tool-advisor-encoder-experiment
cargo check --locked --features tool-advisor-encoder-training
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Plus platform probe output on the actual available macOS/Apple Silicon target when available.

## Acceptance

M001 closes only with one explicit framework decision, exact dependency versions/features, local asset hashes/licenses, forward/backward correctness evidence, and default-build isolation.
