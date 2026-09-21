# Tool-Selection Advisor Milestone 003 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/tool-selection-advisor/003-local-rust-training-and-model-lifecycle.md`
Source subsystem roadmap: `plans/subsystems/tool-selection-advisor-roadmap.md#m003--opt-in-local-rust-training-calibration-and-model-lifecycle`
Repository baseline reviewed: `0e0f1af`
Implementation commits: `0bfde1e` — local Rust trainer/model lifecycle

## 1. Executive finding

M003 is complete. The opt-in `tool-advisor-training` feature provides local
Rust-only training, deterministic dataset/config fingerprints, held-out
evaluation, abstention calibration, atomic checkpoints, collision protection,
and installation of an artifact accepted by the M002 runtime. Ordinary builds
remain free of training-only modules/dependencies, and no production model is
bundled or selected automatically.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Training-only boundary | `tool-advisor-training` feature; `src/tool_advisor/training.rs` is cfg-gated; default build/check passes without it |
| Dataset/split fingerprints | `dataset_fingerprint`, `split_for`, config fingerprint recorded in `TrainingReport` and checkpoint |
| Rust-only trainer | deterministic supervised linear relevance/abstention loop; no Python, remote calls, downloads, or native ML dependency |
| Checkpoint/restart/contention | atomic checkpoint rename, resume validation, distinct run directory, create-new lock, collision test |
| Calibration/evaluation | held-out dev abstention threshold selection; Recall@1/3/5, MRR, nDCG, no-tool metrics |
| M002 artifact parity | `artifact_from_checkpoint` emits `hashed-linear-v1` manifest; runtime load round-trip test and CLI `inspect` |
| Explicit lifecycle | `train`, `eval`, and `inspect` commands; training never runs during startup/update |

## 3. Production implementation evidence

The first recipe uses the smallest compatible textual scorer rather than
assuming a pretrained TinyBERT/MiniLM dependency. It trains candidate
descriptor features against M001 graded labels, calibrates abstention on the
dev split (falling back to train when a fixture has no dev cases), and exports
the exact manifest/weight hash required by M002. This keeps the framework choice
reversible while producing a usable empirical baseline for later model-size
experiments.

The repository smoke configuration is
`assets/tool-advisor/tiny-training.json`. A local run over the reviewed corpus
produced:

- dataset fingerprint `b191a2f0eb5051622ebec763e6d614ed844be6cbc3f00791c6e1994fc67963c1`;
- 10 train, 4 dev, and 1 test case;
- 2 epochs, 104 parameters, 3,730-byte artifact;
- train Recall@1 `0.800`, MRR `0.833`, calibrated no-tool F1 `1.000`;
- held-out test Recall@1/3/5 `1.000`, MRR `1.000`, nDCG@5 `1.000`.

These are smoke-corpus results, not a claim of production generalization.

## 4. Verification executed

Local verification:

- `cargo test --features tool-advisor-training --lib tool_advisor` — 11 passed.
- `cargo test --features tool-advisor --lib tool_advisor` — 9 passed.
- `cargo test --workspace --locked -- --test-threads=1` — 11,635 passed, 3 ignored, 245 suites.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --all -- --check` — passed.
- `git diff --check` — passed.
- `cargo run --locked --features tool-advisor-training --bin codegg -- tool-advisor train --config assets/tool-advisor/tiny-training.json --json` — completed.
- `cargo run --locked --features tool-advisor-training --bin codegg -- tool-advisor eval --model target/tool-advisor-smoke/model.json --json` — completed over all 15 cases.
- `cargo run --locked --bin codegg -- tool-advisor inspect --model target/tool-advisor-smoke/model.json` — manifest validation completed without the training feature.

These are local results; no hosted/CI result is claimed.

## 5. Invariant review

Training is explicit and local. The selected runtime artifact is unchanged if a
run is interrupted before final installation. The ordinary build does not
compile `training.rs` or add a training dependency. Artifact output is bounded
by the shared M002 manifest limits.

## 6. Failure and recovery review

Malformed config/dataset, incompatible checkpoints, stale/colliding run locks,
invalid artifacts, and unsupported schema versions fail without replacing the
selected artifact. Checkpoints are written to a temporary sibling, synced, and
renamed atomically. Resume requires matching config and dataset fingerprints.

## 7. Migration and compatibility review

No database or protocol migration. Training config, checkpoint, case, and
artifact schemas are separately versioned. Artifacts are loaded through the
M002 validator and cannot be silently reinterpreted.

## 8. Security review

The trainer reads only an explicitly selected local dataset or the embedded
reviewed corpus. It performs no network access and records no secrets. No
automatic provider/teacher API calls, transcript upload, or environment
capture exists.

## 9. Documentation and operations

`architecture/tool-advisor.md` documents the training feature and smoke config.
CLI output exposes dataset fingerprint, artifact size, parameter count, and
evaluation metrics; `inspect` exposes the manifest for operator review.

## 10. Unresolved findings (severity: critical/high/medium/low)

- Low: the first trainer is a compact linear baseline, not a pretrained encoder;
  M005 qualification must report honestly whether it is useful before any
  architecture expansion.
- Low: hardware/resource measurements are local smoke evidence only; broader
  model-size comparisons remain deferred to qualification.

No critical, high, or medium findings remain.

## 11. Roadmap disposition

M003 is closed. M004 remains ready and is the only remaining hard dependency
before M005 can be considered ready. M005 remains blocked on M004 closure.

## 12. Registry updates

The dependency audit checked every registered tool-selection advisor plan:

- M004 remains `ready` from M001 closure.
- M005 remains `blocked`, now only on M004 closure; M002 and M003 are closed.

No corrective pass is required. The compact baseline limitation is an explicit
qualification input, not a reason to widen M003 scope retroactively.
