# Multi-Project TUI Frontend Convergence M006 — Nonblocking Session Submit Lifecycle

Status: active

Repository baseline: `98bc89fa613f5a1390202a90b497f59d5732d431`

Source roadmap:

- `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs: none.

Primary class: invariant / correctness / polish

Hard dependency: M005 must close so session creation/prompt continuation can capture canonical project/workspace context.

Closure record to create: `plans/closure/tui-project-sessions/006-status.md`

## 1. Objective

Replace the direct awaited `ensure_local_session(app).await` prompt path with the TUI's normal registered spawn-and-complete lifecycle. A prompt submitted when no session exists must start session creation asynchronously, keep the terminal event/render loop responsive, validate the originating project/tab/view/request generation on completion, and submit the prompt exactly once or preserve it for retry on failure.

## 2. Why this milestone is blocked/readiness condition

The async infrastructure already exists: `TuiTaskRegistry`, `spawn_registered_tui_task`, typed `TuiCommand` completions, request generations, tab/view epochs, and `CoreClient`. The remaining prerequisite is M005's explicit project execution context. Do not implement M006 by capturing whichever session/project is active when the async request finishes.

M006 becomes ready immediately after M005 closes without another architecture decision.

## 3. Current implementation evidence

Before editing, inspect current prompt flow end to end:

- terminal event loop in `src/tui/mod.rs` or its current owner;
- `pending_send`, prompt draft/input state, and user-message insertion behavior;
- `ensure_local_session` and exact `CoreRequest::SessionCreate`/response conversion;
- `dispatch_turn_submit_request` and idempotency/correlation fields;
- tab routing/view-switch epoch and session registration hooks;
- `set_session`, asset refresh/session-open requirements, and any automatic project/session selection;
- observer-mode prompt routing to project chat;
- reconnect/shutdown task cancellation.

Record exactly when the user message enters local display state relative to daemon session creation/turn submission. The corrected flow must not duplicate visible user messages or submit stale text.

## 4. Invariants that must not regress

- The main terminal event loop contains no network/daemon wait for session creation.
- One user submit gesture creates at most one session-create continuation and one turn submission.
- Prompt text and routing context are captured consistently; later edits cannot mutate the in-flight payload.
- Session creation completion cannot bind to a different tab/project after a switch.
- A closed/stale originating tab cannot receive completion state.
- Session/runtime asset refresh invariants remain intact before the new turn executes.
- Observer-mode bare input continues to route to project chat and must never enter the session-create path.
- Daemon authorization, session storage, and turn/scheduler ownership remain unchanged.
- Failed session creation never silently drops the user's draft/prompt.

## 5. Scope

### In scope

- A bounded pending-submit state carrying immutable prompt text, M005 execution context/route token, request generation, and any submission correlation/idempotency key already available.
- `TuiCommand` start/completion variants or equivalent typed effect results for session creation continuation.
- Registered async `CoreClient::request` execution.
- Exactly-once/coalesced behavior for repeated Enter while session creation is pending.
- Deterministic tab switch/close/reconnect/shutdown/failure handling.
- Focused responsiveness and stale-completion tests.

### Explicitly out of scope

- General rewrite of all prompt/turn state.
- Provider streaming changes.
- New persistent outbox/offline queue.
- Automatically resubmitting provider turns after unknown transport outcome unless an existing idempotent contract already supports it.
- M008 physical App decomposition.

## 6. Required production changes

### Core/domain

No core semantic change. Reuse the existing `SessionCreate` and turn-submit contracts. If session create already supports a client-generated request/idempotency identity, retain it across safe retry; otherwise do not invent duplicate-session suppression in the frontend without checking daemon semantics.

### Storage/migration

None. Pending prompt continuation is frontend-ephemeral and must not be added to the tab manifest under this milestone.

### Protocol/DTOs

No change expected. Add a protocol field only if exactly-once retry cannot be expressed using an existing request ID and the daemon already has a compatible idempotency seam; otherwise classify the gap and stop.

### Runtime/concurrency

Represent the flow as explicit states, for example:

```text
Idle
  -> SubmitRequested
      -> ExistingSession: submit turn
      -> NeedSession: CreatingSession(request_id, route, prompt)
           -> Created(valid route): bind/register session -> submit turn once
           -> Created(stale route): discard frontend application
           -> Failed: restore/release prompt for user retry
           -> Cancelled/shutdown: no continuation
```

Do not hold a mutable borrow of `App` across `.await`. All async work returns through the command channel.

### Frontend/operator surface

While creation is pending, show a bounded loading/status indication. Repeated submit should either no-op/coalesce with a concise status or be rejected deterministically; do not queue unbounded prompts.

### Security/authorization

Use the captured canonical context only as routing input. Daemon still validates project/session authority. Never route a failed observer input into ordinary prompt submission as fallback.

## 7. Ordered work packages

### Work package A — State-machine census

Document current submit ordering, message insertion, session creation, asset refresh and turn dispatch. Identify all failure paths and duplicate-submit hazards.

Acceptance evidence: state-transition table in closure record.

### Work package B — Async session-create continuation

Add pending state and registered start/completion handler. Completion validates request + route/view epoch before setting session. Reuse canonical `set_session`/routing registration rather than assigning fields piecemeal.

Acceptance evidence: no direct `.await` remains in the event-loop prompt path; fake slow core request does not stop ticks/input/render.

### Work package C — Exactly-once and prompt preservation

Define behavior for Enter during pending create, user editing after submit, create failure, and create success followed by turn-submit failure. Keep immutable in-flight text separate from the editable draft. Do not duplicate local message insertion.

Acceptance evidence: deterministic tests for double Enter and failure/retry.

### Work package D — Route/reconnect/shutdown races

Exercise A submit -> switch B, A submit -> close A, reconnect during create, and shutdown during create. Stale completion must be inert. If daemon created a session after the frontend stopped caring, it may remain a durable session; do not issue destructive cleanup merely to hide it.

Acceptance evidence: no wrong-tab binding or turn submission.

### Work package E — Documentation

Update the async-command and prompt-flow sections of `architecture/tui.md`; update `.opencode/skills/tui/SKILL.md` if it documents the event loop.

## 8. Failure, cancellation, restart, and contention semantics

A known create failure clears loading and preserves/reconstructs the user's editable prompt. An unknown transport outcome must not be blindly repeated unless request idempotency proves safety. Tab close cancels/invalidates the frontend continuation; it does not delete a daemon session that may already have been created. Shutdown cancels registered tasks.

Only one session-create prompt continuation per originating tab/session slot may be in flight. A second user submit must not create an unbounded queue or second session.

Reconnect increments the existing reconnect/view generation. A pre-reconnect completion that fails generation validation cannot mutate current state; the frontend resynchronizes session state through canonical APIs before deciding whether the user should retry.

## 9. Compatibility and migration

Normal submit behavior with an existing session must remain unchanged except for shared state-machine cleanup that is behaviorally neutral. Standalone in-process and stdio CoreClient transports use the same async path. No persisted data format changes.

## 10. Required tests

### Focused unit tests

- pending-submit state transitions;
- double-submit coalescing/rejection;
- immutable in-flight text versus editable draft;
- stale request/route rejection.

### Integration tests

- slow fake `SessionCreate`: ticks/input/render commands continue before completion;
- no-session submit creates/binds/submits once;
- switch project during create does not bind/submit into new project;
- create failure retains prompt and permits explicit retry.

### Restart/recovery tests

- reconnect generation invalidates old completion and resyncs safely;
- no persistent pending-submit state is assumed after process restart.

### Contention/cancellation tests

- rapid Enter does not create multiple sessions;
- tab close/shutdown cancels frontend continuation without deleting daemon state.

### Security/negative tests

- observer mode never triggers SessionCreate from bare input;
- missing canonical project/workspace context fails before request.

## 11. Required verification commands

```bash
# focused prompt/session/TUI tests added by this milestone
cargo test -p codegg --lib -- tui::app
cargo test --test tui_project_routing
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Use narrower exact test targets once implemented. No new CI lane.

## 12. Documentation updates

- `architecture/tui.md`: prompt submit continuation and no-await event-loop contract.
- `.opencode/skills/tui/SKILL.md`: current event-loop/async pattern.
- source comments around pending prompt/session creation.

## 13. Acceptance criteria

M006 closes when a deliberately delayed `SessionCreate` cannot freeze TUI event processing, the resulting prompt is submitted no more than once to the correct originating context, stale/tab-close/reconnect races are inert, failure preserves user work, and observer behavior is unchanged.

## 14. Stop conditions

Stop if:

- M005 is not closed;
- daemon `SessionCreate` retry/idempotency semantics are ambiguous and the implementation would guess;
- correctness requires persisting a new frontend outbox;
- the proposed fix bypasses `set_session`/routing registration or daemon authority;
- current HEAD has already removed the direct-await path and equivalent coverage proves closure.

## 15. Closure evidence required

Include implementation commits, state-transition table, proof that event-loop prompt handling has no daemon await, delayed-core responsiveness test, duplicate-submit result, project-switch/close/reconnect race results, prompt-preservation evidence, observer negative test, focused/broad verification outcomes, and severity-classified residual findings.

## 16. Handoff notes

Implement after M005. Keep the continuation small and explicit. Do not generalize it into a frontend task/workflow framework: CodeGG already has the required `TuiTaskRegistry`, request generations, and command channel.
