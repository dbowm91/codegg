# Agent Runtime, Model Adaptation, and ACP Milestone 016 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/agent-runtime-model-adaptation-acp/016-descendant-admission-cancellation-and-execution-context.md`
Source subsystem roadmap: `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md#milestone-016--descendant-admission-cancellation-and-execution-context`
Repository baseline reviewed: `5a5c0fe23a9c681ff3b6c763ed83524a021aa421`
Implementation commits: `8a29926e` and `5a5c0fe2`

## 1. Executive finding

M016 is strictly closed. Descendant admission is reserved atomically before
queue acceptance, every accepted request owns one idempotent active-capacity
lease, root cancellation is isolated from unrelated roots, and descendant
workspace roots are propagated explicitly into native tool execution context.
No durable AgentRun, worktree isolation, team authorization, or distributed
execution capability was introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Atomic active admission | `AdmissionRegistry::admit` performs capacity, identity, fan-out, and cumulative budget checks and mutations under one mutex; barrier test accepts exactly 2 of 8 requests at capacity 2. |
| Rejection rollback | Lease is created only after all checks pass; queue-send failure drops the lease. Unit coverage verifies rejected admission does not consume active capacity or identity. |
| Exactly-once release | `DescendantAdmissionLease::Drop` is idempotent and owns active release; worker, semaphore cancellation, queue failure, panic/abort, timeout, and shutdown all drop the lease. |
| Distinct limits | Architecture and config docs define `max_concurrent`, accepted queued/running `max_active_descendants`, lifetime direct-child fan-out, and cumulative tool-call budget separately. |
| Root cancellation isolation | Pool stores one token per root lineage, queues select on lineage and global tokens, and exposes `cancel_lineage`; global shutdown remains independent. |
| Explicit workspace context | `SubAgentRequest.workspace_root` propagates from task tools, security review, specialized runtime, scheduler payloads, and durable job allowed roots into `AgentLoop`. |
| No process-global cwd authority | `build_tool_execution_context` uses the explicit loop workspace root with a non-authoritative `.` compatibility fallback; the cwd guard passes and no agent/tool execution context reads `current_dir()`. |

## 3. Production implementation evidence

- `SubAgentPool` now has one synchronous admission registry rather than
  independent async rollback maps and a check-then-increment counter.
- Accepted task/delegation identities and cumulative fan-out/tool budgets remain
  retained for deterministic duplicate and lifetime-budget semantics; active
  capacity alone is released on terminal completion.
- Queued and running work observes both the pool token and its root token.
- Aborted worker response channels are recorded as interrupted task results,
  preserving shutdown semantics.
- Descendant task construction inherits the parent workspace root and keeps
  existing narrowing-only denied-tool/path/model/depth behavior.

## 4. Verification executed

Passed locally:

```text
cargo fmt --all -- --check
cargo check -p codegg --all-targets                         # 0 errors, 6 existing warnings
cargo test --lib agent::worker::admission_tests             # 2 passed
cargo test --test subagent -- --test-threads=4              # 22 passed
cargo test --test agent_loop_harness -- --test-threads=4   # 40 passed
python3 scripts/check_daemon_cwd_usage.py                  # passed
python3 scripts/check_project_agent_pwd_inference.py       # passed
python3 scripts/check_scheduler_bypass.py                   # passed
python3 scripts/check_execution_ownership.py                # passed
python3 scripts/check_tool_broker_boundary.py               # passed
python3 scripts/check_builtin_agents.py                     # passed
python3 scripts/generate_builtin_agents.py --check           # passed
```

`cargo test -p codegg tool::task` was started but terminated after it spent
multiple minutes traversing unrelated integration binaries without producing a
completion result. It is not represented as passing evidence; the subagent and
agent-loop suites are the focused coverage for this change.

## 5. Invariant review

The shared pool remains the only descendant admission authority. Limits are
checked before queue acceptance, rejection leaves no active lease, duplicate
identities remain deterministic, and parent authority remains narrowing-only.
Mutation-capable parallel children are not described as isolated.

## 6. Failure and recovery review

Queue send failure, global or lineage cancellation before semaphore acquisition,
provider/tool failure, wall-clock timeout, worker abort, and pool shutdown all
fall through lease drop. Shutdown retains bounded cooperative waiting and abort
fallback. Durable restart reconciliation remains intentionally deferred.

## 7. Migration and compatibility review

`SubAgentRequest.workspace_root` is additive and all in-tree constructors were
updated. Existing configuration names and defaults remain readable. No storage
or protocol migration is required.

## 8. Security review

Workspace roots are inherited explicitly, not reconstructed from session labels
or process cwd. Existing denied tools, allowed paths, model inheritance, and
depth ceilings remain in force. The implementation does not add authorization
claims or widen mutation scope.

## 9. Documentation and operations

Updated `architecture/agent.md`, `architecture/config.md`, and the cwd static
guard documentation/allowlist. The plan is marked implemented and the
corrective addendum now records M016 closed.

## 10. Unresolved findings

- Low: legacy isolated `AgentLoop` fixtures without a workspace root retain an
  explicit non-authoritative `.` context. Production runtime construction and
  descendant paths now provide a workspace root; a future cleanup may remove
  the compatibility fallback after legacy fixtures are migrated.
- No critical, high, or medium M016 finding remains.

## 11. Roadmap disposition

M017 is promoted to `ready`: M012, M013, M014, M015, and M016 now each have
strict closure records. M017 owns the independent cross-milestone audit and
must not be treated as complete merely because M016 is closed.

## 12. Registry updates

Removed M016 from blocked/newly-ready work, recorded it under recently closed
work, and promoted M017 from blocked to ready in `plans/registry.md`. No other
registered blocker was resolved by this closure.
