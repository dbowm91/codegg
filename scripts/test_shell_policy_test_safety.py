#!/usr/bin/env python3
"""Positive/negative fixtures for the shell-policy test-safety guard.

Runs the guard's in-memory self-test (pure-classifier accept vs executable
destructive reject vs stale-identifier reject) plus the live audited-tree
check. This is the `python3 scripts/test_shell_policy_test_safety.py`
verification step required by the C002 plan, alongside
`python3 scripts/check_shell_policy_test_safety.py`.

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GUARD = ROOT / "scripts" / "check_shell_policy_test_safety.py"


def main() -> int:
    proc = subprocess.run(
        [sys.executable, str(GUARD), "--self-test"],
        cwd=str(ROOT),
        capture_output=False,
    )
    if proc.returncode != 0:
        print("test_shell_policy_test_safety: FAILED (guard self-test red)")
        return 1
    print("test_shell_policy_test_safety: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
