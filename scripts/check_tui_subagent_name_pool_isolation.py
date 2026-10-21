#!/usr/bin/env python3
"""Static guard: the TUI may not resolve a subagent name against the pool.

Milestone C005 makes the curated pool's `profile` and `usefulness` reachable
by carrying them on `AppEvent::SubagentStarted` and copying them through the
projection onto `SubagentRow`. That is the whole design: the descriptor is
drawn **once**, at spawn, by the only code that holds the name lease, and every
surface downstream reads what the projection said.

The tempting alternative — and the one this milestone is explicitly written
against — is for a frontend to look the descriptor up itself:

    // src/tui/components/sidebar.rs
    use crate::agent::subagent_names::pool_entry;
    let profile = pool_entry(&row.display_name)?.profile;

That looks equivalent and is not. It reintroduces exactly the defect C001
already recorded once: attribution derived locally rather than carried. A
display name is not a stable key — the registry appends numeric variants
(`Sally-2`) only once every pool name is held, a name that is not in the pool
at all must render *no* descriptor rather than a guessed one, and a released
name can be re-drawn to a different subagent while a stale snapshot is still on
screen. A frontend that resolves names itself will eventually attribute one
subagent's personality to another, with nothing failing.

It also breaks the layering. `src/tui` has no curated pool and must not grow
one; the pool is agent-runtime state, and the frontend is presentation over
projected state.

So: no module under `src/tui/` may name the pool at all. Comments are exempt —
a doc comment may *explain* that the frontend deliberately does not resolve
names — but any `use`, path, or identifier reference is a violation.

Exit code 1 if a TUI module reaches for the pool.

Usage::

    python3 scripts/check_tui_subagent_name_pool_isolation.py
    python3 scripts/check_tui_subagent_name_pool_isolation.py --self-test
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SRC = REPO_ROOT / "src" / "tui"

# Identifiers that only exist to reach the curated pool. Matched as whole
# words so `subagent_names` does not also fire on prose that happens to
# contain it, and so a longer unrelated identifier cannot smuggle one through.
POOL_IDENTIFIERS = (
    "pool_entry",
    "SUBAGENT_NAMES",
    "subagent_names",
    "SubagentName",
    "SubagentNameLease",
)

POOL_PATH = re.compile(r"\bsubagent_names\b")
POOL_WORD = re.compile(r"\b(" + "|".join(POOL_IDENTIFIERS) + r")\b")
BLOCK_COMMENT = re.compile(r"/\*.*?\*/", re.DOTALL)
LINE_COMMENT = re.compile(r"//.*$")


@dataclass(frozen=True)
class Finding:
    file: Path
    line_no: int
    line_text: str


def strip_comments(source: str) -> list[str]:
    """Return `source` line-aligned with comments blanked out.

    Comment text is deliberately exempt: a doc comment that says the frontend
    must not resolve names is documenting the invariant, not breaking it. A
    guard that fired on that prose would be silenced by deleting the
    explanation, which is the opposite of what a guard is for.
    """
    without_blocks = BLOCK_COMMENT.sub(lambda m: "\n" * m.group(0).count("\n"), source)
    out = []
    for line in without_blocks.splitlines():
        out.append(LINE_COMMENT.sub("", line))
    return out


def scan_text(path: Path, lines: list[str]) -> list[Finding]:
    findings = []
    for i, line in enumerate(lines):
        if POOL_WORD.search(line):
            findings.append(Finding(path, i + 1, line.strip()))
    return findings


def collect_findings(root: Path) -> list[Finding]:
    findings: list[Finding] = []
    for path in sorted(root.rglob("*.rs")):
        try:
            source = path.read_text()
        except UnicodeDecodeError:
            continue
        findings.extend(scan_text(path, strip_comments(source)))
    return findings


SELF_TEST_VIOLATIONS = {
    "a use of the pool module": "use crate::agent::subagent_names::pool_entry;",
    "a bare lookup call": "    let p = pool_entry(name);",
    "the pool constant": "    let all = SUBAGENT_NAMES.len();",
    "the lease type": "    fn f(l: SubagentNameLease) {}",
    "an inline path in a call": "    let n = crate::agent::subagent_names::pool_entry(n);",
}

SELF_TEST_ALLOWED = (
    # Prose about the invariant is documentation, not a violation.
    "/// Deliberately does not resolve `pool_entry`: descriptors are carried.",
    "// TODO: replace with crate::agent::subagent_names::pool_entry once carried",
    "let profile: Option<&str> = None;",
)


def self_test() -> int:
    """Prove the guard fails on a planted violation and passes on prose.

    A guard that cannot be shown to fail is not evidence of anything. Each
    case is scanned exactly as a real TUI file would be.
    """
    failures = 0
    synthetic = Path("<self-test>")
    for label, line in SELF_TEST_VIOLATIONS.items():
        if scan_text(synthetic, strip_comments(line)):
            print(f"  ok    detects {label}")
        else:
            print(f"  FAIL  did not detect {label}: {line}")
            failures += 1
    for line in SELF_TEST_ALLOWED:
        if scan_text(synthetic, strip_comments(line)):
            print(f"  FAIL  false positive on an allowed line: {line}")
            failures += 1
        else:
            print("  ok    allows prose about the invariant")

    # The block-comment case has to survive comment stripping intact, so it is
    # checked as a whole source rather than a single line.
    block = "/* pool_entry */ let x = 1;\n"
    if scan_text(synthetic, strip_comments(block)):
        print("  FAIL  did not blank a block comment")
        failures += 1
    else:
        print("  ok    blanks a block comment")

    print()
    if failures:
        print(f"self-test FAILED: {failures} case(s) did not behave as required")
        return 1
    print(f"self-test passed: {len(SELF_TEST_VIOLATIONS)} violations detected, "
          f"{len(SELF_TEST_ALLOWED) + 1} allowed cases not flagged")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard fails on planted violations and passes on prose",
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    if not SRC.is_dir():
        print(f"TUI source directory not found: {SRC}", file=sys.stderr)
        return 1

    findings = collect_findings(SRC)
    if findings:
        print(
            "The TUI reached for the curated subagent name pool.\n"
            "\n"
            "Descriptors are carried: drawn once at spawn from the name lease,\n"
            "put on `AppEvent::SubagentStarted`, copied verbatim by the\n"
            "projection adapter, and read off `SubagentRow.profile`. The\n"
            "frontend resolves nothing.\n"
            "\n"
            "A display name is not a stable key — the registry appends numeric\n"
            "variants (`Sally-2`) only once every pool name is held, a non-pool\n"
            "name must render *no* descriptor rather than a guessed one, and a\n"
            "released name can be re-drawn while a stale snapshot is still on\n"
            "screen. Resolving names in the TUI is how one subagent's\n"
            "personality ends up on another's row.\n"
        )
        for f in findings:
            rel = f.file.relative_to(REPO_ROOT)
            print(f"  {rel}:{f.line_no}: {f.line_text}")
        print(f"\n{len(findings)} violation(s) found.")
        return 1

    print(
        "Subagent name-pool isolation guard passed — "
        "the TUI reads projected descriptors and never resolves a name"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())