# Agent Run, Async Delegation, and Worktree Concurrency M007 — Closure Status

Status: closed

Closure date: 2026-09-01

Implementation commit: `4863765a20d25452665012e55592def3fd3d8ba9`

Implementation plan: `plans/implementation/agent-run-worktree-concurrency/007-durable-lineage-context-and-fanout-corrective-pass.md`

Corrective roadmap: `plans/subsystems/agent-run-worktree-concurrency-corrective-closure-addendum.md`

Superseded strict subsystem disposition retained: `plans/closure/agent-run-worktree-concurrency/006-status.md`

Repository implementation baseline: `b87d1d5b65aca96c700deb27e579374b3d158545`

## 1. Executive finding

M007 is fully implemented and closed. The production delegation boundary now
has explicit turn-owned versus run-owned orchestration, durable parent/root/
depth lineage, store-derived scheduler context, nested TaskTool ownership,
canonical nested worktree allocation, and direct-owner control authorization.
The primary turn is never fabricated as a delegated AgentTask or AgentRun.

M008 remains a separate corrective milestone for model tool-call identity,
authoritative projection depth across all transports, and strict final
subsystem closure. This record closes M007 only and does not claim final
agent-run/worktree subsystem closure.

## 2. Finding-to-evidence disposition

| Finding | Production correction | Evidence | Disposition |
|---|---|---|---|
| F1 root fan-out has no owner | `AgentOrchestrationOwner::Turn` in `src/tool/task.rs`; root factory wiring; `AgentRunGroupOwner::Turn` and owner-scoped group validation in `crates/codegg-core/src/agent_run_group.rs` | turn-owned group acceptance/rejection test; TaskTool root group path accepts durable child IDs and never inserts a root run | closed in M007 |
| F2 current/parent identity conflated | `DelegatedAgentExecutionContext`; scheduler reloads task/run records; worker owns nested TaskTool with `Run(current_run_id)` | `agent_run`, worker, scheduler, and subagent focused suites; durable depth/root relation test | closed in M007 |
| F3 nested context incomplete | worker loads project/repository/workspace/turn/budget from durable records; run-control/group/submission services are attached; scheduler rejects missing durable records | worker and scheduler suites; nested execution context implementation review | closed in M007 |
| F4 depth not authoritative | additive `agent_run.depth`, transactional parent/depth validation, pre-admission max-depth check, durable depth propagation | 15 agent-run tests; max-depth boundary test; nested scheduler path tests; 512-test core suite | closed in M007 |
| F5 control authorization too broad/reversed | `RunControlService::authorize` now permits exact originating-turn top-level control or direct parent-to-child control only | 4 run-control tests cover same-session/different-turn, sibling, child-to-parent, self, and direct-child cases | closed in M007 |
| F6 call identity collapses operations | canonical invocation identity and operation idempotency | owned by M008 per corrective roadmap; no M007 claim | deferred to M008 |
| F7 projection depth is caller-supplied | durable projection depth source | owned by M008 per corrective roadmap; M007 persists authoritative depth and stops scheduler reset | deferred to M008 |

## 3. Delivered implementation

- Root TaskTool receives a typed `Turn(session_id, turn_id)` owner. Durable
  child TaskTool instances receive `Run(current_run_id)`, while
  `parent_run_id` remains lineage data for the current run.
- Run records persist `depth`; top-level runs are depth 1 and descendants must
  be exactly parent depth + 1. Root IDs are inherited from the durable parent.
- Scheduler execution reloads authoritative task/run records before building
  `SubAgentRequest`; missing durable context is a failure, not a fallback.
- Nested workers receive durable project/repository/workspace/turn context,
  budget/depth, run control, group service, submission service, and inherited
  path restrictions.
- Turn-owned and run-owned groups have explicit persisted owner kinds. Turn
  groups accept only top-level runs from the exact originating turn; run groups
  accept only direct children with matching root and depth.
- Control authorization is exact-turn for top-level runs and direct-parent for
  child runs. Same-session sibling, child-to-parent, self, and forged-turn
  control are denied.
- Nested worktree requests preserve `RepositoryId`, resolve linked worktrees
  to the common Git root, use the owning checkout as the effective base, and
  allocate a distinct managed worktree.
- The project-catalog invariant guard was reconciled to the current additive
  storage layout version rather than retaining its stale version-36 literal.

## 4. Migration and compatibility evidence

Migration v42 adds `agent_run.depth` with a conservative default of 1 and a
bounded check. Migration v43 adds persisted run-group owner kind, session, and
turn columns. Existing run-owned groups decode as `Run`; historical rows are
not reclassified as turn-owned. `STORAGE_LAYOUT_VERSION` is 43. The SQLite
run/group/worktree tests and the full core suite passed after migration.

The legacy numeric TaskStore, legacy subagent lifecycle events, existing
run-owned groups, typed run IDs, and old snapshots remain additive-compatible.
No synthetic root task/run or destructive migration was introduced.

## 5. Root/nested fan-out, worktree, and authorization evidence

The root TaskTool `spawn_many` path now submits each accepted child through
the existing durable scheduler boundary, collects only returned durable run
IDs, and creates a turn-owned group. The turn-owned group test verifies exact
session/turn and top-level membership, while mismatched turns are rejected.
Nested run-owned group validation verifies direct-parent, common-root, and
parent-depth-plus-one membership.

The nested worktree regression creates a parent managed worktree, requests a
child from that checkout, and verifies common repository root, inherited base
commit, distinct path, and valid Git worktree registration. Existing
concurrent-worktree coverage continues to verify distinct top-level leases.

Authorization coverage verifies direct parent-to-child control succeeds and
same-session different-turn, sibling, child-to-parent, and self-control are
rejected.

## 6. Cancellation, restart, and contention review

The existing scheduler and group state machines were preserved. Focused
cancellation, contention, restart/recovery, worker, scheduler, and group
tests passed. No second machine-resource admission authority was added;
scheduler-owned execution continues to bypass the standalone pool semaphore.
Worktree lease release/retention behavior remains owned by the existing
service, including dirty/conflicted retention on failure or cancellation.

## 7. Exact focused verification

All commands were run on the implementation commit with the repository RTK
wrapper and bounded test parallelism:

- `rtk cargo test -p codegg-core --locked -- --test-threads=1` — 512 passed.
- `rtk cargo test -p codegg-core agent_run --locked -- --test-threads=1` — 15 passed.
- `rtk cargo test -p codegg-core agent_run_group --locked -- --test-threads=1` — 6 passed.
- `rtk cargo test --lib agent::run_control --locked -- --test-threads=1` — 4 passed.
- `rtk cargo test --lib agent::worker --locked -- --test-threads=1` — 4 passed.
- `rtk cargo test --lib scheduler --locked -- --test-threads=1` — 75 passed.
- `rtk cargo test --test subagent --locked -- --test-threads=1` — 22 passed.
- `rtk cargo test --test worktree --locked -- --test-threads=1` — 14 passed.
- `rtk cargo test --test scheduler_cancellation --locked -- --test-threads=1` — 10 passed.
- `rtk cargo test --test scheduler_contention --locked -- --test-threads=1` — 14 passed.
- `rtk cargo test --test scheduler_restart_recovery --locked -- --test-threads=1` — 15 passed.
- `rtk cargo test --lib tool::task --locked -- --test-threads=1` — 1 passed (max-depth boundary).
- `rtk cargo check --workspace --all-targets --locked` — passed.
- `rtk bash scripts/verify.sh quick` — passed.
- `rtk cargo fmt --all -- --check` — passed.

Relevant boundary, ownership, path, projection-disclosure, and catalog guards
all passed, including core boundary, scheduler bypass, execution ownership,
daemon cwd, identity path, Git forbidden-pattern, projection transport, and
WebSocket bounds checks.

## 8. Verification limitations and unresolved findings

No M007-scoped high, medium, or security finding remains. M008 owns F6/F7 and
is intentionally promoted only after this record is accepted.

Strict workspace Clippy currently reports two pre-existing untouched baseline
issues: `clippy::useless-vec` in
`crates/codegg-protocol/src/projection/reducer.rs:1735` and
`clippy::too-many-arguments` in the existing
`collect_agent_run_result` helper in `src/scheduler/executors.rs:968`.
They are not caused by M007 and are not part of this corrective scope.

## 9. Recommendation

Accept M007 as closed. Promote M008 from blocked to ready for its independent
call-identity, projection-depth, and strict final closure pass. Keep the
historical M001–M006 closure records unchanged and retain M006 as the
superseded strict subsystem disposition.
