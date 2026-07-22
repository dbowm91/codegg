# Session Projections Milestone 010 — Mechanism-Faithful Transport Verification and Final Closure

Status: ready for handoff

Repository baseline: `426dfffec05c9d694f54a816213a6cca514e91b4` (`main`)

Source subsystem:

- `plans/subsystems/session-projections-roadmap.md`

Corrected predecessor closure:

- `plans/closure/session-projections/009-status.md`

Primary class: verification correction / bounded-queue mechanics / Unix transport races / lifecycle observability / closure reconciliation

## 1. Objective

Finish the frontend-neutral session-projections transport line by closing the remaining verification defects found after M009. Preserve the valid M008 and M009 production implementation and tests, but replace nominal or mislabeled fixtures with mechanism-faithful tests whose assertions directly observe the production condition named by the test.

M009 validly added:

- connection-local WebSocket task probes;
- shared cancel/abort-and-await teardown coverage;
- real WebSocket peer-close and abrupt-drop fixtures;
- 100-cycle `/core` and `/tui` connection churn;
- two-client continuity tests;
- exact `/core` interrupted-replay cleanup and retry;
- exact replay/live sequence and identity checks;
- fresh `/core` connection identity evidence;
- additional cancellation and replay durability fixtures.

Post-M009 inspection found no new projection protocol, storage, reducer, or transport architecture defect. It found that several strict closure claims remain broader than the mechanisms actually exercised:

1. the test named for `/core` queue saturation does not fill the 256-item queue and does not directly observe `CriticalDeliveryError::Timeout` from a blocked full-queue send;
2. `/tui` has no actual bounded-queue saturation test;
3. Unix peer-close, write/flush failure, cancellation-versus-response completion, and interrupted-replay fixtures were not added;
4. task tests named `raw_source_first_exit` close the client rather than terminating the raw source first, and the shared owner unit test exercises only send-first;
5. the TUI pending-setup and replay-delivery tests complete setup or replay before disconnecting, so they do not prove interruption while work is pending;
6. the reusable rollback helper does not implement all assertions described by its documentation and is not used by every real failure fixture;
7. static guards verify names and substrings more strongly than semantics;
8. M009 plan, closure, roadmap, registry, commit, command, and test-count evidence are inconsistent.

M010 succeeds only when every remaining claim is demonstrated by the named production mechanism, all applicable cleanup invariants are enforced by one reusable harness, and the planning record matches the executable repository exactly.

## 2. Scope boundaries

### In scope

- Per-connection test configuration for bounded WebSocket queue capacities and writer gates.
- Actual `/core` and `/tui` queue saturation through the real adapter sender and production `bounded_critical_delivery` path.
- Direct observation of `CriticalDeliveryError::Timeout`; elapsed time or client-side read timeout alone is insufficient.
- Deterministic send-first, receive-first, raw-event-first, and panic-first `ConnectionTaskSet` tests.
- Real raw-event source termination in `/core` and `/tui` adapter fixtures.
- Real Unix peer close before canonical response completion.
- Real Unix write/flush failure and cancellation-versus-response-completion race fixtures.
- Unix interrupted replay cleanup, durable retry, fresh client identity, and exact replay-to-live proof.
- TUI cancellation while canonical setup or replay delivery is still pending through a writer-side, cancellation-aware barrier.
- One complete rollback assertion harness used by every real staged-failure fixture.
- Semantic static guards that inspect required mechanism markers rather than test names alone.
- Full focused verification and exact closure/registry reconciliation.

### Explicitly out of scope

- Projection DTO, snapshot, event, cursor, or wire schema changes.
- Projection protocol version changes.
- SQLite schema, retention, checkpoint, sequence authority, or replay-service changes.
- Reducer/controller semantics.
- Disclosure, redaction, artifact, authorization, or team-collaboration policy.
- New TUI, observer, or web product features.
- Cross-daemon replay replication.
- Removal of version-4/raw compatibility.
- General authentication redesign.
- General workspace lint cleanup outside files touched by M010.

Production behavior should remain unchanged unless a mechanism-faithful test reveals a real defect. Test controls must be connection-local, bounded, and absent from normal runtime behavior.

## 3. Required invariants

- A queue-saturation test fills the actual channel to capacity and verifies a subsequent production send is pending because `mpsc::Sender::send` cannot reserve capacity.
- The test captures the actual adapter result and asserts `Err(CriticalDeliveryError::Timeout)`.
- Socket closure, an outer test timeout, elapsed wall time, or an injected timeout classification cannot substitute for the direct result assertion.
- `/core` and `/tui` use their real canonical response senders and normal `bounded_critical_delivery` implementation.
- A test named raw-source-first closes the actual raw-event source while peer and writer remain otherwise viable, and records `RawEvent` as the first terminal task.
- `ConnectionTaskSet` records or exposes the first terminal task kind to tests without changing public protocol behavior.
- Send-first, receive-first, raw-event-first, and panic-first cases all cancel, abort, and await every sibling exactly once.
- A peer-disconnect test closes or fails the real transport peer before canonical response completion.
- Unix write-failure tests fail the actual `write_all` or `flush` path.
- Unix cancellation/completion race tests accept only documented terminal outcomes and require identical cleanup convergence.
- TUI pending-setup and replay interruption occur before the canonical response is successfully delivered.
- Every real failure returns connection ownership, daemon subscription ownership, receiver availability, projection forwarders, connection tasks, and diagnostic counters to baseline.
- Every installed projection forwarder is cancelled and joined; its probe is asserted, not merely recorded.
- Cleanup and daemon unsubscribe are idempotent.
- A second client remains live and receives a unique event after client A fails.
- Replay interruption does not delete history or mutate the durable cursor incorrectly.
- Retry from the same cursor receives the exact missing range once and then the next live event without a gap, duplicate, or reorder.
- Static guards reject tests that claim queue saturation without a capacity fill and direct timeout-result assertion.
- Static guards reject raw-first tests that only close the client.
- Static guards reject replay-interruption tests that receive the replay response before disconnecting.
- Closure evidence records exact implementation and closure commits, test names, executable counts, commands, outputs, exceptions, and CI status truthfully.

## 4. Work package A — Connection-local queue and lifecycle test controls

Primary files:

- `src/server/state.rs`
- `src/server/ws.rs`
- `src/server/http.rs`
- `tests/projection_transport_real.rs`

Add a bounded connection-local test configuration, for example:

```text
ProjectionTransportTestConfig {
    outbound_queue_capacity: usize,
    writer_gate: Option<WriterGate>,
    raw_source_control: Option<RawSourceControl>,
    lifecycle_observer: Option<ConnectionLifecycleObserver>,
}
```

Equivalent designs are acceptable if they meet these constraints:

1. Production defaults remain `WS_OUTBOUND_QUEUE_CAPACITY` and no gates/probes.
2. A test may set capacity to `1` or `2` for one server fixture.
3. A writer gate pauses before the writer drains a selected queue item.
4. Tests may acquire a clone of the real adapter sender through a private test observer, or invoke a test-only helper located beside the production sender.
5. The helper must call the same `staged_critical_send` or `critical_send` implementation used by the adapter.
6. The observer captures:
   - queue capacity;
   - successful filler enqueue count;
   - the point at which `try_send` returns `Full`;
   - the final `CriticalDeliveryError` returned by the production send;
   - first terminal task kind;
   - task joins;
   - projection-forwarder joins;
   - cleanup completion.
7. Controls are connection-local and removed with the fixture.
8. No payload bodies, artifact data, hidden reasoning, or secrets are retained.
9. No new public wire fields are added solely for testing.
10. No process-global mutable test state is introduced.

## 5. Work package B — Actual `/core` and `/tui` queue saturation

Primary files:

- `src/server/ws.rs`
- `tests/projection_transport_real.rs`

### Required `/core` fixture

1. Start `/core` with outbound capacity `1` or `2`.
2. Complete the projection-capable handshake.
3. Pause the writer before it drains the control queue.
4. Fill the actual control queue to the configured capacity through the real sender.
5. Assert the next nonblocking reservation reports `Full` before starting the critical send.
6. Start a real staged subscribe or resume response through `staged_critical_send`.
7. Keep the writer paused beyond `CRITICAL_DELIVERY_TIMEOUT`.
8. Capture and assert the production send returns `Err(CriticalDeliveryError::Timeout)`.
9. Assert no successful canonical response and no live projection event escape.
10. Release the writer and run the complete rollback harness.

### Required `/tui` fixture

Repeat the same mechanism against the actual TUI control/projection response queue. The fact that normal client traffic cannot conveniently fill a capacity-256 queue is not an exception; the connection-local test capacity and sender observer exist specifically to make the production mechanism deterministic.

### Prohibited evidence

The following do not satisfy this work package:

- one queued item in a capacity-256 channel;
- pausing a receive-side lifecycle checkpoint while assuming the writer is paused;
- sending a second client request while the receive task is blocked processing the first;
- accepting a client read timeout, connection closure, or generic error as proof;
- measuring elapsed time without capturing the adapter send result;
- `fail_next(... Timeout)`.

## 6. Work package C — Deterministic connection-task first-exit matrix

Primary file:

- `src/server/ws.rs`

Extend `ConnectionTaskSet` test instrumentation so tests can assert the selected `ConnectionTaskKind` and abnormal termination classification.

Required unit cases:

- send task completes first;
- receive task completes first;
- raw-event task completes first;
- send task panics first;
- receive task panics first;
- raw-event task panics first.

For every case assert:

- the expected first task kind is recorded;
- the selected handle is consumed once;
- connection cancellation is triggered before sibling cleanup;
- both remaining tasks are aborted and awaited;
- all drop probes reach zero;
- no handle remains stored;
- expected cancellation joins are not classified as abnormal;
- panic is classified as abnormal with the correct task kind;
- panic does not prevent sibling cancellation and joins;
- cleanup may proceed exactly once.

Required adapter cases for `/core` and `/tui`:

- reader/peer-close first;
- writer/socket-failure first;
- actual raw-event source first.

A raw-source-first fixture must close the source or sender observed by the raw-event task while keeping the WebSocket peer open until the task owner records `RawEvent` as first exit. Publishing a raw event and then closing the client is not sufficient.

## 7. Work package D — Real Unix lifecycle and replay races

Primary files:

- `src/core/transport/daemon_socket.rs`
- `src/core/transport/daemon_socket_integration_tests.rs`
- `src/core/transport/projection.rs`

Add the minimum connection-local Unix lifecycle controls needed to pause and observe production byte-stream boundaries. Reuse the existing projection lifecycle seam where practical; extend it only at Unix-specific response write/flush boundaries.

Required fixtures:

### Peer close before canonical response completion

- subscribe or resume until daemon subscription and receiver installation complete;
- pause before canonical response write completion;
- close/drop the real Unix peer;
- release the barrier;
- prove the actual write or flush path fails or cancellation wins;
- prove no live event is emitted and all ownership/task state returns to baseline.

### Write and flush failure

- force a real write-side failure using peer shutdown/drop supported by the platform;
- exercise both `write_all` and `flush` failure where deterministically distinguishable;
- retain and join raw/projection forwarder handles;
- assert no subscription growth or retained receiver references.

### Cancellation versus response completion

- use a barrier-controlled race across enough deterministic iterations to observe or force each allowed terminal ordering;
- accept only:
  - response completes, then orderly disconnect cleanup; or
  - cancellation/write failure wins and setup rolls back;
- both outcomes must converge to identical zero-growth resource baselines;
- no outcome may activate a subscription without a successfully completed canonical response.

### Interrupted replay durability

- capture first Unix client ID, stream ID, subscription ID, and cursor;
- disconnect and publish unique missing events;
- reconnect with a fresh client ID and pause before replay response completion;
- close the peer during the pause;
- prove transient cleanup and unchanged durable history/cursor;
- reconnect a third time with another fresh client ID;
- replay the exact missing sequence and identities once;
- publish and receive `replay_end_seq + 1` live;
- assert no duplicate during a bounded quiet period.

Run repeated Unix failure/reconnect cycles sufficient to reveal retained writer, filter, receiver, or forwarder references.

## 8. Work package E — True TUI pending-operation interruption

Primary files:

- `src/server/ws.rs`
- `tests/projection_transport_real.rs`

The current receive task processes TUI lifecycle requests inline, so a receive-side gate can prevent that same task from observing peer close. Do not classify post-snapshot or post-replay disconnects as pending-operation cancellation.

Use one of these mechanism-faithful approaches:

1. pause the writer after canonical response enqueue but before socket write, close the real peer, then release and observe real writer failure/cancellation before receipt success; or
2. introduce a bounded internal request-dispatch task so socket receive and close detection continue while setup is paused, retaining explicit ownership and joined teardown; or
3. add an equivalent connection-local cancellation-aware writer barrier that observes peer failure independently of the inline request handler.

Required TUI fixtures:

- disconnect while initial snapshot response is pending and before delivery success;
- disconnect while replay response is pending and before delivery success;
- prove staged subscription never becomes live;
- prove complete rollback and task/forwarder baseline;
- retry replay from the same cursor and prove exact replay/live continuity.

A fixture that receives `ProjectionSnapshot` or `ProjectionReplay` before disconnecting is durability coverage, not interruption coverage, and must be named/documented accordingly.

## 9. Work package F — Complete rollback harness

Primary files:

- `tests/projection_transport_real.rs`
- `src/core/transport/daemon_socket_integration_tests.rs`
- narrow test support modules

Replace fragmented helpers with one reusable assertion harness per transport shape, backed by common invariant checks.

Required inputs:

- pre-scenario daemon subscription count;
- connection observer/probe;
- failed subscription ID;
- connection/client ID;
- project/session scope;
- unrelated live client and its expected subscription;
- expected response-delivery state;
- expected terminal mechanism and first task kind where applicable.

Required assertions for every real failure fixture:

- no successful canonical response before the failure point, except explicitly documented post-delivery/pre-activation cases;
- no live projection envelope after rollback;
- connection-local ownership no longer contains the failed subscription;
- daemon active subscription count equals the pre-scenario baseline;
- failed receiver cannot be acquired again;
- projection-forwarder joined count returns to baseline and is explicitly asserted;
- send, receive, and raw-event task probes return to baseline;
- handler cleanup completion is observed;
- second connection cleanup and second daemon unsubscribe are harmless and return the expected typed not-owned/no-op outcome;
- artifact-read and diagnostic counters are unchanged where exposed;
- unrelated client remains connected and receives a unique projection event;
- queue depth, task count, and retry count remain bounded;
- no payload or secret data is recorded by probes.

Use the complete harness in:

- `/core` queue timeout;
- `/tui` queue timeout;
- `/core` pending-setup disconnect;
- `/tui` pending-setup disconnect;
- `/core` replay interruption;
- `/tui` replay interruption;
- Unix peer-close/write-failure/race/replay interruption;
- real writer-failure fixtures.

Do not claim per-scenario completeness based on separate tests that cover different invariants.

## 10. Work package G — Semantic static guards

Primary file:

- `scripts/check_projection_transport_lifecycle.py`

Strengthen the guard beyond test-name presence.

Required structural checks:

- both `/core` and `/tui` queue tests configure capacity below the production default;
- each queue test fills until `TrySendError::Full` or an equivalent explicit full-capacity observation;
- each captures and compares the production send result to `CriticalDeliveryError::Timeout`;
- queue tests do not accept outer read timeout or connection closure as sufficient success;
- first-exit unit tests assert all three `ConnectionTaskKind` values and panic classification;
- adapter raw-first tests invoke a raw-source control and assert first kind `RawEvent` before client closure;
- TUI replay-interruption test disconnects before parsing a successful replay response;
- Unix tests contain real peer shutdown/drop and production write/flush boundaries;
- complete rollback helper asserts `forwarder_count`, receiver non-reuse, idempotence, and unrelated-client continuity;
- all real failure fixtures call the complete helper;
- M010 closure record contains implementation and closure commit hashes, exact test count, and command matrix;
- existing bounded-queue, private activation, route-generation, ownership, disclosure, and forbidden-pattern guards remain intact.

A static guard remains supplemental. It must not substitute for runtime assertions.

## 11. Documentation and closure reconciliation

Required documents:

- `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md`
- `plans/closure/session-projections/009-status.md`
- `plans/subsystems/session-projections-roadmap.md`
- `plans/registry.md`
- new `plans/closure/session-projections/010-status.md`

Required changes:

1. Mark M009 conditionally closed, not failed.
2. Preserve accepted M009 WebSocket lifecycle, churn, two-client, replay, and identity evidence.
3. Record the exact residual mechanism and evidence defects owned by M010.
4. Mark the M009 implementation plan conditionally closed/superseded for strict verification rather than ready.
5. Keep M010 as the sole dependency-ready projection plan.
6. Create M010 closure only after every acceptance criterion is met.
7. Record exact implementation commit, follow-up commits, closure commit, and final reviewed head.
8. Enumerate exact test names and executable counts; do not claim more tests than are listed.
9. Record the full verification command matrix and relevant outputs.
10. Distinguish local execution from GitHub CI status.
11. Return roadmap and registry to strict closed status only through accepted M010 closure.

## 12. Required verification matrix

At minimum run:

```bash
cargo fmt -- --check
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
CARGO_BUILD_JOBS=1 cargo clippy -p codegg-protocol --all-targets -- -D warnings
cargo test -p codegg --lib core::transport::projection --all-features -- --nocapture
cargo test -p codegg --lib server::ws --all-features -- --nocapture
cargo test -p codegg --lib core::transport::daemon_socket --all-features -- --nocapture
cargo test --test projection_transport_real --features server -- --test-threads=1 --nocapture
cargo test --test projection_replay_daemon_protocol -- --nocapture
cargo test --test projection_replay_subscription -- --nocapture
cargo test --test projection_replay_resume -- --nocapture
cargo test --test projection_replay_restart_recovery -- --nocapture
cargo test --test projection_replay_transport_isolation -- --nocapture
cargo test --test projection_disclosure_invariants -- --nocapture
cargo test --test projection_artifact_handles -- --nocapture
cargo test --test tui -- --nocapture
cargo test --test tui_render -- --nocapture
cargo test --test tui_project_routing -- --nocapture
cargo test --test tui_project_tabs -- --nocapture
cargo test --test single_daemon_lifecycle -- --nocapture
python3 scripts/check_projection_transport_isolation.py
python3 scripts/check_projection_transport_lifecycle.py
python3 scripts/check_websocket_bounds.py
bash scripts/check-core-boundary.sh
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_scheduler_bypass.py
bash scripts/check_projection_disclosure.sh
git diff --check
```

Add focused commands for:

- `/core` actual full-queue timeout;
- `/tui` actual full-queue timeout;
- six-case `ConnectionTaskSet` first-exit/panic matrix;
- real `/core` and `/tui` raw-source-first adapter tests;
- real TUI snapshot/replay pending-delivery interruption;
- Unix peer-close, write/flush failure, cancellation/completion race, churn, and interrupted replay retry;
- complete rollback harness application for each real failure class.

Run transport tests with one test thread unless all test-local controls are independently connection-scoped and concurrency-safe. Do not claim CI evidence unless a workflow or status check is present for the final reviewed head.

## 13. Acceptance criteria

- M008 and accepted M009 production task ownership and replay behavior remain intact.
- `/core` and `/tui` tests fill their actual configured bounded queues and directly assert the production critical send returns `Timeout`.
- No queue-saturation test relies on one queued item in a capacity-256 queue, elapsed time alone, client read timeout, connection closure, or injected timeout classification.
- Send-first, receive-first, raw-event-first, and all three panic-first task-owner cases cancel and await siblings.
- Real `/core` and `/tui` raw-source termination is observed as the first task exit.
- Real `/core` and `/tui` peer-close and writer-failure paths terminate every connection task.
- TUI snapshot and replay responses are interrupted before delivery success by real peer failure/cancellation.
- Unix has real peer-close-before-response, write/flush failure, deterministic cancellation/completion race, repeated convergence, fresh identity, and interrupted replay retry tests.
- At least 100 WebSocket churn cycles and an appropriate repeated Unix race/churn loop return tasks, forwarders, receivers, ownership, and subscriptions to baseline.
- Client A failure does not perturb client B in every applicable real failure class.
- Every real failure fixture uses the complete rollback harness.
- The complete harness asserts connection ownership removal, daemon subscription removal, receiver non-reuse, forwarder join, task joins, handler completion, idempotence, unrelated-client continuity, and bounded resource counters.
- Injected serialization and pre-activation cases remain clearly classified as seam tests.
- Replay interruption retains durable history and retry is exact and duplicate-free.
- Static guards verify mechanism markers rather than test names alone.
- Full focused transport, replay, disclosure, TUI, lifecycle, formatting, check, and lint gates pass or record unrelated pre-existing findings precisely.
- Closure test names and counts match executable results.
- M009 records conditional historical status and M010 is the sole strict closure authority.
- `plans/closure/session-projections/010-status.md` records exact implementation and closure commits.
- No unresolved high or medium M010 finding remains.
- Registry contains no dependency-ready projection plan only after M010 is strictly closed.

## 14. Handoff order

1. Add connection-local queue capacity, writer, raw-source, and lifecycle controls.
2. Complete the six-case task-owner first-exit/panic matrix.
3. Add genuine `/core` queue saturation and direct timeout-result assertion.
4. Add genuine `/tui` queue saturation and direct timeout-result assertion.
5. Add real raw-source-first adapter fixtures.
6. Add true TUI pending snapshot/replay interruption.
7. Add Unix peer-close/write/flush/cancellation-completion race fixtures.
8. Add Unix interrupted replay retry and identity proof.
9. Implement the complete rollback harness and apply it to every real failure fixture.
10. Strengthen semantic lifecycle guards.
11. Run the complete verification matrix.
12. Reconcile M009 plan/closure, roadmap, registry, and exact evidence.
13. Write and accept M010 closure only after every acceptance criterion passes.

## 15. Final completion definition

This line of work is complete when actual channel fullness, actual peer failure, actual raw-source termination, actual Unix byte-stream failure, and actual pending-response interruption—not test names, elapsed time, or injected classifications—prove bounded timeout, cancellation, deterministic joined teardown, complete rollback, cross-client isolation, and replay durability; every transient task, receiver, forwarder, queue, and subscription returns to baseline; and the planning record precisely matches the executable repository.