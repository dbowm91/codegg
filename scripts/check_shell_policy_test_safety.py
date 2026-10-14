#!/usr/bin/env python3
"""Static guard: shell-policy tests must never dispatch host-destructive commands.

ADR-0028 establishes that built-in catastrophic shell shapes are advisory
risk metadata, not a default terminal denial. The regression tests therefore
must not invoke `BashTool::execute` / `TerminalTool::execute` (or a raw
subprocess) with a host-global destructive spelling merely to assert a
textual hard stop: if the guard ever regressed, the test itself would dispatch
`rm -rf /`, `mkfs`, `shutdown`, and friends on the machine running CI.

What this guard checks (audited locations only):
  - tests/tool_execution.rs (primary shell-policy contract)
  - tests/agent_loop_harness.rs (approval-path marker tests)
  - tests/permission.rs (bash permission matrix)
  - tests/approval_router.rs (approval routing)
  - tests/sandbox_containment.rs (sandbox enforcement)

Rules:
  1. No function in an audited file may contain BOTH a host-global
     destructive literal AND a process-dispatch call (`.execute(`, raw
     `Command::new`, `TerminalTool::execute`). Dangerous strings may appear
     only as inert classifier data passed to the pure `find_ask_pattern` /
     `find_risk_pattern` functions (no process is spawned).
  2. Stale built-in hard-deny identifiers (`HARD_DENY_COMMANDS`,
     `HARD_DENY_PATTERNS`, `test_bash_tool_denied_pattern`,
     `test_bash_tool_hard_deny_tier`) are rejected: they belong to the
     superseded C001 contract.
  3. A function that legitimately needs both shapes requires an explicit
     reviewed exception comment containing `shell-test-safety-exception`
     with a rationale. The guard reports the exception as allowed but still
     lists it so reviewers see it.

Limits (intentionally bounded and transparent):
  - Regex-based, not a Rust AST/dataflow analysis. It sees text, not value
    flow: a destructive literal smuggled through an aliased constant, macro,
    or cross-function variable will not be linked to its `execute` site.
  - Function chunking is `^\\s*(?:pub(?:\\([^)]*\\))?\\s+)?(?:async\\s+)?fn\\s+`
    based; closures and `#[cfg(test)] mod` nesting do not change the rule.
  - It does not execute code and does not prove the absence of every unsafe
    spelling; it complements human review and the safe-fixture design
    (benign `printf`/`echo` sentinels + TempDir markers + zero-dispatch
    assertions), it does not replace them.

Run:
  python3 scripts/check_shell_policy_test_safety.py
  python3 scripts/check_shell_policy_test_safety.py --self-test

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

AUDITED_FILES = [
    "tests/tool_execution.rs",
    "tests/agent_loop_harness.rs",
    "tests/permission.rs",
    "tests/approval_router.rs",
    "tests/sandbox_containment.rs",
]

# Host-global destructive spellings. Kept intentionally narrow: scoped
# workspace shapes (`rm -rf ./target`, `rm a_file.txt`, `> /dev/null`) must
# NOT match. Each entry is a regex searched against the function text.
DESTRUCTIVE_PATTERNS = [
    re.compile(r"rm\s+(-[a-zA-Z]*[rR][fF]|-rf|-fr)\s+/\s*(?:$|\"|'|,|\]|\}|\s*[*);|&`])"),
    re.compile(r"rm\s+(-[a-zA-Z]*[rR][fF]|-rf|-fr)\s+/\*"),
    re.compile(r"rm\s+(-[a-zA-Z]*[rR][fF]|-rf|-fr)\s+~"),
    re.compile(r"rm\s+(-[a-zA-Z]*[rR][fF]|-rf|-fr)\s+\$HOME"),
    re.compile(r"\bmkfs(\.\w+)?\b"),
    re.compile(r"\bdd\s+[^\"\n]*if=/dev/zero\b"),
    re.compile(r":\s*\(\s*\)\s*\{[^}]*\|[^}]*\}\s*;\s*:"),
    re.compile(r"(?:^|[^a-zA-Z_])(?:shutdown|reboot|poweroff|halt)\b"),
    re.compile(r"\bkill\s+-9\s+-1\b"),
    re.compile(r"(?:^|\s)(?:fdisk|parted)(?:\s|$|\"|')"),
    re.compile(r"\bumount\s+/\s*(?:$|[;|&\"]|')"),
    re.compile(r"(?:^|[;&|\s])(?:init|telinit)\s+0\b"),
    re.compile(r"chmod\s+-R\s+777\s+/"),
]

EXECUTE_PATTERNS = [
    re.compile(r"\.execute\s*\("),
    re.compile(r"TerminalTool"),
    re.compile(r"Command::new\s*\("),
    re.compile(r"std::process::Command"),
]

STALE_IDENTIFIERS = [
    "HARD_DENY_COMMANDS",
    "HARD_DENY_PATTERNS",
    "test_bash_tool_denied_pattern",
    "test_bash_tool_hard_deny_tier",
]

EXCEPTION_MARKER = "shell-test-safety-exception"

FN_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z0-9_]+)")


@dataclass
class Finding:
    file: str
    line: int
    rule: str
    message: str


def split_functions(text: str) -> list[tuple[str, int, str]]:
    """Split Rust source into (name, start_line, body) chunks by fn items.

    Each chunk is the brace-balanced function body only, so top-level
    constants, doc comments, and fixtures between functions are NOT attributed
    to the preceding function. Comment-only lines are retained here and
    filtered by the caller.
    """
    lines = text.splitlines()
    starts: list[tuple[str, int]] = []
    for idx, line in enumerate(lines):
        m = FN_RE.match(line)
        if m:
            starts.append((m.group(1), idx + 1))
    if not starts:
        return [("<file>", 1, text)]
    chunks: list[tuple[str, int, str]] = []
    for name, start in starts:
        # Find the opening brace of the function body, then balance.
        body_start = start - 1
        while body_start < len(lines) and "{" not in lines[body_start]:
            body_start += 1
        depth = 0
        end = len(lines)
        for i in range(body_start, len(lines)):
            depth += lines[i].count("{") - lines[i].count("}")
            if i > body_start and depth == 0:
                end = i + 1
                break
        # Fallback: if balancing failed, cap at the next fn start.
        if end == len(lines):
            nxt = [s for _, s in starts if s > start]
            if nxt:
                end = min(nxt) - 1
        body = "\n".join(lines[start - 1 : end])
        chunks.append((name, start, body))
    # Leading file header (imports/constants outside any fn) as its own chunk.
    header = "\n".join(lines[: starts[0][1] - 1])
    chunks.insert(0, ("<file-header>", 1, header))
    return chunks


def code_lines(body: str) -> str:
    """Return non-comment lines for destructive/execute matching.

    Full-line `//` comments (including doc comments that legitimately name
    destructive shapes as prose) are excluded. Inline code, including string
    literals such as `"rm -rf /"`, is preserved.
    """
    kept: list[str] = []
    for line in body.splitlines():
        if line.strip().startswith("//"):
            continue
        kept.append(line)
    return "\n".join(kept)


def check_text(rel: str, text: str) -> list[Finding]:
    findings: list[Finding] = []
    for stale in STALE_IDENTIFIERS:
        for idx, line in enumerate(text.splitlines(), start=1):
            stripped = line.strip()
            if stripped.startswith("//"):
                continue
            if stale in line:
                findings.append(
                    Finding(
                        rel,
                        idx,
                        "stale-hard-deny-contract",
                        f"stale C001 identifier {stale!r} belongs to the superseded "
                        "built-in hard-deny contract; use the ADR-0028 advisory "
                        "fixtures instead",
                    )
                )
    for name, start_line, body in split_functions(text):
        code = code_lines(body)
        has_destructive = any(p.search(code) for p in DESTRUCTIVE_PATTERNS)
        has_execute = any(p.search(code) for p in EXECUTE_PATTERNS)
        has_exception = EXCEPTION_MARKER in body
        if has_destructive and has_execute and not has_exception:
            # Report the first execute line inside the chunk for precision.
            lineno = start_line
            for offset, line in enumerate(body.splitlines()):
                if line.strip().startswith("//"):
                    continue
                if any(p.search(line) for p in EXECUTE_PATTERNS):
                    lineno = start_line + offset
                    break
            findings.append(
                Finding(
                    rel,
                    lineno,
                    "destructive-execution-in-test",
                    f"function {name!r} mixes a host-global destructive literal "
                    "with a process-dispatch call; pass destructive strings only "
                    "to pure classifier functions, or add a reviewed "
                    f"`{EXCEPTION_MARKER}` comment",
                )
            )
    return findings


def check_repo() -> list[Finding]:
    findings: list[Finding] = []
    for rel in AUDITED_FILES:
        path = ROOT / rel
        if not path.exists():
            findings.append(Finding(rel, 1, "audited-file-missing", "audited file is missing"))
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as exc:
            findings.append(Finding(rel, 1, "unreadable", f"cannot read: {exc}"))
            continue
        findings.extend(check_text(rel, text))
    return findings


PASS_SNIPPET = """\
const CATASTROPHIC_ADVISORY_FIXTURES: &[&str] = &["rm -rf /", "mkfs.ext4 /dev/sda1"];
#[tokio::test]
async fn test_bash_catastrophic_strings_are_advisory_not_executed() {
    for cmd in CATASTROPHIC_ADVISORY_FIXTURES {
        assert!(find_ask_pattern(cmd).is_some());
    }
}
"""

FAIL_SNIPPET = """\
#[tokio::test]
async fn test_bash_tool_hard_deny_tier_blocks_catastrophic_commands() {
    let tool = BashTool::new();
    for cmd in ["rm -rf /", "mkfs", "shutdown"] {
        let result = tool.execute(serde_json::json!({ "command": cmd })).await;
        assert!(matches!(result, Err(ToolError::Permission(_))));
    }
}
"""

EXCEPTION_SNIPPET = """\
#[tokio::test]
async fn reviewed_case() {
    // shell-test-safety-exception: reviewed 2026-10-14, classifier-only
    // proof with a mocked dispatcher; no real process is spawned.
    let tool = BashTool::new();
    let _ = tool.execute(serde_json::json!({ "command": "rm -rf /" })).await;
}
"""


def self_test() -> int:
    failures = 0

    pass_findings = check_text("<positive-fixture>", PASS_SNIPPET)
    if pass_findings:
        print("  FAIL self-test: pure classifier fixture was rejected:")
        for f in pass_findings:
            print(f"    {f.file}:{f.line}: {f.rule}: {f.message}")
        failures += 1
    else:
        print("  ok  self-test accepts pure classifier fixture (no execute)")

    fail_findings = check_text("<negative-fixture>", FAIL_SNIPPET)
    if not any(f.rule == "destructive-execution-in-test" for f in fail_findings):
        print("  FAIL self-test: executable destructive fixture was NOT rejected")
        failures += 1
    else:
        print("  ok  self-test rejects executable destructive fixture")

    stale_findings = check_text(
        "<stale-fixture>", "const HARD_DENY_COMMANDS: &[&str] = &[\"rm -rf /\"];"
    )
    if not any(f.rule == "stale-hard-deny-contract" for f in stale_findings):
        print("  FAIL self-test: stale HARD_DENY_COMMANDS was NOT rejected")
        failures += 1
    else:
        print("  ok  self-test rejects stale hard-deny identifier")

    exc_findings = check_text("<exception-fixture>", EXCEPTION_SNIPPET)
    if exc_findings:
        print("  FAIL self-test: reviewed exception was rejected:")
        for f in exc_findings:
            print(f"    {f.file}:{f.line}: {f.rule}: {f.message}")
        failures += 1
    else:
        print("  ok  self-test honors a reviewed exception comment")

    # The real tree must also pass: this catches regressions in the audited
    # files themselves, not just the synthetic fixtures.
    repo_findings = check_repo()
    if repo_findings:
        print("  FAIL self-test: audited tree has findings:")
        for f in repo_findings:
            print(f"    {f.file}:{f.line}: {f.rule}: {f.message}")
        failures += 1
    else:
        print("  ok  self-test audited tree is clean")

    if failures:
        print(f"self-test FAILED with {failures} problem(s)")
        return 1
    print("self-test ok (pure-classifier accept + destructive reject + stale reject)")
    return 0


def main() -> int:
    args = sys.argv[1:]
    if "--self-test" in args:
        return self_test()
    findings = check_repo()
    if findings:
        print("check_shell_policy_test_safety: FAILED")
        for f in findings:
            print(f" - {f.file}:{f.line}: [{f.rule}] {f.message}")
        print(
            "\nAudited locations: " + ", ".join(AUDITED_FILES) + "\n"
            "This guard is regex-based, not a Rust AST analysis: it cannot see "
            "through aliases, macros, or cross-function value flow. Human review "
            "of every shell-policy test change is still required."
        )
        return 1
    print("check_shell_policy_test_safety: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
