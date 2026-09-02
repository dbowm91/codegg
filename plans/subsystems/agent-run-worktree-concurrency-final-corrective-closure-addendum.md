# Agent Run, Async Delegation, and Worktree Concurrency Final Corrective Closure Addendum

Status: active — M009 ready

Repository baseline reviewed: `d08f089f7a72319eb343a070c93369cbb4fc50a4`

Historical corrective roadmap retained:

- `plans/subsystems/agent-run-worktree-concurrency-corrective-closure-addendum.md`

Historical strict closure records retained:

- `plans/closure/agent-run-worktree-concurrency/006-status.md`
- `plans/closure/agent-run-worktree-concurrency/007-status.md`
- `plans/closure/agent-run-worktree-concurrency/008-status.md`

Current corrective implementation plan:

- `plans/implementation/agent-run-worktree-concurrency/009-root-turn-notification-invocation-scope-and-exact-head-closure.md`

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

## 1. Corrective disposition

M007 and M008 fixed the major production composition failures discovered after M006: root/run orchestration ownership, nested durable lineage and depth, context propagation, direct-parent control authorization, managed nested worktree isolation, request fingerprints, and authoritative projection depth.

A subsequent audit of the exact M008 closure head found a smaller final set of defects. These do not justify reopening the durable-run architecture, but they do invalidate the M008 statement that no corrective finding remained and that the subsystem had returned to strict closed status.

M008 therefore remains a historical closed implementation record whose strict subsystem disposition is superseded by this addendum until M009 closes.

## 2. Findings owned by M009

### F8 — root turn does not receive top-level child completion push

Nested completion is routed by `parent_run_id` to a live parent-run handle. Top-level delegated runs intentionally have no parent run because M007 made their owner the originating turn. `RunControlService` currently has no equivalent live `(session_id, turn_id)` follow-up endpoint, so primary orchestration still relies on `wait`/`status` for ordinary top-level child completion.

### F9 — turn-owned group completion/projection still routes through run-centric seams

Turn-owned groups persist an explicit `AgentRunGroupOwner::Turn`, but live terminal delivery still consults compatibility `owner_run_id`. Member-terminal recomputation also does not consistently publish the authoritative terminal `AgentRunGroupUpdated` projection unless a later TaskTool group action causes another publication.

### F10 — accepted tool-call identity is not occurrence-scoped

The normal AgentLoop currently derives `ToolExecutionContext.invocation_key` from `session_id + provider tool_call_id` and leaves `turn_id` unset. Provider IDs are not session-global. Codegg's own text-repair path restarts IDs such as `text-repair-0` for each repaired response, so separate accepted tool calls can alias.

The correct scope is the explicit execution owner (root turn or durable current run) plus the provider-turn/accepted-response occurrence plus provider call ID.

### F11 — durable delegation identity still incorporates request content

M008 added a separate request fingerprint, but the delegation key still hashes request payload fields. Same-call/different-request replay can therefore become a different delegation identity instead of exercising the intended fingerprint conflict.

### F12 — exact M008 closure head is red in the existing normal CI lane

The push workflow for `d08f089f` (`33566227214`, verify job `100049920583`) failed in Workspace Clippy on `collect_agent_run_result` having too many arguments; Workspace tests were skipped. This is a narrow repository-health issue, not evidence that the M007/M008 behavioral fixes regressed, but strict closure must clear the existing lane on the exact accepted final candidate.

## 3. M009 scope

M009 owns exactly these corrections:

1. register an active root-turn follow-up endpoint in the existing run-control service;
2. route top-level child completion to the exact originating active turn;
3. route turn-owned group completion by persisted group owner rather than compatibility run metadata;
4. publish group projection from member-terminal recomputation;
5. scope accepted model tool calls by explicit turn/run owner and provider-turn occurrence;
6. separate delegation identity from request fingerprint validation;
7. clear the existing Workspace Clippy blocker with the smallest maintainable change;
8. independently re-ratify strict subsystem closure.

## 4. What remains accepted and must not be redesigned

The following M007/M008 results remain accepted dependencies:

- root orchestration is turn-owned;
- nested orchestration is current-run-owned;
- parent/root/depth lineage is durable and store-derived;
- max depth is enforced before descendant admission;
- scheduler is sole daemon machine-resource admission authority;
- nested TaskTool receives project/repository/workspace/turn/group/control context;
- mutation-capable children receive isolated managed worktrees;
- control authorization is exact-turn for top-level runs and direct-parent for nested runs;
- call/request fingerprints are bounded durable metadata;
- projection depth comes only from `AgentRunRecord.depth`;
- group owner kind is persisted and projected additively;
- child completion never integrates Git state implicitly.

M009 MUST NOT replace these contracts merely to make the final correction easier.

## 5. M009 architecture boundary

The intended final execution/notification shape is:

```text
Root Turn(session, turn)
  AgentLoop
    Tool call identity = Turn owner + provider-turn occurrence + provider call id
    TaskTool spawn -> scheduler -> AgentRun(depth 1)
                         |
                         +-- terminal -> RunControlService
                                          -> exact live Turn(session, turn) follow-up
                                          -> run/group projection update

AgentRun(parent)
  child AgentLoop
    Tool call identity = current AgentRun + provider-turn occurrence + provider call id
    TaskTool spawn -> scheduler -> AgentRun(depth + 1)
                         |
                         +-- terminal -> exact live parent AgentRun follow-up
```

Turn-owned group delivery follows the same exact turn endpoint. Run-owned group delivery follows the current live run endpoint.

There is no synthetic root run and no second notification authority.

## 6. Verification posture

Keep verification deliberately narrow:

- focused root/nested completion-routing tests;
- focused turn/run group terminal routing and projection tests;
- focused invocation identity and request-conflict tests;
- restart-readable idempotency coverage;
- existing quick verification;
- exact workspace fmt/Clippy;
- the normal existing CI push lane on the exact final candidate.

Do not add CI jobs, matrices, scanners, benchmarks, coverage gates, dependency bots, release automation, or provider-dependent tests.

The failed M008 hosted run is evidence to be superseded, not a reason to expand verification architecture.

## 7. Closure criteria

This addendum may return to `closed` only when M009's closure record establishes:

- active root turns receive top-level child completion without polling;
- ended/non-owning turns cannot receive that completion;
- nested direct-parent completion remains correct;
- turn-owned and run-owned group completion route by explicit owner kind;
- terminal member recomputation publishes authoritative group projection;
- terminal completion push is not duplicated by restart/reconciliation;
- model invocation identities cannot alias merely because provider call IDs repeat across responses, turns, or runs;
- same accepted call replay is idempotent;
- changed request under the same accepted spawn identity conflicts explicitly;
- different accepted calls with identical request remain distinct;
- no M007 worktree/lineage/depth/authorization/scheduler invariant regresses;
- the existing normal `CI / verify` lane is green on the exact accepted final candidate and reaches Workspace tests;
- no critical/high/medium corrective finding remains;
- the registry and architecture docs reflect the final contract without rewriting M006–M008 history.

## 8. Registry policy

While M009 is open:

- subsystem status: `active`;
- controlling roadmap: this addendum;
- current milestone: `M009 ready`;
- M009 is the only dependency-ready plan for this subsystem;
- M006, M007, and M008 remain historical closure records;
- M008's strict subsystem disposition is superseded by this final corrective addendum.

After accepted M009 closure:

- mark M009 implemented/closed;
- mark this addendum closed;
- return the subsystem row to strict `closed`;
- preserve the failed M008 hosted run and post-closure findings as historical evidence rather than rewriting them away.
