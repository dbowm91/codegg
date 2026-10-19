# Agent Tool-Use Operability Corrective C004 — Closure Status

Status: blocked
Source plan: `plans/implementation/agent-tool-use-operability-post-audit-corrective/004-task-and-todo-status-contracts.md`
Implementation commit: `590c713ea377f50a741a69284060f3fdd483bf12` (`plans/agent-tool-use-operability-20261019`)
Recommendation: blocked pending status/get and cross-session denial coverage through the model-visible TaskTool path.

## 1. Finding and disposition

The explicit todo default now accepts multiple `in_progress` items, the strict policy remains configurable, and TaskTool's schema and descriptions distinguish task, run, and group identifiers. The required caller-path matrix is incomplete: no new trajectory exercises `status(run_id)`, `get(task_id)`, group status/wait, and cross-session retrieval denial together. Tests and docs alone do not satisfy the plan's stated real trajectory gate.

## 2. Changes landed

- Explicit todo profile defaults `require_single_in_progress` to false; guided profile and explicit strict configuration retain the constraint.
- Failure under strict policy leaves state and revision unchanged.
- Task schema documents `run_id` vs `task_id`, supports durable string IDs and legacy integer IDs for task retrieval, and removes the false `status`/`get` alias implication.
- AgentLoop test fixtures now carry an actual workspace service lease, preserving fail-closed workspace authority for subagent calls.

## 3. Requirement-to-evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| Parallel explicit todo status and strict override | `codegg-core` `task_state` tests | Pass |
| Task/run identifier schema and error clarity | `tool::task` schema tests and docs | Pass |
| Real spawn and successful result trajectory | `test_task_tool_integration_with_subagent` and existing subagent tests | Pass for spawn/result |
| `status(run_id)` and `get(task_id)` normal-path retrieval | No scripted follow-up trajectory added | Missing |
| Cross-session retrieval denial and group status/wait | No specific new TaskTool trajectory covering these cases | Missing — blocks closure |
| Failed input leaves state unchanged | Strict-state tests assert unchanged revision/state | Pass for todo policy |

## 4. Verification

- `cargo test --locked -p codegg-core task_state` — 18 tests passed.
- `cargo test --locked -p codegg --lib tool::task` — 6 tests passed.
- `cargo test --locked -p codegg --test agent_loop_harness` — 62 tests passed, including spawn/result coverage.
- `scripts/verify.sh quick` — passed.

The `todowrite` focused target was not separately run. No hosted CI or live provider was used.

## 5. Compatibility and security

No durable task schema or protocol migration. Numeric task identifiers remain accepted for compatibility. The change does not widen cross-session access; however, the requested model-visible denial trajectory is absent, so no strict security closure is claimed.

## 6. Failure, restart, and contention

Strict todo rejection is state-preserving. Existing task persistence/runtime ownership is unchanged. Restart and task completion tests elsewhere remain in place, but the corrective-specific status/get and cross-session checks are not tied together through the TaskTool caller path.

## 7. Limitations and severity

Severity: medium model-interface/cross-session qualification gap. The production schema change is implemented; closure awaits the specified successful retrieval and denial trajectories.

## 8. Unblock audit

C007 remains blocked on C004. No downstream plan became eligible. Add real AgentLoop TaskTool trajectories for status/get/group operations and a cross-session denial before changing this status.
