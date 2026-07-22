# Session Projections Milestone 011 — Evidence Correctness and Mechanism Verification Closure

Status: ready for handoff

Repository baseline: `8bd59b22662a289f3124c9b3113e545faa9446d7` (`main`)

Source subsystem:

- `plans/subsystems/session-projections-roadmap.md`

Corrected predecessor closure:

- `plans/closure/session-projections/010-status.md`

Primary class: verification correction / mechanism attribution / per-connection evidence / Unix I/O races / closure integrity

## 1. Objective

Close the remaining evidence defects found after M010 without reopening projection protocol, storage, reducer, disclosure, or product behavior.

M010 delivered useful production-safe test controls and materially improved the transport test surface:

- connection-local queue capacity overrides and writer gates;
- transport lifecycle observers;
- first-task-kind recording;
- `/core` raw-source cancellation control;
- deterministic capacity-one `Full` observation;
- TUI pending snapshot and replay writer-barrier fixtures;
- broader Unix lifecycle and replay fixtures;
- a full focused local verification matrix;
- probe-completion flake correction and repeated clean runs.

Post-M010 inspection found that strict closure remains unsupported because several tests do not prove the exact mechanism claimed:

1. the `/core` timeout fixture can time out while waiting for writer receipt rather than while `tx.send()` is blocked on a full queue;
2. `/tui` still has no full-queue critical-send timeout fixture;
3. the task-owner matrix covers three panic classifications through a mirror helper that aborts but does not await siblings, rather than all six cases through the production teardown path;
4. `/tui` raw-source-first remains a normal peer-close test;
5. Unix fixtures mostly exercise post-subscription disconnect, injected writer failure, listener shutdown, and normal replay retry rather than pre-response peer failure, actual I/O failure, deterministic completion races, and interruption during replay delivery;
6. the rollback helper omits connection ownership, projection-forwarder joins, handler completion, no-live-leakage, unrelated-client continuity, and bounded resource assertions, and it is not used by every real failure fixture;
7. static guards remain predominantly name/substr-based;
8. M010 plan, closure, roadmap, registry, final commit, and CI evidence are inconsistent.

M011 succeeds only when the test establishes a causal chain from a directly observed mechanism to the exact production result and then to complete per-connection cleanup.

## 2. Scope boundaries

### In scope

- Deterministic full-queue critical-send timeout fixtures for both `/core` and `/tui`.
- Operation-correlated transport observations that distinguish queue reservation timeout from writer-receipt timeout.
- Six faithful `ConnectionTaskSet` first-exit cases using the production cancel/abort-and-await path.
- Real raw-source-first adapter fixtures for both `/core` and `/tui`.
- Per-connection probes rather than one aggregate probe shared by multiple connections.
- Real Unix peer failure before response completion.
- Actual Unix write failure at the production I/O operation present in the adapter.
- Deterministic Unix cancellation-versus-response-completion outcomes.
- Unix interruption during replay response delivery followed by exact retry.
- Repeated Unix race/churn convergence.
- One complete rollback harness used by every applicable real failure fixture.
- Static guards that verify ordering and mechanism markers, not only names.
- Exact closure evidence and truthful CI/local-execution distinction.

### Explicitly out of scope

- Projection DTO, snapshot, event, cursor, or wire schema changes.
- Projection protocol version changes.
- SQLite schema, retention, checkpoint, sequence authority, or replay service changes.
- Reducer/controller semantics.
- Disclosure, redaction, artifact, authentication, or authorization policy.
- New TUI, observer, web, ACP, or team-collaboration features.
- Cross-daemon replay replication.
- Version-4/raw compatibility removal.
- General workspace lint cleanup outside touched files.

Production queue sizes, timeout values, and transport semantics must remain unchanged. New controls must be test-only or dormant under normal server state.

## 3. Evidence model

Every M011 real-mechanism test must record four distinct layers:

1. **Precondition** — the exact production resource is in the required state.
2. **Operation** — the exact production operation begins after the precondition.
3. **Result** — the operation returns the exact typed result claimed.
4. **Convergence** — every connection-local and daemon-owned resource returns to its defined baseline.

A test fails closure if it proves only elapsed time, connection closure, a generic error, an injected classification, or an unrelated timeout elsewhere in the connection.

### Required operation correlation

Replace `TransportLifecycleObserver::any_timeout()` as closure authority with operation-scoped records such as:

```text
CriticalSendObservation {
    operation_id,
    request_id_or_kind,
    stage,
    queue_capacity,
    queue_remaining_capacity_before_send,
    queue_full_before_send,
    enqueue_started,
    enqueue_completed,
    receipt_wait_started,
    final_result,
}
```

Equivalent representation is acceptable if it proves:

- which request/response generated the observation;
- whether the queue was full before `tx.send()` started;
- whether enqueue completed;
- whether timeout occurred during enqueue reservation or receipt wait;
- the final `CriticalDeliveryError`.

Do not infer queue-reservation timeout from a connection-wide history containing any timeout.

## 4. Work package A — Per-connection probe ownership

Primary files:

- `src/server/state.rs`
- `src/server/ws.rs`
- `tests/projection_transport_real.rs`

The current test server can reuse one `Arc<ConnectionTaskProbe>` across multiple upgraded connections. Replace aggregate ambiguity with one probe per connection.

Required design:

1. Add a connection-probe factory or registry owned by the test fixture.
2. Create a fresh `ConnectionTaskProbe` during each `/core` or `/tui` upgrade.
3. Register it by server-issued connection/client identity or a private test connection sequence.
4. Allow tests to await and retrieve the probe for a specific connection.
5. Remove the registry entry after handler completion, while retaining an immutable final observation for assertions.
6. Record:
   - first terminal task kind;
   - first terminal classification;
   - send/receive/raw task joins;
   - projection-forwarder joins;
   - cleanup entry count;
   - handler-completed flag;
   - owned-subscription count before and after cleanup.
7. Ensure no probe is shared between client A and client B.
8. Keep probe state bounded and payload-free.

Acceptance requires per-connection exact counts, not aggregate `>= N` assertions.

## 5. Work package B — Deterministic `/core` full-queue timeout

Primary files:

- `src/server/ws.rs`
- `tests/projection_transport_real.rs`

Implement a single fixture where queue fullness and the critical send are causally ordered.

Required sequence:

1. Start `/core` with capacity `1` and `gate_before_recv = true`.
2. Complete handshake with the writer released as needed.
3. Wait until the writer re-enters the pre-`recv()` gate.
4. Fill the actual outbound channel with one filler using the real `WsSender`.
5. Attempt a second `try_send` and assert the result is specifically `TrySendError::Full`.
6. Record the queue-full observation in the operation-scoped observer.
7. Only after step 5, send a real projection subscribe or resume request over the WebSocket.
8. The receive task must process that request and enter the production `staged_critical_send` while the writer remains blocked and the channel remains full.
9. Assert the correlated observation for that request records:
   - queue full before enqueue;
   - enqueue started;
   - enqueue did not complete;
   - receipt wait did not begin;
   - final result `Err(CriticalDeliveryError::Timeout)`.
10. Assert no successful canonical response and no live projection envelope escaped.
11. Release the gate, close the connection, and invoke the complete rollback harness.

Prohibited substitutions:

- enqueueing the filler after the request;
- manually calling `mark_fill_full()` without observing `TrySendError::Full`;
- accepting `any_timeout()` from another operation;
- timing out while waiting for a writer receipt;
- using an outer client read timeout as the main assertion.

## 6. Work package C — Deterministic `/tui` full-queue timeout

Primary files:

- `src/server/ws.rs`
- `tests/projection_transport_real.rs`

Add a TUI counterpart to Work Package B.

Required sequence:

1. Start `/tui` with capacity `1` or `2` and a pre-`recv()` writer gate.
2. Complete the two-message capability handshake without leaving diagnostic traffic queued.
3. Wait for the writer to re-enter the pre-`recv()` gate.
4. Fill the actual outbound queue and explicitly observe `TrySendError::Full`.
5. Send `ProjectionSubscribe` or `ProjectionResume` only after fullness is established.
6. Correlate the production staged-send observation to that TUI operation.
7. Assert timeout occurred before enqueue completion, not during receipt wait.
8. Assert the initializing subscription never becomes live.
9. Assert no snapshot/replay response or live envelope escapes.
10. Apply the complete rollback harness and unrelated-client continuity assertion.

The existing TUI writer-barrier interruption tests remain valid cancellation coverage but cannot satisfy this queue-timeout requirement.

## 7. Work package D — Faithful six-case task-owner matrix

Primary file:

- `src/server/ws.rs`

Tests must invoke `ConnectionTaskSet::join_after_first_exit` or a private wrapper that calls it unchanged. Remove closure reliance on `first_exit_classification_for_test` if that helper does not perform production cancellation and joins.

Required clean-exit cases:

- send completes first;
- receive completes first;
- raw-event completes first.

Required panic cases:

- send panics first;
- receive panics first;
- raw-event panics first.

Use deterministic barriers or one-shot triggers so the selected task cannot race with siblings.

For every case assert:

- expected first task kind;
- expected clean/panic classification;
- connection cancellation token is cancelled;
- selected handle is consumed exactly once;
- both sibling handles are aborted and awaited;
- sibling drop guards reach zero;
- all three handles are `None` after teardown;
- each task completion counter is exactly one;
- panic does not prevent sibling joins;
- cleanup can execute once after task teardown;
- no abort-without-await path exists.

Add a unit-level test that intentionally delays sibling cancellation to prove `join_after_first_exit` waits for their terminal join results rather than returning immediately after `abort()`.

## 8. Work package E — Real raw-source-first for both adapters

Primary files:

- `src/server/ws.rs`
- `tests/projection_transport_real.rs`

Retain the valid `/core` raw-source cancellation fixture, but rebind it to a per-connection probe and the complete rollback harness.

Add the equivalent `/tui` fixture:

1. complete capability negotiation and subscription;
2. keep peer and writer healthy;
3. trigger the connection-local raw-source termination control;
4. assert the TUI connection's first terminal task is `RawEvent`;
5. assert send and receive siblings are cancelled and joined;
6. assert no client close occurred before the first-task observation;
7. invoke complete rollback and unrelated-client continuity checks.

Rename or reclassify legacy TUI tests that publish an event and close the client; they are peer-close lifecycle tests, not raw-source-first tests.

## 9. Work package F — Actual Unix I/O and completion races

Primary files:

- `src/core/transport/daemon_socket.rs`
- `src/core/transport/daemon_socket_integration_tests.rs`
- `src/core/transport/projection.rs`

First inspect the actual Unix writer implementation and name fixtures for the operation it truly performs. Do not claim a flush failure if the adapter has no separately observable buffered flush step.

Add connection-local Unix test controls:

- pre-write barrier;
- optional post-write/pre-flush barrier only if the implementation has that boundary;
- operation observer with actual `std::io::ErrorKind` or typed transport error;
- handler/forwarder completion probes;
- per-connection owned-subscription observer.

### F1. Peer closes before canonical response write

1. Install daemon subscription and receiver.
2. Pause before the canonical response's production write operation.
3. Close both halves of the real Unix client peer.
4. Release the barrier.
5. Assert the real write operation returns an actual I/O failure such as `BrokenPipe`, `ConnectionReset`, or the platform-equivalent documented error.
6. Assert no response completed and the subscription never became live.
7. Apply complete rollback.

No `fail_next` injection is permitted in this fixture.

### F2. Actual writer failure

Create a distinct fixture that fails the actual write path through peer shutdown/drop. If the adapter uses buffered output and a separate flush operation, add a second fixture for actual flush failure; otherwise document that write is the only production I/O boundary and do not claim flush-specific coverage.

### F3. Deterministic completion race

Use the pre-write barrier to force both allowed orders:

- **completion-first:** release write, observe successful canonical response completion, then close peer and clean up;
- **cancellation-first:** close peer/cancel connection before release, observe write/cancellation failure and rollback.

Run each forced ordering repeatedly, at least 25 cycles, and assert identical final resource baselines. A listener-wide shutdown after an already active subscription is supplementary coverage, not this race.

### F4. Interrupted replay delivery

1. First connection subscribes and records client ID, subscription ID, stream ID, and cursor.
2. Disconnect and publish a unique missing range.
3. Second connection has a fresh client ID and resumes from the cursor.
4. Pause before the replay response write completes.
5. Close the second real peer during the pause.
6. Assert actual I/O/cancellation result and complete transient cleanup.
7. Assert durable events and original cursor authority remain unchanged.
8. Third connection has another fresh client ID and resumes from the same cursor.
9. Assert exact event sequence and identities once.
10. Publish the next live event and assert `replay_end_seq + 1`.
11. Assert no duplicate during a bounded quiet period.

A normal replay retry after only the first connection disconnects does not satisfy this requirement.

### F5. Repeated convergence

Run at least 50 Unix peer-failure/race/replay-interruption cycles in bounded batches. After each cycle or batch assert:

- active subscriptions at baseline;
- no retained receiver;
- no writer/raw/projection forwarder growth;
- handler completion count matches opened connections;
- temporary socket paths and tasks are released;
- a fresh unrelated client can connect and receive its own event.

## 10. Work package G — Complete rollback and non-interference harness

Primary files:

- `tests/projection_transport_real.rs`
- `src/core/transport/daemon_socket_integration_tests.rs`
- narrow shared test-support modules

Implement common invariant checks with transport-specific adapters.

Required inputs:

- failed connection identity and per-connection probe;
- failed subscription ID and project scope;
- pre-scenario daemon subscription count;
- expected operation/result observation;
- unrelated client B, its subscription ID, and a unique event marker;
- expected projection-forwarder count;
- expected response-delivery state.

Required assertions:

1. exact operation result matches the intended mechanism;
2. no successful canonical response before the permitted point;
3. no live projection envelope for the failed subscription;
4. failed connection ownership map contains no subscription;
5. daemon active subscription count equals baseline;
6. failed receiver cannot be reacquired;
7. projection-forwarder joins equal the expected count;
8. send, receive, and raw task joins are exactly one for the failed connection;
9. handler-completed flag is true;
10. cleanup count is exactly one;
11. second connection cleanup is a no-op;
12. second daemon unsubscribe returns the expected typed not-owned/no-op result;
13. artifact/diagnostic/resource counters return to baseline where exposed;
14. queue depth and retry/task counts remain bounded;
15. unrelated client B remains connected and receives its unique projection event;
16. probes retain no payload, artifact, hidden-reasoning, or secret data.

Apply this harness to every M011 real failure fixture and to retained M010 fixtures that remain closure evidence:

- `/core` full-queue timeout;
- `/tui` full-queue timeout;
- `/core` and `/tui` raw-source-first;
- TUI pending snapshot/replay interruption;
- WebSocket writer failure where retained;
- Unix pre-response peer failure;
- Unix actual writer failure;
- Unix cancellation-first race;
- Unix interrupted replay.

Do not claim complete rollback from separate tests covering different subsets of invariants.

## 11. Work package H — Semantic guards

Primary file:

- `scripts/check_projection_transport_lifecycle.py`

Strengthen guards to reject the exact forms of false closure found after M010.

Required checks:

- `/core` and `/tui` full-queue timeout fixtures both exist.
- Each fixture establishes pre-`recv()` writer gating, observes `TrySendError::Full`, and sends the lifecycle request only after the full observation.
- Each asserts a correlated operation record with `queue_full_before_send = true`, `enqueue_completed = false`, and `final_result = Timeout`.
- Neither fixture uses `any_timeout()` as sole authority.
- Six task-owner tests invoke the production teardown wrapper and assert cancellation plus sibling joins.
- Raw-source-first tests exist for both `/core` and `/tui` and assert `RawEvent` before peer close.
- Unix real-I/O fixtures contain peer shutdown/drop and actual error-kind assertions.
- Unix real-I/O fixtures contain no `fail_next` injection.
- Unix replay fixture drops the second resumed peer before replay response completion.
- Repeated Unix race/churn loop exists and checks baselines.
- Complete rollback harness asserts forwarder joins, handler completion, connection ownership, unrelated-client continuity, and bounded counters.
- Every closure-relevant real failure fixture calls the complete harness.
- M010 remains conditionally closed and links M011.
- M011 closure contains exact full commit hashes and no placeholders such as `next commit`.
- Registry and roadmap contain M011 as the sole ready projection plan until closure.

Static checks supplement runtime tests; they are not evidence by themselves.

## 12. Documentation and closure reconciliation

Required documents:

- `plans/implementation/session-projections/010-mechanism-faithful-transport-verification-and-final-closure.md`
- `plans/closure/session-projections/010-status.md`
- `plans/subsystems/session-projections-roadmap.md`
- `plans/registry.md`
- new `plans/closure/session-projections/011-status.md`

Required changes:

1. Mark M010 conditionally closed, not failed.
2. Preserve accepted M010 instrumentation, TUI interruption, `/core` raw-source, capacity-one, test-matrix, and flake-fix outcomes.
3. Record exact M010 commits:
   - `a3ab136868236ff56ec221813c3da9f299993967`;
   - `7e31d573e4b02334751ce0fcb2ebf3c2c7614acf`;
   - `0d68dca516ba1df7a59c3d55d5863381b2d6788b`;
   - `e729c3abbfc45c862e6636d29a3ea9d64e5c28a9`;
   - `131adaac6941f9276d7dd9c96cb2e086dee1f4d8`;
   - `8bd59b22662a289f3124c9b3113e545faa9446d7`.
4. Remove placeholder commit descriptions.
5. Mark M010 implementation plan conditionally closed/superseded for strict evidence.
6. Keep M011 as the sole dependency-ready projection plan.
7. Create M011 closure only after every acceptance criterion passes.
8. Record exact implementation, follow-up, closure, and final reviewed commits.
9. Record exact test names, counts, commands, outputs, repeated-run counts, and residual findings.
10. Distinguish local execution from GitHub workflow/status evidence.
11. Return roadmap and registry to strict closed status only through accepted M011 closure.

## 13. Required verification matrix

At minimum run:

```bash
cargo fmt -- --check
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
CARGO_BUILD_JOBS=1 cargo clippy -p codegg-protocol --all-targets -- -D warnings
cargo test -p codegg --lib core::transport::projection --all-features -- --nocapture
cargo test -p codegg --lib server::ws --all-features -- --test-threads=1 --nocapture
cargo test -p codegg --lib core::transport::daemon_socket --all-features -- --test-threads=1 --nocapture
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
cargo test --test single_daemon_lifecycle -- --test-threads=1 --nocapture
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

Required focused executions:

- `/core` operation-correlated full-queue timeout;
- `/tui` operation-correlated full-queue timeout;
- six faithful task-owner first-exit cases;
- `/core` and `/tui` real raw-source-first;
- TUI snapshot/replay interruption;
- Unix pre-response peer failure;
- Unix actual write failure;
- Unix forced completion-first and cancellation-first race loops;
- Unix interrupted second-replay delivery and third-connection retry;
- complete rollback harness application tests.

Required stability runs:

```bash
for i in $(seq 1 25); do
  cargo test --test projection_transport_real --features server \
    -- --test-threads=1 || exit 1
done

for i in $(seq 1 25); do
  cargo test -p codegg --lib core::transport::daemon_socket \
    --all-features -- --test-threads=1 || exit 1
done
```

Record total runtime and any variance. Do not label local repetition as CI.

## 14. Acceptance criteria

- Accepted M008–M010 production behavior remains intact.
- `/core` and `/tui` each observe `TrySendError::Full` before the target lifecycle request begins its production critical send.
- Each correlated critical-send observation proves enqueue did not complete and returns `CriticalDeliveryError::Timeout`.
- Neither queue fixture relies on receipt timeout, elapsed time, generic connection closure, `any_timeout()`, or injected classification.
- Six task-owner cases execute the production cancel/abort-and-await path and prove sibling joins.
- `/core` and `/tui` actual raw-source termination is observed as first exit while peer remains healthy.
- Per-connection probes are not shared across connections.
- Unix peer closes before canonical response completion and causes an actual production I/O failure or cancellation result.
- Unix writer failure uses real peer shutdown/drop, not `fail_next`.
- Unix completion-first and cancellation-first outcomes are forced and converge identically over repeated cycles.
- Unix replay is interrupted on the second connection before response completion, then retried exactly on a third connection.
- Unix fresh client and subscription identities are asserted.
- At least 100 WebSocket churn cycles and at least 50 Unix race/churn cycles return all resources to baseline.
- Every real failure fixture uses the complete rollback harness.
- Complete rollback asserts connection ownership, daemon ownership, receiver non-reuse, forwarder joins, task joins, handler completion, idempotence, unrelated-client continuity, and bounded counters.
- Static guards reject the M010 false-positive patterns.
- Full verification matrix and 25-run stability loops pass.
- Closure evidence contains exact commits and no placeholders.
- Absence of GitHub checks is reported as absence, not inferred success.
- M010 is recorded as conditionally closed and M011 is the sole strict closure authority.
- No unresolved high or medium M011 finding remains.
- Registry contains no dependency-ready projection plan only after M011 strict closure.

## 15. Handoff order

1. Replace aggregate connection probes with per-connection probe ownership.
2. Add operation-correlated critical-send observations.
3. Implement deterministic `/core` full-queue timeout.
4. Implement deterministic `/tui` full-queue timeout.
5. Replace classification-only task tests with six production-path teardown tests.
6. Add TUI real raw-source-first coverage.
7. Add Unix pre-write barriers and actual I/O observations.
8. Implement forced Unix completion/cancellation outcomes and repeated convergence.
9. Implement second-connection Unix replay interruption and third-connection retry.
10. Complete and apply the rollback/non-interference harness.
11. Strengthen semantic guards.
12. Run focused tests, full matrix, and stability loops.
13. Reconcile M010 plan/closure, roadmap, registry, commits, counts, and CI status.
14. Write and accept M011 closure only after all criteria pass.

## 16. Final completion definition

This line is complete when the same fixture proves the required precondition, invokes the exact production operation after that precondition, observes the exact typed result from that operation, and verifies complete per-connection convergence; all three transports pass their mechanism-faithful failure and replay matrices; closure guards reject prior false-positive patterns; and the planning record exactly matches executable and independently available evidence.