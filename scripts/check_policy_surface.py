#!/usr/bin/env python3
"""Static guard: M007 policy user-surface ownership.

M007 exposes approval/sandbox selection without moving security truth
into frontends. This guard pins three boundaries:

1. The TUI never forges effective state. Only
   `src/tui/commands/policy.rs` (the daemon-DTO cache) may build the
   cached policy view or issue `RuntimePolicySet`; no TUI file may touch
   `RuntimePreferenceStore`, capture an `ExecutionPolicySnapshot`, route
   escalations, or mutate loop approval/sandbox services.
2. `src/policy_surface.rs` stays frontend-neutral: presentation logic
   only (warning matrix, rendering, CLI resolution). It must not import
   daemon/TUI/server/agent/plugin/scheduler authority or perform I/O.
3. Capability scopes stay signed: `compute_signature` must cover the
   scope, and `verify_signature` must keep the legacy fallback for
   unscoped rows (so pre-M007 grants stay readable but scoped rows never
   verify against a scope-free signature).

Run:
  python3 scripts/check_policy_surface.py

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TUI = ROOT / "src" / "tui"
POLICY_SURFACE = ROOT / "src" / "policy_surface.rs"
PERMISSION_MOD = ROOT / "src" / "permission" / "mod.rs"
TOOL_BATCH = ROOT / "src" / "agent" / "tool_batch.rs"

# Only this TUI file may translate daemon policy DTOs into the cached
# view or issue RuntimePolicySet (via CoreRequest through CoreClient).
POLICY_OWNER = "src/tui/commands/policy.rs"

TUI_FORBIDDEN = [
    (re.compile(r"RuntimePreferenceStore"), "preference-store access"),
    (re.compile(r"ExecutionPolicySnapshot::capture"), "policy-snapshot capture"),
    (re.compile(r"route_escalation"), "approval routing"),
    (re.compile(r"request_human_approval"), "human-approval wait"),
    (re.compile(r"set_approval_mode"), "approval-mode mutation"),
    (re.compile(r"set_sandbox_profile"), "sandbox-profile mutation"),
    (re.compile(r"set_policy\s*\("), "policy-store write"),
    (re.compile(r"add_decision"), "permission-store write"),
]

SURFACE_FORBIDDEN = [
    (re.compile(r"crate::tui"), "TUI import"),
    (re.compile(r"crate::server"), "server import"),
    (re.compile(r"crate::agent"), "agent import"),
    (re.compile(r"crate::core::"), "core import"),
    (re.compile(r"crate::plugin"), "plugin import"),
    (re.compile(r"crate::scheduler"), "scheduler import"),
    (re.compile(r"crate::permission::"), "permission-checker import"),
    (re.compile(r"use tokio"), "tokio import"),
    (re.compile(r"std::fs"), "filesystem import"),
    (re.compile(r"std::env"), "environment import"),
    (re.compile(r"std::process"), "process import"),
    (re.compile(r"CoreRequest"), "protocol request construction"),
    (re.compile(r"CoreResponse"), "protocol response handling"),
]


def is_test_code(rel: str, line: str) -> bool:
    return (
        rel.startswith("tests/")
        or rel.endswith("_test.rs")
        or "#[cfg(test)]" in line
    )


def check_tui() -> list[str]:
    errors: list[str] = []
    for path in sorted(TUI.rglob("*.rs")):
        rel = str(path.relative_to(ROOT))
        if rel == POLICY_OWNER:
            continue
        try:
            lines = path.read_text().splitlines()
        except OSError as exc:
            return [f"cannot read {rel}: {exc}"]
        in_test = False
        for idx, line in enumerate(lines):
            stripped = line.strip()
            if stripped.startswith("//"):
                continue
            if "#[cfg(test)]" in line:
                in_test = True
            if in_test or "tests::" in rel:
                continue
            for pattern, label in TUI_FORBIDDEN:
                if pattern.search(line):
                    errors.append(f"  {rel}:{idx + 1}: {label}: {stripped}")
                    break
    return errors


def check_surface() -> list[str]:
    errors: list[str] = []
    try:
        lines = POLICY_SURFACE.read_text().splitlines()
    except OSError as exc:
        return [f"cannot read policy_surface.rs: {exc}"]
    in_test = False
    for idx, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith("//"):
            continue
        if "#[cfg(test)]" in line:
            in_test = True
        if in_test:
            continue
        for pattern, label in SURFACE_FORBIDDEN:
            if pattern.search(line):
                errors.append(f"  src/policy_surface.rs:{idx + 1}: {label}: {stripped}")
                break
    text = "\n".join(lines)
    for required in [
        "pub fn warning_for",
        "pub fn format_policy_line",
        "pub fn format_policy_detail",
        "pub fn format_restore_summary",
        "pub fn resolve_cli_policy",
        "confirmations_required",
    ]:
        if required not in text:
            errors.append(f"  src/policy_surface.rs: missing required item {required}")
    return errors


def check_scope_signing() -> list[str]:
    errors: list[str] = []
    try:
        text = PERMISSION_MOD.read_text()
    except OSError as exc:
        return [f"cannot read permission/mod.rs: {exc}"]
    if "scope: Option<&str>" not in text:
        errors.append("permission/mod.rs: compute_signature must take the scope")
    if "mac.update(s.as_bytes())" not in text:
        errors.append("permission/mod.rs: scope must enter the HMAC material")
    if "compute_legacy_signature" not in text:
        errors.append("permission/mod.rs: legacy-signature fallback missing")
    if "decision.scope.is_none()" not in text:
        errors.append("permission/mod.rs: legacy fallback must be unscoped-rows only")
    return errors


def check_wiring() -> list[str]:
    errors: list[str] = []
    try:
        owner = (ROOT / POLICY_OWNER).read_text()
    except OSError as exc:
        return [f"cannot read {POLICY_OWNER}: {exc}"]
    for required in [
        "RuntimePolicySet",
        "ApprovalPreferenceGet",
        "ExecutionPolicyGet",
        "expected_revision",
        "pending_policy_confirm",
    ]:
        if required not in owner:
            errors.append(f"  {POLICY_OWNER}: missing required wiring {required}")
    try:
        batch = TOOL_BATCH.read_text()
    except OSError as exc:
        return [f"cannot read tool_batch.rs: {exc}"]
    if "decision_scope_for_tool_call" not in batch:
        errors.append("tool_batch.rs: Always-choice persist must derive the capability scope")
    if "always_allow_scoped" not in batch or "always_deny_scoped" not in batch:
        errors.append("tool_batch.rs: Always-choice persist must use the scoped store variants")
    return errors


def main() -> int:
    errors: list[str] = []
    errors.extend(check_tui())
    errors.extend(check_surface())
    errors.extend(check_scope_signing())
    errors.extend(check_wiring())
    if errors:
        print("check_policy_surface: FAILED")
        for error in errors:
            print(f" - {error}")
        return 1
    print("check_policy_surface: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
