#!/usr/bin/env python3
"""Guard connection-owned projection transport lifecycle invariants."""

from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parent.parent


def main() -> int:
    failures: list[str] = []
    unix = (ROOT / "src/core/transport/daemon_socket.rs").read_text()
    ws = (ROOT / "src/server/ws.rs").read_text()
    projection = (ROOT / "src/core/transport/projection.rs").read_text()
    closure_path = ROOT / "plans/closure/session-projections/008-status.md"
    closure = closure_path.read_text() if closure_path.exists() else ""
    unix_production = unix.split("#[cfg(test)]", 1)[0]

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

    required_closure = (
        ("Status: closed", "M008 closure record is not strictly closed"),
        ("6975050af530eb5bd7a640c1f7ac9a31859dfda3", "M008 implementation commit is missing"),
        ("ea6e38d5182f42ae70c5f379415dd8ee1eb470e2", "M008 closure commit is missing"),
        (
            "projection_transport_real`: 20 listed and 20 passed",
            "M008 closure record lacks the exact real transport count",
        ),
    )
    for needle, message in required_closure:
        if needle not in closure:
            failures.append(f"008-status.md: {message}")
    if re.search(r"(?:tokio::sync::)?mpsc::unbounded_channel", ws):
        failures.append("ws.rs: unbounded outbound channel is forbidden")
    if re.search(r"pub\s+fn\s+mark_live", projection):
        failures.append("projection.rs: mark_live must remain private to the activation helper")
    if "fn mark_live(&mut self)" not in projection:
        failures.append("projection.rs: private activation transition is missing")
    if "activate_after_delivery" not in projection:
        failures.append("projection.rs: approved activation helper is missing")

    if failures:
        for failure in failures:
            print(f"ERROR: {failure}")
        return 1

    print("OK: projection transport lifecycle ownership and stale-route guards are present.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
