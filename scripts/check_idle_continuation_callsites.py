#!/usr/bin/env python3
"""Static guard for the C005 delegated idle-continuation entry (ADR-0031).

`CoreDaemon::submit_idle_continuation_turn` is daemon-internal delegated
admission: no CoreRequest variant, no transport route, no bearer. The ADR
restricts it to exactly two call sites — the post-commit fanout wake
(`maybe_dispatch_idle_continuation`) and the bounded restart twin
(`reconcile_idle_continuation_wakes`). Any other caller would be a
general daemon-may-submit-as-anyone backdoor.

Checks:
1. The definition lives in `src/core/daemon_continuation.rs`.
2. Every *call* of `submit_idle_continuation_turn(` outside the definition
   is inside one of the two permitted caller functions, in an allowed file.
3. No `CoreRequest` variant mentions dispatch (dispatch is never a wire op).

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CONTINUATION_MODULE = REPO_ROOT / "src" / "core" / "daemon_continuation.rs"
PROTOCOL_CORE = REPO_ROOT / "crates" / "codegg-protocol" / "src" / "core.rs"

ENTRY = "submit_idle_continuation_turn"
PERMITTED_CALLERS = {
    "maybe_dispatch_idle_continuation",
    "reconcile_idle_continuation_wakes",
}
PERMITTED_FILES = {
    "src/core/daemon_continuation.rs",
    "src/core/daemon_bootstrap.rs",
}


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def check_definition_site() -> bool:
    source = _read(CONTINUATION_MODULE)
    if f"pub(crate) async fn {ENTRY}" not in source and f"pub async fn {ENTRY}" not in source:
        print(f"  FAIL: {ENTRY} definition not found in daemon_continuation.rs")
        return False
    return True


def check_call_sites() -> bool:
    ok = True
    # Definition + declaration lines that are not calls.
    definition = re.compile(rf"async fn {ENTRY}\s*\(")
    for path in sorted((REPO_ROOT / "src").rglob("*.rs")):
        # Skip our own guard-adjacent unit tests only if they never call.
        source = _read(path)
        for match in re.finditer(rf"{ENTRY}\s*\(", source):
            start = match.start()
            line_start = source.rfind("\n", 0, start) + 1
            line_end = source.find("\n", match.end())
            line = source[line_start:line_end if line_end != -1 else len(source)]
            if definition.search(line):
                continue  # the definition itself
            if "maybe_dispatch_idle_continuation" in line and "async fn" in line:
                continue
            # Enclosing function: nearest preceding `async fn F(` / `fn F(`.
            preceding = source[:start]
            fns = re.findall(r"(?:async\s+)?fn\s+(\w+)\s*[<(]", preceding)
            caller = fns[-1] if fns else "<top-level>"
            rel = str(path.relative_to(REPO_ROOT))
            if caller not in PERMITTED_CALLERS or rel not in PERMITTED_FILES:
                print(
                    f"  FAIL: {ENTRY} called from {caller}() in {rel} "
                    f"(permitted: {sorted(PERMITTED_CALLERS)})"
                )
                ok = False
    return ok


def check_no_wire_dispatch() -> bool:
    source = _read(PROTOCOL_CORE)
    match = re.search(r"pub enum CoreRequest \{(.*?)\n\}", source, re.DOTALL)
    if not match:
        print("  FAIL: CoreRequest enum not found")
        return False
    body = match.group(1)
    for forbidden in ("Dispatch", "dispatch", "IdleContinuationSubmit", "idle_continuation_turn"):
        if forbidden in body:
            print(f"  FAIL: CoreRequest carries a dispatch surface ({forbidden})")
            return False
    return True


def main() -> int:
    checks = [
        ("delegated entry defined once", check_definition_site),
        ("call sites restricted to fanout + twin", check_call_sites),
        ("dispatch is never a wire operation", check_no_wire_dispatch),
    ]
    failed = []
    for name, check in checks:
        try:
            passed = check()
        except OSError as error:
            print(f"  FAIL: {name}: {error}")
            passed = False
        print(f"  {'ok' if passed else 'FAIL'}: {name}")
        if not passed:
            failed.append(name)
    if failed:
        print(f"check_idle_continuation_callsites: {len(failed)} failing")
        return 1
    print("check_idle_continuation_callsites: all checks pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
