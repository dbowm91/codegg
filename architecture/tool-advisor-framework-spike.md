# Tool-advisor contextual runtime framework spike

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

The selected runtime is a compact hashed-token embedding encoder with a learned
context/candidate interaction score. Small and medium capacity points are
5,242,881 and 15,728,641 parameters. This is an explicit local model family,
not a relabeling of hashed-linear-v1. It supports unseen tool names by scoring
bounded textual descriptors and remains advisory-only.

The runtime is compiled only with the optional tool-advisor feature. Training
commands additionally require tool-advisor-training. The default build keeps
the existing linear artifact path and does not contain model weights or a
download path.
