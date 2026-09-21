# Tool-advisor contextual runtime framework spike

This document records two distinct decisions. The historical M002 decision
selected the repository-local hashed contextual scorer for the bounded
corrective. C004 later demoted `contextual-embedding-v2` to a research/observe
baseline after clean offline qualification found no useful gain. There is no
currently qualified learned architecture; the sequence-encoder experiment is
an explicitly separate research track governed by ADR-0009.

The M002 spike compared the plausible pure-Rust choices against the bounded
advisor contract: CPU inference, local autograd/training, auditable local
weights, tokenizer control, feature isolation, deterministic load parity,
quantization options, native-library footprint, and redistribution terms.

| Option | Result | Decision |
|---|---|---|
| Candle | Strong tensor/inference surface, but introducing a tensor/native feature graph for this small advisory scorer would make training/runtime isolation and redistribution evidence larger than the capability. | Not selected for this bounded corrective. |
| Burn | Good training abstractions, but the backend and artifact choices add a second framework contract and do not improve the repository’s deterministic CPU-only deployment boundary enough to justify it here. | Not selected for this bounded corrective. |
| Tract | Useful inference path, but it is not the local Rust training owner required by M002. | Rejected as the sole stack. |
| Repository-local contextual embedding runtime | Pure Rust, no download/native sidecar, deterministic binary artifact, explicit tokenizer/hash, and the same code owns training and inference. | Selected for M002. |

The historical M002-selected runtime was a compact hashed-token embedding encoder with a learned
context/candidate interaction score. Small and medium capacity points are
5,242,881 and 15,728,641 parameters. This is an explicit local model family,
not a relabeling of hashed-linear-v1. It supports unseen tool names by scoring
bounded textual descriptors and remains advisory-only.

The runtime is compiled only with the optional tool-advisor feature. Training
commands additionally require tool-advisor-training. The default build keeps
the existing linear artifact path and does not contain model weights or a
download path.

## Qualification protocol provenance

Future model qualification manifests may use schema version 2. The canonical
protocol hash covers the complete manifest with `protocol_hash` and the
operator-supplied `preregistration_commit_sha` removed. The harness verifies
that hash before loading data or inspecting final-test labels, and reports the
dataset/split fingerprints, model/config hashes, declared gate identifiers,
protocol hash, and preregistration commit provenance unchanged. Positive
qualification requires two commits: a frozen preregistration commit that
passes verification, followed by a separate evaluation/closure commit.

The historical C004 schema remains loadable for reproducibility and is not
retroactively rewritten.

## Sequence-encoder experiment status

The sequence-encoder workstream is recorded in
`plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md`.
It must select and qualify an actual pure-Rust pretrained sequence encoder
before any production claim is made; the historical hashed scorer is not
silently reinterpreted as that architecture.

## Sequence-encoder asset spike — 2026-09-21

M001 selected Candle 0.11.0 for the experiment-only stack. The exact optional
dependencies are `candle-core = 0.11.0`, `candle-nn = 0.11.0`, and
`candle-transformers = 0.11.0`, all with default features disabled. The
experiment features are `tool-advisor-encoder-experiment` and
`tool-advisor-encoder-training`; neither is enabled by the default build.
Candle is MIT OR Apache-2.0. Asset manifests require explicit config,
WordPiece vocabulary, safetensors weights, and license/provenance files with
SHA-256 hashes. The probe has no Hub, HTTP, or implicit download path.

The local spike verified the following on macOS CPU using a generated tiny
two-layer BERT fixture (8 hidden dimensions, 16 vocabulary slots):

| Probe | Result |
|---|---|
| local config/vocabulary/weights/license manifest | pass; all four hashes are checked before load |
| deterministic WordPiece pair encoding | pass; bounded `[CLS] context [SEP] candidate [SEP]` layout |
| repeated forward output | pass; maximum delta `0.0` |
| head-only backward/optimizer step | pass; finite loss |
| top-one-layer backward/optimizer step | pass; finite loss |
| artifact/manifest tamper rejection | pass; changed vocabulary is rejected |
| default build isolation | pass; Candle appears only behind the two experiment features |

The repository does not contain redistributable TinyBERT or MiniLM-class
pretrained files. The probe accepts those assets when an operator supplies a
local manifest and records their exact source/license hashes; no reference
checkpoint was silently downloaded or committed by this spike. Burn was not
selected for a parallel implementation because Candle already met the
required local loading and staged-autograd contract with a smaller change
surface; a Burn comparison remains a research follow-up, not a production
dependency.

Run the probe with:

```text
cargo run --features tool-advisor-encoder-experiment -- tool-advisor sequence-encoder-probe --manifest /path/to/manifest.json --json
```

This is an experiment/infrastructure decision only. It does not qualify a
pretrained model, enable advisor ranking, or unblock M003 until M002's context
contract and real local reference-asset evidence are available.

## Sequence-encoder reference qualification — 2026-09-21 (M001A)

The pinned `sentence-transformers/all-MiniLM-L6-v2@1110a24…` checkpoint
(Apache-2.0) is materialized under the ignored
`target/tool-advisor/reference-assets/all-minilm-l6-v2/` path with a
metadata-only receipt at
`assets/tool-advisor/reference-models/all-minilm-l6-v2.json` (file
hashes, 91,102,259 total bytes, manifest fingerprint). The probe reports:
101/101 expected variables loaded with zero missing (pooler + position
ids correctly ignored; 22,565,376 loaded of 22,713,728 source params),
repeated-forward max delta `0.0`, finite embeddings, finite head-only
loss with encoder-unchanged/head-changed scoping, and pooling separation
(CLS margin +0.11, mean margin +0.26) on hand-written train/dev-only
pairs. macOS CPU dev-profile timings: load ~17s, forward ~180ms,
training steps ~130ms, max RSS ~478 MiB.

Framework limitation found by this qualification: Candle 0.11 fused
`layer_norm` records `BackpropOp::none`, so no gradient reaches any
encoder weight and top-layer unfreezing silently does nothing. The M001
"top-layer backward" evidence (finite loss only) never proved a weight
update. Frozen-encoder/head-only work is unaffected; encoder unfreezing
is gated on the M001B framework corrective. Full evidence:
`plans/closure/tool-selection-advisor-sequence-encoder-experiment/007-status.md`.

## Differentiable encoder restoration — 2026-09-22 (M001B)

M001B restores unfreezing through a CodeGG-owned composite-norm BERT
forward (`DifferentiableBertModel` in the experiment-gated
`sequence_encoder.rs`). Variable creation still delegates to
`candle_nn::layer_norm`/`embedding`/`linear` (identical tensor names,
so the qualified MiniLM safetensors loads 101/101 with zero missing);
only the LayerNorm forward kernel is substituted (composite
`layer_norm_slow` instead of fused `apply_op3_no_bwd`). No Candle crate
is forked or patched and no dependency version changed.

Measured on the unchanged M001A manifest: parity max 2.3e-06 / mean
3.9e-07 against upstream `BertModel::forward` (tolerance 1e-4);
correctly-scoped top-layer update (head-only leaves the encoder
unchanged; top stage changes head + top layer, lower layers unchanged);
deterministic forward (max delta 0.0) with pooling CLS +0.11 / mean
+0.26 reproducing M001A. M003 stages 2–3 are authorized through this
path. Full evidence:
`plans/closure/tool-selection-advisor-sequence-encoder-experiment/008-status.md`.
