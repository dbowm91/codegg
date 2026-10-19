# Agent Tool-Use Operability Corrective C006 — Closure Status

Status: closed
Source plan: `plans/implementation/agent-tool-use-operability-post-audit-corrective/006-batch-permit-admission-liveness.md`
Source subsystem roadmap: `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md#7-milestones`
Repository baseline reviewed: `4c86bac2a608dd717be085247838fd2f37d81ab6`
Implementation commit: `2913da8a` (`plans/agent-tool-use-operability-20261019`)
Implementation summary: decoupled batch admission from tool deadlines, then qualified parent/subagent overlap, deadline races, and bounded native timeout output.
Recommendation: closed; C007 remains blocked on C002.

## 1. Finding and disposition

Batch-local permit admission is bounded independently from execution time. Qualification now covers permit release at widths one/two/four, queued cancellation and timeout accounting, parent/subagent overlap, a queue-admission deadline race, and a native command timeout after admission that preserves partial output. The race tests exposed that batch watchdog grace had been incorrectly added to the tool's own timeout, extending a one-second command to six seconds. The fix keeps the tool budget unchanged, applies broker watchdog grace only at the broker boundary, caps it at the inherited hard deadline, and preserves bounded timeout details. C006's acceptance criteria are met.

## 2. Changes landed

- Added an AgentLoop queue-deadline race around admission and asserted queued accounting returns to zero.
- Added concurrent parent and independent AgentMode::Subagent batch barriers; each batch peaks at two admitted tools and aggregate overlap reaches four, demonstrating per-batch semaphore ownership.
- Added native Bash timeout qualification through AgentLoop and preserved partial output in the transcript.
- Separated the broker's bounded watchdog grace from the tool execution budget and bounded the grace by the inherited hard deadline.
- Preserved `ToolError::Timeout` details in the visible timeout result while enforcing the normal maximum output-display size.
- Updated `architecture/agent.md` to document the deadline and output behavior.

## 3. Requirement-to-evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| Bounded admission separate from execution deadline | `agent::tool_batch` tests; batch context timeout remains unchanged while broker applies bounded watchdog grace | Pass |
| 1/2/4 permit release matrix | `admission_succeeds_after_release_at_one_two_and_four_permits` | Pass |
| Queued cancellation and timeout accounting | `cancelling_queued_acquire_releases_queue_accounting`, `admission_timeout_is_separate_and_says_execution_did_not_start`, and AgentLoop deadline-race test | Pass |
| Parent and subagent simultaneous batches | AgentLoop concurrency barrier asserts parent peak 2, child peak 2, aggregate peak 4 | Pass |
| Timeout after admission and partial output | `admitted_native_command_timeout_preserves_partial_output_in_agent_transcript` | Pass |
| No retry widening for mutating calls | Existing retry eligibility and idempotency coverage in `agent::tool_batch` | Pass |

## 4. Verification

Passed:

- `cargo test --locked -p codegg --lib agent::tool_batch` — 28 tests.
- `cargo test --locked -p codegg --test agent_loop_harness` — 66 tests passed on implementation commit `2913da8a`, including parent/subagent overlap, deadline-race accounting, and native timeout with partial output.
- `scripts/verify.sh quick` — passed on the implementation tree before the planning-only closure updates.

The AgentLoop harness also includes the parent/subagent overlap and queue deadline race. No global fairness or shared scheduler admission claim is made.

## 5. Compatibility, security, and ownership

Admission remains local to one `execute_tool_batch` call. It does not claim global fairness or parent/subagent semaphore sharing. Tool execution timeout starts after admission; queued expiration returns without dispatch. No scheduler bypass or mutating retry expansion was introduced.

## 6. Failure, restart, and contention

Cancellation while queued drops the queue-depth guard. In-memory batches remain process-local and are discarded on restart as before. Parent and subagent batches independently respect their configured width under overlap. Native execution timeout is bounded by the inherited hard deadline and retains only capped output.

## 7. Limitations and severity

No remaining C006 acceptance gap was found. Global scheduler fairness is outside this plan's per-batch admission contract.

## 8. Unblock audit

C006 is strictly closed and discharges one hard C007 dependency. C007 remains blocked solely on C002 after C001 and C004 also close. No other registered plan was newly eligible.

## 9. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| None | No C006 acceptance finding remains | No in-scope issue identified; global fairness is an explicit non-goal | None |

## 10. Roadmap disposition

C006 is closed. C007 remains blocked solely by C002's unresolved workspace-to-Eggsearch scope contract. No other plan became eligible.

## 11. Registry updates

`plans/registry.md` and the source addendum mark C006 closed and C007 blocked on C002.
