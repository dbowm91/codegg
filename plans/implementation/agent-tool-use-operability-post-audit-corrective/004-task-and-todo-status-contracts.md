# Agent Tool-Use Operability Corrective C004 — Make Todo and Subagent Retrieval Contracts Truthful

Status: closed — model-visible status/get/group and cross-session denial qualification passed; see `plans/closure/agent-tool-use-operability-post-audit-corrective/004-status.md`.
Repository baseline: `4c86bac2a608dd717be085247838fd2f37d81ab6` (main, 2026-10-19)
Source roadmap: `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md#7-milestones`
Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `#28-observability`; `#29-system-invariants`; `plans/001-terminology-and-domain-model.md`.
Applicable ADRs: ADR-0001 (programmatic authority), ADR-0004 (approval routing), ADR-0028 (execution authority), only insofar as touched. No ownership-changing decision is authorized.
Primary class: polish

## 1. Objective
Make explicit todo updates compatible with genuine parallel work and make task/run retrieval unambiguous to models without losing identity or state.

## 2. Why this milestone is ready
No hard dependencies: `TodoState` policy, durable AgentTask/AgentRun IDs and the task tool are already in-tree. C007 will later validate the full workflow.

## 3. Current implementation evidence
- `crates/codegg-core/src/task_state/mod.rs::replace_from_model` rejects two `InProgress` items when `require_single_in_progress` is true and does not persist the attempted replacement. `model_profile/types.rs::explicit_todo` defaults that setting true; guided mode is specifically single-task. `src/tool/task.rs` schema describes status as get alias but status/wait use `run_id`, get uses `task_id`; get supports durable string and legacy numeric IDs. The field `task_id` omits an explicit type in JSON schema.

## 4. Invariants that must not regress
- Persisted task identities and revisions must not be silently changed or discarded. A rejected todo write leaves previous state intact with an explicit error; do not auto-demote in-progress statuses or pretend failed write succeeded. Legacy numeric task retrieval remains supported. Caller may only retrieve authorized tasks/runs. GuidedCurrentTask remains a genuine single-current-task contract.

## 5. Scope
In: the bounded tool contract and owning files described here, focused negative/positive regression tests, documentation and explicit compatibility corrections.
Out: new schedulers or authorization engines, Python sandbox expansion, new public network clients, multi-provider live CI, new release/packaging work, unrelated UI refactors, full repository feature sweeps and SBC compile/startup benchmarks.

## 6. Required production changes
Set explicit-todo's default policy to permit multiple in-progress items, retaining user-configurable `require_single_in_progress: true` for strict workflows and existing guided-mode enforcement. Explicitly test that previous state is not lost on invalid writes and no implicit merge rewrites IDs. Correct task action schema/docstrings: `status|wait|message|interrupt|cancel` require `run_id`; `get` requires durable string or legacy numeric `task_id`; group actions require `group_id`. Better missing/wrong-identifier diagnostics and returns; keep canonical task_id/run_id separation.
Storage: no migration planned; stop if a persisted schema change becomes necessary.
Protocol: preserve existing durable identities and transport messages; any model-visible schema evolution must be backward compatible or explicitly justified.
Security: no widening of command, workspace, tool or user authority. No explicit Deny recovery by retry or silent fallback.
Documentation/static guards: prefer focused existing Rust tests over new broad static checkers.

## 7. Ordered work packages
### A — Profile and todo policy
Distinguish explicit vs guided defaults; maintain config overrides and persist update semantics. Positive multi-active state; negative strict-mode refusal with old revision preserved.
### B — Task retrieval schema and error UX
Update action enum descriptions, `oneOf`/accepted `task_id` types if broker schema supports it, and error messages for mismatched identifier; preserve legacy numeric path.
### C — Real spawn/result trajectory
Spawn a test subagent, capture task/run IDs, use status/wait/get/group methods and verify authorized success, bad IDs, wrong kind, and unauthorized lookup.

## 8. Failure, cancellation, restart, and contention semantics
Rejected todo writes are atomic. Parallel todo updates follow the existing session revision semantics; do not implement ad hoc last-write-wins. Completed/cancelled runs remain queryable under the established retention policy. Restart preserves documented durable state; notifications remain primary wake path.

## 9. Compatibility and migration
Existing saved config `require_single_in_progress` explicit true remains honored; default false only for explicit-todo mode is an intentional behavior adjustment documented for operators. No durable schema changes, no removal of `get` or legacy numeric identifiers; `status` was never a functional alias for `get` and should be described accurately.

## 10. Required tests
- Model profile default/override tests for both todo modes; multi-active save and strict rejection retaining prior revision; restart persistence.
- Schema validation for durable and legacy task IDs and all action/identifier combinations.
- Spawn/wait/status/get test with successful completion and denied cross-session retrieval; group status/wait.
- No silent state mutation on failed input.

## 11. Required verification commands
```bash
cargo test -p codegg-core --lib task_state
cargo test -p codegg --lib tool::task
cargo test -p codegg --lib tool::todowrite
scripts/verify.sh quick
```
Required handoff commands, not executed during this planning-only pass. Constrain tests to the touched crate/feature and record any unavailable binary/platform evidence.

## 12. Documentation updates
`docs/tools.md`, `architecture/model_profile_task_state.md`, `architecture/agent.md`; reflect explicit/guided differences.

## 13. Acceptance criteria (externally observable)
- Explicit todo with two in-progress items successfully persists without silent mutation under new default.
- When operator turns strict rule on, rejection is clear and previous state remains durable.
- `task(status,run_id)` / `task(get,task_id)` work and wrong identifier receives direct correction.

## 14. Stop conditions
If cross-session information would be disclosed, if model schema cannot express union types without backward incompatibility, or if changing the default alters durable work-plan projection invariants; escalate scope explicitly.
Do not improvise an unapproved ADR, silently weaken security, substitute mocks for required production-path qualification or rewrite historical closure records.

## 15. Closure evidence required
Closure `plans/closure/agent-tool-use-operability-post-audit-corrective/004-status.md`: exact implementation SHAs, before/after repro, evidence matrix, tests/commands with pass/fail/unrun, failure/restart/contention/security analysis, residual findings and unblock audit; update roadmap and registry in the same closure commit.

## 16. Handoff notes
Keep `work_plan/todo_projection.rs`'s one-current-item projection semantics separate from the explicit model todo list. Do not silently claim the durable work plan supports arbitrarily many primary current items.
