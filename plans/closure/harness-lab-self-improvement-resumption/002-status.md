# Harness Lab Self-Improvement S002 — Closure

Status: **closed for credential-free current-main qualification; live proposer admission remains blocked**.

## Identity and hosted baseline

- Implementation source: `9a80b060021abef36679506b9b0f84847b5b5d51`.
- Resulting main: `58f4c5ec9a273b246e3303b0cb5fd615d0157ae1`.
- Hosted CI on that exact main SHA: run [41003530150](https://github.com/dbowm91/codegg/actions/runs/41003530150), success; Desktop E2E run [41003529978](https://github.com/dbowm91/codegg/actions/runs/41003529978), success.
- Built `codegg` and `codegg-sandbox-helper` from a detached checkout of that exact SHA with Rust 1.89 for `aarch64-apple-darwin`; `codegg doctor` passed. CodeGG SHA-256: `62ecc84113b2399352ead4faea6a2cb7641c0b98f505cb36d2a2f1960d99cb94`.

## Requirement results

| Requirement | Evidence | Result |
|---|---|---|
| Distinct isolated subjects; stopping one leaves the other usable | `evidence/s002-offline-qualification.json#/two_subject_smoke` | Pass on exact SHA: two distinct daemon IDs, generations and roots; subject 2 created a session after subject 1 stopped; zero provider calls. |
| Native ACP write path and event accounting | `#/native_tool_path` | Pass on exact SHA: loopback fixture selected and completed one write; event gaps, permission requests and upstream calls were zero. |
| Explicit Deny is effective and accurately counted | `#/native_deny_path`; `ToolTrialCliTests.test_explicit_deny_expects_one_refused_tool_call` | Pass on exact SHA: one Bash tool ended failed, no receipt file or changed workspace paths, zero event gaps. A narrow CLI expectation bug was fixed in `__main__.py`; CodeGG policy behavior was already refusing the call. |
| Provider request N+1 is refused before transport | `#/subject_request_cap` and provider regression `provider_request_cap_refuses_before_transport_and_writes_complete_receipt` | Pass on exact SHA: two loopback requests observed at limit 2; request 3 refused before transport; complete receipt asserted; focused Cargo test passed (1/1). |
| Candidate, score, checkpoint and resume behavior | `dev/harness_lab/tests/test_improve.py`, `test_ledger.py`, `test_scoring.py`, `test_autonomy_run.py` | Covered by offline tests including SIGKILL around checkpoint/ledger boundaries, stale fingerprint refusal, no duplicate resumed rows, incomplete evidence, correctness-only acceptance and protected-control regressions. |
| Current checkout verification | Rebased clean checkout, `scripts/verify.sh quick` | Pass: complete quick guard set, 430 Harness Lab tests passed, 1 optional test skipped (431 total), locked all-target workspace check passed. |

## Live boundary and limits

No authenticated provider or proposer call occurred. USD is `unknown`; there was no spend. The subject-side CodeGG provider cap is pretransport-enforced. At this S002 source revision, external proposer request-level admission was not proven and live proposer execution remained **blocked**. Follow-up S007 separately qualifies the actual OpenCode CLI request N/N+1 boundary with a loopback HTTP fixture; this does not rewrite S002's source revision or make a live provider route available. S002 itself does not qualify an autonomous accepted improvement.

Evidence is machine-readable in [`evidence/s002-offline-qualification.json`](evidence/s002-offline-qualification.json). The two local fixture edits in the original worktree were preserved and excluded from the clean-worktree packaged-fixture comparison.
