# Agent Loop Completion Reliability Corrective Addendum

Status: closed

Audit date: 2026-11-22
Repository baseline: `d585e75f895cf41cd2241512d8d7e7c37fcece99`
Agent-loop sources were read in this worktree at that baseline. This review did not modify them.

Long-term references:
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#7-agent-terms`

Related ADRs:
- None required for these milestones. C001 uses the existing `stop_reason` string on `CoreEvent::TurnCompleted` and `AppEvent::AgentFinished`. C004 reads the existing `is_error` flag. C005 uses the existing scoped unregister. A new protocol variant, a new continuation engine, or a mid-tool wall-clock kill would be a stop, not a silent decision.
- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md` stays accepted. This addendum does not restore a stall terminal.

Predecessor, not reopened:
- `plans/subsystems/agent-loop-operational-reliability-roadmap.md` (closed, M001–M007). Its audit recorded `ExecutionLimits` defaults of 100 turns, 1,000,000 tokens, and 600 seconds as safeguards. It did not require the stop to be distinguishable from a normal completion, and it did not require the token counter the cap reads to move.
- `plans/closure/agent-recovery-escalation-authority/001-status.md` and `002-status.md`. The recovery ladder does not terminate the turn. `progress_recovery.rs` records that the outer bound is `ExecutionLimits::max_turns`. That split is kept.
- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md` and `plans/subsystems/long-horizon-work-execution-roadmap.md`. Provider retry, approval, and Goal/WorkPlan stay with their owners.

## 1. Purpose and ownership boundary

Make a long tool-using turn stop honestly, and make a failed tool call look failed, so a trajectory that needs many tool rounds is not reported as finished when it is not.

Owns: the production consumption of `AgentLoop::check_limits`; the follow-up drain that can outlive that budget; an exhausted unparseable `tool_calls` stop; an MCP `isError` result marked successful; and a question batch that keeps one receiver for every question.

Does not own: tool-executor timeouts, the documented indefinite approval wait, provider retry policy, compaction strategy, Goal/WorkPlan design, or a new continuation service. Those stay with their current owners. This addendum may only require that a loop-budget stop, a drain failure, an unparseable tool-call stop, an MCP tool error, and a mis-paired question are not treated as success.

## 2. Work classification

### Invariants

- A turn that stops because `check_limits` returned a reason is not published as a successful completion.
- The reason that stopped the loop is the reason that is published. The previous provider `Finish` reason must not overwrite it.
- User interrupt and cancellation stay `interrupted`. A limit break does not steal that classification, and an interrupt does not get relabeled as a budget stop.
- `AgentLoopState::total_tokens` is either updated from provider usage the run already records, or the cap that reads it is deleted. A cap that cannot fire is not a safeguard.
- No mid-tool wall-clock kill is added. `architecture/agent.md` forbids a hard total-turn deadline because it cuts off the long builds the per-tool timeout exists to allow.

### Capabilities

- A person watching the session can tell that the agent stopped because it hit its turn, tool-call, token, or iteration-boundary timeout budget, and that the task was not finished.
- The next user turn can see that handoff. The model is not left to guess from a partial assistant message that the previous turn completed.

### Infrastructure

- The existing `stop_reason` string on the completion event and on `AppEvent::AgentFinished`. No new store.

### Polish

- Align the architecture note that says `ExecutionLimits.timeout` is consulted only after a turn completes with the iteration-boundary check that actually exists.

## 3. Non-goals

- Raising `max_turns` above 100, or any other numeric cap, as the fix. A higher silent cap still ends a hundreds-of-tool-calls trajectory as a normal completion.
- Restoring `RecoveryDecision::Stalled` or making unknown progress terminal. ADR-0040 Option B stands. The ladder's bound remains `max_turns`.
- Adding a wall-clock kill around an in-flight tool batch. The 600-second check runs at the start of a provider iteration. That is the accepted shape.
- Auto-continuing a budget stop into another identical capped loop.
- A new Goal, WorkPlan, idle-continuation, or queue design.
- Changing the documented approval default. `approval_timeout_secs` omitted means the turn stays parked until the user answers. That is not a defect to close by adding a deadline.
- Provider retry policy. Attempts stay capped; a dropped partial stream is not replayed; context-overflow remediation stays one changed-request attempt. Those bounds end as errors, not as empty success.
- Replacing the `compaction.auto` default or the `auto=false` `DropMiddleMessages` compatibility path. That path is the accepted compaction contract. Context continuity and ADR-0054 are not reopened.
- A durable stall event or a mid-tool kill for the TUI silence watchdog. The watchdog is local and the turn is still running. A new terminal event would be an ADR, not a milestone here.
- Goal redesign. `MAX_CONTINUATIONS` remains the closed long-horizon emergency cap. The goal stays durable when it halts. This addendum does not reopen that line.
- Routing `mcp__` through `ToolBroker`. C004 only stops an `isError` result from being marked successful.

## 4. Current state

`ExecutionLimits::default` is 100 turns, 1,000,000 tokens, and 600 seconds (`src/agent/loop.rs`, `impl Default for ExecutionLimits`). Built-in agents leave `steps: None` (`src/agent/definition.rs`), so the graceful `agent.steps` path does not run for a normal turn.

`AgentLoop::check_limits` (`src/agent/turn_completion.rs`) returns a reason for max steps, max turns, max tool calls, max tokens, timeout, and user interrupt. The only caller that stops the run is `src/agent/loop.rs`: it logs `Agent loop stopping: {reason}` at info and `break`s. The reason is not stored.

`turn_count` increments once per provider iteration, before the provider call. A trajectory that needs one provider round per tool batch stops at 100 rounds. Hundreds of sequential tool rounds cannot finish inside one user-facing turn. Batched calls can pass 100 tool calls sooner; they still stop at 100 provider rounds with no handoff.

After the loop returns `Ok`, `DefaultTurnRuntime` publishes `CoreEvent::TurnCompleted`. `turn_usage` takes `stop_reason` from the last `ChatEvent::Finish`, or the literal `completed` when there is none. A limit break therefore publishes the previous provider finish reason, or `completed`. `publish_agent_finished` has the same hole: steering and cancel become `interrupted`; every other finish-less stop becomes `completed`.

`agent.steps` is the contrasting path. On the iteration that would hit the step cap, the loop injects a termination instruction, strips tools, and lets the model summarize. `ExecutionLimits` breaks before that injection.

`AgentLoopState::total_tokens` is initialized to 0. A search of `src/agent` found no assignment. The 1,000,000-token cap cannot fire. Run usage is accumulated separately on `run_input_tokens` and `run_output_tokens`.

`host/turn.rs` maps the strings `max_turns`, `max_steps`, `max_tool_calls`, and `tool_calls` to `StopReason::MaxTurns`. That mapper is not on the path that discards `check_limits`'s reason. Do not treat it as evidence that the session already sees the stop.

`maybe_continue_goal` runs after the loop (`src/agent/loop.rs`) for a session that has an active goal. It is bounded by `MAX_CONTINUATIONS = 32` and refuses goals whose own budget decision says stop. It does not read the loop-limit reason, because that reason was discarded. A loop-budget stop must not be mistaken for "the model finished."

The architecture note under "Tool-call timeouts" says there is still no total turn deadline and that `ExecutionLimits.timeout` is consulted only after a turn completes, while the batch is awaited without an outer wrapper. The code consults the timeout at the next iteration boundary, which matches the "do not kill a long tool" rule and contradicts the "only after a turn completes" wording. The note's prohibition on a hard cut-off stays.

## 5. Target architecture

Provider iteration starts. `check_limits` either allows the iteration or returns a reason. A reason is retained on the loop. User interrupt and cancel stay `interrupted`. Any other limit reason is published on `TurnCompleted.stop_reason` and `AppEvent::AgentFinished.stop_reason`, and is not replaced by the previous provider `Finish`.

The model-visible handoff follows the existing `agent.steps` shape: at most one summary provider call with tools removed, and no further tool batch. If that call cannot be made without crossing a timeout or a cancel, the stop is published without a second call. The next user turn's prepared context includes the same reason, using the continuation checkpoint or the transcript the daemon already owns. No new store.

`total_tokens` moves from the usage the run already records. When it crosses `max_tokens`, the same visible stop fires. The cap is not deleted unless wiring it would double-count or invent a usage source; that case is a stop, not a quiet deletion.

Goal continuation, if it runs, sees the loop-budget stop and does not start another identical capped loop.

## 6. Dependency graph

```text
closed operational-reliability M001–M007
closed recovery-escalation C001/C002
accepted ADR-0040 Option B
        |
        v
C001 budget stop is a visible handoff
        |
        v
C002 follow-up drain stays inside the turn

C003 unparseable tool-call stop is not success   (no predecessor)
C004 MCP tool error is not a successful result   (no predecessor)
C005 each question call keeps its own answer     (no predecessor)
```

No hard dependency on an open predecessor outside this addendum. C002 uses C001's published budget reason. C003, C004, and C005 do not. The tool-failure, retry/compaction, and stall-reporting audits have returned. Their confirmed defects are C004 and C005, or are already C001–C003. The rest are recorded under risks and are not milestones.

## 7. Milestones

### C001 — Budget stop is a visible handoff

Class: invariant, with a capability consequence.

Objective: a `check_limits` stop is published as that stop, the token cap reads a counter that moves, and the model gets at most the summary handoff `agent.steps` already uses.

Dependencies: none. The predecessor workstreams are closed.

Deliverable boundary: `src/agent/loop.rs`, `src/agent/turn_completion.rs`, the `TurnCompleted` publication in `src/agent/turn_runtime.rs`, and the architecture note that describes the timeout check. Tests in the existing agent-loop family. No protocol variant. No new continuation service.

Exit conditions: a max-turns stop, an iteration-boundary timeout stop, and a token-cap stop each publish a reason other than `completed` and other than the previous provider finish reason; an interrupt still publishes `interrupted`; `total_tokens` changes when provider usage is recorded; a goal continuation does not relaunch an identical capped loop after the stop; the daemon notification does not classify that stop as an ordinary completion; no `Stalled` variant appears; no in-flight tool is cancelled by the 600-second check.

### C002 — Follow-up drain stays inside the turn

Class: invariant.

Objective: a queued follow-up is processed inside the same execution budget as the turn that drains it, or it is visibly not processed. A tool-execution failure during that drain is not returned as success.

Dependencies: C001's published budget reason. Independently executable against that contract; implement after C001 or stop if the publication is absent.

Deliverable boundary: `src/agent/follow_up.rs`, the goal-continuation `try_send` failure in `src/agent/turn_completion.rs`, and the sender-unregister order only if the post-drain window drops an accepted prompt. No new channel. Capacity stays 32.

Exit conditions: the drain calls `check_limits` and the cancel check before each provider call; a limit or cancel uses C001's publication; a tool-executor `Err` is not `Ok(())`; a full-channel `try_send` is visible on the turn, not only a warn log.

### C003 — An unparseable tool-call stop is not success

Class: invariant.

Objective: when the model stops with `stop_reason=tool_calls` and the existing parse retries are exhausted, the turn is not published as a successful completion.

Dependencies: none. Both loop copies must change. Fixing one is not closure.

Deliverable boundary: the exhausted `tool_calls` arm in `src/agent/loop.rs` and the copy in `src/agent/follow_up.rs`. No new parser and no new retry.

Exit conditions: both arms fail the turn, or set a retained terminal reason that `completed` cannot overwrite; `NeedsUserJudgment` and `BlockedReport` are unchanged; the existing 600-character preview is not grown.

### C004 — An MCP tool error is not a successful result

Class: invariant.

Objective: an MCP `tools/call` result with `isError: true` is not inserted as a successful tool result.

Dependencies: none.

Deliverable boundary: the `mcp__` arm in `src/agent/tool_batch.rs`. Read `is_error` from the structured result the service already returns. Do not move the route onto `ToolBroker`.

Exit conditions: the local fixture shape (`isError: true`, text `done`) is a non-success outcome and a non-success session tool event; an `isError: false` result is still success; the 60-second timeout is unchanged.

### C005 — Each question call keeps its own answer

Class: invariant.

Objective: two `question` calls in one batch do not share one receiver or one answer, and cleanup removes each question by its own id in the owning session.

Dependencies: none.

Deliverable boundary: the question register and consume path in `src/agent/tool_batch.rs`. Use `unregister_scoped`. Do not change the 300-second wait or the approval-timeout default. Do not merge this with the exec-mode single channel.

Exit conditions: two questions return two distinct answers; cleanup does not pass the session id to `unregister`; a single question still works.

## 8. Cross-cutting requirements

- Storage: none. Do not add a table or bump `STORAGE_LAYOUT_VERSION`.
- Protocol: use the existing `stop_reason` string. If that field is a closed enum that cannot carry the reason, stop and write an ADR. Do not add a variant inside this milestone.
- Security: no permission, sandbox, or writer-fence change.
- Concurrency: the reason is loop-local. Do not publish it from a spawned hook task.
- Observability: the info log may stay. The session event is the authority. A log-only stop is the defect.
- Docs: correct `architecture/agent.md` so the timeout check, the visible stop, and the prohibition on a mid-tool kill are the same story.

## 9. Verification strategy

Focused agent-loop tests that drive `check_limits` to each production reason and assert the published `stop_reason`. One test that records provider usage and asserts `total_tokens` moves. One test that an in-flight tool batch is not wrapped in the 600-second loop timeout. No live-provider run. No hour-long trajectory. `scripts/verify.sh quick` is the merge bar for the implementation turn, not for this planning turn.

## 10. Risks and decision points

- The summary handoff is one extra provider call. It must be impossible for that call to dispatch tools or to recurse into another summary. If the existing steps injection cannot be reused without recursing, publish the stop with no extra call and record that as the handoff. Do not invent a third shape.
- Wiring `total_tokens` from `run_input_tokens + run_output_tokens` may count cached or reasoning tokens differently from the provider's context window. Use the counters already stored on `AgentLoopState`. Do not add a second usage parser. If those counters are not yet populated at the check site, that is the wiring work, not a reason to invent a source.
- Goal continuation's interaction is a required test, not a redesign. If the test shows it already refuses, record that and do not change it.
- A later reading must not fold a closed line back into C001 to "finish the review." The audits below were read against the baseline. Rows that are not milestones are not forgotten; they are owned elsewhere or not defects.

Audited, and not milestones:

- Human approval with `approval_timeout_secs` omitted waits indefinitely. The schema documents that as the default: the dialog stays up and the turn stays parked until the user answers. Do not add a deadline.
- The TUI silence watchdog latches a local warning and does not publish a terminal event. The daemon turn is still running. Do not add a mid-tool kill, and do not add a terminal event without an ADR.
- `maybe_continue_goal` logs and returns `Ok(())` when the goal store fails to load or when `MAX_CONTINUATIONS` is hit. The closed long-horizon line owns that emergency cap. The goal remains durable. Do not reopen it.
- Default compaction (`compaction.auto` unset, so false) uses `DropMiddleMessages` and installs no continuation candidate. That is the documented compatibility path. Context continuity and ADR-0054 are not reopened. The audit did not prove that kept messages lack artifact handles.
- Context-overflow recovery sends one changed request on a fresh one-attempt retry context, then propagates the error. That bound is intentional. Do not widen it.
- Provider idle timeouts and the recovery ladder are bounded. Provider errors publish `TurnFailed`. Recovery does not restore `Stalled`.
- Checkpoint and artifact awaits have no explicit deadline. No stall was reproduced. Not a milestone.
- Cooperative cancel projecting as completion is C001's interrupt rule, not a new milestone. The projection stores `stop_reason` and sets `TurnStatus::Completed`. C001 does not add a status variant.

## 11. Completion definition

C001 is closed only when its closure record shows the published reason, the live token counter, the non-recursion of the summary handoff, the goal-continuation non-relaunch test, and a notification that does not call the budget stop an ordinary completion. C002–C005 close only on their own records. A compile-only change is not closure. This addendum is not a claim that every stall in the agent loop has been found. It is the claim that the confirmed defects from this review are the five milestones, and that the other audited rows were refused as milestones for the reasons above.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 | closed | `plans/implementation/agent-loop-completion-reliability-corrective/001-budget-stop-is-a-visible-handoff.md` | `plans/closure/agent-loop-completion-reliability-corrective/001-status.md` | none |
| C002 | closed | `plans/implementation/agent-loop-completion-reliability-corrective/002-follow-up-drain-cannot-bypass-the-turn.md` | `plans/closure/agent-loop-completion-reliability-corrective/002-status.md` | C001 publication (satisfied) |
| C003 | closed | `plans/implementation/agent-loop-completion-reliability-corrective/003-unparseable-tool-call-stop-is-not-success.md` | `plans/closure/agent-loop-completion-reliability-corrective/003-status.md` | none |
| C004 | closed | `plans/implementation/agent-loop-completion-reliability-corrective/004-mcp-tool-error-is-not-a-successful-result.md` | `plans/closure/agent-loop-completion-reliability-corrective/004-status.md` | none |
| C005 | closed | `plans/implementation/agent-loop-completion-reliability-corrective/005-each-question-call-keeps-its-own-answer.md` | `plans/closure/agent-loop-completion-reliability-corrective/005-status.md` | none |
