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
