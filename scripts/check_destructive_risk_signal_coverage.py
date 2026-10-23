#!/usr/bin/env python3
"""Static guard: the destructive-delete risk signal keeps its coverage.

ADR-0045 makes command syntax an *advisory* signal. The C002 milestone
repaired that signal in both directions: the classifier now recognises the
effective root-delete spellings (`rm -rf --no-preserve-root /`,
`rm -rf / --no-preserve-root`), and the built-in human-shell block no longer
hard-blocks a legitimate scoped absolute path such as `rm -rf /tmp/build`.

This guard keeps both properties from regressing:

- **fixture-coverage** — the destructive-classification fixture set is
  covered by a test member of the `security_permission` family, and that
  member asserts the classification's *verdict* (the canonical recogniser and
  the human-shell decision), not a pattern name.
- **scoped-absolute-path** — no built-in regular expression in the human-shell
  policy (or in the bash policy) matches a scoped removal. A built-in pattern
  that matches `rm -rf /tmp/build` is exactly the obstruction ADR-0045
  identifies: it teaches the caller to reword.
- **effective-spelling** — a built-in pattern that recognises the bare
  `rm -rf /` must recognise the effective spellings too. A pattern that only
  sees the bare form is the flag-anchored shape C002 removed.
- **single-recogniser** — the tool-side classifier, the bash-policy seam and
  the human-shell block all route through one canonical recogniser, so they
  cannot disagree about the same spelling.
- **no-guarantee-claim** — the human-shell refusal says it is a best-effort
  text rule and not a security boundary, and it does not describe the
  command's effect as a guaranteed catastrophe.

The Rust parsing is done by `scripts/rust_test_scan.py`, the shared Rust-aware
scanner: a plain regex scan of Rust misreads lifetimes as character literals
and cannot tell a display name from a pattern.

Run:
  python3 scripts/check_destructive_risk_signal_coverage.py
  python3 scripts/check_destructive_risk_signal_coverage.py --self-test

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import re
import shutil
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import rust_test_scan  # noqa: E402  (path is set above)

REPO_ROOT = Path(__file__).resolve().parent.parent

FAMILY_DIR = "tests/security_permission"
FAMILY_MOD = "tests/security_permission/mod.rs"
DESTRUCTIVE_MODULE = "src/tool/destructive.rs"
BASH_POLICY = "src/tool/bash/policy.rs"
SHELL_POLICY = "src/shell/policy.rs"

# The fixture set the C002 plan names. Every row must be covered.
REQUIRED_SPELLINGS = (
    "rm -rf /",
    "rm -rf --no-preserve-root /",
    "rm -rf / --no-preserve-root",
    "rm -fr /",
    "rm -rf /*",
    "rm -rf ~",
    "rm -rf $HOME",
    "rm -rf /tmp/build",
    "rm -rf ./build",
    "rm -rf /home/user/project/cache",
    'rm -rf "$TARGET"',
)

# The scoped *absolute* removals no built-in pattern may match. A relative
# path is deliberately not in this set: the human shell's current-directory
# warning legitimately covers `rm -rf .`, and that warning is not the
# root-delete obstruction C002 removed.
SCOPED_REMOVALS = (
    "rm -rf /tmp/build",
    "rm -rf /home/user/project/cache",
)

# The spellings coreutils actually executes against the filesystem root.
EFFECTIVE_ROOT_SPELLINGS = (
    "rm -rf --no-preserve-root /",
    "rm -rf / --no-preserve-root",
)

# The bare form coreutils refuses; kept as an explicit signal.
BARE_ROOT_SPELLING = "rm -rf /"

# The recogniser the three owners share.
CANONICAL_RECOGNISER = "classify_root_delete"

# A literal is treated as a regular expression only when it carries a regex
# escape. A display name ("rm -rf /") is plain text: it is a label, not a
# matcher, and matching it against a scoped removal would be meaningless.
REGEX_ESCAPE = re.compile(r"\\")

# The reason text the human-shell refusal must carry.
BOUNDARY_LABEL = "not a security boundary"
GUARANTEE_WORD = "catastrophic"


@dataclass
class Finding:
    file: str
    rule: str
    message: str


def _unescape(content: str) -> str:
    """The runtime value of a Rust string literal's source text."""
    return content.replace('\\"', '"').replace("\\\\", "\\")


def _literals(text: str) -> list[tuple[int, str]]:
    """(offset, runtime value) for every string literal in `text`."""
    scan = rust_test_scan.scan_source(text)
    return [(lit.start, _unescape(lit.content)) for lit in scan.literals]


def _pattern_literals(text: str) -> list[tuple[int, str]]:
    """The literals that carry a regular expression, not a display name."""
    return [(offset, value) for offset, value in _literals(text) if REGEX_ESCAPE.search(value)]


def _matches(pattern: str, text: str) -> bool | None:
    """Whether `pattern` matches `text`; None when it is not Python-compatible.

    Rust's regex crate and Python's `re` agree on the constructs this
    repository's patterns use. A literal that does not compile here is
    skipped rather than reported, and the caller prints it so a reviewer sees
    that the guard did not evaluate it.
    """
    try:
        return re.search(pattern, text) is not None
    except re.error:
        return None


def _read(root: Path, rel: str) -> str | None:
    path = root / rel
    if not path.is_file():
        return None
    return path.read_text(encoding="utf-8")


def _family_member_files(root: Path) -> list[str]:
    """The `.inc` members the `security_permission` family binds."""
    text = _read(root, FAMILY_MOD)
    if text is None:
        return []
    members = []
    for _, value in _literals(text):
        if value.endswith(".inc"):
            members.append(f"{FAMILY_DIR}/{value}")
    return members


def _covering_members(root: Path) -> list[str]:
    """The members that cover every required spelling."""
    covering: list[str] = []
    for rel in _family_member_files(root):
        text = _read(root, rel)
        if text is None:
            continue
        present = {value for _, value in _literals(text)}
        if all(row in present for row in REQUIRED_SPELLINGS):
            covering.append(rel)
    return covering


def check_tree(root: Path) -> list[Finding]:
    findings: list[Finding] = []

    covering = _covering_members(root)
    if not covering:
        findings.append(
            Finding(
                FAMILY_DIR,
                "fixture-coverage",
                "no test member of the security_permission family covers the "
                f"destructive-classification fixture set ({len(REQUIRED_SPELLINGS)} "
                "rows); the classifier's verdict is unproven",
            )
        )
    else:
        bound = _read(root, FAMILY_MOD) or ""
        for rel in covering:
            member = _read(root, rel) or ""
            name = Path(rel).name
            if f'"{name}"' not in bound:
                findings.append(
                    Finding(
                        rel,
                        "member-not-bound",
                        "the covering member is not bound in the family module, so "
                        "it never compiles into the family target",
                    )
                )
            for identifier in (CANONICAL_RECOGNISER, "evaluate_command"):
                if identifier not in member:
                    findings.append(
                        Finding(
                            rel,
                            "verdict-level-assertion",
                            f"the covering member must assert the classification's "
                            f"verdict through {identifier!r}, not against a pattern name",
                        )
                    )

    destructive = _read(root, DESTRUCTIVE_MODULE)
    if destructive is None:
        findings.append(
            Finding(DESTRUCTIVE_MODULE, "missing-module", "the destructive module is missing")
        )
    else:
        if f"pub fn {CANONICAL_RECOGNISER}" not in destructive:
            findings.append(
                Finding(
                    DESTRUCTIVE_MODULE,
                    "single-recogniser",
                    f"the canonical recogniser {CANONICAL_RECOGNISER!r} is missing; the "
                    "root-delete classification must have exactly one owner",
                )
            )
        for offset, pattern in _pattern_literals(destructive):
            bare = _matches(pattern, BARE_ROOT_SPELLING)
            if bare is None:
                continue
            if not bare:
                continue
            for spelling in EFFECTIVE_ROOT_SPELLINGS:
                matched = _matches(pattern, spelling)
                if matched is None:
                    continue
                if not matched:
                    findings.append(
                        Finding(
                            DESTRUCTIVE_MODULE,
                            "effective-spelling",
                            f"a built-in pattern recognises {BARE_ROOT_SPELLING!r} but not "
                            f"{spelling!r}; the flag-anchored shape C002 removed is back",
                        )
                    )

    for rel in (BASH_POLICY, SHELL_POLICY):
        text = _read(root, rel)
        if text is None:
            findings.append(Finding(rel, "missing-module", "the policy module is missing"))
            continue
        if CANONICAL_RECOGNISER not in text:
            findings.append(
                Finding(
                    rel,
                    "single-recogniser",
                    f"the policy does not route through the canonical recogniser "
                    f"{CANONICAL_RECOGNISER!r}; it may own a second, differently-bypassed "
                    "matcher",
                )
            )
        for _, pattern in _pattern_literals(text):
            for scoped in SCOPED_REMOVALS:
                if _matches(pattern, scoped):
                    findings.append(
                        Finding(
                            rel,
                            "scoped-absolute-path",
                            f"a built-in pattern matches the scoped removal {scoped!r}; "
                            "a legitimate scoped delete must not be treated as a root delete",
                        )
                    )
                    break

    shell_text = _read(root, SHELL_POLICY)
    if shell_text is not None:
        values = [value for _, value in _literals(shell_text)]
        if not any(BOUNDARY_LABEL in value for value in values):
            findings.append(
                Finding(
                    SHELL_POLICY,
                    "no-guarantee-claim",
                    "the human-shell refusal must label itself as a best-effort text "
                    f"rule ({BOUNDARY_LABEL!r}); it must not read as a guarantee",
                )
            )
        for value in values:
            if GUARANTEE_WORD in value:
                findings.append(
                    Finding(
                        SHELL_POLICY,
                        "no-guarantee-claim",
                        f"the human-shell refusal still claims {GUARANTEE_WORD!r}; the "
                        "classifier cannot promise that effect is impossible",
                    )
                )
                break

    return findings


def _copy_tree(root: Path, destination: Path) -> None:
    for rel in (
        FAMILY_MOD,
        DESTRUCTIVE_MODULE,
        BASH_POLICY,
        SHELL_POLICY,
    ):
        source = root / rel
        target = destination / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    for member in _family_member_files(root):
        source = root / member
        target = destination / member
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)


def self_test() -> int:
    failures = 0

    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        _copy_tree(REPO_ROOT, root)

        clean = check_tree(root)
        if clean:
            print("  FAIL self-test: the unmodified tree has findings:")
            for finding in clean:
                print(f"    {finding.file}: [{finding.rule}] {finding.message}")
            failures += 1
        else:
            print("  ok  self-test accepts the unmodified tree")

        # Regression 1: the human-shell policy grows a built-in pattern that
        # matches a scoped absolute path again.
        shell = root / SHELL_POLICY
        text = shell.read_text(encoding="utf-8")
        injected = (
            "static BLOCK_RM_RF_ROOT: LazyLock<Regex> = LazyLock::new(|| "
            "Regex::new(r\"rm\\s+-[a-zA-Z]*r\\s*-?[a-zA-Z]*f\\s*-?[a-zA-Z]*\\s+/\").unwrap());\n"
        )
        shell.write_text(injected + text, encoding="utf-8")
        findings = check_tree(root)
        if not any(finding.rule == "scoped-absolute-path" for finding in findings):
            print("  FAIL self-test: a scoped-absolute-path pattern was NOT rejected")
            failures += 1
        else:
            print("  ok  self-test rejects a human-shell pattern matching a scoped path")
        shell.write_text(text, encoding="utf-8")

        # Regression 2: the fixture set loses one effective spelling.
        member = root / f"{FAMILY_DIR}/destructive_delete_classification.inc"
        original = member.read_text(encoding="utf-8")
        member.write_text(
            original.replace("rm -rf --no-preserve-root /", "rm -rf --preserve-root /"),
            encoding="utf-8",
        )
        findings = check_tree(root)
        if not any(finding.rule == "fixture-coverage" for finding in findings):
            print("  FAIL self-test: an uncovered fixture set was NOT rejected")
            failures += 1
        else:
            print("  ok  self-test rejects an uncovered fixture set")
        member.write_text(original, encoding="utf-8")

        # Regression 3: the flag-anchored root pattern comes back.
        destructive = root / DESTRUCTIVE_MODULE
        original = destructive.read_text(encoding="utf-8")
        destructive.write_text(
            original.replace(
                "const DESTRUCTIVE_BASH_PATTERNS: &[(&str, &str)] = &[",
                "const DESTRUCTIVE_BASH_PATTERNS: &[(&str, &str)] = &[\n"
                '    ("rm -rf /", r"rm\\s+(-[a-zA-Z]*[rR][fF]|-rf|-fr)\\s+/\\s*$"),',
                1,
            ),
            encoding="utf-8",
        )
        findings = check_tree(root)
        if not any(finding.rule == "effective-spelling" for finding in findings):
            print("  FAIL self-test: the flag-anchored root pattern was NOT rejected")
            failures += 1
        else:
            print("  ok  self-test rejects the flag-anchored root pattern")
        destructive.write_text(original, encoding="utf-8")

    if failures:
        print(f"self-test FAILED with {failures} problem(s)")
        return 1
    print("self-test ok (coverage + scoped-path + effective-spelling regressions)")
    return 0


def main() -> int:
    args = sys.argv[1:]
    if "--self-test" in args:
        return self_test()
    findings = check_tree(REPO_ROOT)
    if findings:
        print("check_destructive_risk_signal_coverage: FAILED")
        for finding in findings:
            print(f" - {finding.file}: [{finding.rule}] {finding.message}")
        return 1
    print("check_destructive_risk_signal_coverage: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
