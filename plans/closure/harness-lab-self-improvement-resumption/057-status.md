# Harness Lab Self-Improvement Resumption S057 Closure — Post-Run Tool-Call Projection Order

Status: **closed at post-run ordering correction scope; attempt 76 unqualified**

Repository baseline: `e3fce7b99f13380d9e36fafc49c844ccc646be69`

Implementation commit:

- `a8c6e93a25e7421b200b3d8995f1d8e38bbf65cf` — collect the raw ACP tool-call projection before deriving H007's pause-adjusted qualification calls.

Source plan: `plans/implementation/harness-lab-self-improvement-resumption/057-postrun-tool-call-projection-order.md`.

## 1. Executive finding

The S057 ordering correction is verified offline and on the exact live head. Attempt 75's `NameError` interrupted post-run qualification before safe task/verifier projections existed. Attempt 76 ran the corrected head, produced those projections, and exited without a runner exception. The task and horizon remain unqualified: the agent timed out after 54.84 minutes of active work, qualified only one of two repair pairs, left the continuation document invalid, and the independent verifier reported two public failure domains. This is a completed narrow runtime correction, not a successful H007 qualification.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Collect tool calls before qualification projection | `test_postrun_projection_collects_calls_before_h007_pause_adjustment`; attempt 76 safe completion projection | pass | The post-run path completes on the exact corrective head without the attempt75 `NameError`. |
| Preserve raw call timestamps while adjusting only qualification timing | Focused production-shaped regression | pass | Raw calls are returned unchanged; the H007 list alone subtracts verifier pauses. |
| Run focused suite and quality checks | 85 autonomy-run tests, Python compilation, both test-quality guards, `git diff --check` | pass | All ran before commit `a8c6e93a2`. |
| Exercise exact corrective head live | Attempt 76 | pass | Safe task and verifier projections were produced; 303 provider admissions matched the campaign row. |
| Qualify H007 | Attempt 76 safe projections | fail | Task outcome `failed`, agent terminal `timeout`, and verifier assertion failures in `continuation_boundary` and `transcript_job_lifecycle`. |

## 3. Attempt 76 safe result

Attempt 76 used the unchanged v97 task/verifier, `opencode_go/muse-spark-1.3-contributor`, the same ARM64 binary/helper pair, and exact source head `a8c6e93a25e7421b200b3d8995f1d8e38bbf65cf`. The shell was translated x86_64 under Rosetta 2 on an ARM64 Apple Silicon host; the ARM64 binary probe passed. The run lasted 3,314.570 seconds, with 3,290.375 seconds active, and admitted 303/1,200 provider requests. The 338 observed tool starts had 338 terminals, zero unpaired starts, and zero event gaps. No disk, inactivity, or external wall stop fired. Minimum and ending free disk were 156,819,832,832 bytes. Billed USD is unknown if unreported.

The safe task result is failed, not unknown: the agent terminal timed out near the 3,300-second prompt-work boundary with the verifier reserve intact. All seven stages, five contract phases, and twelve gates were documented complete, but only one of two required repair pairs qualified; the `catalog_store` pair lacked productive work between failure and rerun. The continuation document was invalid because its pointer block was not terminal. The public verifier reconciliation reported failures in `continuation_boundary` and `transcript_job_lifecycle`. Exact job lifecycle and productive release overlap were observed. One failed test submission was classified as `execution_backend_unavailable` and had no job ID; that allowlisted class does not establish a daemon, scheduler, or ACP defect by itself.

## 4. Evidence and privacy

- Safe campaign diagnostic: `evidence/s019-attempt76-safe-diagnostic-20261114.json`, SHA-256 `f30c730156f85d62d5fbf6bdaf1c4cb01f4b187fe7268ba46087cf7c974d22f4`.
- Safe preflight: `evidence/s019-attempt76-preflight-20261114.json`, SHA-256 `f08a29e65fac661fdf6ea010e62e628f75f135466e66ac3ea9d8c6c29a9298b5`.
- Safe monitor summary: `evidence/s019-attempt76-monitor-summary-20261114.json`, SHA-256 `c9e9c30812e924eb9fa84a6042a2f71d9315276fd178a9c74649f7e4ba35578f`.

The provider-boundary aggregate and campaign row both report 303 requests. No private receipt, runner stdout/stderr, transcript, continuation body, raw provider/tool payload, hidden verifier source/output, or credential value was inspected or retained in the committed evidence. The initial local architecture check refused before provider admission because it did not recognize Rosetta; the corrected ARM64-kernel/translation check passed and the refusal is recorded as non-consuming in the preflight.

## 5. Authorization and disposition

Attempt 76 consumed one start: 76/200 combined, 32/40 in grant `s019-supplemental-20261111-04`, and 47/120 in the task authorization. Remaining: 124 combined, eight grant, and 73 task-authorized starts. S057 is closed at the runtime ordering scope. H007 remains unqualified; attempt 76 supplies no reproduced event-loss, request-cap, disk, daemon, or scheduler failure that warrants expanding production architecture. Keep the frozen task, verifier, route, and caps unchanged in subsequent runs.
