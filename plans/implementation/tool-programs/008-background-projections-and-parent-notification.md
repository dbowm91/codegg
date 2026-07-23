# Tool Programs Milestone 008 — Background Programs, Projections, and Parent Notification

Status: blocked pending Milestone 007 closure

Repository baseline: `2f715941516a1d49be578fdef56714ad3ddfe8bf` (`main`)

Source roadmap:

- `plans/subsystems/tool-programs-roadmap.md#milestone-8--background-programs-projections-and-parent-notification`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#15-read-only-session-observation`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/001-terminology-and-domain-model.md` — session, turn, job, run, artifact, agent run

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Primary class: capability

## 1. Objective

Add background Tool Program submission so the parent agent can continue immediately, while frontend-neutral projections expose program progress and a durable parent-session follow-up delivers exactly one bounded terminal or recoverable notification without requiring model-visible polling.

## 2. Readiness boundary

Hard dependency: M007 closure. Program execution, child-job composition, artifacts, and recovery must already converge reliably before background detachment is permitted.

## 3. Current implementation evidence

- Session projections already represent sessions, turns, runs, jobs, replay, visibility, and bounded artifact handles.
- Scheduler events expose submit, admission, progress, completion, and failure.
- Existing subagent events produce transient notifications/toasts but do not provide the durable notification contract required here.
- AgentLoop supports follow-up/continuation concepts and durable sessions, but there is no canonical exactly-once background-work completion inbox tied to a parent turn.
- The foreground `tool_program` tool waits for completion and returns one result.

## 4. Invariants that must not regress

- Foreground and background modes share one submission, execution, storage, recovery, and policy implementation.
- Background submission returns only after durable program/job creation.
- Parent continuation does not cancel or lose detached work unless policy explicitly requests cancellation.
- Every background program produces at most one actionable terminal notification for its parent session/agent lineage.
- Notification delivery is durable, idempotent, bounded, redacted, and replayable.
- A notification is not the program result body; large details remain behind handles.
- Frontends render projections and never own program state or terminal-delivery truth.
- Observer visibility follows existing session/project redaction policy and cannot expose source, secrets, or unbounded tool output.
- A failed notification delivery cannot leave the program logically running or cause repeated model turns indefinitely.

## 5. Scope

### In scope

- `execution: await | background` in the model-facing tool schema.
- Durable parent/causation metadata and terminal-notification disposition.
- Program summary/detail/call-page projection events and snapshots.
- Parent-session follow-up inbox/event with exactly-once claim/ack semantics.
- AgentLoop handling for terminal, incomplete, cancelled, timed-out, stalled, interrupted, and recoverable notifications.
- TUI/native protocol status and read-only inspection of program progress/calls/artifacts.
- Disconnect/reconnect/replay behavior and notification backpressure.

### Explicitly out of scope

- Full subagent transcript UX or agent-tree redesign.
- Human/team chat delivery.
- Automatic recursive model continuation after every progress event.
- Steering or modifying a running program’s source/manifest.
- Hosted OpenAI adapter.
- Full ACP protocol implementation.

## 6. Required production changes

### Background submission contract

`tool_program` background mode returns a compact handle containing program/job IDs, display status, submission time, effective limits, and inspection/cancel references. It must not wait for completion.

Persist:

- originating principal/project/session/turn/agent run/tool-call identity;
- notification target and policy;
- foreground/background mode;
- terminal notification state: pending, claimed, delivered/acknowledged, suppressed, expired, or failed terminally;
- idempotency key and notification payload digest.

### Projection model

Add bounded versioned projection types/events:

- `ToolProgramSubmitted`, `Admitted`, `Started`, `Progress`, `WaitingForCall`, `WaitingForJob`, `RetryBackoff`, `Completed`, `Incomplete`, `Failed`, `Cancelled`, `TimedOut`, `Stalled`, `Interrupted`;
- summary fields: program/job/attempt, parent turn/agent, language, state, phase, budgets used/remaining, call counts, child-job counts, timestamps, failure class, terminal handle;
- detail query for manifest metadata, source/IR hashes, checkpoint version, recent call summaries, and artifacts;
- paginated call history with redacted structured arguments/results.

Do not add raw source or output bodies to normal snapshots/events.

### Parent-session notification service

Introduce one daemon-owned durable follow-up queue or equivalent session event store. Required behavior:

1. program terminal publication creates one notification record transactionally or through deterministic reconciliation;
2. notification includes compact status, result/recovery summary, program/job handles, selected evidence, and artifact references;
3. AgentLoop/session owner claims with compare-and-set and records acknowledgement;
4. duplicate scheduler/program terminal events produce the same notification identity;
5. crash after claim but before model consumption re-delivers according to explicit lease/ack rules without creating a second logical notification;
6. terminal failure to deliver remains inspectable and does not mutate program status;
7. progress events never enqueue model follow-ups.

Define whether delivery appends a system/tool event to the next parent turn, creates a queued follow-up turn, or exposes an explicit inbox consumed by AgentLoop. It must not fabricate a reply from the original provider response.

### AgentLoop behavior

- Parent may continue after background submission.
- At safe turn boundaries, consume pending notifications in deterministic order with a bounded per-turn count/bytes.
- The prompt distinguishes completed, incomplete/recoverable, and failed terminal work.
- Do not automatically resubmit or continue a failed program; the model may choose a narrower continuation.
- Cancellation of the parent session/agent follows explicit detached-work policy rather than implicit task ownership.

### TUI and native protocol

- Add program badges/status to existing job/run views or a dedicated read-only detail pane.
- Support list/filter/inspect/cancel operations through daemon protocol.
- Reconnect/replay reconstructs the same state and pending notification disposition.
- TUI toasts are supplemental only and cannot be closure evidence for delivery.

## 7. Ordered work packages

### Work package A — Background submission and durable causation

- Extend tool schema and program records.
- Return durable handle after submit.
- Define detached cancellation/retention policy.

### Work package B — Frontend-neutral projections

- Add reducer/snapshot/events and bounded query DTOs.
- Integrate scheduler/program/call state without duplicate owners.
- Add visibility/redaction and old-client compatibility.

### Work package C — Exactly-once logical notification

- Add persistent notification records, identity, claim lease, acknowledgement, retry, expiry, and reconciliation.
- Correlate terminal publication and session delivery.
- Bound queue count/bytes and define backpressure.

### Work package D — AgentLoop and TUI adoption

- Consume notifications at safe boundaries.
- Add compact prompt/rendering.
- Add read-only program inspection and explicit cancel.

### Work package E — Lifecycle and failure verification

- Test disconnect, daemon restart, parent inactivity, duplicate terminal events, claim crash, queue saturation, cancellation races, and unrelated-session isolation.
- Add semantic guards preventing transient toasts from being the sole notification path.

## 8. Failure, cancellation, restart, and contention semantics

- Submission response is returned only after durable program and job identity exists.
- Parent frontend/provider disconnect after submission does not lose the program or notification target.
- Program terminal state is authoritative even if notification persistence/delivery temporarily fails; reconciliation retries within bounded policy.
- Notification claim uses lease/ack or equivalent to survive daemon crash. At-least-once transport is acceptable only with one logical notification identity and idempotent AgentLoop insertion.
- Parent session deletion/archive follows documented notification retention/suppression behavior.
- Background program cancellation is explicit and idempotent; cancelling the parent turn does not necessarily cancel detached work.
- Queue saturation blocks/suppresses according to typed bounded policy and never silently drops terminal state.
- Many background programs are bounded per session/project/root and cannot flood prompts or projection queues.

## 9. Compatibility and migration

- Foreground default remains unchanged unless explicitly configured otherwise.
- Older clients see generic jobs and can ignore unknown program events.
- Existing session projection sequence/replay semantics remain authoritative.
- Background notifications are additive and do not rewrite prior turns.
- TUI can operate without the new detail view while protocol capability negotiation hides unsupported operations.

## 10. Required tests

### Focused unit tests

- background schema/defaults;
- projection reducers and redaction;
- notification identity, claim, lease, ack, expiry, size/count bounds;
- detached cancellation policy.

### Integration tests

- parent submits background program, continues, and receives one terminal notification;
- complete/incomplete/fail/cancel/timeout/stall variants;
- TUI/native snapshot and call inspection;
- explicit cancel from frontend.

### Restart and recovery tests

- restart before program terminal, after terminal before notification, after notification before claim, after claim before ack, and after ack;
- replay reconstructs exact state and no duplicate logical insertion.

### Contention and cancellation tests

- many programs completing simultaneously;
- queue/backpressure limits;
- parent session inactive/disconnected;
- cancellation during terminal/notification race;
- unrelated sessions remain live and receive only their own events.

### Security and negative tests

- cross-session notification forgery;
- secret/source/raw output leakage;
- observer visibility restrictions;
- oversized result/failure payload;
- repeated fake terminal events cannot trigger repeated model turns.

## 11. Required verification commands

```bash
cargo test -p codegg --test tool_program_background
cargo test -p codegg --test tool_program_notifications
cargo test -p codegg --test tool_program_projection
cargo test -p codegg --test projection_replay
cargo test -p codegg --test agent_loop_harness
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
```

Run claim/restart and simultaneous-completion fixtures repeatedly and report exact clean-run counts.

## 12. Documentation updates

- `architecture/tool_programs.md` background lifecycle
- session projection/replay docs
- AgentLoop follow-up/notification docs
- TUI job/program inspection guide
- operator troubleshooting for undelivered/suppressed/expired notifications

## 13. Acceptance criteria

1. Background submission returns promptly with a durable handle and the parent can continue.
2. Foreground and background modes use the same runtime and policy.
3. Every terminal program creates at most one logical parent notification.
4. Restart/disconnect/duplicate events do not lose or multiply notification insertion.
5. Progress never triggers uncontrolled model turns.
6. Projections are bounded, replayable, redacted, and frontend-neutral.
7. Parent/child cancellation and detached retention are explicit and deterministic.
8. No unresolved high or medium finding remains.

## 14. Stop conditions

Stop and report if M007 is not closed, session storage cannot support durable idempotent notification delivery, delivery would require mutating historical turns, projection changes would alter established replay authority, or background work would be represented only by in-memory tasks/toasts.

## 15. Closure evidence required

Create `plans/closure/tool-programs/008-status.md` with foreground/background parity, notification state/identity matrix, restart/claim/ack repetition evidence, projection snapshot/replay results, backpressure/resource convergence, security/redaction evidence, TUI/native inspection results, and residual findings.

## 16. Handoff notes

Do not conflate “exactly once transport” with the required exactly-once logical notification. Use durable idempotency and acknowledgement. Keep progress visible but model-silent unless the user or agent explicitly inspects it.
