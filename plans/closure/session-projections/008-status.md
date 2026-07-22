# Session Projections Milestone 008 — Closure Status

Status: conditionally closed — Milestone 009 production-shaped transport verification and strict closure required

Source implementation plan:

- `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md`

Repository baseline reviewed: `8b547a3d02e571a480a826f5dea9c81d79d95cc4`

Implementation commit:

- `6975050af530eb5bd7a640c1f7ac9a31859dfda3` — shared joined WebSocket teardown, lifecycle seam matrices, exact replay-to-live assertions, and lifecycle guard extension.

Original closure evidence commit:

- `ea6e38d5182f42ae70c5f379415dd8ee1eb470e2`

Strict-verification follow-up:

- `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md`

## 1. Closure decision

The principal M008 production changes remain accepted. M008 fixed the WebSocket abort-without-await lifecycle defect and strengthened replay continuity substantially. It is not reverted or classified as a failed implementation.

Strict closure was invalidated by post-closure source and test inspection. The current tests use lifecycle-seam error classification for several scenarios that the approved plan described as real queue saturation, peer disconnect, or connection-cancellation races. The matrices also do not apply the complete lifecycle baseline assertion set to every production-shaped failure case.

M009 must close the remaining evidence gaps before the session-projections subsystem returns to strict closed status.

## 2. Accepted M008 outcomes

### 2.1 Structured WebSocket teardown

`/core` and `/tui` use one shared connection-task owner. The owner retains send, receive, and raw-event handles; consumes the first completed handle once; cancels the connection; aborts remaining siblings; awaits every retained handle; and only then permits projection and daemon cleanup.

### 2.2 Existing bounded and atomic setup semantics

Bounded queues, critical-send timeout/cancellation, writer receipts, `Initializing -> Live` after canonical response delivery, rollback, connection-local lifecycle seams, and projection-forwarder cleanup remain accepted.

### 2.3 Exact replay-to-live evidence

Unix, `/core`, and `/tui` reconnect fixtures now assert:

- stable stream identity;
- changed subscription identity;
- replay sequences `[1, 2]`;
- distinct replay turn identities;
- first live sequence `3` / `replay_end_seq + 1`;
- bounded absence of duplicate replay/live traffic.

The `/core` fixture also publishes while replay response delivery is paused.

### 2.4 Static lifecycle guard

The lifecycle guard rejects adapter-local abort-only cleanup and requires the shared joined owner, connection cancellation, awaited handles, bounded WebSocket queues, private activation, and TUI route-generation checks.

## 3. Post-closure findings

### 3.1 Real queue saturation is not yet demonstrated

The adapter matrices inject `CriticalDeliveryError::Timeout` at `BeforeControlEnqueue`. This proves rollback for a timeout-classified lifecycle boundary, but it does not fill the actual bounded adapter queue and wait for the production critical-send timeout.

Required M009 correction:

- pause the actual writer;
- fill the real bounded queue;
- initiate staged subscribe/resume response delivery;
- observe the actual timeout;
- prove complete lifecycle cleanup.

### 3.2 Real peer disconnect and connection cancellation are not yet demonstrated completely

The matrices inject `Cancelled` at lifecycle checkpoints. This does not prove that closing the real WebSocket or Unix peer wakes and terminates pending staged setup through the production cancellation path.

Required M009 correction:

- close/drop real peers during paused setup;
- cover writer/socket failure and Unix response-completion races;
- prove connection-task, projection-forwarder, receiver, and daemon-subscription baselines.

### 3.3 Per-scenario rollback assertions are incomplete

Current matrices principally prove daemon active subscription count returns to zero and no live projection event leaks. M009 must add, per applicable real failure scenario:

- connection-local ownership removal;
- receiver non-reuse;
- projection-forwarder termination;
- send/receive/raw task termination;
- task/drop baseline;
- idempotent second cleanup;
- unrelated-client continuity.

### 3.4 WebSocket first-exit and churn coverage is incomplete

The shared task-owner drop-probe test exercises one first-exit shape. M009 must cover send-first, receive-first, and raw-event-first, plus real adapter peer-close, writer-failure, raw-source, paused-setup cancellation, repeated churn, and two-client continuity paths.

### 3.5 Replay interruption and connection identity remain incomplete

M008 proves exact successful replay. M009 must additionally:

- assert fresh daemon-issued connection identity where exposed;
- disconnect during a paused replay response;
- prove transient cleanup;
- reconnect from the same durable cursor;
- prove the same missing range is still replayable and transitions live exactly once.

## 4. Verification evidence retained

The following recorded M008 results remain historical evidence:

- formatting and changed-surface workspace check;
- focused `server::ws` and daemon-socket suites;
- `projection_transport_real` exact replay and lifecycle matrix tests;
- replay, disclosure, artifact, TUI, and lifecycle guard suites.

These results support the accepted M008 implementation. They do not substitute for the production-shaped queue/disconnect verification now owned by M009.

No GitHub workflow/status check was present for the reviewed head; do not describe the recorded local commands as independently confirmed CI.

## 5. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Queue timeout is represented by an injected timeout classification rather than actual bounded queue saturation | Strict claim about production timeout mechanism is unproven | Implement real queue saturation tests for `/core` and `/tui` under M009 |
| medium | Disconnect/cancellation cases are represented primarily by injected cancellation classifications | Real peer-close and pending-operation cancellation cleanup is incompletely proven | Add real WebSocket and Unix disconnect/cancellation race fixtures |
| medium | Complete per-scenario lifecycle baseline assertions are absent | Task, receiver, forwarder, idempotence, and unrelated-client guarantees are broader than evidence | Add reusable complete rollback assertions and connection-local probes |
| low | Fresh connection identity and disconnect-during-replay cleanup are not fully asserted | Replay success is exact, but interruption durability evidence is incomplete | Add identity and interrupted replay retry fixtures |
| repository baseline | Existing unrelated warnings/clippy findings outside the changed transport surface | Does not affect accepted M008 production behavior | Track separately; do not fold into M009 unless touched |

## 6. Roadmap disposition

M008 remains conditionally closed. M009 is the sole dependency-ready session-projections plan and owns final production-shaped verification and strict closure.

The subsystem roadmap and registry may return to strict closed status only through an accepted `plans/closure/session-projections/009-status.md` record.