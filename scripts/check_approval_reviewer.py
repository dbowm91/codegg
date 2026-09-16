#!/usr/bin/env python3
"""Static guard: automatic approval reviewer isolation (M006).

The reviewer (`src/permission/reviewer.rs`) is an authorization helper, not a
general reasoning subagent. It must keep a small read-only tool surface and
must never become a second policy or containment owner:

- exact tool palette: read/glob/grep/list/diff/git_read only;
- no PermissionPending registration, human-approval wait, or ApprovalRouter
  recursion (the router owns the single human-wait path);
- no mutating/process/shell/network/subagent construction (bash, terminal,
  edit/write/patch/replace, task/subagent, webfetch/websearch/research);
- no approval-mode/sandbox-profile mutation (set_approval_mode,
  set_sandbox_profile);
- Automatic escalations in `src/agent/tool_batch.rs` resolve through the
  reviewer helper rather than inventing a second routing path.

Run:
  python3 scripts/check_approval_reviewer.py

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REVIEWER = ROOT / "src" / "permission" / "reviewer.rs"
TOOL_BATCH = ROOT / "src" / "agent" / "tool_batch.rs"

EXPECTED_PALETTE = ["read", "glob", "grep", "list", "diff", "git_read"]

FORBIDDEN_PATTERNS = [
    (re.compile(r"PermissionRegistry::register"), "PermissionRegistry registration"),
    (re.compile(r"PermissionPending"), "PermissionPending publish"),
    (re.compile(r"request_human_approval"), "human-approval wait"),
    (re.compile(r"ApprovalRouter"), "ApprovalRouter recursion"),
    (re.compile(r"set_approval_mode"), "approval-mode mutation"),
    (re.compile(r"set_sandbox_profile"), "sandbox-profile mutation"),
    (re.compile(r"TaskTool"), "subagent/task construction"),
    (re.compile(r"SubAgent"), "subagent construction"),
    (re.compile(r"tool::bash"), "shell tool import"),
    (re.compile(r"tool::terminal"), "terminal tool import"),
    (re.compile(r"tool::(edit|write|apply_patch|replace)"), "mutation tool import"),
    (re.compile(r"tool::(webfetch|websearch|research)"), "network tool import"),
    (re.compile(r'"bash"'), "bash tool reference"),
    (re.compile(r'"terminal"'), "terminal tool reference"),
    (re.compile(r'"task"'), "task tool reference"),
]


def check_palette(text: str) -> list[str]:
    errors: list[str] = []
    match = re.search(
        r"pub const REVIEWER_ALLOWED_TOOLS[^=]*=\s*&\[([^\]]*)\]", text
    )
    if not match:
        return ["REVIEWER_ALLOWED_TOOLS constant not found"]
    names = re.findall(r'"([^"]+)"', match.group(1))
    if names != EXPECTED_PALETTE:
        errors.append(
            f"reviewer tool palette is {names}, expected {EXPECTED_PALETTE}"
        )
    return errors


def check_forbidden(path: Path, text: str) -> list[str]:
    errors: list[str] = []
    lines = text.splitlines()
    in_test_cfg = False
    for idx, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith("//"):
            continue
        if "#[cfg(test)]" in line:
            in_test_cfg = True
        if in_test_cfg:
            continue
        for pattern, label in FORBIDDEN_PATTERNS:
            if pattern.search(line):
                errors.append(f"  {path}:{idx + 1}: {label}: {stripped}")
                break
    return errors


def check_tool_batch_wiring() -> list[str]:
    errors: list[str] = []
    try:
        text = TOOL_BATCH.read_text()
    except OSError as exc:
        return [f"cannot read tool_batch.rs: {exc}"]
    if "resolve_automatic_escalation" not in text:
        errors.append("tool_batch.rs: Automatic reviewer helper not wired")
    if "RegistryReviewerInvestigator" not in text:
        errors.append("tool_batch.rs: read-only reviewer investigator not wired")
    if "ProviderReviewerBackend" not in text:
        errors.append("tool_batch.rs: provider reviewer backend not wired")
    if "reviewer_denial_counts" not in text:
        errors.append("tool_batch.rs: equivalent-denial backstop not wired")
    return errors


def main() -> int:
    errors: list[str] = []
    try:
        text = REVIEWER.read_text()
    except OSError as exc:
        print(f"approval-reviewer guard failed: cannot read reviewer.rs: {exc}")
        return 1
    errors.extend(check_palette(text))
    errors.extend(check_forbidden(Path("src/permission/reviewer.rs"), text))
    errors.extend(check_tool_batch_wiring())
    if errors:
        print("approval-reviewer guard failed:")
        for error in errors:
            print(error)
        print(
            "\nKeep the reviewer read-only, recursion-free, and ceiling-preserving; "
            "route Automatic escalations through the reviewer helper."
        )
        return 1
    print("approval-reviewer guard passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
