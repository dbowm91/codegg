#!/usr/bin/env python3
"""Static guard: production permission escalation goes through ApprovalRouter.

M003 establishes `ApprovalRouter` (`src/permission/approval.rs`) as the
single production owner that resolves deterministic escalations. Tool
implementations and `AgentLoop` call sites must not independently invent
human/reviewer/bypass behavior by registering `PermissionPending` directly.

Allowed direct registration:
  - Inside `src/permission/approval.rs` (the router itself)
  - Inside `crates/codegg-core/src/bus/` (registry definition)
  - Inside `#[cfg(test)]` blocks and `tests/` fixtures

The responder side (`respond_scoped` / `unregister_scoped` in daemon/TUI)
is not restricted by this guard; only the requester side (`register*` +
`PermissionPending` publish) is owned by the router.

Run:
  python3 scripts/check_approval_router.py

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src"

REGISTER_PATTERNS = [
    re.compile(r"PermissionRegistry::register_with_session\s*\("),
    re.compile(r"PermissionRegistry::register\s*\("),
]

APPROVAL_OWNER = "src/permission/approval.rs"


def is_test_file(rel: str) -> bool:
    return (
        "/tests/" in rel
        or rel.startswith("tests/")
        or rel.endswith("_test.rs")
        or "/test_support" in rel
    )


def check_file(path: Path) -> list[str]:
    rel = str(path.relative_to(ROOT))
    if rel == APPROVAL_OWNER:
        return []
    if is_test_file(rel):
        return []
    # Registry definition itself is not a requester.
    if rel.startswith("crates/codegg-core/src/bus/"):
        return []
    try:
        lines = path.read_text().splitlines()
    except Exception:
        return []
    # Skip files with no relevant tokens quickly.
    text = "\n".join(lines)
    if "PermissionRegistry" not in text:
        return []
    errors: list[str] = []
    in_test_cfg = False
    brace_depth = 0
    for idx, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith("//") or stripped.startswith("///") or stripped.startswith("//!"):
            continue
        # Track #[cfg(test)] modules loosely: if the file contains a
        # cfg(test) mod, allow register sites inside it by checking
        # proximity to the attribute (best-effort, matches existing guards).
        if "#[cfg(test)]" in line:
            in_test_cfg = True
        brace_depth += line.count("{") - line.count("}")
        if brace_depth < 0:
            brace_depth = 0
            in_test_cfg = False
        for pattern in REGISTER_PATTERNS:
            if pattern.search(line):
                # Allow responder-adjacent comments? No: any register
                # outside the owner is a violation, even in daemon code
                # (daemon only responds, never registers).
                if not in_test_cfg:
                    errors.append(f"  {rel}:{idx + 1}: {stripped}")
                break
    return errors


def main() -> int:
    violations: list[str] = []
    for rust_file in sorted(SRC.rglob("*.rs")):
        violations.extend(check_file(rust_file))
    if violations:
        print(
            "approval-router guard failed: production permission escalation "
            "must go through ApprovalRouter (src/permission/approval.rs):"
        )
        for violation in violations:
            print(violation)
        print(
            "\nMove new escalation paths behind ApprovalRouter::request_human_approval "
            "or route_escalation instead of registering PermissionPending directly."
        )
        return 1
    print("approval-router guard passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
