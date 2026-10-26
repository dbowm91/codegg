#!/usr/bin/env python3
"""Guard production progress observations and the bounded recovery contract.

The guard keeps `ProgressObservation` honest: every declared field must be
written at the production construction site, and the removed dead stall knob
and unreachable terminal API must not return. `--self-test` proves the detector
rejects an omitted field and a reintroduced dead control.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
STRUCT_RE = re.compile(r"pub struct ProgressObservation\s*\{(?P<body>.*?)\n\}", re.S)
FIELD_RE = re.compile(r"^\s*pub\s+([a-z][a-z0-9_]*):", re.M)
CONSTRUCTION_RE = re.compile(r"let observation = ProgressObservation\s*\{(?P<body>.*?)\n\s*\};", re.S)
ASSIGNMENT_RE = re.compile(r"^\s{20}([a-z][a-z0-9_]*)(?:\s*:|\s*,)", re.M)
DEAD_CONTROLS = (
    "recovery_parallel_limit",
    "RecoveryDecision::Stalled",
    "RecoveryAction::Stall",
    "AutonomyPhase::Stall",
)


def inspect(recovery: str, loop: str, coordinator: str) -> list[str]:
    findings: list[str] = []
    struct = STRUCT_RE.search(recovery)
    construction = CONSTRUCTION_RE.search(loop)
    if struct is None:
        findings.append("ProgressObservation declaration was not found")
    if construction is None:
        findings.append("production ProgressObservation construction was not found")
    if struct is not None and construction is not None:
        declared = set(FIELD_RE.findall(struct.group("body")))
        written = set(ASSIGNMENT_RE.findall(construction.group("body")))
        missing = sorted(declared - written)
        extra = sorted(written - declared)
        if missing:
            findings.append("production observation omits fields: " + ", ".join(missing))
        if extra:
            findings.append("production observation writes unknown fields: " + ", ".join(extra))

    for token in DEAD_CONTROLS:
        if token in recovery or token in loop or token in coordinator:
            findings.append(f"removed dead recovery control returned: {token}")
    return findings


def self_test() -> bool:
    recovery = """pub struct ProgressObservation {\n    pub action: ActionClass,\n    pub state_changed: bool,\n}\n"""
    loop = """let observation = ProgressObservation {\n                    action: action,\n                    state_changed: changed,\n                };\n"""
    clean = inspect(recovery, loop, "")
    if clean:
        print(f"self-test clean fixture unexpectedly failed: {clean}", file=sys.stderr)
        return False
    missing = inspect(recovery, loop.replace("                    state_changed: changed,\n", ""), "")
    if not any("state_changed" in finding for finding in missing):
        print("self-test did not detect an omitted observation field", file=sys.stderr)
        return False
    dead = inspect(recovery, loop, "recovery_parallel_limit")
    if not any("recovery_parallel_limit" in finding for finding in dead):
        print("self-test did not detect a reintroduced dead control", file=sys.stderr)
        return False
    print("progress recovery observation guard self-test passed")
    return True


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return 0 if self_test() else 2
    recovery = (ROOT / "src/agent/progress_recovery.rs").read_text()
    loop = (ROOT / "src/agent/loop.rs").read_text()
    coordinator = (ROOT / "src/agent/coordinator.rs").read_text()
    findings = inspect(recovery, loop, coordinator)
    if findings:
        for finding in findings:
            print(f"FAIL: {finding}", file=sys.stderr)
        return 1
    print("progress recovery observation wiring passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
