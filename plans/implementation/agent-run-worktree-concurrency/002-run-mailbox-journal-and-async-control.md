# Agent Run, Async Delegation, and Worktree Concurrency Milestone 002 — Run Mailbox, Journal, and Async Control

Status: implemented

Repository baseline: `b08d33b7e52bde1bde1ddcddeeee3c7c157a4103`

Source roadmap:

- `plans/subsystems/agent-run-worktree-concurrency-roadmap.md#m002--run-mailbox-stable-boundary-journal-and-asynchronous-completion-delivery`

Long-term requirements:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`

Applicable ADRs:

- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

Primary class: capability/infrastructure

Hard blocker: M001 must close with canonical durable `AgentTask`/`AgentRun` ownership.

## 1. Objective

Give durable agent runs a bounded parent/child control plane and a stable-boundary execution journal so the parent can continue working while children run, receive useful progress/completion without polling loops, and safely send `message`, `interrupt`, `wait`, or `cancel` operations.

The mailbox is run control, not team chat. The journal is restart/recovery evidence, not hidden reasoning or a duplicate session projection log.

## 2. Why this milestone becomes ready after M001

M001 provides:

- stable run/task IDs and lineage;
- canonical scheduler/job/attempt ownership;
- durable terminal state and restart reconciliation;
- a single child execution service boundary;
- compatibility routing from the model-facing task tool.

Existing reusable runtime seams include:

- `AgentLoop` follow-up, steering, question, cancellation, provider-turn, and recovery state;
- scheduler cancellation and job wait/completion mechanisms;
- bounded broadcast/projection infrastructure;
- session store and artifact handles.

Without M001, mailbox messages would attach to transient pool/task identities and restart semantics would be ambiguous.

## 3. Current implementation evidence

Reconfirm before editing:

- the task tool is primarily `spawn`/`get` and parents commonly need explicit polling to inspect a child;
- `SubAgentSpawner::send_async()` queues work and returns, so asynchronous execution already exists underneath the model surface;
- `GlobalEventBus` publishes subagent start/progress/completion/failure but does not provide durable ordered control delivery;
- session projection replay is derived/frontend-oriented and must not become execution authority;
- `AgentLoop` already has in-memory follow-up/steer/cancel channels that can be fed from a durable mailbox at safe boundaries;
- `src/agent/team.rs` has file-backed inbox/outbox coordination but is unrelated to durable run ownership and should not be reused as the authority.

## 4. Invariants that must not regress

- Only authorized ancestors/owners can control a run.
- Mailbox input cannot widen child capabilities, path scope, model/tool budgets, worktree rights, or Git/network authority.
- Message/control ordering is deterministic per run.
- Delivery is at safe boundaries; `interrupt` does not claim to preempt an irreversible side effect already executing.
- Cancellation remains scheduler/run-owned and downward by default.
- Stable-boundary journal events are bounded and never contain hidden reasoning, credentials, or unbounded tool output.
- A journal replay must never cause a completed non-idempotent tool/Git operation to execute again.
- Session projection remains a derived consumer of authoritative run/mailbox/journal state.
- A disconnected parent/frontend does not cause child completion or control state to disappear.

## 5. Scope

### In scope

- durable `AgentRunMailboxMessage` records with typed message ID, run ID, sender/owner lineage, operation kind, bounded payload, state, and timestamps;
- ordered mailbox delivery and acknowledgement;
- operations: `message`, `interrupt`, `cancel`, `status`, bounded `wait`;
- direct parent↔child and root/parent→owned-group broadcast seam where group identity exists later;
- safe-boundary integration into `AgentLoop` follow-up/steering/cancellation;
- append-only stable-boundary `AgentRunEvent` journal;
- lifecycle events sufficient for restart and completion delivery;
- push of bounded progress/completion summaries to an active owning parent/session through existing control/follow-up/projection notification seams;
- TaskTool actions/additive aliases for status/message/interrupt/wait/cancel;
- bounded long-poll/wait response semantics that do not tie up unbounded scheduler resources;
- restart reconciliation of queued/delivered mailbox messages and terminal completion notices.

### Explicitly out of scope

- sibling-to-sibling free-form chat;
- project/team chat or human collaboration channels;
- arbitrary user-defined workflow messages;
- worktree lease creation;
- automatic run groups/join policies beyond a future-compatible message target seam;
- storing provider token deltas, hidden chain-of-thought, complete tool stdout/stderr, or full prompts in the journal;
- arbitrary rewind.

## 6. Required production changes

### Core/domain

Define small typed contracts, for example:

```rust
pub enum AgentRunControlKind {
    Message,
    Interrupt,
    Cancel,
}

pub enum MailboxState {
    Queued,
    Delivered,
    Acknowledged,
    Superseded,
}

pub struct AgentRunMailboxMessage {
    pub message_id: AgentRunMessageId,
    pub run_id: AgentRunId,
    pub sender_run_id: Option<AgentRunId>,
    pub kind: AgentRunControlKind,
    pub payload: String,
    pub sequence: u64,
    pub state: MailboxState,
    pub created_at: i64,
    pub delivered_at: Option<i64>,
}
```

Use existing typed identity conventions; add a new typed ID only if a stable message identifier is necessary. Keep payload bounds explicit.

Define a journal envelope with run ID, monotonic sequence, event kind, causation/correlation IDs, bounded metadata, and timestamp. Prefer versioned typed event variants over arbitrary JSON event names.

### Storage and migrations

Add mailbox and journal tables with:

- per-run monotonic sequence/unique ordering;
- indexed pending messages and recent journal retrieval;
- bounded payload constraints enforced before insert;
- idempotency/causation fields so retries do not duplicate controls;
- retention policy seam for terminal historical events.

A control message and its durable state must be committed before a live in-memory signal is published.

### Runtime and concurrency

Create a `RunControlService` owned by the daemon/run service. Its responsibilities:

1. authorize control against run lineage/session owner;
2. persist the message/control intent;
3. signal the live run if attached;
4. deliver at the next safe loop boundary;
5. persist delivery/ack state;
6. on restart, rediscover queued controls and apply only those still semantically valid;
7. surface terminal status/wait completion.

Safe boundaries should include before provider turn, after provider response before launching the next tool batch, after a tool batch completes, and before autonomous continuation/replan. Do not mutate the model transcript concurrently with an in-flight provider request.

`interrupt` means “reconsider trajectory before the next safe action.” If a long scheduler-owned child/tool job is active, interruption may request cancellation of that owned work only when the operation contract allows it; otherwise deliver the interrupt after the operation reaches a terminal boundary.

### Journal semantics

Record stable execution events only. At minimum:

- run created/queued/started;
- control queued/delivered;
- safe boundary/checkpoint reached;
- child-progress milestone if explicitly published;
- cancellation requested;
- terminal result produced;
- cleanup/recovery transitions relevant to restart.

Do not log every token/tool delta. Existing RunStore/tool/job records remain authoritative for detailed execution artifacts.

### Model-facing task/delegation surface

Extend the existing tool rather than proliferating unrelated tools. Representative actions:

- `spawn`;
- `status` (`get` retained as alias/compatibility);
- `message`;
- `interrupt`;
- `wait`;
- `cancel`.

Return structured/bounded text or JSON-like result appropriate to the existing tool style. `wait` takes a bounded timeout or join mode and must return control to the agent rather than indefinitely occupy a worker.

### Async completion delivery

When a child transitions meaningfully or terminally:

- append authoritative journal/run state;
- emit bounded derived projection/event notification;
- if the parent run is active, queue a compact follow-up notification such as child ID, status, summary/result handle, and whether attention is required;
- if the parent is inactive/disconnected, retain the completion in durable state so the next turn/status read sees it.

Avoid injecting noisy progress every few seconds. Push only terminal state, explicit child messages, blocking questions/permission outcomes, or bounded milestone progress configured by the child/runtime.

### Security

- sender lineage is derived from execution context, never trusted from model payload;
- children cannot address unrelated runs by guessing IDs;
- payload size and message rate are bounded per root/run;
- control text is ordinary model input at the child boundary and cannot bypass tool permissions;
- journal/projection redaction follows existing visibility rules.

## 7. Ordered work packages

### A — Mailbox/journal domain and storage

Implement typed records, bounds, ordering, store APIs, migrations, and transition tests.

Acceptance evidence:

- concurrent senders produce deterministic per-run sequence;
- duplicate control IDs do not duplicate delivery;
- oversized payloads fail before persistence.

### B — Live run control bridge

Connect `RunControlService` to active `AgentLoop` instances through a small runtime handle registry keyed by `AgentRunId`.

Acceptance evidence:

- message delivered at safe boundary;
- interrupt changes the next model trajectory without racing transcript mutation;
- cancel reaches scheduler/run cancellation.

### C — Restart replay and terminal semantics

Rehydrate pending controls after daemon restart and reconcile against terminal run status.

Acceptance evidence:

- already delivered/acknowledged messages are not re-applied;
- queued message is delivered once after restart;
- controls for terminal runs return terminal/no-op semantics rather than resurrecting work.

### D — Task tool actions and bounded wait

Add actions/aliases, input validation, ownership checks, and bounded wait.

Acceptance evidence:

- parent can spawn, continue other work, then wait/status/message/cancel by durable run ID;
- legacy `get` still works.

### E — Completion notification

Route bounded child completion/progress to active parent/session follow-up and projection paths.

Acceptance evidence:

- parent does not need repeated polling to learn terminal completion;
- disconnected parent sees durable completion on resume;
- notification storm is bounded.

### F — Documentation and compatibility

Document run-control semantics, safe boundaries, ordering, and distinction from team/project chat.

## 8. Failure, cancellation, restart, and contention semantics

- Persist-before-signal: if persistence fails, do not deliver an untracked control.
- Signal failure after persistence: message remains queued and is delivered on the next live attachment/restart.
- Parent cancellation cancels descendants through run/scheduler authority; pending informational messages may be superseded, but cancellation intent is preserved.
- Interrupt racing terminal completion returns terminal state and does not reopen the run.
- Multiple interrupts may coalesce only under a documented rule; otherwise preserve sequence and apply the newest meaningful trajectory instruction at the next boundary.
- Wait timeout is not run failure. It returns `still_running` with current status/sequence cursor.
- Restart never replays an acknowledged control and never repeats completed non-idempotent side effects merely to reconstruct a model turn.
- Mailbox backpressure returns an explicit bounded-capacity error; it must not allocate unbounded memory/disk.

## 9. Compatibility and migration

- Retain `task get` as a status/result alias during the compatibility window.
- Existing subagent progress/completion events remain derived outputs.
- No existing team inbox/outbox files are migrated into the run mailbox.
- Older clients that do not understand new run-control protocol fields can continue using spawn/get.
- Do not remove in-memory AgentLoop channels; make them the live transport fed by the durable service.

## 10. Required tests

### Focused unit tests

- message ordering/sequence;
- payload/rate bounds;
- authorization/lineage checks;
- duplicate send idempotency;
- mailbox state transitions;
- journal event bounds/serialization;
- bounded wait timeout semantics.

### Integration tests

- parent spawns child, sends message, child incorporates it at next safe boundary;
- interrupt during multi-turn child changes next provider turn;
- parent receives completion follow-up without `get` polling;
- legacy get/status compatibility.

### Restart and recovery tests

- restart with queued undelivered message;
- restart after delivery before acknowledgement;
- restart after child completion before parent notification consumption;
- terminal run rejects stale control without reopening.

### Contention and cancellation tests

- concurrent parent controls maintain order;
- cancel races message/interrupt/completion;
- mailbox full/backpressure;
- unrelated sibling/root cannot control target run.

### Security and negative tests

- forged sender run/session rejected;
- mailbox cannot modify authority/budget/worktree fields;
- hidden reasoning/secret-bearing values absent from journal/projection fixtures.

## 11. Required verification commands

Expected focused shape after M001 closes:

```bash
cargo test -p codegg-core agent_run
cargo test --lib agent
cargo test --test scheduler_cancellation
cargo test --test session_projection_consumer
cargo fmt --all -- --check
```

Run existing execution/projection guards only where touched. Use the repository’s current quick verification once at milestone closure; do not create new CI lanes.

## 12. Documentation updates

- `architecture/agent.md` — mailbox safe-boundary integration.
- new or existing run architecture doc — journal/control contract.
- `architecture/projection.md` — derived run notifications versus authoritative journal.
- tool/task documentation — action semantics.
- source roadmap status after closure.

## 13. Acceptance criteria

1. Parent can message, interrupt, status, wait, and cancel a durable child run.
2. Controls are persist-before-deliver, ordered, bounded, attributable, and restart-safe.
3. Interrupts apply only at documented safe boundaries.
4. Control messages cannot widen authority or access unrelated runs.
5. Terminal completion/progress can notify an active parent without polling.
6. Disconnected parents retain durable completion state.
7. Journal events are sufficient for lifecycle/control recovery without storing hidden reasoning or replaying completed non-idempotent actions.
8. Cancellation/terminal races produce one final run state.
9. Legacy spawn/get behavior remains available.
10. Focused tests and touched existing guards pass.

## 14. Stop conditions

Stop if:

- M001 has not established canonical durable run identity;
- implementing control requires mutating provider message history concurrently with an active provider request;
- safe interrupt semantics cannot be defined without provider-specific hidden-state assumptions;
- the design begins turning the mailbox into general team/project chat;
- journal replay would require re-executing completed non-idempotent operations;
- the work expands into worktree leases or run groups beyond interface seams.

## 15. Closure evidence required

- implementation/review commits;
- mailbox/journal schema and bounds;
- safe-boundary delivery matrix;
- restart/idempotency tests;
- cancellation/terminal race evidence;
- async completion notification fixture;
- authorization/negative tests;
- compatibility evidence for spawn/get;
- unresolved findings and closure recommendation.

## 16. Handoff notes

Prefer a small durable mailbox feeding existing `AgentLoop` control channels over a new actor framework. Do not persist high-frequency token/tool stream events. The useful Muse-style behavior is timely asynchronous communication, not constant chatter.
