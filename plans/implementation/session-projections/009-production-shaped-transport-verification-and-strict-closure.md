# Session Projections Milestone 009 — Production-Shaped Transport Verification and Strict Closure

Status: ready for handoff

Repository baseline: `33c7cc4a9515263015d644f5bf713178bf5fbcb9` (`main`)

Source subsystem:

- `plans/subsystems/session-projections-roadmap.md`

Corrected predecessor closure:

- `plans/closure/session-projections/008-status.md`

Primary class: verification closure / production-shaped transport faults / lifecycle evidence / planning reconciliation

## 1. Objective

Close the frontend-neutral session-projections transport line with one final verification pass. Preserve the valid M008 production implementation while replacing synthetic or helper-level evidence with production-shaped transport tests for the remaining lifecycle and failure claims.

M008 correctly delivered:

- shared cancel/abort-and-await ownership for `/core` and `/tui` send, receive, and raw-event tasks;
- joined Unix client and raw-forwarder teardown;
- bounded critical response delivery and activation-after-delivery semantics;
- route-generation rejection of stale `/tui` raw traffic;
- adapter-local lifecycle seams;
- exact replay envelope sequence and event-identity checks for Unix, `/core`, and `/tui`;
- a paused `/core` replay-response/live-publication race;
- static rejection of WebSocket abort-without-await cleanup.

Post-M008 review found verification gaps rather than a new architecture defect:

1. queue timeout, cancellation, and disconnect scenarios are represented mainly by injected error classifications at lifecycle checkpoints rather than by an actually saturated queue, a closed peer, or connection cancellation winning a pending production operation;
2. staged failure matrices prove daemon subscription removal and no live leakage, but do not prove every required connection-local ownership, receiver, forwarder, task/drop, idempotence, and unrelated-client invariant for each real failure mechanism;
3. the shared task-owner drop probe exercises one first-exit shape, while the real adapters lack complete receive-first, writer-first, raw-source-first, paused-setup cancellation, churn, and cross-client lifecycle evidence;
4. reconnect fixtures do not explicitly prove fresh connection identity where it is observable, and no adapter disconnects during a paused replay response and proves cleanup plus replay durability;
5. the M008 plan remains marked ready and its closure/roadmap/registry claim strict completion more broadly than executable evidence supports.

M009 succeeds when the real transport mechanisms are exercised, cleanup baselines are directly observed, replay interruption remains durable, and all planning documents match the executable repository.

## 2. Scope boundaries

### In scope

- Real bounded queue saturation and timeout through `/core` and `/tui` production adapter send paths.
- Real peer close, socket write failure, half-close where supported, and connection cancellation while staged response work is pending.
- Unix peer disconnect before canonical response completion and cancellation/response-completion races.
- Real adapter first-exit coverage for reader, writer, and raw-event tasks.
- Repeated connection churn and staged-failure cycles with connection-local task/drop probes.
- Per-scenario assertions for transport ownership, daemon subscription ownership, receiver single-take, forwarder termination, task termination, idempotent cleanup, and unrelated-client continuity.
- Fresh connection identity assertions where the protocol exposes connection/client identity.
- Disconnect during paused replay response, cleanup-to-baseline proof, and subsequent successful replay from the same durable cursor.
- Focused static guards preventing synthetic-only closure claims for queue/disconnect scenarios.
- Correction of M008 plan/closure, subsystem roadmap, registry, and a dedicated M009 closure record.

### Explicitly out of scope

- Projection DTO, snapshot, event, cursor, or protocol schema changes.
- Projection protocol version changes.
- SQLite schema, retention, checkpoint, sequence authority, or replay service changes.
- Reducer/controller semantics.
- Disclosure, redaction, or artifact policy changes.
- New TUI or observer product features.
- Team authorization, presence, chat, or organization policy.
- Cross-daemon replay replication.
- Removal of version-4/raw compatibility.
- General authentication redesign.
- General workspace lint cleanup outside files touched by M009.

Production behavior should remain unchanged unless a production-shaped test reveals a real defect. Test instrumentation must be connection-local, bounded, and absent from normal runtime behavior.

## 3. Required invariants

- A test named for queue saturation must fill the actual bounded adapter queue and observe the real bounded timeout path.
- A test named for peer disconnect must close or fail the real transport peer; an injected `Cancelled` label is not sufficient.
- Connection cancellation must be observed winning a pending staged send, writer receipt, lifecycle barrier, or socket operation.
- Every `/core`, `/tui`, and Unix connection-scoped task is terminated before its handler returns.
- Every failed staged subscription returns daemon active subscription count to its pre-test baseline.
- Failed connection-local ownership is removed before teardown completes.
- A failed subscription receiver cannot be acquired a second time.
- Every installed projection forwarder is cancelled and joined.
- Repeated cleanup and daemon unsubscribe are idempotent.
- A second independently connected client remains live and receives its own traffic after client A fails or disconnects.
- Repeated cycles leave task/drop/subscription counters at baseline.
- Replay interruption never deletes committed history or advances the durable cursor incorrectly.
- A reconnect after interrupted replay receives the same exact missing range once, then the next live envelope without a gap or duplicate.
- Stream identity remains stable; subscription identity changes; connection identity changes where observable without a protocol change.
- Closure evidence distinguishes real transport mechanisms from seam-classification tests.
- Closure claims, commands, test names, counts, commits, and residual failures match the repository.

## 4. Work package A — Connection-local verification instrumentation

Primary files:

- `src/server/ws.rs`
- `src/core/transport/daemon_socket.rs`
- `src/core/transport/projection.rs`
- test support modules

Add only the minimum test-only or internal instrumentation needed to observe lifecycle completion.

Required capabilities:

1. Count active connection tasks by kind:
   - send/writer;
   - receive/reader;
   - raw-event forwarder;
   - projection forwarder.
2. Observe handler completion after all joins.
3. Observe connection-local owned-subscription count.
4. Observe daemon active-subscription count.
5. Observe projection-forwarder drop or completion.
6. Allow a test to hold the writer or response path without introducing process-global mutable state.
7. Allow a test to reduce queue capacity for one connection or deterministically fill the actual bounded queue without changing production defaults.
8. Allow controlled raw-event source termination in a test connection without shutting down unrelated clients.

Constraints:

- Prefer a connection-local test observer, injected adapter configuration, or cfg-gated probe.
- Do not add a process-global counter shared across concurrently running tests.
- Do not expose new wire protocol fields solely for testing.
- Do not retain payloads, artifact data, or secrets in probes.
- Probe state must be bounded and removed with the connection.

## 5. Work package B — Real WebSocket task-lifecycle matrix

Primary files:

- `src/server/ws.rs`
- `tests/projection_transport_real.rs`

### `/core` required cases

- Peer sends a normal close frame; receive task exits first; send and raw tasks are aborted and awaited.
- Peer drops the TCP/WebSocket connection without completing the application protocol; receive or writer failure triggers the same joined path.
- Writer fails while another task is pending; all tasks terminate before projection cleanup completes.
- Raw event source is deliberately closed for this fixture; raw task exits first and reader/writer are joined.
- Connection is closed while a staged response is paused after receiver installation; cancellation wakes pending setup and all tasks terminate.
- Repeat connect/handshake/subscribe/close at least 100 cycles with task, forwarder, and daemon subscription baselines checked after every cycle or bounded batch.
- Two clients remain connected; client A is closed or failed; client B continues to receive a unique projection and raw-compatible event.

### `/tui` required cases

Mirror the `/core` cases using the same shared task owner:

- normal peer close;
- abrupt peer drop or writer failure;
- raw event source first exit;
- cancellation during paused snapshot/replay setup;
- 100-cycle churn baseline;
- client A teardown with client B continuity.

### Shared task-owner unit matrix

Extend `ConnectionTaskSet` tests so each task kind is the first to complete:

- send first;
- receive first;
- raw-event first.

For each case assert:

- cancellation token is cancelled;
- completed handle is consumed once;
- both remaining handles are aborted and awaited;
- every drop probe reaches zero;
- no handle remains stored;
- expected cancellation joins are not logged as abnormal;
- a panic in a task is surfaced as abnormal without preventing sibling joins.

## 6. Work package C — Actual queue saturation and cancellation races

Primary files:

- `src/server/ws.rs`
- `src/core/transport/daemon_socket.rs`
- `tests/projection_transport_real.rs`
- `src/core/transport/daemon_socket_integration_tests.rs`

### `/core` queue saturation

1. Establish a projection-capable connection.
2. Pause the writer before it drains the control queue.
3. Fill the actual bounded control queue through the adapter sender.
4. Start a staged projection subscribe or resume that must enqueue a canonical response.
5. Keep the queue saturated beyond `CRITICAL_DELIVERY_TIMEOUT`.
6. Assert the real send returns `CriticalDeliveryError::Timeout` rather than an injected timeout label.
7. Assert no successful canonical response and no live event escape.
8. Assert connection ownership, daemon subscription, receiver, forwarder, and task probes return to baseline.
9. Release the writer and prove cleanup is idempotent.

### `/tui` queue saturation

Repeat the same mechanism against the real TUI control/projection queue and canonical snapshot/replay response path.

### Connection cancellation races

For `/core` and `/tui`:

- pause after receiver installation or while writer receipt is pending;
- close the real client connection;
- prove connection cancellation wins the pending operation;
- prove the staged subscription never becomes live;
- prove all connection tasks and projection forwarders terminate;
- prove daemon subscription count returns to baseline.

### Unix peer and completion races

- Pause after receiver installation and close the Unix peer before canonical response completion.
- Force a real write/flush error by closing the read side or full peer as supported by the platform.
- Race peer close/cancellation against response completion in a deterministic loop or barrier-controlled test.
- Assert either valid terminal ordering is accepted, but both converge to zero ownership, zero active subscription growth, joined forwarders, and no live event leakage.
- Repeat enough cycles to detect retained writer/filter/receiver references.

Injected lifecycle errors remain useful for boundary reachability, serialization-equivalent coverage, and deterministic pre-activation failure. They must not be presented as proof of real queue saturation or real peer disconnect.

## 7. Work package D — Complete per-scenario rollback assertions

Create a reusable assertion harness for staged-subscription failures. Every real failure fixture must verify all applicable invariants rather than relying on separate helper tests.

Required assertions:

- no successful canonical subscription response before the failure point, except explicitly permitted post-delivery/pre-activation cases;
- no live projection envelope after rollback;
- connection-local state no longer owns the subscription;
- daemon active subscription count equals the pre-scenario baseline;
- `take_subscription_receiver` or the equivalent test seam cannot reacquire the failed receiver;
- projection forwarder completion/drop probe reached baseline;
- send, receive, and raw task probes reached baseline after handler completion;
- calling connection cleanup or daemon unsubscribe a second time is harmless;
- no artifact-read or diagnostic counter remains elevated;
- an unrelated client remains connected and receives a unique event;
- no unbounded retry, task creation, or queue growth occurred.

Where direct transport-state access is intentionally private, expose a narrow test observer rather than weakening encapsulation in production APIs.

## 8. Work package E — Interrupted replay durability and identity proof

Primary files:

- `tests/projection_transport_real.rs`
- `src/core/transport/daemon_socket_integration_tests.rs`

### Fresh connection identity

- For `/core`, record the first and resumed `ServerHello.client_id` or equivalent daemon-issued connection identity and assert they differ.
- For Unix, record the first and resumed server-issued client ID and assert they differ.
- For `/tui`, do not change the public wire contract solely for this assertion. Use a connection-local test observer where available; otherwise document that fresh socket plus fresh subscription identity is the externally observable guarantee and avoid claiming a wire-visible connection ID.

### Disconnect during replay

Implement at least one real WebSocket fixture and one Unix fixture:

1. create and persist a cursor;
2. disconnect the first connection;
3. publish a uniquely identified missing range;
4. reconnect and pause replay response delivery after the daemon subscription and receiver are installed;
5. close/drop the resumed peer before response completion;
6. assert connection tasks, projection forwarder, connection ownership, and daemon active subscription count return to baseline;
7. assert committed replay events remain stored and the supplied durable cursor is unchanged;
8. connect a third time and resume from the same cursor;
9. assert the exact missing sequence and event identities are replayed once;
10. publish the next live event and assert `replay_end_seq + 1` with no duplicate during a bounded quiet period.

This proves that transport interruption cleans transient state without deleting replay authority.

## 9. Work package F — Static guards and evidence hygiene

Primary files:

- `scripts/check_projection_transport_lifecycle.py`
- optional dedicated verification guard

Extend guards to require stable test names or structural evidence for:

- shared joined WebSocket task ownership;
- three first-exit task-owner cases;
- real adapter peer-close lifecycle tests for `/core` and `/tui`;
- actual queue-saturation tests distinct from `fail_next(... Timeout)`;
- actual peer-disconnect tests distinct from `fail_next(... Cancelled)`;
- interrupted replay cleanup and retry coverage;
- exact connection identity assertions where exposed;
- M009 closure record with exact test count and commits;
- no `unbounded_channel` in WebSocket server paths;
- no direct live-transition bypass;
- no detached Unix connection/raw forwarder.

The guard may inspect stable test/helper names and bounded structural patterns. It must not infer semantic success merely from an error enum appearing in a scenario array.

## 10. Documentation and final closure reconciliation

Required documents:

- `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md`
- `plans/closure/session-projections/008-status.md`
- `plans/subsystems/session-projections-roadmap.md`
- `plans/registry.md`
- new `plans/closure/session-projections/009-status.md`

Required changes:

1. Mark M008 conditionally closed, not failed.
2. Preserve accepted M008 production outcomes:
   - shared joined WebSocket task ownership;
   - exact replay envelope continuity;
   - adapter-local lifecycle seams;
   - existing seven-boundary rollback matrices;
   - lifecycle static guard.
3. Record that M009 owns only production-shaped mechanism verification and complete per-scenario lifecycle evidence.
4. Mark the M008 implementation plan as conditionally closed/superseded for strict verification rather than ready for handoff.
5. Keep M009 as the sole dependency-ready projection plan.
6. Create M009 closure only after all acceptance criteria pass.
7. Record exact implementation and closure commits.
8. Record exact test names, executable counts, command lines, and relevant output.
9. Separate unrelated workspace warnings or clippy findings from M009-local results.
10. Return the roadmap and registry to strict closed status only through the M009 closure record.

## 11. Required verification matrix

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

- `/core` actual queue saturation;
- `/tui` actual queue saturation;
- `/core` and `/tui` real peer-close lifecycle;
- Unix peer-disconnect and cancellation/response race;
- three first-exit `ConnectionTaskSet` cases;
- 100-cycle WebSocket churn;
- client-A failure/client-B continuity;
- interrupted replay cleanup and successful retry.

Run transport tests with one test thread unless the test-local instrumentation is explicitly concurrency-safe. Do not claim CI evidence unless a workflow/status check is actually present.

## 12. Acceptance criteria

- M008 production task ownership and exact replay behavior remain intact.
- `/core` and `/tui` have tests that saturate their actual bounded queues until the real critical-send timeout fires.
- `/core`, `/tui`, and Unix have real peer-close/disconnect tests during staged setup.
- Connection cancellation is observed winning a real pending setup operation.
- Unix has a deterministic cancellation-versus-response-completion race test.
- Send-first, receive-first, and raw-event-first task-owner cases all cancel and await siblings.
- Real `/core` and `/tui` peer-close paths terminate and await every connection task.
- Writer failure and raw-event source closure use the same joined teardown path.
- At least 100 repeated connection/staged-failure cycles return task, forwarder, and subscription probes to baseline.
- Client A teardown or setup failure does not perturb client B.
- Every real failure fixture proves connection ownership removal, daemon subscription removal, receiver non-reuse, forwarder termination, task termination, and idempotent cleanup.
- Serialization-equivalent and pre-activation injected cases are clearly classified as seam tests, not real socket/queue mechanisms.
- Fresh connection identity is asserted where exposed without changing the protocol.
- A replay response is interrupted by a real disconnect; transient state returns to baseline and a later connection replays the same missing range successfully.
- Replay retry transitions to the next live sequence without a gap, duplicate, or reorder.
- Static guards distinguish actual queue/disconnect tests from injected error classifications.
- Focused transport, replay, disclosure, TUI, and lifecycle suites pass.
- Closure test names and counts match executable results.
- No unresolved high or medium M009 finding remains.
- `plans/closure/session-projections/009-status.md` records exact implementation and closure commits.
- The registry contains no dependency-ready projection plan only after M009 is strictly closed.

## 13. Handoff order

1. Add connection-local test probes and queue-capacity/writer controls.
2. Complete the three-way shared task-owner first-exit matrix.
3. Add real `/core` peer-close, writer-failure, raw-source, and churn tests.
4. Add equivalent real `/tui` lifecycle tests.
5. Add actual `/core` and `/tui` queue-saturation timeout tests.
6. Add Unix peer-disconnect and cancellation/response race tests.
7. Add reusable complete rollback assertions and unrelated-client continuity checks.
8. Add interrupted replay cleanup/retry and connection identity assertions.
9. Extend lifecycle/evidence static guards.
10. Run the full verification matrix.
11. Write M009 closure and reconcile M008, roadmap, and registry.
12. Return the subsystem to strict closed status only after acceptance.

## 14. Final completion definition

This line of work is complete when the production transport mechanisms—not only injected error labels—prove bounded timeout, cancellation, disconnect, joined teardown, complete rollback, cross-client isolation, and replay durability; every transient task, receiver, forwarder, and subscription returns to baseline; and the planning record precisely matches the executable repository.