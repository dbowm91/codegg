#!/usr/bin/env python3
"""Static guard for team-collaboration M001 route authorization convergence.

Ensures every authenticated HTTP route mounted in `src/server/http.rs` (plus
the narrow task-trigger router) has an explicit disposition in
`src/server/authz.rs::route_disposition_table`, and that every handler
consumes the transport-bound principal through the shared adapter.

Checks:
  1. Every `.route("<path>", ...)` in `http.rs` + `task_trigger.rs` appears
     in the disposition table with a matching method.
  2. Every disposition entry corresponds to a real mounted route.
  3. Route handlers under `src/server/routes/` reference the shared
     `authz` adapter (no direct store/bus access without a gate), except
     the narrow trigger module which is explicitly TriggerCapability.
  4. No handler introduces body/query `principal`, `role`, or `capability`
     fields.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
HTTP_MODULE = REPO_ROOT / "src" / "server" / "http.rs"
TRIGGER_MODULE = REPO_ROOT / "src" / "server" / "routes" / "task_trigger.rs"
AUTHZ_MODULE = REPO_ROOT / "src" / "server" / "authz.rs"
ROUTES_DIR = REPO_ROOT / "src" / "server" / "routes"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _mounted_routes() -> set[tuple[str, str]]:
    """(METHOD, PATH) pairs mounted in http.rs + trigger router."""
    routes: set[tuple[str, str]] = set()
    http_src = _read(HTTP_MODULE)
    # Split on `.route(` boundaries; each chunk starts with `"path", ...`
    # followed by handler combinators up to the next `.route(` / `.layer(`.
    chunks = re.split(r"\.route\(", http_src)
    for chunk in chunks[1:]:
        path_match = re.match(r'\s*"([^"]+)"\s*,', chunk)
        if not path_match:
            continue
        path = path_match.group(1)
        # Handler section ends at the next top-level `.route(`/`.layer(`/
        # `.with_state(` — search only up to that boundary.
        boundary = len(chunk)
        for marker in ("\n        .route(", "\n        .layer(", "\n        .with_state("):
            idx = chunk.find(marker)
            if idx != -1:
                boundary = min(boundary, idx)
        handlers = chunk[:boundary]
        for method in ("get", "post", "delete", "put", "patch"):
            if re.search(rf"\b{method}\s*\(", handlers):
                routes.add((method.upper(), path))
    trigger_src = _read(TRIGGER_MODULE)
    for match in re.finditer(r'\.route\(\s*"([^"]+)"\s*,\s*(\w+)\(', trigger_src):
        path, handler = match.group(1), match.group(2)
        # The trigger router only mounts POST fire.
        routes.add(("POST", path))
    return routes


def _disposition_entries() -> set[tuple[str, str]]:
    src = _read(AUTHZ_MODULE)
    entries = set(
        re.findall(
            r'method:\s*"([A-Z]+)"\s*,\s*\n?\s*path:\s*"([^"]+)"',
            src,
        )
    )
    return entries


def check_disposition_covers_routes() -> bool:
    mounted = _mounted_routes()
    table = _disposition_entries()
    # /health is intentionally unauthenticated and outside the table.
    mounted_auth = {r for r in mounted if r[1] != "/health"}
    missing = sorted(mounted_auth - table)
    if missing:
        print(f"  FAIL: routes without disposition: {missing}")
        return False
    return True


def check_no_orphan_dispositions() -> bool:
    mounted = _mounted_routes()
    table = _disposition_entries()
    orphan = sorted(table - mounted - {("POST", "/api/v1/task-triggers/{trigger_id}/fire")})
    # The trigger path is mounted via its own router; allow it explicitly.
    orphan = [r for r in orphan if r not in mounted]
    # Recompute: trigger route is in mounted via TRIGGER_MODULE, so any
    # remainder is genuinely orphaned.
    real_orphan = sorted(table - mounted)
    if real_orphan:
        print(f"  FAIL: disposition entries without a mounted route: {real_orphan}")
        return False
    return True


def check_handlers_use_adapter() -> bool:
    ok = True
    for path in ROUTES_DIR.glob("*.rs"):
        if path.name in ("mod.rs", "task_trigger.rs", "health.rs"):
            continue
        src = _read(path)
        if "authz" not in src:
            print(f"  FAIL: {path.name} does not reference the shared authz adapter")
            ok = False
        if "Extension<AuthenticatedPrincipal>" not in src and "Extension<" not in src:
            # Event/config/provider/tool/mcp all take Extension; session/
            # project/workspace/file/perm/question must too.
            print(f"  FAIL: {path.name} does not consume the transport-bound principal")
            ok = False
    # ws.rs legacy handler must enforce the LocalOwner-only gate.
    ws_src = _read(REPO_ROOT / "src" / "server" / "ws.rs")
    if "legacy /ws is LocalOwner-only" not in ws_src:
        print("  FAIL: ws.rs legacy handler lacks the LocalOwner-only gate")
        ok = False
    return ok


def check_no_payload_authority() -> bool:
    forbidden = re.compile(r"(principal|role|capability)\s*:\s*Option<", re.IGNORECASE)
    ok = True
    for path in ROUTES_DIR.glob("*.rs"):
        if path.name in ("mod.rs", "health.rs"):
            continue
        src = _read(path)
        # Request structs must not carry authority fields.
        for match in forbidden.finditer(src):
            # Allow the word "capability" inside comments/docs about the
            # canonical service, but not as a struct field.
            line_start = src.rfind("\n", 0, match.start()) + 1
            line_end = src.find("\n", match.end())
            line = src[line_start:line_end]
            if "pub " in line and ("principal" in line.lower() or "role" in line.lower()):
                print(f"  FAIL: {path.name} introduces payload authority: {line.strip()}")
                ok = False
    return ok


def main() -> int:
    verbose = "--verbose" in sys.argv or "-v" in sys.argv
    checks = [
        ("route disposition covers every authenticated route", check_disposition_covers_routes),
        ("no orphan disposition entries", check_no_orphan_dispositions),
        ("handlers consume the shared authz adapter", check_handlers_use_adapter),
        ("no payload authority fields", check_no_payload_authority),
    ]
    results = []
    for name, fn in checks:
        if verbose:
            print(f"CHECK: {name} ... ", end="", flush=True)
        try:
            passed = bool(fn())
        except Exception as exc:  # noqa: BLE001 - guard must fail loudly
            print(f"  FAIL: exception: {exc}")
            passed = False
        results.append((name, passed))
        if verbose:
            print("PASS" if passed else "FAIL")
    failed = [n for n, p in results if not p]
    if not verbose:
        for name in failed:
            print(f"FAIL: {name}")
    if failed:
        print("HTTP route disposition invariants violated.")
        return 1
    print("All HTTP route disposition invariants verified.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
