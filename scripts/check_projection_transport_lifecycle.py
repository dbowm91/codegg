#!/usr/bin/env python3
"""Guard connection-owned projection transport lifecycle invariants.

Covers M008 (transport ownership, joined teardown, stale-route rejection)
and M009 (real adapter lifecycle, queue saturation, peer disconnect,
interrupted replay, churn, two-client continuity, closure record).
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

    # ── M009 check 9: ConnectionTaskProbe present in ws.rs ──────────────
    if "struct ConnectionTaskProbe" not in ws:
        failures.append("ws.rs: ConnectionTaskProbe struct is missing")

    # ── M009 check 10: 009-status.md closure record ────────────────────
    if not closure_009:
        failures.append("009-status.md: M009 closure record does not exist")
    else:
        required_fields_009 = (
            ("Status: closed", "M009 closure record is not strictly closed"),
            ("ConnectionTaskSet", "M009 closure record lacks ConnectionTaskSet mention"),
            ("ConnectionTaskProbe", "M009 closure record lacks ConnectionTaskProbe mention"),
            ("queue_saturation", "M009 closure record lacks queue_saturation mention"),
            ("disconnect_during_replay", "M009 closure record lacks disconnect_during_replay mention"),
            ("peer_close", "M009 closure record lacks peer_close mention"),
            ("100_cycle_churn", "M009 closure record lacks 100_cycle_churn mention"),
            ("two_client_continuity", "M009 closure record lacks two_client_continuity mention"),
        )
        for needle, message in required_fields_009:
            if needle not in closure_009:
                failures.append(f"009-status.md: {message}")

    # ── Report ──────────────────────────────────────────────────────────
    if failures:
        for failure in failures:
            print(f"ERROR: {failure}")
        return 1

    print("OK: projection transport lifecycle ownership and stale-route guards are present.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
