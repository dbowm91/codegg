# Session Projections Milestone 008 — Final Transport Lifecycle and Replay Evidence Polish

Status: conditionally closed — Milestone 009 production-shaped transport verification and strict closure required

Repository baseline: `8b547a3d02e571a480a826f5dea9c81d79d95cc4` (`main`)

Implementation commit:

- `6975050af530eb5bd7a640c1f7ac9a31859dfda3`

Original closure evidence commit:

- `ea6e38d5182f42ae70c5f379415dd8ee1eb470e2`

Source subsystem:

- `plans/subsystems/session-projections-roadmap.md`

Closure record:

- `plans/closure/session-projections/008-status.md`

Strict-verification follow-up:

- `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md`

Primary class: correctness polish / task lifecycle / adapter verification / closure reconciliation

## 1. Objective and disposition

M008 was created to finish joined `/core` and `/tui` task teardown, complete adapter-level failure coverage, prove exact replay-to-live continuity, and reconcile closure evidence.

The production implementation materially succeeded:

- `/core` and `/tui` share structured connection-task ownership;
- the first connection task to exit cancels the connection, aborts siblings, and awaits all retained handles;
- Unix client and raw-forwarder lifecycle remains owned and joined;
- TUI raw route generations continue to reject stale queued traffic at the writer boundary;
- staged subscriptions remain initializing until canonical response delivery succeeds;
- exact replay sequence, event identity, first-live sequence, and bounded duplicate checks exist for Unix, `/core`, and `/tui`;
- a paused `/core` replay-response/live-publication race is covered;
- lifecycle guards reject abort-without-await WebSocket cleanup.

Post-closure inspection found that strict verification claims exceeded the executable tests. M009 owns the remaining evidence work without reopening the production architecture.

## 2. Accepted M008 production outcomes

### 2.1 Joined WebSocket task ownership

`ConnectionTaskSet` retains the send, receive, and raw-event tasks for both projection-capable WebSocket adapters. The first terminal task is consumed once, connection cancellation occurs before sibling cleanup, expected cancellation joins are quiet, and abnormal task errors are logged with connection/task identity.

### 2.2 Atomic projection setup

Bounded critical sends, writer receipts, connection-local lifecycle seams, activation-after-delivery, rollback, receiver ownership, and daemon unsubscribe remain the canonical setup/cleanup model.

### 2.3 Exact replay continuity

The reconnect fixtures now inspect complete projection envelopes and assert:

- stable stream identity;
- changed subscription identity;
- exact replay sequence vector;
- exact unique turn/event identities;
- first live sequence `replay_end_seq + 1`;
- bounded absence of replay/live duplication.

### 2.4 Compatibility and security

M008 changed no protocol DTO, protocol version, replay authority, storage schema, reducer semantics, disclosure policy, or artifact bounds. Version-4/raw compatibility and foreign-operation fail-closed behavior remain intact.

## 3. Residual verification findings owned by M009

### 3.1 Synthetic mechanism coverage

The seven-boundary adapter matrices call the lifecycle seam with `Timeout`, `Cancelled`, `WriterClosed`, or `Serialization` classifications. They prove boundary reachability, rollback, daemon subscription removal, and no live leakage.

They do not independently prove that:

- an actual bounded adapter queue remained full until the real critical-send timeout;
- a real peer close or socket failure cancelled staged setup;
- connection cancellation won a pending production writer-receipt or socket operation;
- Unix peer disconnect raced response completion through the real byte stream.

M009 must add production-shaped mechanism tests and classify seam-only cases truthfully.

### 3.2 Complete per-scenario lifecycle assertions

The matrices do not apply every required assertion to every real failure case. M009 must directly prove, where applicable:

- connection-local ownership removal;
- daemon active subscription baseline;
- receiver single-take/non-reuse;
- projection-forwarder termination;
- send/receive/raw task termination;
- task/drop probe baseline;
- idempotent second cleanup;
- unrelated-client continuity.

### 3.3 Real adapter lifecycle matrix

The shared task-owner unit test proves one first-exit configuration. M009 must cover send-first, receive-first, and raw-event-first cases and add real `/core` and `/tui` tests for peer close, writer failure, raw-source closure, paused-setup cancellation, repeated churn, and client-A/client-B isolation.

### 3.4 Interrupted replay and connection identity

M008 proves exact replay-to-live continuity. M009 must additionally:

- assert fresh daemon-issued connection identity where exposed;
- disconnect during a paused replay response;
- prove transient task/subscription cleanup;
- reconnect again from the same durable cursor;
- prove the exact missing range remains replayable and transitions live without duplication.

## 4. Scope boundaries retained from M008

M009 must not reopen:

- projection DTO or protocol schema;
- protocol version;
- SQLite schema, retention, checkpoint, cursor, or sequence authority;
- replay service ownership;
- reducer/controller behavior;
- disclosure/redaction/artifact policy;
- frontend product features;
- team authorization, presence, or chat;
- cross-daemon replication;
- version-4/raw compatibility removal.

Production changes are permitted only if a production-shaped verification test reveals a real defect. Test instrumentation must remain bounded and connection-local.

## 5. Verification authority

The strict verification requirements formerly listed in this plan are now owned by:

- `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md`

M008 remains conditionally closed until M009 demonstrates real queue saturation, real disconnect/cancellation paths, complete lifecycle baselines, interrupted replay durability, and truthful closure evidence.

## 6. Closure transition

M008 becomes strictly closed only through the M009 closure record. The final record must preserve M008’s accepted production implementation while distinguishing:

- seam-based boundary and classification tests;
- real queue, socket, cancellation, and disconnect mechanism tests;
- helper-level lifecycle tests;
- production-adapter lifecycle tests.

No dependency-ready projection plan should remain only after M009 is accepted.