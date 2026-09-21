# Tool-Selection Advisor Post-Closure Corrective M002 — Contextual Encoder Runtime and Local Rust Training

Status: blocked

Repository baseline: `c9087346620988a7c793fb2687732a62e481103a`

Source corrective:

- `plans/subsystems/tool-selection-advisor-post-closure-corrective-addendum.md#m002--contextual-encoder-runtime-and-local-rust-training`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Hard dependency: M001 accepted closure and frozen dataset/split fingerprints.

Primary class: capability corrective.

## 1. Objective

Implement the actual small contextual decision model originally intended for CodeGG tool selection.

The model must jointly encode bounded task/session context and textual tool candidates, be trainable locally through Rust-only tooling, run locally through a pure-Rust in-process runtime, remain optional/default-off, and be directly comparable to the existing `hashed-linear-v1` baseline.

Target model class: approximately **5M-25M parameters**, with at least two capacity points measured before choosing a shipping candidate.

## 2. Why this milestone is blocked

Architecture tuning before the expanded M001 split is frozen would contaminate the final evaluation. M002 must consume M001's stable counterfactual, unknown-tool, and tool-family holdouts.

## 3. Current evidence and defect

The predecessor runtime/trainer correctly proves artifact lifecycle, local Rust training mechanics, fallback, checkpointing, and qualification plumbing. It does not implement a contextual neural model:

- learned weights are token weights over candidate name/description;
- task context affects scores through fixed lexical overlap;
- the smoke artifact has only 104 learned parameters;
- no `tool-advisor` runtime dependency is actually feature-gated;
- no model/tokenizer asset exists.

Keep that implementation as the lowest-cost baseline; do not overwrite or relabel it.

## 4. Invariants

- Default build/run remains usable without neural runtime dependencies or weights.
- `tool-advisor` becomes a real optional feature for contextual-model inference support.
- `tool-advisor-training` remains a separate opt-in superset and is the only surface that compiles autograd/training machinery.
- No Python, PyTorch, required C/C++ sidecar/runtime, or automatic model download.
- Training runs locally from explicit local datasets/assets.
- Pretrained initialization, if used, is an explicit local asset with license/provenance/hash; it is never silently downloaded.
- `ToolAdvisor` remains advisory and authority-free.
- Existing `hashed-linear-v1` artifact loading remains supported for baselines unless a separate compatibility plan supersedes it.
- Model quality is not accepted without contextual counterfactual and unknown-tool evidence.

## 5. Framework qualification

Perform a bounded implementation spike before committing the dependency graph.

Evaluate at minimum the currently plausible pure-Rust paths (for example Candle and Burn) against:

- BERT/TinyBERT/MiniLM-like encoder support;
- local Rust training/autograd;
- safetensors or comparably auditable weight format;
- tokenizer support;
- CPU inference on macOS/Linux/Windows/ARM64 targets;
- required native libraries;
- binary-size delta;
- training-only dependency isolation;
- deterministic checkpoint/load parity;
- quantization feasibility;
- license/redistribution constraints.

Select one train+infer stack if possible. A split train/infer framework requires explicit measured benefit and a stable export parity test.

Do not select tract alone as the training owner because it does not provide the local training path required by this milestone.

## 6. Model experiments

Implement at least two contextual capacity points in the intended range, for example:

- small: approximately 5M-10M parameters;
- medium: approximately 15M-25M parameters.

Exact dimensions/layers/heads are empirical. Keep maximum context deliberately small; the advisor does not need full transcript context.

Compare two scoring formulations if implementation cost is bounded:

### A — Pairwise cross-encoder

```text
[CLS] compact task/context [SEP] tool descriptor [SEP]
                      |
                  encoder
                      |
                relevance head
```

Score each shortlisted candidate independently.

### B — Multi-candidate/marker scorer

```text
[CLS] compact task/context [SEP]
[M] tool A [M] tool B ... [SEP]
              |
           encoder
              |
     marker relevance heads
```

This more closely resembles the Laya typed-choice design and may reduce repeated encoder work for small candidate sets.

Do not require both to ship. Use the benchmark to choose.

## 7. Context and candidate contract

Use host-built bounded context, not raw transcript concatenation. Candidate text should include canonical name plus bounded purpose/category/disclosure metadata and optionally a schema summary only when justified.

The model must support tools unseen during training by textual descriptor.

Add hard contextual tests:

- identical candidate set + different task -> ranking changes appropriately;
- candidate renamed but descriptor retained -> semantic relevance remains;
- misleading name + correct description -> description wins;
- no-tool context -> calibrated abstention;
- hard-negative tool families remain distinguishable.

A model that cannot beat the linear baseline on counterfactual/context-sensitive slices does not qualify regardless of aggregate score.

## 8. Artifact/runtime contract

Introduce an `encoder-v1` (name may differ) manifest under the existing artifact family with explicit:

- architecture/version;
- parameter count/precision;
- layer/hidden/head dimensions;
- tokenizer hash/version;
- context/candidate schema versions;
- maximum sequence/candidate limits;
- calibration data/version;
- weights hash;
- training dataset/split fingerprints;
- initialization provenance/license;
- runtime feature/version requirements.

Prefer safetensors for model tensors if supported cleanly by the selected Rust stack. Keep manifest metadata separate and bounded.

Unknown architecture/version -> diagnostic + `NoopAdvisor`, never a failed turn.

## 9. Training objective

Start with supervised calibrated objectives, not RL for its own sake:

```text
L = lambda_rank * ranking_or_relevance_loss
  + lambda_abstain * abstention_loss
  + lambda_distill * KL(teacher || student)   # only when local dataset carries teacher distributions
```

Teacher probabilities are optional data. Training must not call a remote teacher.

Use M001 train/dev splits for optimization/calibration and do not inspect the frozen final test/tool-family holdout during architecture tuning.

## 10. Feature/dependency isolation

Refactor the existing module boundary so:

- schema/benchmark/`hashed-linear-v1` baseline can remain lightweight;
- contextual runtime dependencies compile only with `tool-advisor`;
- training/autograd dependencies compile only with `tool-advisor-training`;
- ordinary default builds do not link the neural runtime;
- config requesting an unavailable contextual architecture returns a clear diagnostic and falls back safely.

Prove dependency and release-binary differences with `cargo tree`/artifact measurements.

## 11. Ordered work packages

A. Framework/runtime spike and recorded selection.
B. Real Cargo feature/dependency isolation.
C. Contextual model architecture + tokenizer/asset contract.
D. Local Rust training/checkpoint loop.
E. Calibration and artifact export/load parity.
F. Capacity comparison against BM25 + `hashed-linear-v1`.
G. Quantization experiment if it materially improves deployment footprint without unacceptable quality/calibration loss.
H. Resource/target qualification.

## 12. Failure/recovery semantics

Training checkpoints are atomic and resumable. A failed run never replaces a selected runtime artifact.

Inference timeout/error/corrupt model/missing feature/missing weights falls back to deterministic discovery. Do not retry neural inference repeatedly within one turn.

## 13. Required tests

- default build contains no contextual runtime/training dependency;
- feature-on supported-target compilation;
- train -> artifact -> runtime round trip;
- checkpoint resume/collision;
- context-swap counterfactual ranking;
- unknown/renamed tool;
- no-tool abstention;
- tool-family holdout;
- corrupt/incompatible artifact fallback;
- tokenizer/weights hash validation;
- deterministic tiny overfit smoke test;
- calibration metrics;
- linear baseline remains loadable.

## 14. Required verification

Expected minimum:

```bash
cargo check --locked
cargo check --locked --features tool-advisor
cargo check --locked --features tool-advisor-training
cargo test --locked --lib tool_advisor
cargo test --locked --features tool-advisor --lib tool_advisor
cargo test --locked --features tool-advisor-training --lib tool_advisor
cargo run --locked --features tool-advisor-training --bin codegg -- tool-advisor train --config <encoder-config>
cargo run --locked --features tool-advisor-training --bin codegg -- tool-advisor eval --model <encoder-artifact> --dataset <frozen-test>
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Also record `cargo tree`/binary delta and CPU resource measurements.

## 15. Acceptance criteria

M002 closes only when:

1. a contextual encoder in the intended small range trains locally through Rust;
2. the model learns task/candidate interaction demonstrated by counterfactual tests;
3. at least two capacity points have comparable quality/resource reports;
4. the selected candidate beats `hashed-linear-v1` on predeclared contextual/unknown-tool slices and is non-inferior on primary ranking metrics;
5. runtime/training dependencies are actually feature-isolated;
6. no-model/no-feature/off behavior remains equivalent to ordinary CodeGG;
7. artifact/license/provenance are explicit and auditable.

## 16. Stop conditions

Stop if the only viable path requires Python, remote training, mandatory native sidecars, default model download, or a model substantially larger than the declared small-model envelope without a new plan/decision.

## 17. Closure evidence required

- framework comparison and rationale;
- exact model configs/parameter counts;
- initialization provenance/license;
- frozen dataset fingerprints;
- metrics by hard-negative/counterfactual/unknown-tool/tool-family slice;
- calibration;
- train wall time/peak memory;
- artifact/tokenizer size;
- binary delta/cargo dependency evidence;
- p50/p95 inference, RSS, cold load on representative machines;
- fallback and feature-isolation tests.
