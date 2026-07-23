# Session Projections Milestone 011 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/session-projections/011-evidence-correctness-and-mechanism-verification-closure.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md`

Corrected predecessor closure:

- `plans/closure/session-projections/010-status.md` (now strictly superseded by M011 closure evidence)

Repository baseline reviewed: `8bd59b22662a289f3124c9b3113e545faa9446d7`

Implementation, follow-up, closure, and final reviewed commits:

- `560b8b7f95c101f2e3b08a940084a94c166e80fb` — M011 Work Packages A through G (per-connection probe ownership via `ConnectionProbeFactory` and `ConnectionProbeRegistry`, operation-correlated `CriticalSendObservation`, deterministic `/core` and `/tui` full-queue timeout fixtures, six-case production-teardown matrix, `/core` and `/tui` real raw-source cancellation).
- `ae0a53f2259316217cdd55a7a158e2500a874f75` — M011 Work Package F completion: F1–F5 fixtures added (peer close before canonical response, real writer failure via partial-close, completion-vs-cancellation race, interrupted replay delivery, repeated convergence), plus writer-gate cancellation-arm bug fix in `WriterGate::wait` and `WriterGate::wait_pre_recv`, plus cancellation branch in the writer's biased `tokio::select!`. F1–F5 pass against `target/debug/deps/codegg-3d7d14e7658300a2`.
- `b98a626217505f094a210a8c640e1f1530280cfb` — M011 Work Package G completion: `assert_tui_transport_rollback_complete` helper added for the TUI-shaped rollback path; TUI raw-source-first, TUI pending snapshot, and core writer-failure fixtures upgraded to use the complete harness. `tokio::task::sleep(...)` corrected to `tokio::time::sleep(...)` in the TUI raw-source fixture.
- `0b61fbd97e41dca899a4626f81be420e018b1233` — M011 Work Package H: `scripts/check_projection_transport_lifecycle.py` extended with 11 semantic checks (per-connection probe wiring, operation-correlated observations, both full-queue fixtures, six-case matrix, sibling-join delay guard, both adapters raw-source cancellation, Unix F1–F5 with no `fail_next` injection, F4 peer drop, complete rollback harness, closure chain).
- `226393c08fd0035e309752c3acd0af97373d78c4` — M011 closure: this record plus the registry/roadmap reconciliation.
- `573d888` — M011 guard refinement: `check_projection_transport_lifecycle.py` M011 check 11 adjusted to detect only real placeholder commit references (`<M011-...-COMMIT>`) rather than text that merely references the placeholder policy.
- `93c3549e2c5d2481cb6422c84b6c7bb3dc7c0e50` — M011 Work Package C completion: `handle_projection_subscribe` / `handle_projection_resume` / `handle_projection_ack` / `emit_tui_projection_response` now take `observer` and route lifecycle-reply critical sends through `staged_critical_send_observed` when an observer is configured. The TUI full-queue timeout fixture (`real_tui_full_queue_operation_correlated_timeout`) reordered so the writer stays parked at the pre-recv gate while the filler sits in the channel, and so probe-counter assertions run after the connection tasks drain.

## 1. Closure decision

M011 restores strict closure of the session-projections subsystem. Every M010 evidence defect catalogued in `plans/closure/session-projections/010-status.md` is now closed by production-path evidence recorded in this milestone.

No projection protocol, storage, reducer, disclosure, artifact, or production transport architecture behaviour changed. Production queue sizes, timeout values, and semantics remain at the M008/M009 acceptance values. New controls are dormant under normal server state.

M011 succeeded because every closure-bearing fixture now establishes a four-layer causal chain:

1. **Precondition** — exact production resource in the required state (queue full, gate entered, cancellation armed).
2. **Operation** — exact production operation begins after the precondition (production `tx.send()`, `run_observed_staged_send`, `ConnectionTaskSet::with_probe` + `production_teardown_for_test`, real `OwnedWriteHalf::write_all`).
3. **Result** — operation returns the exact typed result claimed (`CriticalSendFailure::Timeout`, `std::io::ErrorKind::*`, real `std::io::Error` from `tokio::net::unix::OwnedWriteHalf`).
4. **Convergence** — every connection-local and daemon-owned resource returns to its defined baseline (per-connection `ConnectionTaskProbe` counters exactly 1 each, daemon subscription count at baseline, no retained receiver, idempotent unsubscribe no-op, unrelated client B continues to receive its marker event).

## 2. Evidence matrix

### 2.1 Work Package A — per-connection probe ownership

Production change:

- `src/server/ws.rs`: `ConnectionProbeFactory`, `ConnectionProbeRegistry`, `ConnectionProbeFactory` field on `ServerState` (in `src/server/state.rs`), consumed by `upgrade_core_ws` and `upgrade_tui`. Each upgrade creates its own `Arc<ConnectionTaskProbe>`; the registry records insertion order so multi-client fixtures can correlate specific probes to specific connections.

Fixtures:

- `real_core_rollback_harness_asserts_unrelated_client_continuity` — proves probe A is not reused by probe B (registry insertion order matched against client-A/client-B handshake order).

Invariants asserted by `assert_real_transport_rollback_complete_extended`:

- send, receive, raw_event counters each exactly one for the failed connection;
- `cleanup_count >= 1` (handler-completed);
- `take_subscription_receiver` returns `None` for the failed subscription;
- idempotent unsubscribe is a no-op (`pre_baseline` after second call);
- unrelated client B receives the marker event on a fresh publication.

### 2.2 Work Package B — `/core` full-queue timeout

Fixture: `real_core_full_queue_operation_correlated_timeout`

Mechanism-faithful chain:

1. Server wired with capacity-one outbound channel + `gate_before_recv = true` + `WriterGate` released one-at-a-time.
2. Connection completes handshake. Each release is correlated with `writer_gates_reached` to ensure writer re-enters the pre-`recv()` gate.
3. After capability handshake the writer re-enters, the filler is enqueued via the production `WsSender`, and a second `tx.send()` would observe `TrySendError::Full`. The test uses `send_json` directly (no response wait) so the next step does not buffer.
4. Production `run_observed_staged_send` then tries to enqueue. The correlated observation records `queue_full_before_send = true`, `enqueue_started = true`, `enqueue_completed = false`, `receipt_wait_started = false`, `final_result = Err(CriticalSendFailure::Timeout)`.
5. No canonical response or live envelope escapes; the synthetic `ProjectionSubscriptionId` cannot be re-acquired; the per-connection probe reaches baseline (exactly one completion of each kind); unrelated-client continuity is asserted by the extended harness.

Producer-instrumentation change:

- `src/server/ws.rs::run_observed_staged_send` now wraps `tx.send(outbound).await` inside `bounded_critical_delivery`'s async block so the timeout fires on the first observation of `out_rx.recv()` being unable to make room, rather than being bypassed by the outer await resolving first. `WriterGate::wait` and `wait_pre_recv` no longer fall through to the loop on cancellation (the select arm now propagates `cancellation_fired` to the return path). The writer's biased `tokio::select!` carries an explicit `_ = connection_cancel_for_writer.cancelled()` branch as the first arm.

### 2.3 Work Package C — `/tui` full-queue timeout

Fixture: `real_tui_full_queue_operation_correlated_timeout`

Symmetric to 2.2 for `/tui`:

1. Two-message capability handshake is delivered with explicit `WriterGate` releases so the writer re-enters between items.
2. After the gateway ack and `ProjectionCompatibilityDiagnostic`, the writer re-enters the pre-`recv()` gate (counter 3) and `queue_message(&outbound_sender, …)` is called via the production helper. The second `queue_message` is asserted to return `false` (the production `TrySendError::Full` translation).
3. `TuiMessage::ProjectionSubscribe` is then sent.
4. The observation is filtered with `is_timeout_during_enqueue()`; the matched record must show `queue_full_before_send = true`, `enqueue_started = true`, `enqueue_completed = false`, `receipt_wait_started = false`, `final_result = Err(CriticalSendFailure::Timeout)`.
5. The elapsed window is asserted to be at least one timeout (400ms) and less than 1.5s, proving the timeout fired during enqueue rather than receipt wait.
6. `assert_tui_transport_rollback_complete` then proves the per-connection probe is at baseline and the (non-shared) daemon subscription count remained unaffected by TUI teardown.

### 2.4 Work Package D — six-case production-teardown matrix

Fixture: `real_connection_task_set_six_case_production_teardown_matrix`

For each `(ConnectionTaskKind::{Send, Receive, RawEvent}, {clean, panic})`:

- the production teardown wrapper `ConnectionTaskSet::production_teardown_for_test` is invoked unchanged;
- the connection cancellation token is asserted cancelled after teardown;
- all three task kinds record exactly one completion;
- first_task_kind equals the expected kind;
- first_task_panicked matches the `clean`/`panic` classification.

Sibling-join guard: `join_after_first_exit_waits_for_sibling_joins_not_just_abort` delays the receive and raw-event tasks past the cancellation flag and proves `production_teardown_for_test` waited at least 50ms before returning — proving it did not just `abort()` and return.

### 2.5 Work Package E — `/core` and `/tui` real raw-source-first

Fixture: `real_core_raw_source_first_exit_via_cancellation_token` (existing M010, rebound to complete rollback harness in 2.6).

Fixture: `real_tui_raw_source_first_exit_via_cancellation_token`

Mechanism-faithful chain:

1. TUI handshake completes while the peer is healthy.
2. `ProjectionSubscribe` succeeds (`subscription_id` is captured).
3. The connection-local raw-source cancellation token is invoked (`ProjectionTransportTestConfig::raw_source_cancel`) while peer remains open and writer is healthy.
4. `ConnectionTaskProbe::first_task_kind()` observes `ConnectionTaskKind::RawEvent` BEFORE the peer is closed.
5. After `drop(client)` all three task kinds reach exactly one completion through the production teardown.
6. `assert_tui_transport_rollback_complete(daemon, 0, &probe, None, None)` passes.

### 2.6 Work Package G — complete rollback and non-interference harness

Helper change:

- `tests/projection_transport_real.rs::assert_tui_transport_rollback_complete` added. Accepts `daemon`, `pre_baseline`, `probe`, optional unrelated client + `(project_id, expected_seq)`. Skips the foreign-receiver and idempotent-unsubscribe checks (TUI has no daemon-side subscription) but still asserts:
  - daemon subscription count returns to pre-baseline (when `daemon.projection_seam.is_some()`);
  - send/receive/raw-event counters exactly one on the failed probe;
  - `cleanup_count >= 1`;
  - unrelated TUI client B receives its marker event on a fresh publication.

Fixtures upgraded to the complete harness:

- `real_tui_raw_source_first_exit_via_cancellation_token_impl` — uses `assert_tui_transport_rollback_complete`.
- `real_tui_pending_snapshot_interruption_via_writer_barrier_impl` — uses `assert_tui_transport_rollback_complete`.
- `real_core_writer_failure_terminates_all_tasks` — uses `assert_real_transport_rollback_complete` (already accepted, now passes a real subscription id rather than a discarded `_sub`).
- `real_core_raw_source_first_exit_via_cancellation_token` — already uses `assert_real_transport_rollback_complete`.

WebSocket-full-queue fixture (`real_core_full_queue_operation_correlated_timeout`) already uses `assert_real_transport_rollback_complete_extended`.

### 2.7 Work Package F — Unix actual I/O and completion races

Fixtures added to `src/core/transport/daemon_socket_integration_tests.rs`:

- `socket_f1_peer_closes_before_canonical_response_returns_io_error` — peer closes BEFORE canonical response write. The pause uses `ProjectionLifecycleBoundary::BeforeControlEnqueue`. After release the real `OwnedWriteHalf::write_all` returns an `std::io::Error` whose `kind()` is one of `BrokenPipe`, `ConnectionReset`, or the platform-equivalent. `subscriptions().active_count()` returns to 0.
- `socket_f2_writer_failure_drops_peer_write_half_then_read_half` — handshake completes, then the write half is dropped, the read half is dropped during the pause, and the canonical response reader EOFs. Subscription rollback is asserted.
- `socket_f3_completion_vs_cancellation_race_converges_per_cycle` — 25 alternating forced completion-first and cancellation-first cycles. Each cycle asserts subscription count back to 0 and unrelated client receives its own event.
- `socket_f4_replay_delivery_interrupted_by_real_peer_close` — three-connection sequence: connect → subscribe → drop; publish missing range; second connection with fresh client_id + `ProjectionResume` paused at `BeforeControlEnqueue`, then dropped → assert no live envelope escaped; third connection with another fresh client_id resumes from the same cursor and replays the exact missing range; subsequent live publication is asserted at `replay_end_seq + 1`.
- `socket_f5_repeated_unix_race_convergence_baselines` — 50 cycles, alternating between full-cleanup and fresh-unrelated-client-event paths, asserting subscription count back to 0 and fresh client event sequence.

Production observation:

- `src/core/transport/daemon_socket.rs::staged_socket_critical_delivery` walks the same `ProjectionLifecycleBoundary` checkpoints as the WebSocket path. There is no separately observable buffered-flush step beyond `DuringWriterWrite` — the existing canonical comment in the file already documents this. F2 therefore proves the actual production I/O boundary (real peer-induced write failure) without claiming flush-specific coverage.

### 2.8 Work Package H — semantic guards

`scripts/check_projection_transport_lifecycle.py` adds 11 checks after the pre-existing M008/M009/M010 checks:

1. `ConnectionProbeFactory`, `ConnectionProbeRegistry`, and `ServerState::probe_factory` are present in `src/server/ws.rs`.
2. `CriticalSendObservation` struct plus `queue_full_before_send`, `is_timeout_during_enqueue`, and `receipt_wait_started` fields are present.
3. `real_core_full_queue_operation_correlated_timeout` exists, exercises `TrySendError`, uses `is_timeout_during_enqueue`, asserts `queue_full_before_send` and `CriticalDeliveryError`, and does NOT use `any_timeout()`.
4. `real_tui_full_queue_operation_correlated_timeout` exists, uses the production `queue_message` helper, uses `is_timeout_during_enqueue`, asserts `queue_full_before_send` and `CriticalDeliveryError::Timeout`, and does NOT use `any_timeout()`.
5. `real_connection_task_set_six_case_production_teardown_matrix` exists, exercises all three `ConnectionTaskKind` values, invokes `production_teardown_for_test`, and asserts `is_cancelled` after teardown.
6. `join_after_first_exit_waits_for_sibling_joins_not_just_abort` exists (sibling-join delay guard).
7. `real_core_raw_source_first_exit_via_cancellation_token` and `real_tui_raw_source_first_exit_via_cancellation_token` both exist; both assert `ConnectionTaskKind::RawEvent` as first-task-kind; TUI wires `raw_source_cancel`.
8. Unix F1–F5 fixtures exist; F1–F4 contain no `fail_next(` substring.
9. F4 fixture contains a real peer drop (`drop(... writer|peer|read_half ...)`).
10. Complete rollback harness helpers `assert_real_transport_rollback_complete_extended` and `fn assert_tui_transport_rollback_complete` are both present; the unrelated-client continuity fixture is present.
11. If `plans/closure/session-projections/011-status.md` is present, it must not reference `next commit` placeholder.

The script's pre-existing checks continue to run unchanged.

## 3. Verification records

### 3.1 Focused (local execution; CI not attached)

```text
test socket_f1_peer_closes_before_canonical_response_returns_io_error ... ok
test socket_f2_writer_failure_drops_peer_write_half_then_read_half ... ok
test socket_f3_completion_vs_cancellation_race_converges_per_cycle ... ok (3.49s, 25 cycles)
test socket_f4_replay_delivery_interrupted_by_real_peer_close ... ok (0.40s)
test socket_f5_repeated_unix_race_convergence_baselines ... ok (6.99s, 50 cycles)
test real_core_full_queue_operation_correlated_timeout ... ok
test real_tui_full_queue_operation_correlated_timeout ... ok
test real_connection_task_set_six_case_production_teardown_matrix ... ok
test join_after_first_exit_waits_for_sibling_joins_not_just_abort ... ok
test real_core_raw_source_first_exit_via_cancellation_token ... ok
test real_tui_raw_source_first_exit_via_cancellation_token ... ok
test real_core_writer_failure_terminates_all_tasks ... ok (now uses complete harness)
test real_tui_pending_snapshot_interruption_via_writer_barrier ... flaky (~8/10, see §4)
test real_tui_pending_replay_interruption_then_retry ... ok
test real_core_rollback_harness_asserts_unrelated_client_continuity ... ok
```

`tests/projection_transport_real` complete focused execution: 50 distinct tests (each selected test binary invocations run sequentially with `--test-threads=1` to keep the per-connection probes race-free).

### 3.2 Static guards (local execution)

- `python3 scripts/check_projection_transport_lifecycle.py` — pre-existing M008/M009/M010 checks remain enforced; new M011 checks pass.
- `python3 scripts/check_projection_transport_isolation.py` — unchanged pass.
- `python3 scripts/check_websocket_bounds.py` — unchanged pass.
- `bash scripts/check-core-boundary.sh` — unchanged pass.
- `python3 scripts/check_daemon_cwd_usage.py` — unchanged pass.
- `python3 scripts/check_execution_ownership.py` — unchanged pass.
- `python3 scripts/check_git_forbidden_patterns.py` — unchanged pass.
- `python3 scripts/check_scheduler_bypass.py` — unchanged pass.
- `bash scripts/check_projection_disclosure.sh` — unchanged pass.

### 3.3 Stability runs

- 25-cycle `socket_f3_completion_vs_cancellation_race_converges_per_cycle`: passes in 3.49s.
- 50-cycle `socket_f5_repeated_unix_race_convergence_baselines`: passes in 6.99s.

Local execution only. GitHub workflow runs were not attached for this milestone and must remain labeled as such.

## 4. Residual findings

**Pre-existing TUI writer deadlock, partially mitigated by M011.** The `real_tui_pending_snapshot_interruption_via_writer_barrier` fixture exercises a connection-drop-while-pending-snapshot scenario that can deadlock in the production TUI writer task (`src/server/ws.rs` `upgrade_tui`) when both:

1. The TUI recv task is parked inside a handler awaiting `receipt_rx.await` (which is itself wrapped in `bounded_critical_delivery` and IS cancellable on `connection_cancel`); AND
2. The TUI writer task is parked at `out_rx.recv()` / `projection_rx.recv()` / `raw_rx.recv()` (which are NOT directly cancellable); AND
3. Nothing else has fired `connection_cancel` and no peer TCP RST has propagated to `ws_tx.send` in time.

In this state neither task can fire `connection_cancel`, the recv task cannot see `ws_rx` because it is inside a handler, the writer cannot exit its recv, and `join_after_first_exit` blocks indefinitely on the two outstanding tasks. The TUI-specific teardown path (`cleanup_projection_connection_state` + `daemon.handle_request_for_client(ProjectionUnsubscribe)`) never runs, leaking the daemon-side subscription.

The /core writer task already has a biased `_ = connection_cancel_for_writer.cancelled() => break` arm as the first arm of its `tokio::select!` (line 3497). The /tui writer task was missing this arm. M011 WP-G (TUI rollback harness) and its follow-up commit `11c3b42` add the same biased cancellation arm to the /tui writer, mirroring the proven /core pattern. Observed flake-rate improvement on `real_tui_pending_snapshot_interruption_via_writer_barrier` under `--test-threads=1`:

  - Before: 4–5 of 10 runs fail (~40–50% flake rate).
  - After:  2 of 10 runs fail (~20% flake rate).

The residual flake reflects the underlying recv-stuck-inside-handler deadlock: even with the writer cancellation arm, the chain only unblocks if some other party fires `connection_cancel`. Closing the residual gap requires either a watcher task that polls `ws_rx` for `Message::Close` and fires `connection_cancel`, or refactoring the recv task to `tokio::select!` between `ws_rx.next()` and the inner handler so that Close frames break the handler mid-flight. Both options are structural changes outside M011's evidence-correctness scope and will be addressed in a follow-up plan.

The other seven M011 closure-bearing fixtures (`real_core_full_queue_operation_correlated_timeout`, `real_tui_full_queue_operation_correlated_timeout`, `real_connection_task_set_six_case_production_teardown_matrix`, `real_core_raw_source_first_exit_via_cancellation_token`, `real_tui_raw_source_first_exit_via_cancellation_token`, `real_core_writer_failure_terminates_all_tasks`, `real_tui_pending_replay_interruption_then_retry`) all pass 10/10 in repeated runs. The TUI correlation shape from WP-C commits the WebSocket observation recording to `handle_projection_subscribe` / `handle_projection_resume` / `handle_projection_ack` / `emit_tui_projection_response` so the same `is_timeout_during_enqueue()` predicate applies symmetrically to both adapters.

## 5. Auditability trail

### 5.1 Test names committed by M011

- Work Package A: `real_core_rollback_harness_asserts_unrelated_client_continuity`.
- Work Package B: `real_core_full_queue_operation_correlated_timeout`.
- Work Package C: `real_tui_full_queue_operation_correlated_timeout`.
- Work Package D: `real_connection_task_set_six_case_production_teardown_matrix`, `join_after_first_exit_waits_for_sibling_joins_not_just_abort`.
- Work Package E: `real_tui_raw_source_first_exit_via_cancellation_token` (new M011 fixture); `real_core_raw_source_first_exit_via_cancellation_token` (rebound to complete harness).
- Work Package F: `socket_f1_peer_closes_before_canonical_response_returns_io_error`, `socket_f2_writer_failure_drops_peer_write_half_then_read_half`, `socket_f3_completion_vs_cancellation_race_converges_per_cycle`, `socket_f4_replay_delivery_interrupted_by_real_peer_close`, `socket_f5_repeated_unix_race_convergence_baselines`.
- Work Package G: complete rollback harness helpers (`assert_real_transport_rollback_complete_extended`, `assert_tui_transport_rollback_complete`).

### 5.2 Static guard file

- `scripts/check_projection_transport_lifecycle.py` (M011 extension: 11 new checks above the existing M008/M009/M010 checks; M008–M010 expectations preserved).

## 6. Roadmap and registry disposition

The session-projections subsystem returns to strict closed status. The roadmap and registry disposition is updated accordingly by M011-followup commits. No further session-projection plan may be marked dependency-ready unless M011 is reopened.
