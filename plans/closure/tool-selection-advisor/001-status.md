# Tool-Selection Advisor Milestone 001 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/tool-selection-advisor/001-evaluation-corpus-and-baselines.md`
Source subsystem roadmap: `plans/subsystems/tool-selection-advisor-roadmap.md#m001--evaluation-corpus-schemas-and-deterministic-baselines`
Repository baseline reviewed: `3c6c1c05`
Implementation commits: `c2881fa` — add evaluation corpus and deterministic baselines

## 1. Executive finding

M001 is complete and strictly infrastructure-only. CodeGG now has a versioned
pure-Rust case/prediction/metric contract, a reviewed 15-case corpus, stable
group-based splitting and synthetic unknown-tool cases, and a reproducible
keyword/BM25 benchmark command. No model, network path, training dependency,
or agent behavior change was introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Versioned case and prediction schemas | `src/tool_advisor/mod.rs`; schema versions 1; round-trip test |
| Hard negatives, multi-tool, no-tool, unknown-tool coverage | `assets/tool-advisor/corpus.jsonl`; fixture validation test; tags `hard-negative`, `multi-tool`, `no-tool`, `unknown-tool` |
| Leakage-resistant splits | `split_for(group_id)` hashes only group identity; duplicate group rejection; unknown holdout test |
| Keyword and BM25 baselines | `ToolCatalog::rank_descriptors`; `codegg tool-advisor bench` |
| Reproducible metrics | Recall@1/3/5, MRR, nDCG@5, candidate coverage, no-tool precision/recall/F1; hand-computed metric test |
| Bounded/negative loading behavior | 16 MiB file cap, bounded fields/candidates, validation errors, no filesystem writes/network |
| Runtime unchanged | Advisor module is not called from agent/runtime surface construction; no production model feature/dependency |

## 3. Production implementation evidence

The benchmark reuses the existing catalog keyword/BM25 implementation through a
descriptor adapter. Candidate descriptors retain category and disclosure
metadata but the benchmark never treats labels as authority. The fixture is
embedded for the default command, while an explicitly supplied file is bounded
and validated before parsing.

The reviewed corpus contains 15 cases, including 8 hard-negative cases, 7
multi-tool cases, 1 explicit no-tool case, and 3 unknown-tool cases. Its
fingerprint is:

`b191a2f0eb5051622ebec763e6d614ed844be6cbc3f00791c6e1994fc67963c1`

## 4. Verification executed

Local verification:

- `cargo test --lib tool_advisor` — 5 passed.
- `cargo test --lib tool::catalog` — 21 passed.
- `cargo run --locked --bin codegg -- tool-advisor bench` — completed; keyword baseline reports zero substring hits on the full-context queries, documenting its limitation.
- `cargo run --locked --bin codegg -- tool-advisor bench --bm25` — completed: Recall@1 `0.667`, Recall@3 `0.933`, Recall@5 `0.933`, MRR `0.800`, nDCG@5 `0.767`, coverage `1.000`.
- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `scripts/verify.sh quick` — passed.
- `git diff --check` — passed.
- Default dependency tree contains no Candle, Burn, tract, ONNX, Torch, or TensorFlow runtime.

These are local results; no hosted/CI result is claimed.

## 5. Invariant review

The default CodeGG dependency graph and normal agent path remain unchanged.
The advisor benchmark is an offline command and cannot register, expose,
authorize, or execute a tool. Candidate names are not used as a fixed model
class head; synthetic descriptors exercise unknown-tool semantics.

## 6. Failure and recovery review

Invalid JSONL, duplicate IDs/groups, unknown labels, invalid abstention labels,
oversized records, and unsupported schema versions fail before evaluation with
actionable errors. Benchmark interruption has no durable side effect and is
safe to restart.

## 7. Migration and compatibility review

No user database, provider protocol, or session migration was added. Case and
prediction schema versions are explicit and future incompatible changes must
use a new version or an explicit migration.

## 8. Security review

The fixture contains no private repository data or secrets. External dataset
loading is bounded to a caller-selected file and does not perform network I/O.
No audit or telemetry stream was reused.

## 9. Documentation and operations

`architecture/tool-advisor.md` documents the data contract, split policy,
unknown-tool guard, and fixture contribution rules. The CLI supports human and
JSON reports.

## 10. Unresolved findings (severity: critical/high/medium/low)

- Low: the seed corpus is intentionally small and manually reviewed; broader
  production-like trajectories and calibration metrics belong to M003/M005.
- Low: keyword baseline queries are intentionally full context and therefore
  expose the existing substring algorithm's limitation; no second keyword
  algorithm was introduced to improve the report.

No critical, high, or medium findings remain.

## 11. Roadmap disposition

M001 is closed. M002 and M004 are dependency-ready because both consume the
now-stable case/candidate schema. M003 remains blocked on M002's runtime/artifact
contract. M005 remains blocked on M002, M003, and M004.

## 12. Registry updates

The closure audit checked every tool-selection advisor dependency entry:

- M002 moved from `blocked` to `ready`.
- M004 moved from `blocked` to `ready`.
- M003 remains `blocked` on M002 closure.
- M005 remains `blocked` on M002, M003, and M004 closure.

No other registered work lists M001 as a dependency, and no corrective pass is
required by this closure.
