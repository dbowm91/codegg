# Session Projections Milestone 012 — TUI Disconnect Lifecycle and Final Evidence Closure

Status: ready for handoff

Repository baseline: `1a93167ee3bdfdc55e4bd2746180443cc19b7c96` (`main`)

Source subsystem:

- `plans/subsystems/session-projections-roadmap.md`

Corrected predecessor closure:

- `plans/closure/session-projections/011-status.md`

Primary class: production lifecycle correction / bounded task ownership / canonical critical-send instrumentation / Unix typed I/O evidence / complete rollback / final closure integrity

## 1. Objective

Close the remaining Session Projections correctness and evidence defects found after M011.

M011 delivered useful and largely valid improvements:

- one probe per WebSocket connection;
- deterministic capacity-one queue preconditions;
- operation-correlated critical-send observations;
- `/core` and `/tui` full-queue fixtures;
- six clean/panic first-exit cases through the production task-owner teardown;
- controlled raw-source-first fixtures for `/core` and `/tui`;
- broader Unix peer-close, replay-interruption, race, and convergence fixtures;
- stronger rollback helpers and static guards;
- writer-gate cancellation fixes and a TUI writer cancellation branch.

Post-M011 review found one confirmed production correctness defect and several remaining evidence defects:

1. `/tui` can deadlock when the receive task is awaiting a projection handler and therefore cannot observe peer close; the writer may also remain parked, no task fires `connection_cancel`, joined teardown never starts, and the daemon-side projection subscription can leak;
2. the M011 mitigation reduces but does not eliminate the failure rate;
3. observer-enabled critical delivery uses a separate implementation with two timeout budgets instead of instrumenting the canonical one-budget production implementation;
4. Unix F1/F2 infer an I/O error from EOF and cleanup but do not capture or assert the production `std::io::Error` result;
5. rollback helpers do not assert projection-forwarder joins, exact connection ownership removal, or complete no-leakage/resource convergence;
6. the TUI rollback helper incorrectly states that TUI has no daemon-side projection subscription, despite the TUI adapter creating, owning, receiving from, and unsubscribing daemon-issued projection subscriptions;
7. full-queue rollback uses a synthetic subscription ID rather than the actual staged subscription identity;
8. `ConnectionProbeRegistry` uses `try_lock()` and can silently drop a probe registration;
9. the sibling-join timing test relies on elapsed sleep duration even though aborted Tokio tasks should resolve promptly;
10. M011 plan, closure, roadmap, registry, final commits, test stability, and CI evidence are contradictory.

M012 succeeds only when peer closure remains observable while TUI lifecycle work is pending, all connection-owned work is bounded and joined, tests exercise the exact production critical-send implementation, Unix error claims assert typed production observations, rollback proves every owned resource converges, and the planning record matches executable evidence.

## 2. Scope boundaries

### In scope

- A production correction to `/tui` receive/handler ownership so peer close cancels pending lifecycle work deterministically.
- Explicit bounded ownership for the TUI socket-reader, request-handler, writer, raw-event, and projection-forwarder tasks.
- One canonical critical-send implementation with optional in-place observation and one timeout budget.
- Correct operation metadata for queue capacity, remaining capacity, enqueue completion, receipt waiting, and final typed result.
- Typed Unix writer observations for actual peer-induced I/O failures.
- Real peer read-side closure/drop before the server write, without listener shutdown as a competing cause.
- Exact projection-forwarder join and connection-ownership observations.
- Correct daemon-side TUI subscription rollback assertions.
- Actual staged subscription identity capture for failure fixtures; no synthetic placeholder IDs.
- Infallible per-connection probe registration correlated by connection identity.
- Deterministic task-join proof based on task completion/drop observations rather than elapsed time.
- Closure-critical stability loops, semantic guards, full verification, and exact planning reconciliation.

### Explicitly out of scope

- Projection DTO, snapshot, event, cursor, or wire-schema changes.
- Projection protocol version changes.
- Replay persistence, SQLite schema, retention, checkpoint, sequence, or cursor-authority changes.
- Reducer/controller semantics.
- Disclosure, redaction, artifact, authentication, authorization, or team-collaboration policy.
- New TUI product features or visual behavior.
- Cross-daemon replay replication.
- Version-4/raw compatibility removal.
- General server task-framework refactoring outside the WebSocket projection connection boundary.
- General workspace lint cleanup outside touched files.

Production queue capacities, timeout values, request ordering, replay meaning, and live-activation semantics must remain unchanged unless the lifecycle correction exposes a directly related production bug.

## 3. Required architecture and invariants

### 3.1 TUI socket ownership

- Exactly one task owns and polls `ws_rx`.
- That task remains able to observe `Message::Close`, stream EOF, and socket error while projection setup, replay, acknowledgement, or artifact work is pending.
- A lifecycle handler cannot monopolize the only `ws_rx` poll loop.
- Peer close or socket error fires `connection_cancel` immediately and exactly once.
- No second task polls the same WebSocket stream.
- No unbounded task is spawned per TUI message.

### 3.2 Bounded request processing

- Parsed TUI messages enter a bounded per-connection request queue.
- A single owned handler task consumes requests sequentially, preserving request order.
- Queue capacity is explicit and finite.
- Queue closure or saturation has a documented fail-closed outcome.
- The handler selects or otherwise responds to `connection_cancel` while work is pending.
- Pending critical delivery, daemon request, replay preparation, or lifecycle checkpoint cannot prevent connection teardown indefinitely.

### 3.3 Task ownership

The connection owner retains and joins every connection-scoped task:

- socket reader;
- request handler;
- writer;
- raw-event forwarder;
- each projection subscription forwarder.

Equivalent grouping is acceptable only when the same ownership and join guarantees are directly asserted.

- Connection cancellation occurs before sibling teardown.
- Every retained task handle is consumed once.
- Normal cancellation joins are not reported as abnormal.
- Panic in any connection task does not prevent cancellation and joining of all siblings.
- Handler completion and connection cleanup are observed separately.

### 3.4 Canonical critical delivery

- `critical_send` and staged critical delivery have one canonical implementation each.
- Optional observation records stage transitions inside that canonical future.
- Enabling an observer cannot change timeout count, timeout duration, ordering, cancellation behavior, or error mapping.
- A staged send has one total bounded-delivery budget, not separate enqueue and receipt budgets.
- Tests with and without observers produce the same typed terminal result for equivalent controlled conditions.

### 3.5 Rollback and replay

- A subscription is not live until the canonical response is delivered successfully.
- Peer close during pending setup or replay rolls back connection ownership and daemon subscription ownership.
- Every projection forwarder is cancelled and awaited.
- Receiver ownership cannot be reacquired after rollback.
- Duplicate unsubscribe remains harmless.
- Replay history and cursor authority remain durable.
- Retry from the same cursor receives the exact missing range once, followed by the next live sequence without duplicate or gap.
- An unrelated client remains connected and receives its unique marker event.

## 4. Work package A — Correct the TUI reader/handler deadlock

Primary files:

- `src/server/ws.rs`
- `src/server/state.rs`
- `tests/projection_transport_real.rs`

Implement a bounded TUI connection pipeline in which socket close detection is independent of handler progress.

Preferred shape:

```text
WebSocket read half
    |
    v
socket-reader task ---- peer close/error ----> connection_cancel
    |
    | bounded TuiMessage queue
    v
ordered request-handler task
    |
    +--> daemon/projection lifecycle operations
    +--> canonical critical sends

connection owner
    |-- writer task
    |-- socket-reader task
    |-- request-handler task
    |-- raw-event task
    `-- owned projection forwarders
```

Required behavior:

1. Split the current inline receive loop into a socket-reader task and a sequential request-handler task.
2. The socket reader exclusively owns `ws_rx`.
3. The reader parses or forwards bounded message data without awaiting projection lifecycle completion.
4. The reader observes:
   - `Message::Close`;
   - WebSocket EOF;
   - WebSocket read error;
   - connection cancellation.
5. On peer close/error, the reader fires `connection_cancel` before returning.
6. The request handler owns `handle_tui_message_with_observer` execution and processes one message at a time.
7. The handler exits promptly when `connection_cancel` fires, including while waiting for critical-delivery receipt or lifecycle checkpoints.
8. The request queue is bounded; use a capacity justified by existing WebSocket bounds.
9. Queue saturation must cancel or fail the connection explicitly; silently dropping projection lifecycle requests is prohibited.
10. Do not spawn a detached task per incoming message.
11. Retain all task handles in the connection owner.
12. Cleanup begins only after all connection tasks have converged through the common teardown path.

Alternative designs are acceptable only if they prove the reader remains close-responsive while a handler is pending and retain bounded, explicit, joined task ownership.

Prohibited designs:

- a second task polling the same `ws_rx`;
- a detached close watcher with unclear stream ownership;
- `tokio::spawn` for every TUI message;
- an unbounded request channel;
- relying on TCP reset timing to wake the writer;
- retaining the current inline `while ws_rx.next() { handle(...).await }` shape.

## 5. Work package B — Extend task ownership and per-connection probes

Primary files:

- `src/server/ws.rs`
- `src/server/state.rs`
- `tests/projection_transport_real.rs`

Extend the task owner and probes to cover the new handler task and projection forwarders.

Required changes:

1. Add an explicit task kind for the TUI request handler, or generalize `ConnectionTaskSet` to a bounded owned-task collection with named kinds.
2. Preserve existing `Send`, `Receive`, and `RawEvent` evidence compatibility.
3. Record exact completion counts for:
   - writer;
   - socket reader;
   - request handler;
   - raw-event task;
   - projection forwarders.
4. Record the first terminal task kind and panic classification.
5. Record connection cancellation fired.
6. Record handler completion separately from final cleanup.
7. Record owned-subscription count before cleanup and after cleanup.
8. Record connection-owner map removal where applicable.
9. Ensure every projection forwarder increments `projection_forwarders_joined` only after its `JoinHandle` is awaited.
10. `assert_all_at_baseline` must include all task kinds relevant to that adapter and expected projection-forwarder joins.

Replace `ConnectionProbeRegistry::factory()` registration with an infallible mechanism:

- a `std::sync::Mutex` with non-poisoning handling;
- a bounded/unbounded test-only registration channel whose send cannot be silently ignored;
- or an equivalent identity-keyed registry.

Requirements:

- no `try_lock()` registration loss;
- correlation by actual connection ID or an explicit private sequence, not only insertion order;
- bounded retained final records;
- exact removal/finalization semantics;
- no payload bodies, artifact bytes, hidden reasoning, or secrets.

## 6. Work package C — One canonical observed critical-send implementation

Primary file:

- `src/server/ws.rs`

Refactor staged critical delivery so observation is integrated into the production implementation.

Required shape:

```text
staged_critical_send(..., observer: Option<&Observer>, operation_context)
    bounded_critical_delivery ONE TIME {
        checkpoint BeforeControlEnqueue
        observe queue state
        tx.send(outbound)
        observe enqueue completion
        checkpoint AfterControlEnqueueBeforeWriterReceipt
        observe receipt wait start
        receipt_rx.await
    }
    observe exact terminal result
```

Equivalent internal APIs are acceptable if:

1. The observer and non-observer paths invoke the same future body.
2. One `bounded_critical_delivery` call covers the complete staged send.
3. The timeout budget is not reset between enqueue and receipt waiting.
4. Cancellation and lifecycle checkpoint errors preserve existing mappings.
5. `queue_capacity` records maximum channel capacity (`max_capacity()` or the configured capacity).
6. `queue_remaining_capacity_before_send` records current remaining capacity.
7. `queue_full_before_send` is derived from remaining capacity being zero.
8. `enqueue_completed` becomes true immediately after `tx.send` succeeds, regardless of later receipt result.
9. `receipt_wait_started` becomes true before awaiting the receipt.
10. The final result is correlated to a stable operation context.

Operation context must identify the target without retaining payloads, for example:

```text
CriticalSendContext {
    operation_id,
    adapter,
    request_id_or_message_kind,
    lifecycle_boundary,
}
```

Remove closure authority from:

- `any_timeout()`;
- connection-wide uncorrelated result history;
- a separate `run_observed_staged_send` implementation;
- elapsed time alone.

Required parity tests:

- observer disabled and observer enabled under a full queue both return the same `Timeout` within one budget;
- observer disabled and enabled under writer receipt failure return the same `WriterClosed`;
- observer disabled and enabled under cancellation return the same `Cancelled`;
- an enqueue success followed by writer failure records `enqueue_completed = true` and `receipt_wait_started = true`.

## 7. Work package D — Deterministic TUI disconnect lifecycle fixtures

Primary file:

- `tests/projection_transport_real.rs`

Add closure-bearing fixtures that fail on the current deadlock and pass only after Work Package A.

### D1 — Close frame during pending snapshot delivery

1. Complete TUI capability negotiation.
2. Begin `ProjectionSubscribe`.
3. Pause after receiver installation and before canonical response completion.
4. Send a real WebSocket Close frame while the handler remains pending.
5. Assert the socket-reader task observes close and fires `connection_cancel` before the barrier is released.
6. Assert the handler exits through cancellation.
7. Release the barrier only for cleanup safety.
8. Assert all tasks and forwarders join.
9. Assert actual daemon subscription count returns to baseline.
10. Assert the actual staged subscription receiver cannot be reacquired.
11. Assert no snapshot/live projection message escaped.

### D2 — Abrupt peer drop during pending snapshot delivery

Repeat D1 using abrupt client drop rather than a graceful Close frame.

### D3 — Close frame during pending replay delivery

1. Establish and disconnect an initial subscription.
2. Publish a unique missing range.
3. Resume from the previous cursor.
4. Pause before replay response completion.
5. Send Close while replay remains pending.
6. Prove complete rollback.
7. Reconnect with a fresh client identity.
8. Retry from the same cursor.
9. Receive the exact missing range once.
10. Receive the next live sequence.
11. Assert no duplicate during a bounded quiet period.

### D4 — Repeated convergence

Run at least 50 alternating graceful-close and abrupt-drop pending-response cycles.

For every cycle assert:

- bounded completion within the test deadline;
- no hung connection task;
- daemon subscription count at baseline;
- per-connection task counts exact;
- projection forwarders joined;
- no receiver reacquisition;
- no queue/probe registry growth.

The former approximately 20% failure rate must become 0 failures in the required repeated runs.

## 8. Work package E — Correct complete rollback assertions

Primary files:

- `tests/projection_transport_real.rs`
- `src/server/ws.rs`

Replace the current split helpers with adapter-aware complete rollback assertions that reflect actual ownership.

The TUI adapter does create daemon-side projection subscriptions. Therefore the TUI helper must accept and assert the actual:

- connection/client ID;
- subscription ID;
- daemon subscription baseline;
- receiver ownership;
- projection forwarder handle;
- owned connection subscription entry.

Required assertions for every closure-relevant failure fixture:

1. Connection cancellation fired.
2. Socket reader completed exactly once.
3. Request handler completed exactly once for TUI.
4. Writer completed exactly once.
5. Raw-event task completed exactly once.
6. Cleanup ran exactly once.
7. Failed connection owns zero subscriptions after cleanup.
8. Daemon subscription count returned to the exact pre-baseline.
9. Actual failed subscription receiver cannot be reacquired.
10. Every installed projection forwarder was cancelled and joined.
11. Projection-forwarder joined count equals installed forwarder count.
12. Duplicate unsubscribe is harmless.
13. No canonical snapshot/replay response escaped after the failure point.
14. No live projection event escaped from the failed subscription.
15. Outbound queues and observer-held sender clones are released.
16. Probe registry contains one finalized record for the failed connection and does not grow after removal.
17. Unrelated client B remains live and receives a unique marker event.
18. Durable replay state remains unchanged except for legitimately published events.

No synthetic subscription ID may satisfy receiver or unsubscribe assertions.

If a failure occurs before the client receives an ID, capture the real staged ID through a payload-free private lifecycle observer or projection-state inspection seam.

## 9. Work package F — Typed Unix production I/O observations

Primary files:

- `src/core/transport/daemon_socket.rs`
- `src/core/transport/daemon_socket_integration_tests.rs`
- `src/core/transport/projection.rs` if the observer belongs in the shared lifecycle seam

Add a connection-local, test-only/dormant Unix transport observer that records the actual production I/O terminal result.

Example:

```text
SocketWriteObservation {
    connection_id,
    operation_id,
    boundary,
    write_started,
    write_completed,
    flush_started,
    flush_completed,
    io_error_kind,
    terminal_result,
}
```

Equivalent representation is acceptable if the test can assert the actual server-side result.

Requirements:

1. Observation is emitted from the same production `write_all`/`flush` path used normally.
2. No error injection satisfies real-I/O closure.
3. No listener shutdown competes with the peer-induced error before observation.
4. The client closes or shuts down its read direction, or drops the entire peer, before the server write.
5. The test waits for the typed server observation.
6. Accepted platform error kinds are explicit and narrow, for example `BrokenPipe`, `ConnectionReset`, `ConnectionAborted`, or `NotConnected` where platform-appropriate.
7. EOF and subscription cleanup remain convergence assertions, not substitutes for the typed error.
8. If `flush` cannot be independently forced or observed, document and test the actual reachable write boundary precisely; do not claim separate flush-failure coverage.

Required corrected fixtures:

- pre-response full peer drop resulting in typed production write error;
- peer read-side shutdown/drop while the server write is paused;
- replay-response peer drop with typed I/O error followed by exact retry;
- completion-first control case with successful write observation;
- repeated error/recovery convergence.

## 10. Work package G — Deterministic join evidence

Primary files:

- `src/server/ws.rs`
- `tests/projection_transport_real.rs`

Replace elapsed-time sibling-join proof with direct completion evidence.

Required test design:

1. Each task owns a drop/completion guard.
2. First task completes or panics deterministically.
3. Production teardown cancels and aborts siblings.
4. Teardown awaits every remaining `JoinHandle`.
5. After return, every guard has fired exactly once.
6. Every handle slot is `None`.
7. Probe counts are exact.
8. Panic classification remains correct.
9. No assumption is made that an aborted sleeping task waits for the original sleep duration.

Retain all clean and panic task-kind cases, extended for the TUI request-handler task if the task owner adds that kind.

## 11. Work package H — Semantic guards

Primary file:

- `scripts/check_projection_transport_lifecycle.py`

Add guards that reject the specific post-M011 false-positive shapes.

Required checks:

- `/tui` has a distinct bounded socket-reader-to-handler queue or equivalent close-responsive structure.
- The socket-reader task owns `ws_rx` and fires `connection_cancel` on Close/EOF/error.
- The request handler is separately retained and joined.
- No unbounded request channel is used.
- No per-message detached spawn is used for TUI handling.
- Observer-enabled and observer-disabled staged sends share one canonical implementation.
- Only one bounded-delivery budget wraps a staged send.
- `run_observed_staged_send` or equivalent duplicate implementation is absent.
- `queue_capacity` and remaining capacity are not conflated.
- Rollback helpers assert `forwarder_count` or equivalent installed/joined equality.
- TUI rollback uses a real subscription ID and daemon unsubscribe/receiver assertions.
- Synthetic M011 full-queue subscription IDs are absent.
- `ConnectionProbeRegistry` registration cannot silently fail through `try_lock()`.
- Unix F1/F2/F4 assert a typed server-side I/O observation.
- M012 closure cannot be marked closed while the pending-snapshot fixture is flaky or residual medium findings are listed.
- M011 remains conditionally closed and points to M012.

Static guards supplement executable tests. They cannot replace race, peer-close, I/O, replay, or cleanup evidence.

## 12. Verification matrix

Run all commands with the repository's resource-constrained policy. Preserve `--test-threads=1` where required.

### Formatting and compilation

- `cargo fmt -- --check`
- `CARGO_BUILD_JOBS=1 cargo check --workspace --all-features`
- `CARGO_BUILD_JOBS=1 cargo clippy -p codegg-protocol --all-targets -- -D warnings`
- `CARGO_BUILD_JOBS=1 cargo clippy -p codegg --lib --all-features -- -D warnings`

### Focused unit and integration tests

- `CARGO_BUILD_JOBS=1 cargo test -p codegg --lib server::ws --all-features -- --nocapture`
- `CARGO_BUILD_JOBS=1 cargo test -p codegg --lib core::transport::daemon_socket -- --test-threads=1 --nocapture`
- `CARGO_BUILD_JOBS=1 cargo test --test projection_transport_real --features server -- --test-threads=1 --nocapture`
- `cargo test --test projection_replay_daemon_protocol -- --nocapture`
- `cargo test --test projection_replay_subscription -- --nocapture`
- `cargo test --test projection_replay_resume -- --nocapture`
- `cargo test --test projection_replay_restart_recovery -- --nocapture`
- `cargo test --test projection_replay_transport_isolation -- --nocapture`
- `cargo test --test projection_disclosure_invariants -- --nocapture`
- `cargo test --test projection_artifact_handles -- --nocapture`
- `cargo test --test tui -- --nocapture`
- `cargo test --test tui_render -- --nocapture`
- `cargo test --test tui_project_routing -- --nocapture`
- `cargo test --test tui_project_tabs -- --nocapture`
- `cargo test --test single_daemon_lifecycle -- --test-threads=1`

### Static and boundary guards

- `python3 scripts/check_projection_transport_isolation.py`
- `python3 scripts/check_projection_transport_lifecycle.py`
- `python3 scripts/check_websocket_bounds.py`
- `bash scripts/check-core-boundary.sh`
- `python3 scripts/check_daemon_cwd_usage.py`
- `python3 scripts/check_execution_ownership.py`
- `python3 scripts/check_git_forbidden_patterns.py`
- `python3 scripts/check_scheduler_bypass.py`
- `bash scripts/check_projection_disclosure.sh`
- `git diff --check`

### Required stability evidence

- pending TUI snapshot graceful-close fixture: 50/50 passes;
- pending TUI snapshot abrupt-drop fixture: 50/50 passes;
- pending TUI replay close/retry fixture: 50/50 passes;
- full `projection_transport_real` binary: 25 consecutive clean runs;
- Unix typed-I/O focused fixtures: 25 consecutive clean runs;
- Unix repeated convergence: at least 50 cycles in one run and 10 clean repeated runs;
- no timeout, hang, leaked subscription, retained probe, or forwarder mismatch.

If CI is unavailable or no checks are attached, record this explicitly. Do not describe local execution as CI evidence.

## 13. Explicit closure criteria

M012 may be marked `closed` only when every criterion below is satisfied by committed executable evidence.

| ID | Closure criterion | Required evidence | Invalid substitute |
|---|---|---|---|
| C1 | TUI peer close is observable while snapshot handler is pending | Close-frame and abrupt-drop fixtures cancel before barrier release and complete without hang | Reduced flake rate, TCP reset timing, outer test timeout |
| C2 | TUI peer close is observable while replay handler is pending | Interrupted replay fixture closes pending connection, rolls back, retries exact range/live tail | Disconnect after replay response, normal reconnect only |
| C3 | TUI reader and handler ownership is bounded | Separate retained reader/handler tasks and bounded request queue | Detached per-message tasks, unbounded queue |
| C4 | All connection tasks are joined | Exact completion/drop guards and consumed handles for writer, reader, handler, raw task | Elapsed time, abort without await |
| C5 | All projection forwarders are joined | Installed count equals joined count for every failure fixture | Subscription count alone |
| C6 | Critical-send observation is production-faithful | Observer and non-observer use the same one-budget implementation and return identical typed results | Separate observed implementation, two timeout budgets |
| C7 | Queue metadata is correct | Maximum capacity and remaining capacity recorded separately; enqueue/receipt stages exact | `tx.capacity()` used for both fields |
| C8 | Core and TUI saturation remain causal | Full observed before target request; exact target enqueue-stage `Timeout` | `any_timeout()`, elapsed-only, request-before-fill |
| C9 | TUI rollback uses real daemon ownership | Actual staged subscription ID, receiver non-reuse, unsubscribe idempotence, daemon baseline | Synthetic ID, claim that TUI has no daemon subscription |
| C10 | Unix write-error claims are typed | Production observer records actual `io::ErrorKind` from the write path | EOF, listener shutdown, cleanup alone |
| C11 | Unix replay interruption remains durable | Typed peer-write failure on second connection; third connection exact retry/live continuity | Normal second-connection replay completion |
| C12 | Probe registration is infallible | Identity-keyed finalized record for every connection under concurrent upgrades | `try_lock()` and silent registration loss |
| C13 | Unrelated-client non-interference holds | Client B receives a unique post-failure marker in WebSocket and Unix applicable fixtures | Connection merely remains open |
| C14 | Resource convergence is exact | Tasks, handler, forwarders, ownership, receiver, queues, probes, subscriptions return to baseline | Aggregate `>=` counts or subscription count only |
| C15 | Stability is clean | Required 50/50, 25-run, and repeated-cycle evidence has zero failures | “Usually passes”, reduced flake percentage |
| C16 | Guards reject known false positives | Updated lifecycle guard passes and fails against targeted mutation fixtures or equivalent guard tests | Function-name presence alone |
| C17 | Planning evidence is exact | Plan, M011 conditional record, M012 closure, roadmap, registry, commits, commands, counts, repetitions, exceptions, and CI status agree | Placeholder commits or contradictory statuses |
| C18 | No unresolved high or medium finding remains | M012 closure residual-findings section says none and source audit confirms it | Deferring a production deadlock as out of scope |

Any failed criterion keeps M012 and the Session Projections subsystem conditionally closed.

## 14. Closure record requirements

Create:

- `plans/closure/session-projections/012-status.md`

The closure record must include:

1. repository baseline;
2. exact implementation commits;
3. exact follow-up/corrective commits;
4. exact final reviewed head;
5. accepted M011 foundations;
6. per-work-package implementation summary;
7. named tests and exact counts;
8. repeated-run results;
9. exact commands and outputs;
10. CI status or explicit absence of attached CI;
11. residual findings by severity;
12. explicit C1–C18 closure table with pass/fail evidence;
13. roadmap and registry disposition.

Do not mark M012 closed in the same commit that introduces unresolved placeholders for implementation or final reviewed commit IDs. Use a later reconciliation commit if necessary.

## 15. Documentation and registry reconciliation

On accepted closure:

- mark this M012 plan closed or implemented;
- keep M011 as conditionally closed and strictly superseded by M012 for final closure;
- update `plans/subsystems/session-projections-roadmap.md` to strict closed only after C1–C18 pass;
- remove M012 from dependency-ready work;
- add M012 to recently closed work with exact commits;
- preserve historical conditional records for M6–M11;
- ensure no roadmap section still says M011 or M012 is ready after closure;
- ensure no closure record reports zero residual findings while documenting a known production flake.

## 16. Handoff sequence

1. Reproduce the pending TUI snapshot deadlock at the M012 baseline and record the failure ordering.
2. Implement Work Package A reader/handler separation.
3. Extend task ownership and per-connection probes under Work Package B.
4. Unify critical-send implementation and observations under Work Package C.
5. Add D1–D4 deterministic TUI disconnect fixtures.
6. Correct complete rollback and real subscription identity under Work Package E.
7. Add typed Unix I/O observations and corrected fixtures under Work Package F.
8. Replace elapsed-time join proof under Work Package G.
9. Add semantic guards under Work Package H.
10. Run focused tests and stability loops.
11. Run the full verification matrix.
12. Write `012-status.md` with C1–C18 evidence.
13. Reconcile exact commits, roadmap, and registry.
14. Perform a final source audit at the reviewed head before asserting strict closure.

## 17. Completion definition

M012 is complete only when a real TUI peer close deterministically interrupts pending lifecycle work, every connection-owned task and projection forwarder is cancelled and joined, the same canonical critical-send implementation supplies both runtime behavior and observations, Unix failure tests assert actual typed production I/O results, rollback uses real subscription identities and proves full per-connection convergence, all required stability runs are clean, and every closure document agrees with the executable repository.
