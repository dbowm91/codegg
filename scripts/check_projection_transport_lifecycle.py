#!/usr/bin/env python3
"""Guard connection-owned projection transport lifecycle invariants.

Covers M008 (transport ownership, joined teardown, stale-route rejection),
M009 (real adapter lifecycle, queue saturation, peer disconnect, interrupted
replay, churn, two-client continuity, conditional closure record),
M010 (mechanism-faithful transport verification: observer-driven queue
saturation, panic-classification matrix, raw-source first-exit via
cancellation token, writer-barrier snapshot/replay interruption, Unix
peer-close/write/flush races, interrupted replay retry, fresh identity
proof, and final closure record), and
M011 (evidence correctness and mechanism verification closure:
operation-correlated /core and /tui full-queue timeout, six-case
production-teardown matrix, per-connection probe ownership, raw-source
cancellation for both adapters, real Unix peer/write races with
fresh-unrelated-client convergence, repeated Unix race/churn baselines,
complete rollback/non-interference harness).
"""

from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parent.parent


def _read(path: Path) -> str:
    return path.read_text() if path.exists() else ""


def main() -> int:
    failures: list[str] = []
    unix = _read(ROOT / "src/core/transport/daemon_socket.rs")
    ws = _read(ROOT / "src/server/ws.rs")
    projection = _read(ROOT / "src/core/transport/projection.rs")
    real_tests = _read(ROOT / "tests/projection_transport_real.rs")
    closure_008 = _read(ROOT / "plans/closure/session-projections/008-status.md")
    closure_009 = _read(ROOT / "plans/closure/session-projections/009-status.md")
    unix_production = unix.split("#[cfg(test)]", 1)[0]

    # ── M008: Unix transport ownership ──────────────────────────────────
    required_unix = (
        ("JoinSet", "Unix connection tasks must be owned by a JoinSet"),
        ("let raw_forwarder = tokio::spawn", "raw forwarder handle is not retained"),
        ("raw_forwarder.await", "raw forwarder is not joined during teardown"),
        ("forward_events(\n", "raw forwarder call is missing"),
        ("cancellation.cancelled()", "raw forwarder lacks cancellation handling"),
        ("DuringWriterWrite", "Unix writer lifecycle seam is not exercised"),
    )
    for needle, message in required_unix:
        if needle not in unix_production:
            failures.append(f"daemon_socket.rs: {message}")

    if re.search(r"tokio::spawn\(forward_events", unix_production):
        failures.append("daemon_socket.rs: raw forwarder is spawned without an owned handle")

    # ── M008: WebSocket transport ownership ─────────────────────────────
    required_ws = (
        ("struct ConnectionTaskSet", "WebSocket connection tasks lack a shared owner"),
        (
            "join_after_first_exit",
            "WebSocket connection tasks do not use joined teardown",
        ),
        ("cancellation.cancel();", "connection cancellation is not required before joins"),
        ("let result = handle.await;", "retained WebSocket task handles are not awaited"),
        ("OutboundRoute::Raw { generation", "raw outbound items lack a route generation"),
        ("async fn deliver_tui_outbound", "writer-side raw delivery check is missing"),
        ("raw_route_generation != generation", "stale raw generation is not rejected at write time"),
        ("queue_raw_json", "raw event queue does not preserve routing generation"),
        ("ProjectionLifecycleBoundary::BeforeControlEnqueue", "TUI lifecycle seam is not wired"),
    )
    for needle, message in required_ws:
        if needle not in ws:
            failures.append(f"ws.rs: {message}")

    # ── M008: joined teardown in upgrade functions ──────────────────────
    def function_body(name: str) -> str:
        marker = f"async fn {name}"
        start = ws.find(marker)
        if start == -1:
            return ""
        brace = ws.find("{", start)
        depth = 0
        for index in range(brace, len(ws)):
            if ws[index] == "{":
                depth += 1
            elif ws[index] == "}":
                depth -= 1
                if depth == 0:
                    return ws[start : index + 1]
        return ws[start:]

    for adapter in ("upgrade_core_ws", "upgrade_tui"):
        body = function_body(adapter)
        if ".abort()" in body:
            failures.append(f"ws.rs: {adapter} contains abort-only sibling cleanup")
        if "ConnectionTaskSet::new" not in body or "join_after_first_exit" not in body:
            failures.append(f"ws.rs: {adapter} does not use joined connection-task teardown")

    # ── M008: unbounded channel guard + projection privacy ──────────────
    if re.search(r"(?:tokio::sync::)?mpsc::unbounded_channel", ws):
        failures.append("ws.rs: unbounded outbound channel is forbidden")
    if re.search(r"pub\s+fn\s+mark_live", projection):
        failures.append("projection.rs: mark_live must remain private to the activation helper")
    if "fn mark_live(&mut self)" not in projection:
        failures.append("projection.rs: private activation transition is missing")
    if "activate_after_delivery" not in projection:
        failures.append("projection.rs: approved activation helper is missing")

    # ── M008: 008-status.md closure record ──────────────────────────────
    # M008 is now conditionally closed; strict closure deferred to M009.
    required_closure_008 = (
        ("Status: conditionally closed", "M008 closure record is not conditionally closed"),
        ("6975050af530eb5bd7a640c1f7ac9a31859dfda3", "M008 implementation commit is missing"),
        ("ea6e38d5182f42ae70c5f379415dd8ee1eb470e2", "M008 closure commit is missing"),
        ("009-status.md", "M008 closure record does not reference M009 status"),
    )
    for needle, message in required_closure_008:
        if needle not in closure_008:
            failures.append(f"008-status.md: {message}")

    # ── M009 check 1: shared joined task ownership ──────────────────────
    # ConnectionTaskSet exists in ws.rs and is used in both upgrade functions
    # (already checked above via function_body loop).  Additionally verify
    # the struct exists and upgrade_core_ws / upgrade_tui both instantiate it.
    if "struct ConnectionTaskSet" not in ws:
        failures.append("ws.rs: ConnectionTaskSet struct is missing (M009 shared task owner)")
    for adapter in ("upgrade_core_ws", "upgrade_tui"):
        body = function_body(adapter)
        if "ConnectionTaskSet::new" not in body and "ConnectionTaskSet::with_probe" not in body:
            failures.append(f"ws.rs: {adapter} does not instantiate ConnectionTaskSet (M009)")

    # ── M009 check 2: three first-exit task-owner cases + TUI mirrors ───
    first_exit_core = (
        "real_core_peer_close_terminates_all_tasks",
        "real_core_writer_failure_terminates_all_tasks",
        "real_core_raw_source_first_exit",
    )
    first_exit_tui = (
        "real_tui_peer_close_terminates_all_tasks",
        "real_tui_writer_failure_terminates_all_tasks",
        "real_tui_raw_source_first_exit",
    )
    for name in first_exit_core + first_exit_tui:
        if f"fn {name}" not in real_tests:
            failures.append(f"projection_transport_real.rs: missing first-exit test '{name}'")

    # ── M009 check 3: real adapter peer-close lifecycle (/core + /tui) ──
    for prefix in ("real_core_peer_close_terminates_all_tasks", "real_tui_peer_close_terminates_all_tasks"):
        if f"fn {prefix}" not in real_tests:
            failures.append(f"projection_transport_real.rs: missing peer-close lifecycle test '{prefix}'")

    # ── M009 check 4: actual queue-saturation test (not just fail_next) ─
    if "queue_saturation" not in real_tests:
        failures.append("projection_transport_real.rs: missing queue_saturation test")
    # Verify the queue saturation test uses a real seam pause, not fail_next(Timeout)
    sat_match = re.search(
        r"fn\s+real_core_queue_saturation.*?\{",
        real_tests,
        re.DOTALL,
    )
    if sat_match:
        # Grab ~120 lines from the match start to inspect the body
        sat_body = real_tests[sat_match.start(): sat_match.start() + 4000]
        if re.search(r"fail_next\(.*[Tt]imeout", sat_body):
            failures.append(
                "projection_transport_real.rs: queue_saturation test uses fail_next(Timeout) "
                "injection instead of a real seam-controlled timeout"
            )
    elif "queue_saturation" in real_tests:
        failures.append("projection_transport_real.rs: could not parse queue_saturation test body")

    # ── M009 check 5: actual peer-disconnect tests (drop(client)) ───────
    # At least two tests must use drop(client) for abrupt disconnect
    peer_drop_tests = re.findall(r"fn\s+(real_\w+)\b.*?", real_tests)
    tests_with_drop = set()
    for m in re.finditer(r"fn\s+(real_\w+)\s*\(", real_tests):
        name = m.group(1)
        body_start = m.end()
        # find the matching closing brace (approximate: next 5000 chars)
        snippet = real_tests[body_start: body_start + 5000]
        if "drop(client" in snippet:
            tests_with_drop.add(name)
    if len(tests_with_drop) < 2:
        failures.append(
            f"projection_transport_real.rs: expected at least 2 tests using drop(client) "
            f"for abrupt peer disconnect, found {len(tests_with_drop)}: {sorted(tests_with_drop)}"
        )

    # ── M009 check 6: interrupted replay cleanup and retry ──────────────
    if "disconnect_during_replay" not in real_tests:
        failures.append(
            "projection_transport_real.rs: missing disconnect_during_replay test"
        )

    # ── M009 check 6b: replay mid-delivery interruption ────────────────
    if "disconnect_during_replay_delivery" not in real_tests:
        failures.append(
            "projection_transport_real.rs: missing disconnect_during_replay_delivery test"
        )

    # ── M009 check 7: 100-cycle churn ──────────────────────────────────
    if "100_cycle_churn" not in real_tests:
        failures.append(
            "projection_transport_real.rs: missing 100_cycle_churn test"
        )

    # ── M009 check 8: two-client continuity (/core + /tui) ──────────────
    if "real_core_two_client_continuity" not in real_tests:
        failures.append(
            "projection_transport_real.rs: missing real_core_two_client_continuity test"
        )
    if "real_tui_two_client_continuity" not in real_tests:
        failures.append(
            "projection_transport_real.rs: missing real_tui_two_client_continuity test"
        )

    # ── M009 check 8b: cancellation-wins pending setup (/core + /tui) ──
    if "real_core_cancellation_wins_pending_setup" not in real_tests:
        failures.append(
            "projection_transport_real.rs: missing real_core_cancellation_wins_pending_setup test"
        )
    if "real_tui_cancellation_wins_pending_setup" not in real_tests:
        failures.append(
            "projection_transport_real.rs: missing real_tui_cancellation_wins_pending_setup test"
        )

    # ── M009 check 8c: paused-setup cancellation (/core + /tui) ────────
    if "real_core_paused_snapshot_setup_cancellation" not in real_tests:
        failures.append(
            "projection_transport_real.rs: missing real_core_paused_snapshot_setup_cancellation test"
        )
    if "real_tui_paused_snapshot_setup_cancellation" not in real_tests:
        failures.append(
            "projection_transport_real.rs: missing real_tui_paused_snapshot_setup_cancellation test"
        )

    # ── M009 check 9: ConnectionTaskProbe present in ws.rs ──────────────
    if "struct ConnectionTaskProbe" not in ws:
        failures.append("ws.rs: ConnectionTaskProbe struct is missing")

    # ── M009 check 10: 009-status.md closure record ────────────────────
    if not closure_009:
        failures.append("009-status.md: M009 closure record does not exist")
    else:
        required_fields_009 = (
            (
                "Status: conditionally closed",
                "M009 closure record is not marked conditionally closed (M010 supersedes strict closure)",
            ),
            ("ConnectionTaskSet", "M009 closure record lacks ConnectionTaskSet mention"),
            ("ConnectionTaskProbe", "M009 closure record lacks ConnectionTaskProbe mention"),
            ("queue_saturation", "M009 closure record lacks queue_saturation mention"),
            ("replay", "M009 closure record lacks replay evidence mention"),
            ("peer", "M009 closure record lacks peer lifecycle mention"),
            ("churn", "M009 closure record lacks churn evidence mention"),
            ("continuity", "M009 closure record lacks continuity evidence mention"),
            (
                "010-mechanism-faithful-transport-verification-and-final-closure.md",
                "M009 closure record does not point to M010 follow-up plan",
            ),
        )
        for needle, message in required_fields_009:
            if needle not in closure_009:
                failures.append(f"009-status.md: {message}")

    # ── M010 check 1: transport instrumentation API in ws.rs ──────────
    required_ws_m010 = (
        ("ProjectionTransportTestConfig", "ws.rs: ProjectionTransportTestConfig seam is missing"),
        ("WriterGate", "ws.rs: WriterGate seam is missing"),
        ("TransportLifecycleObserver", "ws.rs: TransportLifecycleObserver seam is missing"),
        ("first_task_kind", "ws.rs: ConnectionTaskProbe/Set first_task_kind is missing"),
        ("first_task_panicked", "ws.rs: first_task_panicked classification is missing"),
        (
            "assert_first_task_kind",
            "ws.rs: assert_first_task_kind helper is missing",
        ),
        (
            "ConnectionTaskKind",
            "ws.rs: ConnectionTaskKind enum is missing",
        ),
        (
            "fill_outbound_queue_to_capacity",
            "ws.rs: outbound queue saturation helper is missing",
        ),
    )
    for needle, message in required_ws_m010:
        if needle not in ws:
            failures.append(message)

    # ── M010 check 2: real_capacity_fill, raw_source_first_exit, panic matrix ──
    required_m010_tests = (
        (
            "real_core_queue_saturation_observer_records_timeout",
            "real queue-saturation observer test is missing",
        ),
        (
            "real_core_outbound_queue_capacity_is_one_when_configured",
            "real outbound capacity=1 fill test is missing",
        ),
        (
            "real_core_connection_task_owner_first_exit_classifies_panic_per_kind",
            "real panic-classification matrix test is missing",
        ),
        (
            "real_core_raw_source_first_exit_via_cancellation_token",
            "real raw-source first-exit via cancellation test is missing",
        ),
        (
            "real_tui_pending_snapshot_interruption_via_writer_barrier",
            "real TUI writer-barrier snapshot interruption test is missing",
        ),
        (
            "real_tui_pending_replay_interruption_then_retry",
            "real TUI replay-interruption retry test is missing",
        ),
    )
    for needle, message in required_m010_tests:
        if f"fn {needle}" not in real_tests:
            failures.append(f"projection_transport_real.rs: {message}")

    # ── M010 check 3: Unix-side M010 fixtures exist ──────────────────────
    unix_tests = _read(ROOT / "src/core/transport/daemon_socket_integration_tests.rs")
    required_unix_m010 = (
        (
            "socket_peer_close_during_writer_delivery_removes_subscription_and_eofs",
            "Unix peer-close race fixture is missing",
        ),
        (
            "socket_writer_failure_during_flush_closes_stream_and_rolls_back",
            "Unix writer-flush-failure fixture is missing",
        ),
        (
            "socket_listener_shutdown_completes_active_writer_and_cleans_subscriptions",
            "Unix cancellation-completion race fixture is missing",
        ),
        (
            "socket_interrupted_replay_retry_resumes_with_fresh_identity",
            "Unix interrupted-replay retry fixture is missing",
        ),
        (
            "socket_consecutive_subscriptions_yield_distinct_identities_and_isolation",
            "Unix fresh-identity proof fixture is missing",
        ),
    )
    for needle, message in required_unix_m010:
        if f"fn {needle}" not in unix_tests:
            failures.append(f"daemon_socket_integration_tests.rs: {message}")

    # ── M010 check 4: capacity-fill test must observe real timeout, not fail_next ──
    sat_match = re.search(
        r"fn\s+real_core_queue_saturation_observer_records_timeout.*?\{",
        real_tests,
        re.DOTALL,
    )
    if sat_match:
        sat_body = real_tests[sat_match.start(): sat_match.start() + 6000]
        if re.search(r"fail_next\(.*[Tt]imeout", sat_body):
            failures.append(
                "projection_transport_real.rs: capacity-fill observer test uses "
                "fail_next(Timeout) injection instead of observing a real timeout"
            )
        if "any_timeout" not in sat_body and "Timeout" not in sat_body:
            failures.append(
                "projection_transport_real.rs: capacity-fill observer test does not "
                "observe a Timeout from the observer"
            )

    # ── M010 check 5: panic-classification matrix covers all three kinds ──
    panic_match = re.search(
        r"fn\s+real_core_connection_task_owner_first_exit_classifies_panic_per_kind.*?\{",
        real_tests,
        re.DOTALL,
    )
    if panic_match:
        panic_body = real_tests[panic_match.start(): panic_match.start() + 8000]
        # Must explicitly reference all three task kinds
        for kind in ("Send", "Receive", "RawEvent"):
            if kind not in panic_body:
                failures.append(
                    f"projection_transport_real.rs: panic-classification matrix does not "
                    f"exercise ConnectionTaskKind::{kind}"
                )

    # ── M010 check 6: raw-source cancellation token is wired ────────────
    raw_match = re.search(
        r"fn\s+real_core_raw_source_first_exit_via_cancellation_token.*?\{",
        real_tests,
        re.DOTALL,
    )
    if raw_match:
        raw_body = real_tests[raw_match.start(): raw_match.start() + 6000]
        if "raw_source_cancel" not in raw_body and "raw_cancel" not in raw_body:
            failures.append(
                "projection_transport_real.rs: raw-source cancellation token is not exercised"
            )
        if "RawEvent" not in raw_body:
            failures.append(
                "projection_transport_real.rs: raw-source test does not classify "
                "first_task_kind as RawEvent"
            )

    # ── M010 check 7: writer-barrier test exercises real WriterGate ────
    for name in (
        "real_tui_pending_snapshot_interruption_via_writer_barrier",
        "real_tui_pending_replay_interruption_then_retry",
    ):
        m = re.search(rf"fn\s+{name}.*?\{{", real_tests, re.DOTALL)
        if m:
            body = real_tests[m.start(): m.start() + 8000]
            if "writer_gate" not in body and "WriterGate" not in body:
                failures.append(
                    f"projection_transport_real.rs: {name} does not exercise the WriterGate"
                )

    # ── M010 check 8: 010-status.md closure record (strict closed) ────
    closure_010 = _read(ROOT / "plans/closure/session-projections/010-status.md")
    if not closure_010:
        failures.append("010-status.md: M010 closure record does not exist")
    else:
        required_fields_010 = (
            ("Status: closed", "M010 closure record is not strictly closed"),
            ("ConnectionTaskSet", "M010 closure record lacks ConnectionTaskSet mention"),
            ("ConnectionTaskProbe", "M010 closure record lacks ConnectionTaskProbe mention"),
            ("WriterGate", "M010 closure record lacks WriterGate mention"),
            ("TransportLifecycleObserver", "M010 closure record lacks TransportLifecycleObserver mention"),
            ("real_core_queue_saturation_observer_records_timeout", "M010 closure record lacks queue-saturation observer test mention"),
            (
                "real_core_connection_task_owner_first_exit_classifies_panic_per_kind",
                "M010 closure record lacks panic-classification matrix mention",
            ),
            (
                "socket_peer_close_during_writer_delivery_removes_subscription_and_eofs",
                "M010 closure record lacks Unix peer-close fixture mention",
            ),
            (
                "socket_interrupted_replay_retry_resumes_with_fresh_identity",
                "M010 closure record lacks Unix interrupted-replay retry mention",
            ),
            (
                "socket_consecutive_subscriptions_yield_distinct_identities_and_isolation",
                "M010 closure record lacks Unix fresh-identity proof mention",
            ),
            (
                "checked by",
                "M010 closure record does not reference the static guard",
            ),
        )
        for needle, message in required_fields_010:
            if needle not in closure_010:
                failures.append(f"010-status.md: {message}")

    # ── M011 check 1: per-connection probe factory / registry ───────────
    for needle, message in (
        (
            "ConnectionProbeFactory",
            "ws.rs: per-connection probe factory type is missing",
        ),
        (
            "ConnectionProbeRegistry",
            "ws.rs: per-connection probe registry type is missing",
        ),
        (
            "probe_factory",
            "ws.rs: ServerState does not retain a probe_factory",
        ),
    ):
        if needle not in ws:
            failures.append(f"ws.rs: {message}")

    # ── M011 check 2: operation-correlated critical-send observation ────
    for needle, message in (
        (
            "struct CriticalSendObservation",
            "ws.rs: CriticalSendObservation struct is missing",
        ),
        (
            "queue_full_before_send",
            "ws.rs: operation-correlated record must record queue_full_before_send",
        ),
        (
            "is_timeout_during_enqueue",
            "ws.rs: operation-correlated record must expose is_timeout_during_enqueue",
        ),
        (
            "receipt_wait_started",
            "ws.rs: operation-correlated record must record receipt_wait_started",
        ),
    ):
        if needle not in ws:
            failures.append(f"ws.rs: {message}")

    # ── M011 check 3: `/core` operation-correlated full-queue timeout ──
    if "fn real_core_full_queue_operation_correlated_timeout" not in real_tests:
        failures.append(
            "projection_transport_real.rs: M011 /core full-queue operation-correlated "
            "timeout fixture is missing"
        )
    else:
        m = re.search(
            r"fn\s+real_core_full_queue_operation_correlated_timeout.*?\{",
            real_tests,
            re.DOTALL,
        )
        body = real_tests[m.start(): m.start() + 8000]
        for needle, message in (
            (
                "TrySendError",
                "/core full-queue fixture does not exercise production TrySendError::Full precondition",
            ),
            (
                "is_timeout_during_enqueue",
                "/core full-queue fixture must use is_timeout_during_enqueue correlation",
            ),
            (
                "queue_full_before_send",
                "/core full-queue fixture must assert queue_full_before_send",
            ),
            (
                "CriticalDeliveryError",
                "/core full-queue fixture must assert CriticalDeliveryError",
            ),
        ):
            if needle not in body:
                failures.append(
                    f"projection_transport_real.rs: real_core_full_queue_operation_correlated_timeout {message}"
                )
        if re.search(r"\bany_timeout\b", body):
            failures.append(
                "projection_transport_real.rs: real_core_full_queue_operation_correlated_timeout "
                "uses any_timeout() instead of operation-correlated evidence"
            )

    # ── M011 check 4: `/tui` operation-correlated full-queue timeout ───
    if "fn real_tui_full_queue_operation_correlated_timeout" not in real_tests:
        failures.append(
            "projection_transport_real.rs: M011 /tui full-queue operation-correlated "
            "timeout fixture is missing"
        )
    else:
        m = re.search(
            r"fn\s+real_tui_full_queue_operation_correlated_timeout.*?\{",
            real_tests,
            re.DOTALL,
        )
        body = real_tests[m.start(): m.start() + 8000]
        for needle, message in (
            (
                "queue_message",
                "/tui full-queue fixture must use production queue_message helper",
            ),
            (
                "is_timeout_during_enqueue",
                "/tui full-queue fixture must use is_timeout_during_enqueue correlation",
            ),
            (
                "queue_full_before_send",
                "/tui full-queue fixture must assert queue_full_before_send",
            ),
            (
                "CriticalDeliveryError",
                "/tui full-queue fixture must assert CriticalDeliveryError::Timeout",
            ),
        ):
            if needle not in body:
                failures.append(
                    f"projection_transport_real.rs: real_tui_full_queue_operation_correlated_timeout {message}"
                )
        if re.search(r"\bany_timeout\b", body):
            failures.append(
                "projection_transport_real.rs: real_tui_full_queue_operation_correlated_timeout "
                "uses any_timeout() instead of operation-correlated evidence"
            )

    # ── M011 check 5: six-case production-teardown first-exit matrix ───
    if "fn real_connection_task_set_six_case_production_teardown_matrix" not in real_tests:
        failures.append(
            "projection_transport_real.rs: M011 six-case production-teardown matrix "
            "fixture is missing"
        )
    else:
        m = re.search(
            r"fn\s+real_connection_task_set_six_case_production_teardown_matrix.*?\{",
            real_tests,
            re.DOTALL,
        )
        body = real_tests[m.start(): m.start() + 12000]
        for kind in ("Send", "Receive", "RawEvent"):
            if kind not in body:
                failures.append(
                    "projection_transport_real.rs: six-case production-teardown matrix "
                    f"must exercise ConnectionTaskKind::{kind}"
                )
        if "production_teardown_for_test" not in body:
            failures.append(
                "projection_transport_real.rs: six-case matrix must invoke the "
                "production teardown wrapper"
            )
        if "is_cancelled" not in body:
            failures.append(
                "projection_transport_real.rs: six-case matrix must assert connection "
                "cancellation token is cancelled"
            )
        # Sentinels must be present for sibling-join proof
        if "production_teardown_for_test" not in ws:
            failures.append(
                "ws.rs: production teardown wrapper must be retained on ConnectionTaskSet"
            )

    # ── M011 check 6: six-case matrix sibling-join guard ───────────────
    if (
        "fn join_after_first_exit_waits_for_sibling_joins_not_just_abort"
        not in real_tests
    ):
        failures.append(
            "projection_transport_real.rs: M011 sibling-join delay guard test is missing"
        )

    # ── M011 check 7: per-adapter raw-source cancellation fixture ───────
    if "fn real_core_raw_source_first_exit_via_cancellation_token" not in real_tests:
        failures.append(
            "projection_transport_real.rs: M011 /core raw-source cancellation fixture is missing"
        )
    else:
        m = re.search(
            r"fn\s+real_core_raw_source_first_exit_via_cancellation_token.*?\{",
            real_tests,
            re.DOTALL,
        )
        body = real_tests[m.start(): m.start() + 6000]
        if "RawEvent" not in body:
            failures.append(
                "projection_transport_real.rs: /core raw-source cancellation must classify "
                "first_task_kind as RawEvent"
            )

    if "fn real_tui_raw_source_first_exit_via_cancellation_token" not in real_tests:
        failures.append(
            "projection_transport_real.rs: M011 /tui raw-source cancellation fixture is missing"
        )
    else:
        m = re.search(
            r"fn\s+real_tui_raw_source_first_exit_via_cancellation_token.*?\{",
            real_tests,
            re.DOTALL,
        )
        body = real_tests[m.start(): m.start() + 6000]
        if "RawEvent" not in body:
            failures.append(
                "projection_transport_real.rs: /tui raw-source cancellation must classify "
                "first_task_kind as RawEvent"
            )
        if "raw_source_cancel" not in body and "raw_cancel" not in body:
            failures.append(
                "projection_transport_real.rs: /tui raw-source cancellation must wire "
                "raw_source_cancel"
            )

    # ── M011 check 8: Unix real-I/O fixtures (no fail_next) ────────────
    unix_required_m011 = (
        (
            "fn socket_f1_peer_closes_before_canonical_response_returns_io_error",
            "Unix F1 pre-response peer-close fixture is missing",
        ),
        (
            "fn socket_f2_writer_failure_drops_peer_write_half_then_read_half",
            "Unix F2 writer-failure fixture is missing",
        ),
        (
            "fn socket_f3_completion_vs_cancellation_race_converges_per_cycle",
            "Unix F3 completion/cancellation race fixture is missing",
        ),
        (
            "fn socket_f4_replay_delivery_interrupted_by_real_peer_close",
            "Unix F4 interrupted replay fixture is missing",
        ),
        (
            "fn socket_f5_repeated_unix_race_convergence_baselines",
            "Unix F5 repeated convergence fixture is missing",
        ),
    )
    for needle, message in unix_required_m011:
        if needle not in unix_tests:
            failures.append(f"daemon_socket_integration_tests.rs: {message}")

    for fixture in (
        "socket_f1_peer_closes_before_canonical_response_returns_io_error",
        "socket_f2_writer_failure_drops_peer_write_half_then_read_half",
        "socket_f3_completion_vs_cancellation_race_converges_per_cycle",
        "socket_f4_replay_delivery_interrupted_by_real_peer_close",
    ):
        m = re.search(rf"fn\s+{fixture}.*?\{{", unix_tests, re.DOTALL)
        if m:
            body = unix_tests[m.start(): m.start() + 6000]
            if "fail_next(" in body:
                failures.append(
                    f"daemon_socket_integration_tests.rs: {fixture} uses fail_next "
                    "injection (M011 must use real peer shutdown/drop)"
                )

    # ── M011 check 9: Unix F4 fixture must drop resumed peer before replay ──
    f4_match = re.search(
        r"fn\s+socket_f4_replay_delivery_interrupted_by_real_peer_close.*?\{",
        unix_tests,
        re.DOTALL,
    )
    if f4_match:
        f4_body = unix_tests[f4_match.start(): f4_match.start() + 8000]
        if not re.search(r"drop\(.*?(writer|peer|read_half)", f4_body):
            failures.append(
                "daemon_socket_integration_tests.rs: socket_f4 must drop the resumed peer "
                "before replay response completion (M011)"
            )

    # ── M011 check 10: complete rollback harness ────────────────────────
    if (
        "assert_real_transport_rollback_complete_extended" not in real_tests
        or "fn assert_tui_transport_rollback_complete" not in real_tests
    ):
        failures.append(
            "projection_transport_real.rs: complete M011 rollback harness "
            "(assert_real_transport_rollback_complete_extended + assert_tui_transport_rollback_complete) is missing"
        )
    if (
        "real_core_rollback_harness_asserts_unrelated_client_continuity" not in real_tests
    ):
        failures.append(
            "projection_transport_real.rs: M011 rollback unrelated-client continuity "
            "fixture is missing"
        )

    # ── M011 check 11: closure records and roadmap state ────────────────
    m011_status = _read(ROOT / "plans/closure/session-projections/011-status.md")
    if not m011_status:
        # Pre-closure: the M011 implementation record must remain referenced
        # from 010-status.md so the chain is auditable.
        if (
            "Status: conditionally closed" not in closure_010
            and "011-evidence-correctness-and-mechanism-verification-closure.md"
            not in closure_010
        ):
            failures.append(
                "010-status.md: must remain conditionally closed and link to "
                "011-evidence-correctness-and-mechanism-verification-closure.md"
            )
    else:
        if "next commit" in m011_status:
            failures.append(
                "011-status.md: must not reference `next commit` placeholders"
            )

    # ── Report ──────────────────────────────────────────────────────────
    if failures:
        for failure in failures:
            print(f"ERROR: {failure}")
        return 1

    print("OK: projection transport lifecycle ownership, stale-route guards, M010 mechanism-faithful instrumentation, and M011 evidence-correctness closure guards are present.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
