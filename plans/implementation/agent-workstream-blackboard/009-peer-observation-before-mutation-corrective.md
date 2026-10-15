# Agent Workstream Blackboard Corrective 009 — Peer Evidence Before Mutation

Status: active

Planning baseline: `6f3c9c91bac5d0e3a46c930d2d992d8f7b7ce790` (2026-10-15)

Source roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md#corrective-m009--peer-evidence-before-mutation`
Original milestone: `plans/implementation/agent-workstream-blackboard/004-peer-observation-and-tools.md`
Original closure: `plans/closure/agent-workstream-blackboard/004-status.md`
Applicable ADRs: ADR-0001, ADR-0002, ADR-0004, ADR-0031; ADR-0032 remains proposed until M004 strict closure.
Primary class: invariant / capability corrective.

## 1. Objective

Close M004's strict before-next-mutation timing and qualification gaps without
mutating an in-flight provider transcript, adding sibling control authority,
or waking idle runs. A group member must consume a newly available relevant
peer observation before dispatching a model-returned mutation; an observation
that arrives during a provider request must fence the returned mutation batch.

## 2. Readiness

M004 production behavior is implemented and conditionally closed. This
corrective is limited to the named medium timing finding and required
deterministic acceptance evidence. Existing M003 claim CAS, run-group scope,
ToolBroker authority, and provider-boundary user-message encoding remain the
authorities. It does not authorize shared-checkout mutation under M005.

## 3. Existing evidence and unclosed findings

- `plans/closure/agent-workstream-blackboard/004-status.md` §4 records that a
  hint arriving during an in-flight provider request can miss the returned
  tool batch, and that the two-worktree fake-provider/Git acceptance test is
  absent.
- `src/agent/loop.rs` drains peer hints before provider requests;
  `src/agent/tool_batch.rs` currently dispatches accepted calls without a
  second peer-observation boundary.
- `tests/agent_loop_harness.rs` already provides scripted provider request
  capture and can be extended for deterministic group trajectories.

## 4. Invariants

- Provider request messages remain immutable while the provider is in flight.
- Peer payload remains allowlisted, bounded, same-session/group scoped and
  explicitly untrusted user data; no raw tool output is elevated.
- Only ToolBroker executes production model tool calls; heavy work keeps its
  existing scheduler/job authority.
- The correction adds no filesystem, Git, scheduler, sandbox or sibling-control
  authority. It does not claim to serialize shared checkout writers.
- No-group and single-agent execution sends no peer payload and does not pay a
  new per-tool coordination wait.
- If continuity is unknown, no mutation is dispatched until the model has an
  explicit gap and can refresh the canonical board.

## 5. Scope

In scope: a safe post-provider/pre-mutation observation fence, deterministic
test seams for peer-event timing, two-member fake-provider/fake-result
qualification, and evidence for bounded delivery, gap behavior, project/group
isolation, hostile payload handling, and single-agent behavior.

Out of scope: shared worktree placement, a shared-write lease, mid-stream
provider transcript mutation, idle wake, sibling messaging, new durable event
storage, standalone scheduler test-job evidence, and unrelated provider Clippy
warnings.

## 6. Required changes

1. Add an observation fence after a provider response is complete and before
   any returned tool call can execute. The fence drains only when a peer group
   is active. If it discovers a new observation or continuity gap, preserve
   the observation in the next request transcript and return a typed
   non-executed outcome for every call in that response. Refusing the whole
   response batch keeps provider-issued read and write actions in one coherent
   ordering and prevents any action from obscuring the newly arrived fact.
2. Ensure the caller records the assistant tool call and a bounded typed result
   consistently, then gives the model the untrusted observation before it can
   issue a replacement action. A gap requires a board refresh before retry.
3. Scope the fence to known same-group runs. No receiver or per-tool work for
   non-group/single-agent turns. Preserve behavior for stale/unrelated events.
4. Add deterministic tests that control event publication while the provider
   is blocked, between response completion and tool dispatch, and during a
   multi-call batch. Assert no affected mutation executes before the peer fact
   is visible in a subsequent provider request.
5. Add integrated two-worktree fixtures: peer A produces a host-owned commit
   and changed-path evidence; peer B has an overlapping claim and a scripted
   mutation. Assert one relevant observation precedes B's next mutation,
   unrelated changes are filtered, and neither prompt prose nor tool output
   becomes authoritative evidence.
6. Cover duplicate delivery, lag/reconnect/restart gaps, compaction, cancel,
   overflow, 16-member/large-payload bounds, cross-project isolation, sibling
   authorization, and the no-group/single-agent fast path. Reuse existing M003
   authorization and CAS tests where they already prove the same contract;
   cite the exact test rather than duplicating it.

## 7. Failure and recovery semantics

An observation or gap discovered at the fence cannot silently disappear if a
mutation is refused. The event remains in the message history for the next
provider request. A broadcast lag yields a gap and requires a canonical board
refresh; it never fabricates continuity or retries the refused mutation
automatically. Cancellation before dispatch still prevents execution. A
provider error before a complete response produces no tool batch and cannot
execute abandoned calls.

## 8. Compatibility and migration

No database migration, public protocol change, or persisted event format is
expected. Internal typed outcomes may add an enum case only if all projection,
tool-result, and recovery matches are updated. Existing single-agent and
non-group child behavior remains unchanged.

## 9. Required verification

- Focused peer-fence and scripted two-worktree tests, including event timing
  barriers and exact provider-visible message order.
- Existing peer-observation, task-tool, M003 claim, projection, agent-loop,
  compaction, run-control, and authorization tests affected by changes.
- `cargo fmt --all -- --check`
- `cargo test --offline -p codegg --test agent_loop_harness`
- focused M004/M003 tests and `bash scripts/verify.sh quick`
- `cargo clippy --offline --workspace --lib --locked -- -D warnings`
- applicable scheduler, execution-ownership, core-boundary, projection, and
  authorization guards; record all-target Clippy separately if the known
  provider-test warning persists.

## 10. Acceptance criteria

- A relevant peer commit/evidence event published during B's provider request
  is model-visible before B's returned mutation can execute; that mutation is
  refused once and is not replayed automatically.
- A subsequent scripted provider response can refresh the canonical board and
  choose a new action; unrelated changed paths do not block or notify B.
- Duplicate, lagged, restarted and compacted event paths yield bounded
  de-duplicated facts or an explicit gap, never false continuity.
- Cross-project events and untrusted output cannot leak into B's observation.
- A single-agent baseline receives no observation and performs no peer-event
  work at the tool boundary.
- All original M004 acceptance criteria have a requirement-to-evidence entry;
  unresolved findings are low-severity only and do not falsify an M004
  invariant/capability.

## 11. Stop conditions

Stop and revise the contract if enforcing the fence requires mutating an
in-flight provider request, creating a second execution authority, parsing
agent prose as Git/test evidence, or providing a shared mutation lease. If
events cannot be ordered strongly enough to establish the criterion, report
that and retain conditional closure rather than claiming strict completion.

## 12. Closure evidence

Write `plans/closure/agent-workstream-blackboard/009-status.md`, with a full
M004 acceptance matrix, explicit relationship to the immutable conditional
004 closure, test/guard commands and results, failure/restart/security review,
and the M005/M006/M007/M008 dependency audit. On strict success, mark M004
strictly closed through this corrective; M005 still requires accepted ADR-0032
and verified writer-path containment.
