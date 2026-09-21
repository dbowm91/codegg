# Tool-Selection Advisor Milestone 003 — Local Rust Training and Model Lifecycle

Status: active

Repository baseline: `ed960f2ae7043acd816970f7d06e74dc69b09ae8`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-roadmap.md#m003--opt-in-local-rust-training-calibration-and-model-lifecycle`

Applicable ADRs:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: capability

Hard dependencies: M001 and M002 accepted closure.

## 1. Objective

Provide an opt-in Rust-only local training/fine-tuning, calibration, evaluation, and artifact-production pipeline for a very small tool-selection model compatible with the M002 runtime.

The first supported recipe should prioritize the smallest encoder that meets benchmark quality, beginning experimentally around the TinyBERT-L2/MiniLM-L2 scale rather than assuming Laya's hundreds-of-millions-parameter backbones are necessary.

## 2. Why this milestone is blocked

M001 owns dataset/evaluation semantics; M002 owns the runtime model/artifact contract and framework qualification. Training must emit exactly that runtime contract.

## 3. Current implementation evidence

No local ML trainer exists. The intended task is discriminative/listwise tool relevance, not text generation.

Initial research suggests a compact bidirectional encoder plus decision/ranking head is sufficient to test. The first objective should be supervised/listwise distillation and calibration; RL-style proper-scoring optimization may be tested later if it materially improves calibrated decisions.

## 4. Invariants that must not regress

- Training never runs implicitly.
- Training compute/data remain local unless a separately configured M004 export/remote path is explicitly invoked.
- Python/PyTorch are not required.
- Default production builds do not link training-only dependencies.
- Training artifacts are accepted only through the M002 manifest contract.
- Dataset provenance/splits are recorded to prevent accidental evaluation leakage.

## 5. Scope

### In scope

- Separate training crate/binary or Cargo feature isolated from normal CodeGG.
- Loading M001 fixtures and M004 local events/exports.
- Local pretrained encoder initialization from explicit local assets when used.
- Pairwise cross-encoder baseline and/or Laya-style multi-candidate head experiment.
- Multi-label/listwise loss, abstention target, teacher soft-label support when present.
- Post-training temperature calibration.
- Reproducible seed/config/checkpoint metadata.
- Evaluation against M001 metrics plus Brier/log-loss/ECE.
- Artifact export directly in M002 format.
- CLI for train/eval/calibrate/inspect.

### Explicitly out of scope

- Remote training jobs.
- Automatic teacher API calls.
- Default model download.
- General language-model pretraining.
- Making training part of normal update/startup.
- Promotion into production without M005.

## 6. Required production changes

Prefer a separable workspace shape:

- lightweight runtime/data crate consumed by CodeGG when advisor support is compiled;
- training-only crate/binary depending on the selected Rust autograd/training features;
- shared architecture/serialization definitions to prevent train/infer drift.

A training config must record encoder architecture, dimensions/layers, max lengths, candidate limit, tokenizer, loss weights, optimizer, schedule, seed, dataset fingerprints, and calibration split.

Start with straightforward losses:

```text
L = lambda_relevance * BCE_or_listwise
  + lambda_distill * KL(teacher || student)   [when teacher probabilities exist]
  + lambda_abstain * BCE(needs_tool)
```

Do not introduce RLCD merely to imitate Laya. Add it only behind an experiment if supervised/calibrated objectives show a concrete deficiency.

## 7. Ordered work packages

A. Training-only package/feature boundary and dependency guard.
B. Reproducible dataset loader/split fingerprints.
C. Small encoder + ranking/abstention head with checkpoint save/load parity against runtime.
D. Local optimizer/training/checkpoint loop.
E. Calibration and evaluation report.
F. Artifact export + runtime round-trip test.
G. Reproducibility and resource documentation.

## 8. Failure, cancellation, restart, and contention semantics

Training should write checkpoints atomically and never replace the currently selected runtime model until an explicit install/select action succeeds. Interruptions leave the previous runtime artifact intact.

Concurrent training runs must use distinct run directories or reject collisions. A corrupt/incomplete checkpoint is not promoted.

## 9. Compatibility and migration

Training configuration and artifact formats are versioned. Runtime compatibility remains controlled by M002. Old datasets may be read only through explicit supported schema versions.

## 10. Required tests

- training feature absent from default dependency/build path;
- tiny fixture overfit smoke test;
- deterministic seeded short run within numeric tolerance;
- checkpoint resume/atomicity;
- train artifact -> M002 runtime load round trip;
- held-out calibration;
- leave-one-tool-out evaluation;
- no-tool and hard-negative evaluation;
- invalid dataset/provenance rejection.

## 11. Required verification commands

Expected minimum:

```bash
cargo test --workspace
cargo test --features tool-advisor
cargo test --features tool-advisor-training
cargo run --features tool-advisor-training -- tool-advisor train --config <tiny-smoke-config>
cargo run --features tool-advisor-training -- tool-advisor eval --model <artifact> --dataset <test-fixtures>
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git diff --check
```

Record training wall time, peak memory, artifact size, and evaluation metrics on at least one ordinary developer machine.

## 12. Documentation updates

- local training guide;
- dataset/provenance guide;
- artifact promotion/rollback;
- framework/backend limitations;
- calibration/evaluation interpretation.

## 13. Acceptance criteria

M003 closes when a developer can opt into Rust-only local training, produce a small calibrated artifact, load it in M002, reproduce held-out metrics, and remove every training feature/dependency from an ordinary default build.

## 14. Stop conditions

Stop if training requires Python, remote compute, silent dataset upload, a different incompatible runtime architecture, or a framework whose required native dependencies violate ADR-0009.

## 15. Closure evidence required

- exact training config and dataset fingerprints;
- model parameter/artifact size;
- train/dev/test metrics including calibration and unknown-tool split;
- resource measurements;
- default-vs-training dependency/build evidence;
- runtime round-trip and rollback evidence.

## 16. Handoff notes

The goal is the smallest useful decision model, not the highest benchmark score at any size. Preserve comparable runs across approximately 5M, 15M, and 20-30M candidates before increasing capacity.
