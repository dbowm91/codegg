# Tool-Selection Advisor Sequence-Encoder Experiment M001A — Reference Checkpoint Materialization and Qualification

Status: implemented

Repository baseline: `1d9f4b9952083954145d6b2459b0b6717c2375c3`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#m001a--reference-checkpoint-materialization-and-qualification`

Predecessor closure:

- `plans/closure/tool-selection-advisor-sequence-encoder-experiment/002-status.md` — M001 conditionally closed; Candle 0.11 selected, real pretrained reference asset outstanding.

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: operational evidence / experiment prerequisite.

## 1. Objective

Close the only remaining M001 condition with one real, provenance-qualified pretrained BERT-compatible checkpoint, without adding model weights to the CodeGG repository or introducing an automatic model-download path.

M003 must remain blocked until this plan closes positively.

## 2. Why this is separate work

The M001 implementation already proved:

- Candle 0.11 local BERT loading;
- deterministic WordPiece pair encoding;
- deterministic forward execution;
- head-only backward/optimizer step;
- top-layer backward/optimizer step;
- hash/tamper rejection;
- default-build feature isolation.

Those tests used a generated tiny BERT fixture. They prove the machinery, but not compatibility with a real pretrained semantic checkpoint.

Do not reimplement those mechanisms. This plan supplies and qualifies the missing real asset.

## 3. Primary reference checkpoint

Use the following first reference unless the pinned revision becomes unavailable:

- repository: `sentence-transformers/all-MiniLM-L6-v2`
- revision: `1110a243fdf4706b3f48f1d95db1a4f5529b4d41`
- architecture: BERT-compatible MiniLM, 6 layers, hidden size 384, 12 attention heads;
- expected parameter class: approximately 22M;
- license metadata: Apache-2.0;
- required files:
  - `config.json`;
  - `model.safetensors`;
  - `vocab.txt`;
  - `tokenizer_config.json` and/or `special_tokens_map.json` for provenance validation;
  - upstream `README.md`/license metadata captured into a local provenance record.

Rationale:

- the checkpoint has native safetensors, avoiding pickle conversion;
- it uses a 30,522-token WordPiece vocabulary compatible with the existing M001 tokenizer contract;
- it is already trained for semantic sentence similarity, making it a stronger first semantic reference than an unlicensed/ambiguous TinyBERT mirror;
- it remains inside the intended small-model range.

Do not substitute a fine-tuned safety/classification fork merely because it provides safetensors.

## 4. Asset materialization boundary

Model files are **not committed to Git**.

Materialize under an ignored operator/training path such as:

```text
target/tool-advisor/reference-assets/all-minilm-l6-v2/
```

Acquisition is an explicit implementation/experiment action, not runtime behavior. The normal CodeGG binary and ordinary advisor features must gain no Hub/HTTP/model-fetch path from this plan.

Acceptable acquisition methods for the implementation operator include:

- exact-revision HTTPS download;
- Hugging Face CLI outside CodeGG;
- a pre-existing local snapshot supplied by the operator.

Regardless of acquisition method, CodeGG only consumes the completed local files through the existing manifest.

## 5. Repository-owned reference receipt

Add a small text/JSON receipt under `assets/tool-advisor/reference-models/` that contains **metadata only**, not weights:

- upstream repository;
- immutable upstream revision;
- source file names;
- source URLs or canonical repository reference;
- upstream license identifier;
- model-card/config facts needed for compatibility;
- SHA-256 for each locally materialized required file;
- total bytes;
- acquisition date;
- CodeGG asset-manifest schema/framework/architecture values;
- generated local asset-manifest fingerprint.

If exact file hashes cannot be known until materialization, the implementation commit may add them after the files are downloaded/probed. The receipt is the reproducible evidence artifact; the large weights remain ignored.

## 6. Compatibility validation before probe

Before constructing Candle tensors, validate the real config against the current implementation:

- `model_type == "bert"` when present;
- hidden size 384;
- 6 layers;
- 12 heads;
- vocabulary size 30,522;
- type vocab size 2;
- position embeddings <=/compatible with 512;
- hidden activation supported by Candle BERT;
- tensor names in `model.safetensors` load completely into the Candle `BertModel` VarMap.

Fail closed on missing/unexpected required encoder tensors. Do not silently leave encoder weights randomly initialized.

Add a load-coverage report:

- expected model variables;
- loaded variables;
- missing variables;
- unexpected source tensors;
- loaded parameter count;
- source safetensors parameter count when derivable.

Positive closure requires zero missing encoder variables.

## 7. Real-checkpoint probe

Run the existing `sequence-encoder-probe` against the materialized checkpoint.

Required evidence:

1. local manifest hash validation passes;
2. parameter count is in the expected MiniLM range;
3. repeated forward max delta is zero or within a documented deterministic floating-point tolerance;
4. embeddings are finite;
5. head-only training step returns finite loss and changes only head variables;
6. top-one-layer training step returns finite loss and changes head + selected top-layer variables;
7. non-selected encoder layers remain bitwise/approximately unchanged according to the framework representation;
8. two semantically related task/tool pairs and one obviously unrelated pair yield finite distinct representations/scores before CodeGG fine-tuning;
9. artifact/RSS/load/forward/backward timings are recorded on the available target;
10. all default/no-feature builds remain independent of the checkpoint.

This milestone does not require the pretrained checkpoint to beat CodeGG baselines; that is M003.

## 8. Pooling sanity probe

`all-MiniLM-L6-v2` was trained in a sentence-transformers stack using mean pooling. The current M001 encoder exposes the first-token representation.

Before M003, compare on a small **train/dev-only semantic sanity set**:

- first-token/[CLS] representation;
- attention-mask-aware mean pooling over token embeddings.

Do not inspect frozen final-test/family-holdout labels.

Record cosine/ranking separation for obvious related/unrelated descriptor pairs. If mean pooling is materially more coherent, expose pooling strategy as an explicit experiment parameter for M003 rather than hard-coding CLS.

## 9. TinyBERT disposition

Do not block M003 on a second capacity point.

The Huawei `TinyBERT_General_4L_312D` Hub repository currently exposes PyTorch/Flax weights rather than native safetensors and does not provide sufficiently clear model-card license metadata for this immediate qualification path.

TinyBERT or another 10–15M reference may be added later only if:

- provenance/license is explicit;
- a native safetensors source is available or a reproducible local conversion is separately justified;
- the 22M MiniLM reference demonstrates enough signal to make a second capacity point worthwhile.

## 10. M001/M003 state transition

On positive closure:

- add supplemental M001 closure evidence rather than rewriting `002-status.md`;
- change M001 from `conditionally closed` to `closed`;
- change M003 from `blocked` to `ready`;
- leave M004/M005 blocked on their existing dependencies.

On negative closure:

- if Candle cannot fully load the pinned real checkpoint, keep M003 blocked and register a narrow loader/framework corrective;
- if asset provenance/license cannot be established, choose another explicit reference asset rather than weakening the manifest contract;
- do not fall back to generated/random weights and call M003 attempted.

## 11. Verification

Expected minimum:

```bash
cargo check --locked
cargo check --locked --features tool-advisor-encoder-experiment
cargo check --locked --features tool-advisor-encoder-training
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib sequence_encoder
cargo run --locked --features tool-advisor-encoder-experiment -- \
  tool-advisor sequence-encoder-probe \
  --manifest target/tool-advisor/reference-assets/all-minilm-l6-v2/manifest.json \
  --json
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Capture the real-checkpoint probe JSON as closure evidence or summarize all values with the manifest/receipt hashes.

## 12. Acceptance

M001A closes positively only when one real pretrained MiniLM-class checkpoint is locally materialized, provenance/license/hashes are recorded, Candle loads every required encoder variable, staged backward probes are finite and correctly scoped, pooling behavior is characterized, and default CodeGG remains model-free.

That positive closure is the sole condition needed to mark M003 ready.
