# Tool-Selection Advisor Sequence-Encoder Experiment M001 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/002-rust-sequence-encoder-framework-asset-spike.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#m001--rust-sequence-encoder-framework-and-local-asset-spike`

Repository baseline reviewed: `31d16ad`

Implementation commits:

- `31d16ad` — advisor: add Candle sequence encoder spike

## 1. Executive finding

M001 selected Candle 0.11.0 as the experiment-only Rust sequence-encoder
framework and landed a verified local-asset loader, deterministic WordPiece
pair projection, BERT forward path, scalar ranking head, and staged
head/top-layer optimizer probes. The repository contains no TinyBERT- or
MiniLM-class pretrained files, so the milestone is conditionally closed until
an operator supplies and hashes at least one real reference checkpoint. M003
must remain blocked on that condition; no production advisor feature is
enabled.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Candle local BERT loading | `src/tool_advisor/sequence_encoder.rs` + tiny fixture | pass | Config, vocabulary, safetensors, and license/provenance are explicit local inputs. |
| Deterministic tokenization/forward | `tiny_local_bert_proves_forward_and_staged_backward` | pass | Repeated forward max delta is `0.0`. |
| Head-only backward | same fixture test and `probe_local_assets` | pass | Finite loss and optimizer step. |
| Top-one-layer backward | same fixture test and `probe_local_assets` | pass | VarMap layer selection is explicit and finite. |
| Artifact/hash/tamper handling | `local_asset_hashes_are_required` | pass | Modified vocabulary is rejected before model load. |
| CLI/local-path boundary | `tool-advisor sequence-encoder-probe` | pass | No HTTP/Hub/download path exists. |
| Burn comparison | architecture record | partial | Not added; Candle met the required experimental contract and Burn was not justified as a second dependency. |
| TinyBERT/MiniLM reference qualification | asset inventory and architecture record | not available | No redistributable reference files exist in this checkout. This is the explicit condition on M003. |
| Default-build isolation | `scripts/verify.sh quick` and feature declarations | pass | Candle dependencies are optional and feature-gated. |

## 3. Production implementation evidence

The `tool-advisor-encoder-experiment` feature owns Candle core, neural-network,
and transformer dependencies; `tool-advisor-encoder-training` additionally
selects the existing training feature. `AssetManifest` requires exact hashes
for config, vocabulary, weights, and license/provenance metadata. The loader
resolves only paths relative to the manifest and fails closed on hash or
framework/architecture mismatch.

The BERT model is created through a Candle `VarMap`, allowing the experiment to
load local safetensors and choose head-only, top-layer, or full optimizer
parameter sets. The CLI probe emits architecture, dimensions, parameter count,
determinism, losses, and weight hash as JSON or concise text.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all
rtk cargo check --locked --features tool-advisor-encoder-experiment
rtk cargo check --locked --features tool-advisor-encoder-training
rtk cargo test --locked --features tool-advisor-encoder-training -p codegg --lib sequence_encoder
rtk scripts/verify.sh quick
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk git diff --check
```

### Results

- Both locked feature checks passed.
- Three sequence-encoder tests passed, including actual tiny local BERT forward and staged backward.
- `scripts/verify.sh quick` passed.
- All-feature Clippy passed with `-D warnings`.
- Diff check passed.
- No Apple accelerated backend was claimed; this host's CPU fixture is the measured platform evidence.

## 5. Invariant review

- Default CodeGG remains independent of model weights, tokenizer assets, Candle, and network download.
- Runtime inference and training are in-process Rust only.
- Assets are explicit, hashed, local inputs with license/provenance metadata.
- The sequence encoder only produces embeddings/scores; it has no policy, permission, execution, or argument-construction authority.
- No fixed tool classifier IDs were introduced; candidates remain textual descriptors.

## 6. Failure and recovery review

Missing files, unsupported schema/framework, malformed config, and hash drift
fail closed. No mutable persistent cache or restart protocol was introduced.
Training probes use an explicit optimizer parameter selection and do not
silently fall back from a requested top-layer/full stage.

## 7. Migration and compatibility review

The feature is additive and default-off. Existing `tool-advisor` and
`tool-advisor-training` behavior is unchanged. No storage, protocol, or model
artifact migration is needed because no production sequence artifact was
created.

## 8. Security review

The loader performs no network access and accepts only operator-specified
paths. Hashes and license metadata are checked before loading weights. The
experiment remains outside authority and does not widen the resolved tool
surface.

## 9. Documentation and operations

Updated `architecture/tool-advisor-framework-spike.md` with dependency
versions, licenses, measured fixture results, asset condition, and probe
command. The CLI is available only with the explicit experiment feature.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | No TinyBERT/MiniLM-class pretrained assets are present locally | M003 cannot honestly compare a real pretrained reference encoder | Supply an explicit local manifest with source-weight/config/tokenizer hashes and license metadata before unblocking M003. |
| low | Burn was not independently built/probed | No impact to selected Candle experiment; comparative framework evidence is narrower | Revisit only if Candle fails on real reference assets or a future plan explicitly requires Burn. |

## 11. Roadmap disposition

M001 is conditionally closed with Candle selected for the experimental stack.
M002 is independently ready and is unblocked. M003 remains blocked because
the hard M001 reference-asset evidence condition is not satisfied, even though
the framework API itself is available. M004 and M005 remain blocked downstream.

## 12. Registry updates

- M001 moved from active implementation to conditionally closed history.
- M002 remains dependency-ready.
- M003's blocker was refined to name the missing local pretrained-asset evidence.
- No plan was silently unblocked past that condition.
