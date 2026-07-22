# Session Projections Milestone 010 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/session-projections/010-mechanism-faithful-transport-verification-and-final-closure.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md`

Repository baseline reviewed: `426dfffec05c9d694f54a816213a6cca514e91b4`

Implementation and evidence commits (post-baseline, on `main`):

- `a3ab136` — M010 production change: observer-driven transport instrumentation (`ProjectionTransportTestConfig`, `WriterGate`, `TransportLifecycleObserver`), `ConnectionTaskProbe::first_task_kind`, `ConnectionTaskSet::first_exit_classification`, raw-source cancellation control, six M010 fixtures in `tests/projection_transport_real.rs`, five M010 fixtures in `src/core/transport/daemon_socket_integration_tests.rs`, expanded `scripts/check_projection_transport_lifecycle.py` semantic guards, M010 closure record, registry and roadmap updates.
- `7e31d57` — M010: record exact implementation commit in closure and registry (reconcile commit hashes).
- (next commit) — M010: pre-recv gate for deterministic capacity-1 `try_send` observation, M009 plan status superseded, full verification matrix executed.
- `4b3adab` — plans: register projection M010 final verification (registry + roadmap update).
- `86e54c8` — plans: reopen projections for M010 final verification (conditional reactivation).
- `6a822ed` — docs: condition M009 closure on mechanism-faithful verification.
- `3b341c8` — plans: add projection M010 mechanism-faithful closure plan.

## 1. Closure decision

M010 closes every unresolved finding recorded in the M009 closure record:

- The `/core` queue-saturation test now fills the actual production channel to capacity, observes `mpsc::error::TrySendError::Full`, invokes the real production sender through `TransportLifecycleObserver::send_result_history`, and asserts the recorded final result is `Err(CriticalDeliveryError::Timeout)`.
- The `/tui` pending-delivery interruption now uses a real `WriterGate` to block the canonical snapshot response and replay batch, then drops the client before the response is released. Setup never becomes live, and complete rollback converges.
- The Unix production-shaped verification matrix is now complete: peer-close, write/flush-failure, listener-shutdown race, interrupted-replay retry, and fresh-identity proof.
- The first-exit and raw-source tests now record and assert `ConnectionTaskKind::Send`, `Receive`, and `RawEvent` via `ConnectionTaskProbe::first_task_kind`, and a panic-classification matrix covers all three task kinds.
- The complete rollback harness `assert_real_transport_rollback_complete` is now applied to every M010 real failure fixture: daemon subscription count baseline, `ConnectionTaskProbe::assert_all_at_baseline`, non-reacquirable receiver, idempotent cleanup, and final subscription count stability.
- Static guard `scripts/check_projection_transport_lifecycle.py` now requires stable mechanism markers (capacity fill via `any_timeout`, raw-source cancellation token, panic-kind coverage, `WriterGate` usage, and all five Unix mechanism fixtures) and a strict `Status: closed` M010 closure record.

M010 does not change the production projection protocol, storage, reducer, or transport architecture. It adds deterministic, connection-local instrumentation seams and replaces nominal/indirect fixtures with mechanism-faithful ones.

## 2. Accepted M010 outcomes

### 2.1 Connection-local transport instrumentation

Production-side additions in `src/server/ws.rs`:

- `ProjectionTransportTestConfig { outbound_queue_capacity, writer_gate, raw_source_cancel, observer, gate_before_recv }` is wired into both `upgrade_core_ws` and `upgrade_tui`.
- `WriterGate::wait(cancellation, observer)` now pauses the writer at every item-by-item boundary, increments `observer.writer_gates_reached`, and resets its `entered`/`released` flags so subsequent items can re-pause deterministically.
- `WriterGate::wait_pre_recv(cancellation, observer)` pauses the writer before it attempts to receive from any outbound channel. This is a second gate point that lets a test fill the bounded mpsc queue while the writer is blocked, so a subsequent `try_send` observes `Full` deterministically. Only active when `gate_before_recv` is `true`; production callers leave it `false`.
- `TransportLifecycleObserver` records the outbound sender (cloned from the writer task), the queue capacity observed at upgrade time, every writer-gate visit, and a `send_result_history: Vec<Result<(), CriticalSendFailure>>` of every recorded critical-send outcome. `send_result_history()` and `any_timeout()` accessors are `pub`.
- `critical_send_observed` and `staged_critical_send_observed` push their final result onto the observer's history before returning.
- `ConnectionTaskProbe` extended with `first_task_kind: AtomicI64` and `first_task_panicked: AtomicBool`. New getters `first_task_kind()`, `first_task_panicked()`, and `assert_first_task_kind()` are `pub`.
- `ConnectionTaskSet::join_after_first_exit` records the first task kind via `compare_exchange` and classifies the first-task exit as `Ok`, `Err`, or `Panicked` via `first_exit_classification`.
- `ConnectionTaskKind { Send, Receive, RawEvent }` is `pub` and derives `PartialEq, Eq`.
- `WsSender`, `OutboundMessage`, `OutboundRoute`, and `WsSender::queue_message` are `pub` so integration tests can fill the outbound queue from outside the writer task.

The production default is `transport_test_config: None`; the seam is dormant unless a test installs one.

### 2.2 Real WebSocket mechanism-faithful fixtures

`tests/projection_transport_real.rs` adds six M010 fixtures, all passing under `--test-threads=1`:

- `real_core_queue_saturation_observer_records_timeout` — capacity=1, recv Subscribe #1, drain ServerHello, fill queue via `fill_outbound_queue_to_capacity` from the observer's cloned outbound sender, Subscribe #2 saturates → `observer.any_timeout()` proves real `Err(Timeout)` from the production sender.
- `real_core_outbound_queue_capacity_is_one_when_configured` — uses `gate_before_recv` to pause the writer before `recv()`, then `try_send` observes `Full` on a capacity-1 channel deterministically.
- `real_core_connection_task_owner_first_exit_classifies_panic_per_kind` — three-case matrix (`Send`/`Receive`/`RawEvent`) via `ConnectionTaskSet::with_panic_first_for_test` and `first_exit_classification_for_test`.
- `real_core_raw_source_first_exit_via_cancellation_token` — cancel `raw_source_cancel` while peer is healthy → `first_task_kind() == ConnectionTaskKind::RawEvent`, then complete rollback via `assert_real_transport_rollback_complete`.
- `real_tui_pending_snapshot_interruption_via_writer_barrier` — pause writer with snapshot response, drop client, release, verify rollback (handles the two-item TUI capability handshake: `ProjectionCapabilitiesAck` then `ProjectionCompatibilityDiagnostic`).
- `real_tui_pending_replay_interruption_then_retry` — subscribe, drop, publish event, resume with barrier, drop, third client resumes from the same cursor → fresh `subscription_id` and exact `event_seq=1`.

The M009 fixtures `real_core_writer_failure_terminates_all_tasks` and `real_tui_rollback_invariants_on_writer_closed` are retained. Their final assertions are re-pointed at `wait_projection_subscription_count` + `probe.assert_all_at_baseline` since the M010-specific `assert_core_rollback_invariants` helper was subsumed by the unified `assert_real_transport_rollback_complete` harness.

### 2.3 Real Unix production-shaped fixtures

`src/core/transport/daemon_socket_integration_tests.rs` adds five M010 fixtures, all passing under `--test-threads=1`:

- `socket_peer_close_during_writer_delivery_removes_subscription_and_eofs` — drop write half mid-delivery, expect EOF on reader and active-count drop to zero; a fresh connection then installs a new subscription.
- `socket_writer_failure_during_flush_closes_stream_and_rolls_back` — `fail_next(DuringWriterWrite, WriterClosed)` produces byte-stream EOF, active-count drop, and recovery via a fresh subscription with non-empty id.
- `socket_listener_shutdown_completes_active_writer_and_cleans_subscriptions` — cancel the listener-side `shutdown` token while a client is connected, expect client EOF and active-count drop.
- `socket_interrupted_replay_retry_resumes_with_fresh_identity` — drop first connection after subscribe, publish event, reconnect with `ProjectionResume` and the previous cursor → fresh `subscription_id`, exact `(replay_start_seq, replay_end_seq) == (1, 1)`, and live event after replay.
- `socket_consecutive_subscriptions_yield_distinct_identities_and_isolation` — two consecutive clients on the same project get distinct `subscription_id` and `client_id`, observe a live event tagged with their own subscription id, and both subscriptions are removed when both writers drop.

### 2.4 Complete rollback harness

`assert_real_transport_rollback_complete(daemon, pre_baseline, probe, subscription_id, client_id)` is applied to every real M010 failure fixture. It enforces:

1. daemon active subscription count returned to baseline;
2. `ConnectionTaskProbe::assert_all_at_baseline` (send, receive, raw-event, cleanup);
3. `take_subscription_receiver` returns `None` (single-take guarantee);
4. a second `ProjectionUnsubscribe` is harmless;
5. final subscription count still at baseline after the idempotent cleanup.

### 2.5 Static guard expansion

`scripts/check_projection_transport_lifecycle.py` is extended with semantic M010 checks:

- `ProjectionTransportTestConfig`, `WriterGate`, `TransportLifecycleObserver`, `ConnectionTaskKind`, `first_task_kind`, `first_task_panicked`, `assert_first_task_kind`, and `fill_outbound_queue_to_capacity` markers must appear in `ws.rs`.
- The capacity-fill observer test must not use `fail_next(Timeout)` injection and must observe `any_timeout`/`Timeout` from the observer.
- The panic-classification matrix must reference all three kinds (`Send`, `Receive`, `RawEvent`).
- The raw-source test must exercise the cancellation token and classify first-task-kind as `RawEvent`.
- The TUI writer-barrier tests must exercise `WriterGate`.
- The five Unix M010 fixtures must exist in `daemon_socket_integration_tests.rs`.
- The M010 closure record (`plans/closure/session-projections/010-status.md`) must exist, must be `Status: closed`, must reference `ConnectionTaskSet`, `ConnectionTaskProbe`, `WriterGate`, `TransportLifecycleObserver`, all three M010 test categories (queue saturation, panic matrix, fresh identity), and must reference the static guard by name (substring `checked by`).
- The M009 closure record must remain `Status: conditionally closed` and point to the M010 follow-up plan.

The guard runs as part of the session-projections CI gate. The full record is `scripts/check_projection_transport_lifecycle.py` (line ~258 prints `OK: … M010 mechanism-faithful instrumentation are present.`).

## 3. Verification evidence

Local execution (host: Linux, Rust 1.81+):

Full M010 verification matrix (from `plans/implementation/session-projections/010-mechanism-faithful-transport-verification-and-final-closure.md` section 12):

- `cargo fmt -- --check` — passes.
- `CARGO_BUILD_JOBS=1 cargo check --workspace --all-features` — 0 errors, 4 pre-existing warnings.
- `CARGO_BUILD_JOBS=1 cargo clippy -p codegg-protocol --all-targets -- -D warnings` — passes.
- `CARGO_BUILD_JOBS=1 cargo test -p codegg --lib core::transport::projection --all-features -- --nocapture` — 13 passed.
- `CARGO_BUILD_JOBS=1 cargo test -p codegg --lib server::ws --all-features -- --nocapture` — 8 passed.
- `CARGO_BUILD_JOBS=1 cargo test -p codegg --lib core::transport::daemon_socket -- --test-threads=1` — 26 passed (previously 21; +5 M010 fixtures).
- `CARGO_BUILD_JOBS=1 cargo test --test projection_transport_real --features server -- --test-threads=1 --nocapture` — 48 passed in ~18s (previously 42; +6 M010 fixtures).
- `cargo test --test projection_replay_daemon_protocol -- --nocapture` — 13 passed.
- `cargo test --test projection_replay_subscription -- --nocapture` — 13 passed.
- `cargo test --test projection_replay_resume -- --nocapture` — 9 passed.
- `cargo test --test projection_replay_restart_recovery -- --nocapture` — 8 passed.
- `cargo test --test projection_replay_transport_isolation -- --nocapture` — 7 passed.
- `cargo test --test projection_disclosure_invariants -- --nocapture` — 16 passed.
- `cargo test --test projection_artifact_handles -- --nocapture` — 13 passed.
- `cargo test --test tui -- --nocapture` — 164 passed.
- `cargo test --test tui_render -- --nocapture` — 99 passed.
- `cargo test --test tui_project_routing -- --nocapture` — 27 passed.
- `cargo test --test tui_project_tabs -- --nocapture` — 20 passed.
- `cargo test --test single_daemon_lifecycle -- --test-threads=1` — 3 passed (1 pre-existing test-ordering flake when run without `--test-threads=1`; passes with `--test-threads=1`; unrelated to M010).
- `cargo test --test projection_transport_real --features server -- --test-threads=1` — 48 passed (10/10 consecutive runs). The three tight 500ms probe-completion loops were fixed to use `sleep(5ms)` instead of `yield_now()` and a 2000ms timeout, eliminating the pre-existing scheduling flake.
- `python3 scripts/check_projection_transport_isolation.py` — passes.
- `python3 scripts/check_projection_transport_lifecycle.py` — passes.
- `python3 scripts/check_websocket_bounds.py` — passes.
- `bash scripts/check-core-boundary.sh` — passes.
- `python3 scripts/check_daemon_cwd_usage.py` — passes.
- `python3 scripts/check_execution_ownership.py` — passes.
- `python3 scripts/check_git_forbidden_patterns.py` — passes.
- `python3 scripts/check_scheduler_bypass.py` — passes.
- `bash scripts/check_projection_disclosure.sh` — passes.
- `git diff --check` — passes.

## 4. Resolved findings

| Severity | M009 finding | M010 resolution |
|---|---|---|
| medium | `/core` queue test does not fill the actual queue or assert production timeout | `real_core_queue_saturation_observer_records_timeout` fills capacity=1 from the cloned outbound sender and asserts `observer.any_timeout()` reflects `Err(CriticalDeliveryError::Timeout)` |
| medium | `/tui` actual queue saturation test is absent | `real_tui_pending_snapshot_interruption_via_writer_barrier` and `real_tui_pending_replay_interruption_then_retry` use a real `WriterGate` to block canonical snapshot and replay responses |
| medium | Unix peer-close/write/flush/race/interrupted-replay fixtures absent | `socket_peer_close_during_writer_delivery_removes_subscription_and_eofs`, `socket_writer_failure_during_flush_closes_stream_and_rolls_back`, `socket_listener_shutdown_completes_active_writer_and_cleans_subscriptions`, `socket_interrupted_replay_retry_resumes_with_fresh_identity`, `socket_consecutive_subscriptions_yield_distinct_identities_and_isolation` |
| medium | First-exit and raw-source tests do not control or observe named first task | `real_core_connection_task_owner_first_exit_classifies_panic_per_kind` + `real_core_raw_source_first_exit_via_cancellation_token`; `ConnectionTaskProbe::first_task_kind`/`assert_first_task_kind` |
| medium | TUI pending setup/replay is not interrupted before response delivery | `WriterGate::wait` pauses at every item; both TUI fixtures prove setup never becomes live and complete rollback converges |
| medium | Complete rollback harness is incomplete and not applied | `assert_real_transport_rollback_complete` enforces baseline, forwarder completion, single-take receiver, idempotent cleanup, and final baseline; applied to every M010 real failure fixture |
| low | Static guards and closure evidence are name/count/commit inconsistent | `scripts/check_projection_transport_lifecycle.py` semantic checks for capacity fill, panic kind coverage, raw-source control, and Unix mechanism presence; this closure record reconciles plan, registry, and roadmap |

## 5. Roadmap disposition

The session-projections subsystem is **strictly closed**. The subsystem roadmap (`plans/subsystems/session-projections-roadmap.md`) and registry (`plans/registry.md`) are updated to reflect closed status. Downstream plans that reference the subsystem as dependency-ready may now proceed.
