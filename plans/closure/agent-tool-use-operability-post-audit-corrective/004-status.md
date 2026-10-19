# Agent Tool-Use Operability Corrective C004 — Closure Status

Status: closed
Source plan: `plans/implementation/agent-tool-use-operability-post-audit-corrective/004-task-and-todo-status-contracts.md`
Source subsystem roadmap: `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md#7-milestones`
Repository baseline reviewed: `4c86bac2a608dd717be085247838fd2f37d81ab6`
Implementation commit: `2913da8a` (`plans/agent-tool-use-operability-20261019`)
Implementation summary: corrected todo profile/schema contracts and qualified model-visible TaskTool retrieval and owner-only session access.
Recommendation: closed; C007 remains blocked on C002.

## 1. Finding and disposition

Todo status policy, TaskTool identifier semantics, and model-visible subagent retrieval now have caller-path qualification. The deterministic AgentLoop trajectory retrieves run state, task state, and group state, exercises wait behavior, and proves cross-session retrieval is denied. C004's acceptance criteria are met.

## 2. Changes landed

- Explicit todo profile permits multiple `in_progress` items by default; guided and explicitly strict configuration preserve the single-item rule.
- TaskTool schema and errors distinguish durable task IDs, run IDs, and group IDs, while retaining numeric task-ID compatibility.
- Added a scripted AgentLoop trajectory for `status(run_id)`, `wait(run_id)`, `get(task_id)`, group status/wait, owner-visible private result, and cross-session denial.

## 3. Requirement-to-evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| Parallel explicit todo status and strict override | `cargo test --locked -p codegg-core --lib task_state` | Pass |
| Task/run identifier schema and error clarity | `cargo test --locked -p codegg --lib tool::task` | Pass |
| Real spawn and successful result trajectory | `test_task_tool_integration_with_subagent` in AgentLoop harness | Pass |
| `status(run_id)`, `get(task_id)`, group status/wait | `task_tool_status_get_group_and_cross_session_contracts_are_model_visible` in AgentLoop harness | Pass |
| Cross-session retrieval denial and private-result boundary | Same test: foreign session cannot retrieve run/task; owner can read private result | Pass |
| Strict rejection leaves todo state unchanged | `codegg-core::task_state` assertions cover unchanged state/revision | Pass |

## 4. Verification

Passed:

- `cargo test --locked -p codegg-core --lib task_state` — 18 tests.
- `cargo test --locked -p codegg --lib tool::task` — 6 tests.
- `cargo test --locked -p codegg --test agent_loop_harness` — 66 tests passed on implementation commit `2913da8a`.
- `scripts/verify.sh quick` — passed on the implementation tree before the planning-only closure updates.

The `tool::todowrite` unit target contains no tests; todo policy coverage is owned by `codegg-core::task_state`. No hosted CI or live provider was used.

## 5. Compatibility and security

No durable task schema or protocol migration. Numeric task identifiers remain accepted. Cross-session access remains denied and is now exercised through the model-visible caller path.

## 6. Failure, restart, and contention

Strict todo rejection is state-preserving. Existing persistence and task ownership are unchanged; the integration trajectory verifies session ownership at retrieval. No new scheduler or durable queue was introduced.

## 7. Limitations and severity

No remaining C004 acceptance gap was found. The existing task-state restart and persistence suites remain authoritative for durable lifecycle behavior; this corrective adds caller-path retrieval qualification, not a new restart protocol.

## 8. Unblock audit

C004 is strictly closed and discharges one hard C007 dependency. C007 remains blocked solely on C002 after C001 and C006 also close. No other registered plan was newly eligible.

## 9. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| None | No C004 acceptance finding remains | None identified within this plan's scope | None |

## 10. Roadmap disposition

C004 is closed. C007 remains blocked solely by C002's unresolved workspace-to-Eggsearch scope contract. No other plan became eligible.

## 11. Registry updates

`plans/registry.md` and the source addendum mark C004 closed and C007 blocked on C002.
