# Developer Harness Lab Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/harness-lab/003-trajectory-evaluation-and-ledger.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-roadmap.md#milestones`

Repository baseline reviewed: `7a1b9d9a1d5a28e7919e3d69851a3ae710b0afbf`

Implementation commits or pull requests:

- `d64da3d8951d743b6aae8a29f6fa3a390e6d4cb5` — native ACP trajectory evaluator and immutable ledger

## 1. Executive finding

M003 is closed. The harness runs a fixed 12-case native ACP suite against isolated subjects, verifies independent outcomes, records append-only evidence, and compares paired baseline/candidate runs without claiming unavailable metrics.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Versioned deterministic task suite | `dev/harness_lab/fixtures/smoke.jsonl`; fixture taxonomy tests | pass | Twelve cases cover tools, recovery, cancellation, timeout, failure and denial. |
| Real ACP/daemon/provider-fixture trajectory | complete baseline evaluation | pass | 12/12 expected terminal states matched; 19 tool starts. |
| Independent verification and terminal taxonomy | per-case verifiers and terminal receipts | pass | Counts: 8 passed, 1 assertion_failed, 1 execution_failed, 1 cancelled, 1 timeout. |
| Bounded repeated tool error behavior | `keep-repeated-identical-read-error` case | pass | Five observed read errors; agent stopped before eight planned attempts; execution_failed is expected. |
| Append-only ledger and query index | JSON manifests, event logs, SQLite index, immutability tests | pass | Fresh ledger cross-checks JSONL and SQLite; update/delete triggers reject mutation. |
| Replay, report and paired comparison | offline replay and baseline/candidate-copy comparison | pass | 12 paired IDs, 8 passing per side, zero regressions. |
| Missing metric provenance | report output | pass | Tokens, provider cost, CPU/RSS, turn ID and model-visible context are null/unavailable. |

## 3. Production implementation evidence

The development-only Python package supervises native ACP, captures sequenced events and verifier artifacts, stores immutable JSON manifests and append-only event streams, and maintains a SQLite query index. Offline replay, reporting, and paired comparison are supported. A context recovery case confirms the tail marker is delivered through `context_read` after a bounded read. No Rust production database or runtime schema changed.

## 4. Verification executed

### Commands run

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 -m dev.harness_lab evaluate --binary target/debug/codegg --suite dev/harness_lab/fixtures/smoke.jsonl --ledger /tmp/codegg-ledger-m003-closed --candidate baseline
python3 -m dev.harness_lab replay --ledger /tmp/codegg-ledger-m003-closed --run-id 8d12f73f-f36f-4e81-86e7-86a95c919beb
python3 -m dev.harness_lab evaluate --binary target/debug/codegg --suite dev/harness_lab/fixtures/smoke.jsonl --ledger /tmp/codegg-ledger-m003-closed --candidate candidate-copy
python3 -m dev.harness_lab compare --ledger /tmp/codegg-ledger-m003-closed --left baseline --right candidate-copy
scripts/verify.sh quick
```

### Results

The recorded clean baseline is `/tmp/codegg-ledger-m003-closed`, with JSON report `/tmp/codegg-harness-evaluate-closed.json`; 12/12 expected terminal outcomes matched. The paired run is `/tmp/codegg-harness-evaluate-candidate.json`; 12 task IDs paired, 8 pass on each side, zero regressions. Both use the same binary hash, so this validates the evaluation/comparison path and makes no performance-improvement claim. Offline replay passed. Python suite passed 12 tests and quick verification passed locally. Negative terminal cases return evaluator exit code 2 by design. No hosted CI run is claimed.

## 5. Invariant review

The controller owns global limits, timeouts and subject cleanup. Evaluator outcomes are independently checked against case expectations. Missing metrics remain unavailable instead of being imputed. Candidate trials append records without rewriting baseline manifests or index history.

## 6. Failure and recovery review

Timeout and cancellation are separate terminal categories. Repeated read errors are recorded as observed tool outcomes, not inferred from a completed ACP process. Replay operates offline on persisted receipts. Evaluation budget exhaustion or missing trials cannot be reported as a complete paired comparison.

## 7. Migration and compatibility review

No CodeGG schema or protocol migration. The ledger is a versioned development artifact with JSONL source history and a rebuildable SQLite query index.

## 8. Security review

The evaluator runs on a trusted development workstation and uses M001 private subject lanes. Fixture and report data are local; no provider secrets or live provider calls were used for the recorded baseline. This is not hostile-code isolation.

## 9. Documentation and operations

`dev/harness_lab/README.md` documents evaluate, replay, report and compare. The CLI emits unavailable metrics explicitly and preserves terminal evidence.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No repeated-seed variance or live-model cost/performance claim | Current comparison validates plumbing and deterministic correctness only | Gather those measurements in an explicitly budgeted experiment if needed. |

## 11. Roadmap disposition

Milestone closed. M004's hard M003 dependency is discharged and M004 may proceed. The baseline/candidate-copy comparison itself does not qualify a candidate improvement.

## 12. Registry updates

M003 is removed from blocked/ready work and listed as closed. M004 moves from blocked to dependency-ready.
