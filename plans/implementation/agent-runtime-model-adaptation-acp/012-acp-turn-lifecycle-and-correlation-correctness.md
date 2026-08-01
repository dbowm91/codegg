# Agent Runtime, Model Adaptation, and ACP Milestone 012 — ACP Turn Lifecycle and Correlation Correctness

Status: implemented

Repository baseline: `7d8657e60aad85f677144b1bd0e7fb5d2929faa3`

Source corrective addendum:

- `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md#milestone-012--acp-turn-lifecycle-and-correlation-correctness`

Historical source plan:

- `plans/implementation/agent-runtime-model-adaptation-acp/010-acp-v1-daemon-projection-adapter.md`

Corrective disposition:

- `plans/closure/agent-runtime-model-adaptation-acp/011-corrective-status.md`

Primary class: protocol/correctness

## 1. Objective

Correct the ACP v1 adapter so one ACP `session/prompt` request is bound to exactly one native turn, cancellation and close requests are not lost before the native turn ID is known, stale or neighboring session events cannot complete the wrong prompt, and `session/load` replays message roles faithfully.

The milestone must retain ACP as a thin stdio adapter over the singleton daemon and canonical projections. It must not add a second agent runtime, a second session store, editor-specific behavior, or ACP v2 features.

## 2. Dependency readiness

Hard dependencies are already present:

- canonical session projections and scoped subscriptions are closed;
- the native daemon owns session creation, turn submission, cancellation, replay, and projection publication;
- `src/acp.rs` implements ACP v1 framing and supported methods;
- `tests/acp_stdio.rs` provides a real-process stdio fixture.

No external editor, model, provider, or network service is required. This is the first dependency-ready corrective handoff.

## 3. Current implementation evidence

Re-audit the current head before editing. At the reviewed baseline:

- `ActivePrompt` stores ACP request ID, session ID, optional native turn ID, and a cancellation flag;
- `session/prompt` receives `CoreResponse::Ack` and sets one connection-global active prompt without an unambiguous native submission/turn correlation token;
- `session/cancel` and `session/close` issue `TurnCancel` only when `turn_id` is already known and do not retain a pending cancellation intent;
- `$/cancel_request` does retain a pending flag and attempts delivery once a turn ID appears, creating inconsistent cancellation semantics;
- `handle_event` may set `turn_id` from the first session projection event seen while a prompt is active;
- projection terminal events are accepted by session and terminal payload without consistently requiring the active native turn ID;
- `session/load` emits every stored text part as `agent_message_chunk`, losing user/assistant role semantics;
- subscription failure can degrade to `(None, None)` rather than a typed unsupported/error result;
- only one active prompt per ACP connection is currently supported, which is acceptable if advertised and enforced consistently.

## 4. Invariants that must not regress

- ACP stdout contains only newline-delimited UTF-8 JSON-RPC frames; diagnostics remain on stderr.
- The native daemon remains authoritative for sessions, turns, permissions, cancellation, storage, replay, and projections.
- One ACP prompt request receives exactly one terminal JSON-RPC response.
- An ACP prompt cannot bind to an event emitted before its native submission boundary.
- Events for another session or another turn cannot update or complete the active prompt.
- Cancellation is idempotent and eventually delivered even when requested before `TurnStarted` is observed.
- Closing a session cancels its active prompt, unsubscribes its projection stream, prevents further updates, and releases adapter-owned correlation state.
- Replay preserves public role/content semantics and does not expose private reasoning, secrets, or raw unbounded payloads.
- Frame, prompt, replay, and outbound update bounds remain explicit.
- Unsupported capabilities and malformed methods produce bounded JSON-RPC errors rather than panics or silent acceptance.

## 5. Scope

### In scope

- Replace the loose `ActivePrompt` fields with an explicit bounded lifecycle/state machine.
- Establish an unambiguous native submission boundary and turn correlation mechanism.
- Buffer cancellation/close intent before the matching native turn ID is observed and deliver it exactly once afterward.
- Require matching native turn identity for projection updates and terminal events.
- Ignore or diagnose stale, replayed, pre-submission, and neighboring-turn events.
- Make `session/close` teardown atomic from the ACP adapter's perspective.
- Preserve user/assistant roles during `session/load`; map supported tool/message history only when ACP has a correct bounded representation.
- Fail clearly when projection subscription cannot be established.
- Add process-level and state-machine regression tests.
- Reconcile `architecture/acp.md` and the M010/M011 closure claims.

### Explicitly out of scope

- More than one active prompt per ACP connection unless required by the stable ACP v1 contract.
- ACP v2 draft methods or editor-specific extensions.
- Network transport, authentication redesign, or remote ACP multiplexing.
- Replacing session projections or introducing ACP-owned durable replay.
- Durable AgentRun persistence/restart recovery.
- Provider/model changes.

## 6. Required production changes

### ACP prompt state machine

Introduce an explicit state such as:

```rust
Submitted {
    request_id,
    session_id,
    submission_id,
    event_floor,
    cancel_requested,
    close_requested,
}
Running {
    request_id,
    session_id,
    submission_id,
    turn_id,
    cancel_requested,
    close_requested,
}
Terminal
```

The exact type may differ. It must make illegal transitions impossible or return typed diagnostics. Avoid a collection or general workflow engine while one-active-prompt-per-connection remains the supported contract.

### Submission/turn correlation

Use the strongest existing native identity seam. Preferred order:

1. return or expose a native submission/turn correlation ID from `TurnSubmit` acknowledgement;
2. propagate a client submission ID into `TurnStarted` and projection envelopes;
3. use a captured projection/event cursor floor plus exact session/turn matching only if it is sufficient and deterministic.

Do not correlate only by session ID or by “first event after active state.” Any additive native protocol field must be optional/backward-compatible for existing consumers and documented in the native protocol/projection architecture.

### Pending cancellation and close

- `session/cancel` and `$/cancel_request` use one internal cancellation path;
- if the turn ID is known, issue `TurnCancel` once;
- if not known, retain pending intent and issue `TurnCancel` immediately when the matching `TurnStarted`/correlation event arrives;
- repeated cancellation returns success without duplicate native cancellation side effects;
- `session/close` sets close intent, requests cancellation, unsubscribes, and suppresses subsequent updates for that binding;
- EOF/shutdown performs bounded cleanup for all open bindings.

### Event filtering and terminal response

- Reject events below the prompt's submission/event floor;
- require exact session and correlated turn identity before emitting updates;
- require exact active turn identity before treating completion/failure/cancellation as terminal;
- do not allow a projection event for an older turn to initialize `turn_id`;
- produce one terminal ACP response and clear active state before processing later events;
- map failure, cancellation, and ordinary completion to truthful ACP stop reasons.

### Session load/replay

Map durable public history according to role:

- user text to the ACP user/history representation supported by v1;
- assistant public text to `agent_message_chunk` or the correct load-history update;
- tool calls/results only when the ACP schema supports a faithful bounded mapping;
- omit provider-private reasoning and non-public projection content;
- retain ordering and collection/string bounds;
- avoid converting all historical text into live assistant output.

If ACP v1 cannot represent a native item faithfully, document and omit it rather than relabeling it.

### Subscription/error handling

Projection subscription must return a typed success or error. Do not continue a session binding with `None` subscription after an unexpected native response. Unsubscribe should be idempotent and tolerate already-closed native state.

## 7. Ordered work packages

### Work package A — Lifecycle contract and correlation inventory

- inventory native request, event, projection cursor, session, and turn identifiers;
- define the minimal correlation seam and state transitions;
- document which ACP methods can race before a native turn ID exists;
- add pure state-machine tests before transport changes.

Acceptance evidence:

- transition table for submit/start/cancel/close/complete/fail;
- stale/pre-floor/neighbor event fixtures;
- no session-only terminal rule remains.

### Work package B — Native correlation and pending cancellation

- implement the selected correlation seam;
- unify `session/cancel` and `$/cancel_request` handling;
- retain pending cancellation/close intent;
- deliver cancellation once the matching turn is known;
- make duplicate cancel/close idempotent.

Acceptance evidence:

- cancel immediately after `Ack` reaches the eventual native turn;
- close immediately after `Ack` reaches the eventual native turn and suppresses updates;
- cancellation for another request ID or session does not affect the active prompt.

### Work package C — Event and terminal isolation

- filter by event floor/correlation/session/turn;
- bind exactly one turn;
- complete exactly once;
- ignore delayed terminal events from prior turns;
- map terminal reasons accurately.

Acceptance evidence:

- interleaved two-turn same-session fixture cannot cross-complete;
- replayed terminal event does not produce a second response;
- neighboring session event produces no update.

### Work package D — Role-correct load and teardown

- replace text-only relabeling with role-aware replay;
- enforce replay bounds and private-content omission;
- make close/EOF/shutdown unsubscribe and cleanup deterministic;
- return typed subscription errors.

Acceptance evidence:

- loaded conversation preserves user/assistant ordering;
- private reasoning never appears;
- failed subscription prevents a false-success binding;
- closed session emits no later updates.

### Work package E — Documentation and closure handoff

- update ACP architecture and capability matrix;
- update M010/M011 corrective references;
- create `plans/closure/agent-runtime-model-adaptation-acp/012-status.md` only after independent review;
- promote M013 only if this milestone is strictly closed.

## 8. Failure, cancellation, restart, and contention semantics

- Invalid lifecycle transitions return a bounded internal/JSON-RPC error and leave prior state unchanged.
- Cancellation requested before native turn identification is durable only for the lifetime of the ACP process/connection; daemon restart durability remains out of scope.
- Native cancellation failure produces a truthful terminal/error path and does not silently report success if the prompt continues.
- Close is idempotent. A second close may return success or an explicit not-found response according to ACP v1, but cannot resurrect state.
- EOF cancels or detaches according to the documented adapter policy and releases subscriptions.
- Slow stdout consumers remain bounded by existing adapter framing/queue policy; do not add an unbounded buffering layer.
- Only the active prompt may own the connection-level prompt slot; concurrent prompt attempts receive the existing bounded error.

## 9. Compatibility and migration

- Existing `initialize`, `session/new`, `session/prompt`, `session/cancel`, `session/load`, `session/resume`, `session/close`, `shutdown`, and `exit` method names remain unchanged.
- Capability negotiation remains truthful and additive.
- Any native correlation field is optional for older native clients and ignored safely by consumers that do not use ACP.
- No durable storage migration is required.
- Existing ACP clients that follow v1 continue to receive newline-delimited JSON-RPC and the same supported capability surface.

## 10. Required tests

### Focused state-machine tests

- submit → start → complete;
- submit → cancel before start → start → native cancel → terminal;
- submit → close before start → start → native cancel/unsubscribe → no updates;
- duplicate cancel and duplicate close;
- pre-submission event rejection;
- same-session stale-turn event rejection;
- neighboring-session event rejection;
- duplicate terminal event rejection.

### Process-level ACP tests

- real `codegg acp` initialize/new/prompt/completion flow;
- cancel immediately after prompt acknowledgement;
- close immediately after prompt acknowledgement;
- same-session prior terminal event does not complete current prompt;
- stdout contains only JSON-RPC frames;
- stderr diagnostics do not contaminate stdout.

### Replay tests

- user and assistant roles remain distinguishable and ordered;
- tool history is mapped or explicitly omitted according to ACP support;
- private reasoning and non-public projection data are absent;
- oversized replay data truncates or becomes handles according to existing bounds.

### Negative tests

- unsupported protocol version;
- prompt before initialize;
- invalid JSON and oversized frame;
- missing/relative cwd;
- failed projection subscription;
- cancellation for wrong request/session;
- event after close.

## 11. Required verification commands

Run focused commands first:

```bash
cargo fmt --all -- --check
cargo check -p codegg --all-targets
cargo test -p codegg acp::
cargo test --test acp_stdio -- --nocapture
cargo test --test session_projection_transport -- --test-threads=4
python3 scripts/check_projection_transport_isolation.py
python3 scripts/check_projection_transport_lifecycle.py
scripts/check_projection_disclosure.sh
```

Then run the repository's canonical quick verification command. Do not add a new editor matrix, live ACP client dependency, or long-running CI workflow.

## 12. Documentation updates

- `architecture/acp.md`: lifecycle, correlation, pending cancel, close, replay-role mapping, and capability limits;
- `architecture/projection.md` or protocol documentation only if an additive correlation field is introduced;
- corrective addendum and registry status;
- M012 closure record with exact commands and commit hashes.

## 13. Acceptance criteria

- One ACP prompt binds to one native submission and one native turn.
- Cancel and close before native turn identification are not lost.
- Stale/replayed/neighbor events cannot bind, update, or complete the active prompt.
- Terminal response is emitted exactly once with truthful reason.
- Session load preserves supported message roles and ordering.
- Private reasoning and non-public content remain absent.
- Subscription failure and teardown are explicit and bounded.
- ACP remains a thin adapter over native daemon/projection ownership.
- Focused lifecycle and real-process tests pass.

## 14. Stop conditions

Stop and report rather than improvise if:

- existing native events cannot provide unambiguous correlation without a protocol addition;
- the proposed addition would break existing native clients rather than being additive;
- correct replay requires replacing the projection/session-message model;
- more than one active prompt per connection becomes necessary for basic ACP v1 compliance;
- durable restart behavior requires the deferred AgentRun roadmap;
- a discovered issue belongs to the general session-projection subsystem rather than this adapter.

## 15. Required closure evidence

The closure record must include:

- lifecycle transition table and correlation mechanism;
- pending cancel/close delivery evidence;
- stale/neighbor event isolation evidence;
- role-correct load fixture output;
- stdout-purity and bound evidence;
- focused command results;
- compatibility statement for any additive native field;
- remaining low-severity limitations;
- explicit recommendation to promote or block Milestone 013.
