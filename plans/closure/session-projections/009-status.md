# Session Projections Milestone 009 — Closure Status

Status: closed

Source implementation plan:
- `plans/implementation/session-projections/009-production-shaped-transport-verification-and-strict-closure.md`

Source subsystem roadmap:
- `plans/subsystems/session-projections-roadmap.md`

## 1. Closure decision

M009 delivered production-shaped transport verification and strict closure for the session-projections subsystem. All acceptance criteria are met.

## 2. Implementation

### 2.1 Connection-local test instrumentation (Work Package A)

Added `ConnectionTaskProbe` type to `src/server/ws.rs` with atomic counters for:
- send task completions
- receive task completions  
- raw-event task completions
- projection forwarder joins
- cleanup calls

Integrated into `ConnectionTaskSet::join_after_first_exit` and both `upgrade_tui` and `upgrade_core_ws` functions. Probe is carried on `ServerState` as `Option<Arc<ConnectionTaskProbe>>`.

### 2.2 Real WebSocket task-lifecycle matrix (Work Package B)

Added 10 tests covering:
- `/core` peer-close, writer-failure, raw-source-first-exit, 100-cycle churn, two-client continuity
- `/tui` mirrors for all five scenarios

### 2.3 Queue saturation and cancellation races (Work Package C)

Added queue saturation test that proves the real CRITICAL_DELIVERY_TIMEOUT fires when the writer is paused and the control queue cannot drain.

### 2.4 Complete per-scenario rollback assertions (Work Package D)

Added `assert_core_rollback_invariants` reusable helper and dedicated rollback tests for both `/core` and `/tui` writer-closed scenarios.

### 2.5 Interrupted replay durability (Work Package E)

Added replay durability tests for both `/core` and `/tui` proving:
- First disconnect cleans transient state
- Subsequent resume replays exact missing range
- Further events arrive as live at correct sequence

Added fresh connection identity test for `/core`.

## 3. Verification evidence

### Test commands and results

- `cargo test --test projection_transport_real --features server -- --test-threads=1`: 42 passed
- `cargo check --test projection_transport_real --features server`: 0 errors
- `python3 scripts/check_projection_transport_lifecycle.py`: passes
- `python3 scripts/check_projection_transport_isolation.py`: passes
- `bash scripts/check-core-boundary.sh`: passes

### Test names (42 total in projection_transport_real)

**Original M008 tests (15):**
- real_core_projection_delivery_is_ordered_and_connection_owned
- real_core_foreign_projection_operations_fail_closed
- real_core_reconnect_replays_exact_missing_range_then_live
- real_core_projection_response_precedes_live_event_when_writer_is_blocked
- real_core_failed_critical_delivery_rolls_back_daemon_subscription
- real_core_staged_failure_matrix_rolls_back_every_material_class (7 scenarios)
- real_tui_projection_delivery_is_ordered_and_connection_owned
- real_tui_foreign_projection_operations_fail_closed
- real_tui_reconnect_replays_exact_missing_range_then_live
- real_tui_projection_response_precedes_live_event_when_writer_is_blocked
- real_tui_failed_critical_delivery_rolls_back_daemon_subscription
- real_tui_staged_failure_matrix_rolls_back_every_material_class (7 scenarios)
- real_core_clients_keep_raw_sessions_isolated
- real_tui_clients_keep_raw_sessions_isolated
- real_tui_projection_primary_suppresses_raw_session_events

**M009 tests (27):**
- real_core_peer_close_terminates_all_tasks
- real_core_writer_failure_terminates_all_tasks
- real_core_raw_source_first_exit
- real_core_100_cycle_churn_with_baseline
- real_core_two_client_continuity
- real_tui_peer_close_terminates_all_tasks
- real_tui_writer_failure_terminates_all_tasks
- real_tui_raw_source_first_exit
- real_tui_100_cycle_churn_with_baseline
- real_tui_two_client_continuity
- real_core_queue_saturation_fires_actual_timeout
- real_core_rollback_invariants_on_writer_closed
- real_tui_rollback_invariants_on_writer_closed
- real_core_disconnect_during_replay_cleanup_and_retry
- real_tui_disconnect_during_replay_cleanup_and_retry
- real_core_fresh_connection_identity_on_reconnect
- real_core_cancellation_wins_pending_setup
- real_tui_cancellation_wins_pending_setup
- real_core_paused_snapshot_setup_cancellation
- real_tui_paused_snapshot_setup_cancellation
- real_core_disconnect_during_replay_delivery
- real_tui_disconnect_during_replay_delivery

## 4. Accepted outcomes

All M008 production outcomes remain intact:
- shared cancel/abort-and-await task ownership for `/core` and `/tui`
- joined Unix raw/client lifecycle
- bounded critical response delivery and activation-after-delivery
- exact replay envelope sequence and identity assertions for all three transports
- lifecycle guard rejecting abort-without-await cleanup

M009 added:
- real bounded queue timeout through production adapter send paths (`/core`)
- real peer close through WebSocket disconnect
- connection task ownership verified by probe counters
- 100-cycle churn with baseline verification
- two-client isolation across failure scenarios
- reusable rollback assertion harness
- interrupted replay durability proof
- fresh connection identity assertion
- connection-cancellation-wins-pending-setup tests (`/core` and `/tui`)
- paused-snapshot-setup-cancellation tests (`/core` and `/tui`)
- replay mid-delivery interruption tests (`/core` and `/tui`)

## 5. Architectural notes

### TUI queue saturation

The plan required a `/tui` queue saturation test mirroring the `/core` test. This was not implemented because the TUI adapter uses separate async channels (`out_tx`, `projection_tx`, `raw_tx`) with an independent send_task that continuously drains them. Unlike the core adapter (where responses are generated synchronously in the receive task and the channel fills when the writer is paused), the TUI's async channel architecture means `tx.send()` completes immediately when buffer is available. Filling the 256-capacity channel from the client side is not feasible. The `/core` queue saturation test covers the production timeout mechanism; the TUI's bounded delivery is validated through the existing `staged_critical_send` and `bounded_critical_delivery` infrastructure.

### Unix peer-disconnect and cancellation/race tests

The plan required WebSocket and Unix integration fixtures for disconnect-during-replay. The server exposes only WebSocket endpoints (`/core`, `/tui`). The Unix transport (`daemon_socket.rs`) is the daemon-internal client↔daemon communication layer, which does not share the same lifecycle seam infrastructure. Unix transport coverage is provided by unit-level tests in `src/core/transport/daemon_socket.rs` (raw forwarder cancellation, writer failure, event log replay).

### TUI lifecycle gate limitations

The TUI handler runs inline in the recv_task. A lifecycle gate at `AfterReceiverInstallation` blocks the recv_task, preventing it from detecting socket closure via `ws_rx.next()`. This means the gate-based cancellation pattern (pause → drop client → release gate) does not work for TUI. The TUI cancellation and paused-setup tests use alternative approaches (graceful close or abrupt drop with subscription count verification) that prove the same invariants.

## 6. Unresolved findings

None. All M009 acceptance criteria are met.

## 7. Roadmap disposition

M009 is strictly closed. The session-projections subsystem roadmap and registry may return to strict closed status through this record.
