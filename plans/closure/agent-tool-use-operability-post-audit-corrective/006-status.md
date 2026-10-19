# Agent Tool-Use Operability Corrective C006 — Closure Status

Status: blocked
Source plan: `plans/implementation/agent-tool-use-operability-post-audit-corrective/006-batch-permit-admission-liveness.md`
Implementation commit: `590c713ea377f50a741a69284060f3fdd483bf12` (`plans/agent-tool-use-operability-20261019`)
Recommendation: blocked pending the remaining contention and post-admission timeout evidence.

## 1. Finding and disposition

Batch-local permit admission now has its own 30-second bound, queue accounting, queued/admitted/executing tracing, a cancellation-safe depth guard, and a specific timeout that says execution did not start. The deterministic 1/2/4-width release matrix, admission timeout, and queued cancellation pass. The plan also requires simultaneous parent/subagent batch ownership evidence and deadline-race/timeout-after-admission/long-output qualification; those tests were not added or run, so strict closure is withheld.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| Bounded queue admission separate from execution deadline | `acquire_batch_permit`, independent 30-second constant, explicit timeout diagnostic | Implemented |
| 1/2/4 permit release matrix | `admission_succeeds_after_release_at_one_two_and_four_permits` | Pass |
| Timeout reports tool did not start | `admission_timeout_is_separate_and_says_execution_did_not_start` | Pass |
| Cancellation before acquire clears accounting | `cancelling_queued_acquire_releases_queue_accounting` | Pass |
| Parent and subagent simultaneous batches | No integrated concurrency trace demonstrating per-batch semaphore ownership | Missing |
| Deadline race, timeout after acquire, native partial output | Not separately qualified by this corrective | Missing |
| No mutating retry widening | Retry gate checks idempotency plus configured retry policy; focused batch suite passes | Pass by implementation and regression tests |

## 3. Verification

- `cargo test --locked -p codegg --lib agent::tool_batch` — 27 tests passed.
- `cargo test --locked -p codegg --test agent_loop_harness` — 62 tests passed, including existing bounded batch behavior.
- `scripts/verify.sh quick` — passed.

## 4. Compatibility, security, and ownership

Admission remains local to one `execute_tool_batch` call. It does not claim global fairness, parent/subagent sharing, or JobScheduler ownership. Tool-level deadlines begin after admission; queued calls that expire return a typed timeout without dispatch. Non-idempotent calls are not retried by the batch wrapper.

## 5. Failure, restart, and contention

Cancellation while queued drops the queue-depth guard. A process restart discards in-memory batches as before; this change adds no durable queue. Simultaneous parent/subagent contention and long-running partial-output behavior remain unproven for this exact contract.

## 6. Limitations and severity

Severity: medium concurrency/timeout qualification gap. No defect was found in the tested semaphore path, but closure requirements explicitly include the missing concurrent ownership and post-admission cases.

## 7. Unblock audit

C006 remains a hard C007 dependency and does not strictly close. No downstream plan became eligible. The next pass should add deterministic parent/subagent overlapping batches plus timeout-race/post-admission coverage without introducing a global scheduler.
