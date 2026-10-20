# Agent Loop Operational Reliability M002 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-loop-operational-reliability/002-progress-aware-recovery-and-terminal-stall-policy.md`

Source subsystem roadmap: `plans/subsystems/agent-loop-operational-reliability-roadmap.md`

Repository baseline reviewed: `f37840ef5fca36d44b35daec5da6b26cd049a3ee`

Decision: ADR-0040 Option B accepted 2026-10-18. Implementation commit: `c409e1e012d9e599abe67e975c28f4fac9dd66de`.

Verified implementation head: `c409e1e012d9e599abe67e975c28f4fac9dd66de`.

## 1. Executive finding

Recovery termination is evidence-gated. Unknown repeated outcomes remain recoverable; existing run and turn budgets remain the terminal authority. Confirmed recovery actions retain bounded escalation.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Unknown progress is nonterminal | `agent::progress_recovery` tests; repeated-unknown real-loop integration | pass | Hard turn cap test confirms exact stop at three requests. |
| Distinct incidents and recovery actions remain bounded | `agent::progress_recovery`, `agent::tool_inspect` tests | pass | Includes saturated replan and both post-tool soft-stop reasons. |
| No unsupported stagnation claim | 32-observation unit test and 12-turn real-loop regression | pass | No fabricated progress evidence. |
| Ordinary safety checks | `scripts/verify.sh quick`; workspace nextest | pass | Default-feature workspace: 14,614 passed. |

## 3. Production implementation evidence

`ProgressSignal::Unknown` explicitly represents absence of evidence. Repeated unknown observations no longer trigger terminal stall or transition-count termination. Identical incident keys continue to include normalized arguments, and the ladder resets when the incident changes.

## 4. Verification executed

```bash
rtk cargo nextest run -p codegg --test agent_loop_harness --locked --profile ci
rtk cargo test -p codegg --lib agent::progress_recovery --locked
rtk cargo test -p codegg --lib agent::tool_inspect --locked
rtk cargo nextest run --workspace --locked --profile ci --no-fail-fast
rtk scripts/verify.sh quick
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
```

Results: agent-loop harness 61 passed; focused recovery (28 passed) and tool-inspect tests passed; workspace 14,614 passed; quick and clippy passed. The feature-configured `scripts/verify.sh full` run encountered the unchanged TUI fixture failure documented in M007 closure.

## 5. Invariant review

No tool-name-only or error-wording keys were added. New evidence remains distinct from repeated work, mutating success alone does not imply progress, and hard execution limits remain independent from the recovery ladder.

## 6. Failure and recovery review

Unknown work continues until a normal terminal event, cancellation, or hard budget. Confirmed recovery actions saturate rather than growing without bound. No restart state or new durable field was introduced.

## 7. Migration and compatibility review

No schema or wire migration. ADR-0040 records the accepted decision and the absence of a trusted correlated revision producer.

## 8. Security review

Recovery does not alter authorization, permission decisions, or tool effect classes.

## 9. Documentation and operations

Updated `architecture/agent.md` and the M002 decision and handoff text. ADR-0040 now records accepted Option B.

## 10. Unresolved findings

None within M002. Correlated workspace-revision/completeness evidence is not yet produced; unknown observations intentionally remain nonterminal and bounded by existing budgets.

## 11. Roadmap disposition

Milestone closed under ADR-0040 Option B; M007 may proceed after the remaining closure records.

## 12. Registry updates

M002 and ADR-0040 are recorded as closed/accepted. M007 was subsequently unblocked and closed.
