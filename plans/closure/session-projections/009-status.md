# Session Projections Milestone 009 — Closure Status

Status: conditionally closed — Milestone 010 mechanism-faithful transport verification and final closure required

Source implementation plan:

- `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md`

Repository baseline reviewed: `426dfffec05c9d694f54a816213a6cca514e91b4`

Implementation and evidence commits:

- `3406c742a23b6470def32fb7a04cdc7b72a40dea` — initial M009 connection probes, WebSocket lifecycle/churn/two-client fixtures, nominal queue test, replay durability, closure record, roadmap, and registry update.
- `426dfffec05c9d694f54a816213a6cca514e91b4` — cancellation and replay follow-up fixtures, expanded rollback helper, static-guard additions, and reported 42-test result.

Strict-verification follow-up:

- `plans/implementation/session-projections/010-mechanism-faithful-transport-verification-and-final-closure.md`

## 1. Closure decision

The principal M009 additions remain accepted. M009 substantially improved real WebSocket lifecycle evidence and did not reveal a new projection protocol, storage, reducer, or production transport architecture defect.

Strict closure was invalidated by post-closure source and test inspection. Several fixtures are named and documented as production mechanisms that they do not actually exercise. Required Unix mechanism tests were omitted, the complete rollback assertion set is not implemented or applied consistently, and closure evidence remains internally inconsistent.

M010 must close these verification and evidence defects before the session-projections subsystem returns to strict closed status.

## 2. Accepted M009 outcomes

### 2.1 Connection-local WebSocket probes

M009 added `ConnectionTaskProbe` and integrated it with the shared `ConnectionTaskSet`. The probe records completion of send, receive, and raw-event tasks plus cleanup execution. The shared owner continues to cancel, abort, and await retained sibling handles.

### 2.2 Real WebSocket peer lifecycle

The following real behaviors remain useful and accepted:

- graceful `/core` and `/tui` peer close;
- abrupt `/core` and `/tui` peer drop;
- 100-cycle `/core` and `/tui` connection churn;
- `/core` and `/tui` two-client continuity after client A disconnects;
- aggregate task completion and cleanup baseline checks.

These tests establish broad handler teardown and non-interference. They do not independently prove which task exited first unless a first-exit mechanism is directly controlled and observed.

### 2.3 Replay and identity evidence

M009 validly added or strengthened:

- exact `/core` interrupted replay before response completion, cleanup, retry from the same cursor, exact missing sequence, and next-live continuity;
- successful `/tui` replay durability across disconnect and retry;
- fresh `/core` daemon-issued client identity on reconnect;
- exact stream, subscription, sequence, event-identity, and duplicate-free replay assertions retained from M008.

### 2.4 Additional cancellation coverage

The `/core` paused-setup cancellation fixture uses a real dropped peer around the installed-receiver boundary and verifies subscription cleanup and task baseline. TUI abrupt-drop fixtures verify cleanup of active subscriptions. These remain useful, but TUI tests that receive the canonical snapshot or replay before disconnect are durability tests rather than pending-delivery interruption tests.

### 2.5 Static guard expansion

The lifecycle guard now requires M009 test names and broad structural markers for peer close, churn, two-client continuity, replay interruption, task probes, and predecessor closure status. Existing bounded-queue, private-activation, TUI route-generation, and owned-task checks remain accepted.

## 3. Post-closure findings

### 3.1 The `/core` queue test does not saturate the queue

Production WebSocket queue capacity remains 256. The test named `real_core_queue_saturation_fires_actual_timeout` enqueues one response, pauses a lifecycle checkpoint after enqueue, sends a second client request, and accepts an error, connection closure, or outer read timeout after sufficient elapsed time.

It does not:

- fill the actual channel to capacity;
- demonstrate `mpsc::Sender::send` waiting for capacity;
- capture the adapter send result;
- assert `Err(CriticalDeliveryError::Timeout)` directly.

A receive-side checkpoint also blocks processing of the second request, so the test cannot establish the claimed concurrent full-queue send.

Required M010 correction:

- add connection-local capacity control;
- pause the real writer before drain;
- fill until `Full` is observed;
- invoke the real production sender;
- directly assert the returned error is `Timeout`;
- run complete rollback assertions.

### 3.2 `/tui` queue saturation is absent

The approved M009 plan required actual queue saturation for both `/core` and `/tui`. The M009 closure records the TUI case as unimplemented because filling the normal capacity-256 queue from client traffic is inconvenient.

That is not a valid strict-closure exception. M009 explicitly allowed connection-local queue-capacity and sender controls for deterministic tests.

Required M010 correction:

- use a capacity-1 or capacity-2 connection-local TUI fixture;
- fill the actual canonical-response queue;
- directly observe the production timeout result.

### 3.3 Unix production-shaped verification is absent

M009 required real Unix fixtures for:

- peer close before canonical response completion;
- real write/flush failure;
- cancellation racing response completion;
- repeated resource convergence;
- fresh Unix client identity;
- interrupted replay followed by durable retry.

These were not implemented. Existing Unix unit tests for raw forwarder cancellation, selected writer failures, and replay do not satisfy the integrated mechanism matrix.

Required M010 correction:

- add Unix-specific connection-local barriers;
- close the real peer around response write/flush;
- exercise allowed race outcomes;
- prove identical cleanup convergence;
- add interrupted replay retry and identity proof.

### 3.4 Task first-exit tests are nominal

The shared task-owner unit test exercises only send-first. Adapter tests named `raw_source_first_exit` publish an event and then close the client; they do not terminate the raw source first or assert `RawEvent` selected teardown.

Required M010 correction:

- add send-first, receive-first, raw-first, and panic-first unit cases;
- record and assert first task kind;
- add adapter fixtures that close the actual raw source while the peer remains open;
- assert all siblings are cancelled and joined.

### 3.5 TUI pending-operation interruption remains unproven

The TUI receive task processes lifecycle requests inline. Existing tests either cannot observe peer closure while a receive-side gate is held or disconnect after receiving the snapshot/replay response.

Required M010 correction:

- use a writer-side barrier or equivalent bounded dispatch/cancellation design;
- close the real peer before canonical response delivery success;
- prove setup never becomes live and complete cleanup occurs;
- retry replay from the same cursor exactly.

### 3.6 Complete rollback assertions are not complete or consistently applied

The helper documented as comprehensive checks subscription baseline, optional aggregate task baseline, receiver non-reuse, and a second unsubscribe call. It does not enforce all documented requirements, including:

- no live event leakage;
- direct connection ownership removal;
- explicit projection-forwarder join assertion;
- handler completion;
- unrelated-client continuity;
- diagnostic/resource counter baselines;
- bounded queue/retry growth.

`ConnectionTaskProbe::assert_all_at_baseline` also does not assert its projection-forwarder counter. Dedicated writer-failure fixtures call the smaller helper rather than the documented comprehensive helper.

Required M010 correction:

- implement one complete transport rollback harness;
- explicitly assert forwarder completion;
- apply it to every real failure fixture.

### 3.7 Static guards remain name-oriented

The lifecycle guard mostly checks names and substrings. It does not prove that:

- a queue was filled;
- the production send returned `Timeout`;
- a raw source exited first;
- replay was interrupted before response completion;
- Unix write/flush paths failed;
- every real failure used the complete rollback harness.

Required M010 correction:

- require stable mechanism markers and direct result assertions;
- retain runtime tests as the authority.

### 3.8 Planning and evidence are inconsistent

At the reviewed head:

- the M009 implementation plan still says `Status: ready for handoff`;
- the closure does not record exact implementation, follow-up, and closure commits in a strict evidence section;
- the closure reports 27 M009 tests but lists fewer names;
- the registry leaves M009 `Closed at commit` blank;
- the recorded verification command list is narrower than the approved matrix;
- no GitHub workflow/status checks were attached to the reviewed head.

M010 must reconcile exact plan status, commits, commands, executable counts, local results, and CI status.

## 4. Verification evidence retained

The recorded local result of 42 passing `projection_transport_real` tests and passing selected static guards remains historical evidence for the accepted M009 changes. It is not independent CI confirmation and does not prove omitted or nominal mechanisms.

Existing M008 and M009 exact replay, disclosure, artifact, TUI, ownership, queue-bound, and compatibility tests remain part of the final regression matrix.

## 5. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `/core` queue test does not fill the actual queue or assert the production timeout result | Production bounded-queue timeout claim is unproven | Add deterministic capacity control, fill-to-full observation, and direct `Timeout` assertion under M010 |
| medium | `/tui` actual queue saturation test is absent | One production adapter lacks the required mechanism evidence | Add capacity-controlled real TUI queue test |
| medium | Unix peer-close/write/flush/race/interrupted-replay fixtures are absent | Unix lifecycle and replay strict-closure claims remain unproven | Implement the complete Unix mechanism matrix |
| medium | First-exit and raw-source tests do not control or observe the named first task | Joined teardown is broadly tested but mechanism-specific claims are unsupported | Add deterministic task-kind and real raw-source controls |
| medium | TUI pending setup/replay is not interrupted before response delivery | Cancellation-during-pending-work guarantee is incomplete | Add writer-side or equivalent cancellation-aware barrier fixtures |
| medium | Complete rollback harness is incomplete and not applied to every real failure | Ownership, receiver, forwarder, idempotence, and non-interference evidence remains fragmented | Implement and apply one complete harness |
| low | Static guards and closure evidence are name/count/commit inconsistent | Regression and audit confidence is reduced | Strengthen semantic guards and reconcile all planning evidence |
| repository baseline | Existing unrelated warnings or clippy findings outside the touched transport surface | Does not affect accepted projection behavior | Track separately and report precisely |

## 6. Roadmap disposition

M009 remains conditionally closed. M010 is the sole dependency-ready session-projections plan and owns final mechanism-faithful transport verification and strict closure.

The subsystem roadmap and registry may return to strict closed status only through an accepted `plans/closure/session-projections/010-status.md` record.