# Harness Lab Self-Improvement Resumption Milestone S033 — Closure Status

Status: **conditionally closed; H007 remains unqualified**

Source implementation plan:

- `plans/implementation/harness-lab-self-improvement-resumption/033-prompt-timeout-calibration-and-provider-attempt-accounting.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#27-s033--prompt-timeout-calibration-and-provider-attempt-accounting`

Repository baseline reviewed: `ac584c886f03847ff6b829f8509667637e42a052` (shared dirty worktree)

Implementation commits or pull requests: none; changes remain in the shared worktree.

## 1. Executive finding

S033's safe unresolved-provider-attempt counter and bounded prompt-timeout calibration are implemented. Attempt 41 used the exact Muse Spark/OpenCode Go route and v93 task, ran for 38.59 active minutes, and ended normally without an outer prompt timeout. The full H007 task still did not qualify: the unchanged verifier passed 3/5 and reported two pattern assertion failures in continuation-boundary and transcript/job-lifecycle domains. S034 owns only the missing redacted mismatch and event-sequence telemetry.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Track unfinished provider attempts without IDs | `diagnostics.py`, provider diagnostic tests, attempt 41 safe evidence | pass | 247 started, 247 succeeded, zero failed, zero unresolved; counters reconcile. |
| Permit a longer bounded ACP prompt | Attempt 41 preflight and prompt-boundary evidence | pass | Requested timeout 1,500 seconds; all five prompts ended normally in 252–610 seconds. |
| Preserve resource/event limits | Attempt 41 safe evidence and external watcher | pass | 274/274 tool events paired, zero gaps/drops, no provider timeout, minimum disk 101,345,566,720 bytes. |
| Complete H007 and pass unchanged verifier | Attempt 41 safe evidence | fail | Active work, exact-ID overlap, repairs, continuation, reviewer, public gates, and jobs passed; verifier passed 3/5. |

## 3. Production implementation evidence

The change is confined to the Harness Lab Python diagnostics and its tests. It adds an unresolved provider-attempt count only for complete logs and leaves product runtime, protocol, provider adapter, fixture, and verifier unchanged.

## 4. Verification executed

- `python3 -m unittest dev.harness_lab.tests.test_verifier_diagnostics`: 26 tests passed.
- Python compilation, assertion-quality guard, duplication guard, and `git diff --check`: passed.
- Attempt 41 safe evidence: `evidence/s019-attempt41-safe-diagnostic-20261111.json`, mode `0600`, receipt SHA-256 `5f681a96d855ce1e3626dbc5c64faa32aa2ac2c8753334c5c1fada21bddc3fd9`.

## 5. Invariant review

The live route remained `opencode_go/muse-spark-1.3-contributor`; fixture v93 and the independent verifier digest were unchanged. Request, native-call, prompt-work, wall-clock, inactivity, disk, cancellation, concurrency, and USD-unavailable policies were preserved. The requested per-prompt timeout was 1,500 seconds but remained dynamically bounded by remaining wall-clock and the 300-second verifier reserve.

## 6. Failure and recovery review

Attempt 41 had five prompt requests, each ending normally; the final continuation completed. Provider attempt accounting was 247/247 terminal with zero unresolved attempts. The active-work span was 2,315,686 ms, maximum activity gap 80,833 ms, all 21 submitted test jobs reached terminal status, and five repair cycles matched the trace, including both required pairs. The exact-ID priority job was observed running and terminal with productive work during the job. One reviewer Run/Task completed and was fetched in status-then-get order.

The independent verifier reported two `AssertionError` pattern failures in `continuation_boundary` and `transcript_job_lifecycle`; three of five checks passed. Safe failure rows retain only bounded message shape and allowlisted terms. No verifier identity, source, assertion text, or raw output was inspected. The precise mismatch remains unresolved.

## 7. Migration and compatibility review

No persistent product format, protocol, or user data changed. The provider diagnostic fields are additive and null when the input log is unavailable or incomplete.

## 8. Security review

The safe evidence retains no credential, raw job/Task/Run ID, prompt, provider body, tool argument, tool output, transcript, task result body, hidden test identity, or verifier text. Private evidence remains mode `0600`.

## 9. Documentation and operations

Attempt 39, the zero-request attempt 40 launcher error, and attempt 41 are reconciled in the S019 status and authorization ledger. The live model identity is recorded in attempt 41's preflight and safe evidence.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The unchanged verifier still has two pattern failures despite complete public stage, job, overlap, repair, continuation, and reviewer projections. | H007 qualification is unavailable. | S034 adds redacted pattern-signature and pseudonymous exact-job sequence telemetry, then requalifies without changing the fixture. |

## 11. Roadmap disposition

Conditionally closed at timeout-calibration and provider-attempt-accounting scope. H007 remains unqualified; S034 is the corrective successor.

## 12. Registry updates

Move S033 to recently closed as conditionally closed, retain S019 as active/unqualified, and register S034 as active.
