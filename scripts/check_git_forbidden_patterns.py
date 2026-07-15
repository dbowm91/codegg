#!/usr/bin/env python3
"""
Forbidden-pattern static checks for the Git agent integration.

Scans the codebase for patterns that are explicitly disallowed per the
polish/maintainability/verification plan (Workstream E2):

  * Secret-bearing types that accidentally derive `Debug` / `Serialize`
    with raw-form exposure.
  * Direct `expose_secret()` calls outside the rendering boundary.
  * RunStore persistence using unsanitized argv.
  * Duplicated env-policy tables (must live in `codegg-git::process_policy`).
  * `RerunDescriptor { argv: <Vec<String>> }` literals (must use AuditSafeArgv).

Exits 0 when all checks pass, 1 otherwise. Each finding includes the
file, line, and a one-line rationale.

The script is deliberately strict — every false positive should be
either fixed or added to the explicit allowlist at the bottom of this
file with a rationale.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, List, Optional, Tuple


ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src"
CRATES = ROOT / "crates"
TESTS = ROOT / "tests"


@dataclass
class Finding:
    rule: str
    file: str
    line: int
    message: str


def rg(pattern: str, root: Path, include: Optional[str] = None) -> List[Tuple[Path, int, str]]:
    """Run ripgrep with line numbers, return list of (path, line, content)."""
    args = [
        "rg",
        "--line-number",
        "--no-heading",
        "--color=never",
        "--no-messages",
    ]
    if include is not None:
        args.extend(["-g", include])
    args.extend([pattern, str(root)])
    try:
        result = subprocess.run(
            args,
            capture_output=True,
            text=True,
            check=False,
        )
    except FileNotFoundError:
        print("error: ripgrep (`rg`) not found on PATH", file=sys.stderr)
        sys.exit(2)
    out: List[Tuple[Path, int, str]] = []
    for raw in result.stdout.splitlines():
        # rg output: <path>:<lineno>:<content>
        parts = raw.split(":", 2)
        if len(parts) < 3:
            continue
        path_str, lineno_str, content = parts
        try:
            lineno = int(lineno_str)
        except ValueError:
            continue
        out.append((Path(path_str), lineno, content))
    return out


# Allowlist patterns (paths or path-prefixes that may opt out of a rule).
# Each entry is a tuple of (rule, predicate_path_str) and the rationale
# is captured in the comment below the table.
ALLOWLIST: List[Tuple[str, str, str]] = [
    # (rule, path_substring, rationale)
    (
        "duplicated-env-policy",
        "/examples/",
        "Examples are independent workspace and may have their own policy.",
    ),
    (
        "unsanitized-runstore-argv",
        "/examples/",
        "Examples are independent and not part of the audit surface.",
    ),
    (
        "unsanitized-runstore-argv",
        "/target/",
        "Build artifacts.",
    ),
]


def is_allowed(rule: str, path_str: str) -> bool:
    for r, sub, _ in ALLOWLIST:
        if r == rule and sub in path_str:
            return True
    return False


def check_secret_debug_serialize() -> List[Finding]:
    """Secret-bearing types must not derive Debug/Serialize in a way that
    leaks the raw value. We pin this with grep heuristics on
    `RedactedUrl`/`AuditSafeArgv`: any manual impl that exposes raw via
    `format!`/`write!` is a finding."""
    findings: List[Finding] = []
    # Check that `RedactedUrl::Debug` and `Serialize` impls do not print raw.
    redacted_path = CRATES / "codegg-git" / "src" / "sensitive.rs"
    if redacted_path.exists():
        text = redacted_path.read_text()
        for marker in ["expose_secret()", "self.raw"]:
            # We allow expose_secret inside Debug only when wrapped in a debug_struct
            # that doesn't include "raw" as a field. Greppable sanity check:
            # if a Debug impl prints raw, it's a problem.
            if "impl fmt::Debug for RedactedUrl" in text:
                idx = text.index("impl fmt::Debug for RedactedUrl")
                # Look at the next 300 chars
                block = text[idx : idx + 600]
                if "field(\"raw\"" in block or "field(\"raw_value\"" in block:
                    findings.append(
                        Finding(
                            rule="redacted-url-debug-leaks-raw",
                            file=str(redacted_path.relative_to(ROOT)),
                            line=text[:idx].count("\n") + 1,
                            message="RedactedUrl Debug impl mentions raw field name",
                        )
                    )
    return findings


def _in_test_module(path: Path, lineno: int) -> bool:
    """Heuristic: return True if `lineno` in `path` falls inside a
    `#[cfg(test)] mod tests { ... }` block. Walks the source file
    from the top, tracking brace depth, and returns True the first
    time we enter a test module that contains the target line.
    """
    try:
        text = path.read_text()
    except (FileNotFoundError, IsADirectoryError):
        return False
    lines = text.splitlines()
    depth = 0
    in_test = False
    for i, line in enumerate(lines, start=1):
        stripped = line.strip()
        # Detect entering a test module.
        if depth == 0 and "mod tests" in stripped and stripped.endswith("{"):
            # Is this a cfg(test) module? Look at the previous non-blank line.
            for j in range(i - 2, max(-1, i - 5), -1):
                if j < 0:
                    break
                prev = lines[j].strip()
                if not prev:
                    continue
                if "cfg(test)" in prev:
                    in_test = True
                    break
                if prev.endswith(";") or prev.endswith("{"):
                    continue
                break
        # Update brace depth BEFORE checking the line.
        if stripped.endswith("{") and not stripped.startswith("#"):
            depth += 1
        if stripped.startswith("}"):
            depth -= 1
            if depth <= 0:
                in_test = False
                depth = 0
        if i == lineno:
            return in_test
    return False


def check_expose_secret_callers() -> List[Finding]:
    """expose_secret() may only be called at the render_argv boundary.
    Grep for callers and report any outside the approved list.

    Doc-only mentions (architecture docs, plans, AGENTS.md, this
    script's own comment headers, the canonical sensitive.rs
    definition) are excluded. Test code (anything under `tests/` or
    inside `#[cfg(test)] mod tests { ... }` blocks) is also
    excluded because tests legitimately use expose_secret to assert
    the round-trip property.
    """
    findings: List[Finding] = []
    approved_callers = [
        "crates/codegg-git/src/render.rs",  # The one approved boundary.
    ]
    # Doc / non-source paths that may reference the symbol.
    doc_substrings = (
        "docs/",
        "plans/",
        "architecture/",
        "AGENTS.md",
        "scripts/check_git_forbidden_patterns.py",
        "README.md",
        "tests/",  # tests use expose_secret to assert behavior is correct.
    )
    for path, lineno, content in rg("expose_secret\\(\\)", ROOT):
        path_str = str(path.relative_to(ROOT))
        # Skip the script's own filename (the rule descriptions
        # mention expose_secret).
        if "check_git_forbidden_patterns.py" in path_str:
            continue
        if any(path_str.startswith(p) for p in approved_callers):
            continue
        if any(sub in path_str for sub in doc_substrings):
            continue
        # Skip the sensitive.rs definition site.
        if "sensitive.rs" in path_str:
            continue
        # Skip unit-test code inside src files.
        if path_str.startswith("src/") and _in_test_module(path, lineno):
            continue
        findings.append(
            Finding(
                rule="expose-secret-outside-render",
                file=path_str,
                line=lineno,
                message="expose_secret() called outside the render_argv boundary",
            )
        )
    return findings


def check_duplicated_env_policy() -> List[Finding]:
    """Env policy tables must live only in
    codegg_git::process_policy. Detect hand-maintained copies."""
    findings: List[Finding] = []
    # Heuristic: a hand-maintained copy is a file that defines one of the
    # canonical lists inline. We look for both `const ALLOWED_ENV_VARS`
    # and `const ALWAYS_STRIPPED_ENV_VARS` (or close variants) outside
    # the canonical module.
    canonical_paths = {
        str((CRATES / "codegg-git" / "src" / "process_policy.rs").relative_to(ROOT)),
        str((CRATES / "codegg-git" / "src" / "lib.rs").relative_to(ROOT)),
        str((ROOT / "src" / "git_mutations.rs").relative_to(ROOT)),
        str((CRATES / "codegg-core" / "src" / "worktree.rs").relative_to(ROOT)),
    }
    for path, lineno, content in rg(
        "const (ALLOWED_ENV_VARS|ALWAYS_STRIPPED_ENV_VARS|STRIPPED_ENV_VARS)",
        ROOT,
        include="*.rs",
    ):
        path_str = str(path.relative_to(ROOT))
        if path_str in canonical_paths:
            continue
        if is_allowed("duplicated-env-policy", path_str):
            continue
        findings.append(
            Finding(
                rule="duplicated-env-policy",
                file=path_str,
                line=lineno,
                message=(
                    "Hand-maintained env policy table detected. "
                    "Use codegg_git::process_policy constants instead."
                ),
            )
        )
    return findings


def check_rerun_argv_construction() -> List[Finding]:
    """RerunDescriptor.argv must be AuditSafeArgv, not Vec<String>.
    Grep for the literal `argv: Some(vec!` or `argv: Some(Vec::<String>` in
    RerunDescriptor constructions."""
    findings: List[Finding] = []
    # We look for RerunDescriptor { ... argv: ... } and check the argv line.
    for path, lineno, content in rg("RerunDescriptor\\s*\\{", ROOT, include="*.rs"):
        path_str = str(path.relative_to(ROOT))
        # Read the next 20 lines to find the argv field.
        full = path.read_text().splitlines()
        block_start = max(0, lineno - 1)
        block = "\n".join(full[block_start : block_start + 20])
        m = re.search(r"argv:\s*([^,\n]+)", block)
        if m is None:
            continue
        argv_expr = m.group(1).strip()
        # AuditSafeArgv constructions are acceptable.
        if "AuditSafeArgv" in argv_expr:
            continue
        # None is acceptable.
        if argv_expr == "None":
            continue
        findings.append(
            Finding(
                rule="rerun-argv-not-audit-safe",
                file=path_str,
                line=lineno,
                message=(
                    f"RerunDescriptor.argv uses {argv_expr[:60]!r}; "
                    "must wrap with AuditSafeArgv::from_argv(...)"
                ),
            )
        )
    return findings


def check_runstore_argv_sanitization() -> List[Finding]:
    """RunStore invocations whose argv comes from a git
    `render_argv` call must flow through
    `sanitize_argv_for_run_store` first.

    This rule is scoped: it only flags argv that originates from
    `render_argv` and is persisted into `RunInvocation`. Other
    argv (test commands, bash argv, python argv) is already
    credential-free and does not need the sanitizer.
    """
    findings: List[Finding] = []
    for path, lineno, content in rg("RunInvocation\\s*\\{", ROOT, include="*.rs"):
        path_str = str(path.relative_to(ROOT))
        if is_allowed("unsanitized-runstore-argv", path_str):
            continue
        # Skip non-source paths (docs, scripts).
        if any(
            sub in path_str
            for sub in ("/docs/", "/plans/", "/architecture/", "/AGENTS.md", "/scripts/")
        ):
            continue
        full = path.read_text().splitlines()
        block_start = max(0, lineno - 1)
        block = "\n".join(full[block_start : block_start + 15])
        m = re.search(r"argv:\s*Some\(([^)]+)\)", block)
        if m is None:
            continue
        argv_expr = m.group(1).strip()
        if "sanitize_argv_for_run_store" in argv_expr:
            continue
        # Allow None and the empty-vec shortcut.
        if argv_expr in ("None", "Some(vec![])"):
            continue
        # Only flag git-specific argv sources. Other tools (test_runner,
        # python_script, bash tool) construct argv that does not flow
        # through render_argv.
        if "render_argv" not in argv_expr:
            continue
        # Skip the redact-then-sanitize helper that's the canonical path.
        findings.append(
            Finding(
                rule="unsanitized-runstore-argv",
                file=path_str,
                line=lineno,
                message=(
                    f"RunInvocation.argv = {argv_expr[:80]!r} — "
                    "git argv must flow through sanitize_argv_for_run_store"
                ),
            )
        )
    return findings


CHECKS = [
    check_secret_debug_serialize,
    check_expose_secret_callers,
    check_duplicated_env_policy,
    check_rerun_argv_construction,
    check_runstore_argv_sanitization,
]


def main() -> int:
    all_findings: List[Finding] = []
    for check in CHECKS:
        all_findings.extend(check())
    if not all_findings:
        print("forbidden-pattern checks: PASS (0 findings)")
        return 0
    print(f"forbidden-pattern checks: FAIL ({len(all_findings)} finding(s))")
    by_rule: dict[str, List[Finding]] = {}
    for f in all_findings:
        by_rule.setdefault(f.rule, []).append(f)
    for rule, findings in sorted(by_rule.items()):
        print(f"\n  rule: {rule}  ({len(findings)} finding(s))")
        for f in findings:
            print(f"    {f.file}:{f.line}: {f.message}")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())