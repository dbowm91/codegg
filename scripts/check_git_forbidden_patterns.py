#!/usr/bin/env python3
"""
Forbidden-pattern static checks for the Git agent integration.

Scans the codebase for patterns that are explicitly disallowed per the
polish/maintainability/verification plan (Workstream E2):

  * Secret-bearing types that accidentally derive `Debug` / `Serialize`
    with raw-form exposure.
  * Direct `expose_secret()` calls outside the rendering boundary.
  * RunStore persistence using unsanitized argv.
  * Duplicated env-policy tables (must live in `egggit::process`).
  * Direct literal `git` process construction outside the generic owner.
  * `RerunDescriptor { argv: <Vec<String>> }` literals (must use AuditSafeArgv).
  * Hostile Git metadata that could reach code execution through the
    trusted read route: a dangerous configuration key that is not
    neutralised, or that is carried in a form that lets it run.
  * Hostile Git metadata mediation that is declared but never invoked:
    the trusted route must CALL the bounded hostile-config detector from
    every child-construction path — the async one and the sync one. A
    comment that names the detector is documentation, not a call, and
    asserting only one of the two call sites would let the other be
    deleted while this guard stayed green.

Exits 0 when all checks pass, 1 otherwise. Each finding includes the
file, line, and a one-line rationale.

Usage:
    python3 scripts/check_git_forbidden_patterns.py             # scan the tree
    python3 scripts/check_git_forbidden_patterns.py --self-test  # non-vacuity
    python3 scripts/check_git_forbidden_patterns.py --help

`--self-test` copies the trusted Git route into a temporary directory,
injects one defect at a time, and asserts that each injected defect is
rejected. A guard that cannot be shown to fail on an injected defect is
not accepted as evidence, so the self-test fails loudly (exit 1) when an
injected defect slips through. Every injection is reported by name, so
the self-test's own output says which defect it exercised.

The script is deliberately strict — every false positive should be
either fixed or added to the explicit allowlist at the bottom of this
file with a rationale.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Iterable, List, Optional, Tuple


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


def _relative(path: Path, root: Path) -> str:
    """Path relative to `root`, or the absolute path when it is outside it.

    The self-test runs the checks against a staged copy of the sources in a
    temporary directory, so a finding cannot always be expressed relative to
    the repository root.
    """
    try:
        return str(path.relative_to(root))
    except ValueError:
        return str(path)


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
    egggit::process. Detect hand-maintained copies."""
    findings: List[Finding] = []
    # Heuristic: a hand-maintained copy is a file that defines one of the
    # canonical lists inline. We look for both `const ALLOWED_ENV_VARS`
    # and `const ALWAYS_STRIPPED_ENV_VARS` (or close variants) outside
    # the canonical module.
    canonical_paths = {
        str((CRATES / "egggit" / "src" / "process.rs").relative_to(ROOT)),
        str((CRATES / "codegg-git" / "src" / "process_policy.rs").relative_to(ROOT)),
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
                    "Use egggit::process constants instead."
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


def check_direct_git_process_construction() -> List[Finding]:
    """Only the generic owner may construct a literal `git` command.

    Test fixtures are excluded because they need to create temporary
    repositories; production callers must use `egggit::process` or a typed
    workflow service.
    """
    findings: List[Finding] = []
    owner = "crates/egggit/src/process.rs"
    pattern = r"(?:Command::new|StdCommand::new|process::Command::new)\(\s*\"git\"\s*\)"
    for path, lineno, content in rg(pattern, ROOT, include="*.rs"):
        path_str = str(path.relative_to(ROOT))
        if (
            path_str == owner
            or path_str.startswith("tests/")
            or "/tests/" in path_str
            or _in_test_module(path, lineno)
        ):
            continue
        if content.lstrip().startswith("//"):
            continue
        findings.append(
            Finding(
                rule="direct-git-process-construction",
                file=path_str,
                line=lineno,
                message="literal git process construction must use egggit::process",
            )
        )
    return findings


def check_trusted_git_execution_policy(root: Optional[Path] = None) -> List[Finding]:
    """Keep typed Git capability classification at the sole process owner."""
    findings: List[Finding] = []
    root = ROOT if root is None else root
    path = root / "crates" / "egggit" / "src" / "process.rs"
    if not path.exists():
        return [
            Finding(
                rule="trusted-git-policy-missing",
                file="crates/egggit/src/process.rs",
                line=1,
                message="canonical Git process owner is missing",
            )
        ]
    text = path.read_text()
    required = [
        "classify_git_request(argv)",
        "GitExecutionPolicyOutcome",
        "RequiresContainedExecution",
        "add_trusted_git_overrides(&mut cmd, argv, &worktree)",
        'cmd.arg(format!("--work-tree={}", worktree.display()))',
        'cmd.env("GIT_CONFIG_NOSYSTEM", "1".into())',
        "pub fn apply(",
        "pub fn apply_sync(",
        '"core.fsmonitor"',
        '"core.hooksPath"',
        '"diff.external"',
        '"credential.helper"',
        '"protocol.ext.allow"',
    ]
    missing = [token for token in required if token not in text]
    if missing:
        findings.append(
            Finding(
                rule="trusted-git-policy-incomplete",
                file=_relative(path, root),
                line=1,
                message=f"canonical trusted Git policy is missing controls: {', '.join(missing)}",
            )
        )
    forbidden = [
        "DENIED_GIT_EXECUTABLE",
        "trusted_git_request_denied",
        "REPOSITORY_EXECUTABLE_OPERATIONS",
        "TRUSTED_GIT_BUILTINS",
    ]
    present = [token for token in forbidden if token in text]
    if present:
        findings.append(
            Finding(
                rule="fabricated-or-name-banned-git-policy",
                file=_relative(path, root),
                line=1,
                message=f"Git policy must use typed capability outcomes, not: {', '.join(present)}",
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


# ── Hostile Git metadata authority ────────────────────────────────────────────
#
# The trusted read route (`egggit::process`) is the only place a CodeGG-owned
# Git child is constructed. Repository- and user-controlled Git configuration
# can name programs — `core.hooksPath`, `filter.*.process`, `merge.*.driver`,
# `url.*.insteadOf`, `core.sshCommand`, `credential.*.helper` — so the route
# has to neutralise each of those surfaces before the child runs.
#
# The neutralisation a key needs depends on the key:
#
#   * an explicit `-c key=value` override whose value cannot execute
#     (`core.hooksPath=/dev/null`, `credential.helper=`, `diff.external=`);
#   * an environment pin, for the keys Git consults *before* the matching
#     configuration key (`GIT_EDITOR` beats `core.editor`, `GIT_SEQUENCE_EDITOR`
#     beats `sequence.editor`);
#   * or a route denial, for the keys that only matter on operations the trusted
#     route refuses outright (`core.attributesFile` selects clean/smudge filters
#     and merge drivers, and the route refuses those operations;
#     `core.alternateRefsCommand` and `core.gitProxy` are only read by remote
#     operations, and the route refuses those too).
#
# A key that is neither neutralised nor unreachable lets the repository's own
# configuration win. That is the defect this check exists to catch.
#
# The accepted values are the ones the trusted route actually uses today
# (`crates/egggit/src/process.rs`, `add_trusted_git_overrides`).


@dataclass(frozen=True)
class HostileGitConfigKey:
    """A dangerous Git configuration key and how the route must neutralise it."""

    key: str
    why: str
    override_values: Tuple[str, ...]
    env_pins: Tuple[str, ...] = ()
    denial_patterns: Tuple[str, ...] = ()
    denial_why: str = ""


# The remote operations the trusted route refuses. A key that is only read by
# remote operations is unreachable while that refusal stands.
_REMOTE_ROUTE_DENIAL: Tuple[str, ...] = (
    r'"fetch"\s*\|\s*"pull"\s*\|\s*"push"\s*\|\s*"clone"\s*\|\s*"ls-remote"',
    r"return remote_git_requires_containment\(\);",
)

# The operations the trusted route refuses because attributes, filters and
# merge drivers are consulted there, and the read routes that disable the
# external diff/textconv drivers an attributes file can select.
_ATTRIBUTES_ROUTE_DENIAL: Tuple[str, ...] = (
    r'"add"\s*=>\s*vec!\[\s*GitExecutionCapability::CleanSmudgeFilter\s*\]',
    r'"checkout"\s*\|\s*"restore"\s*\|\s*"switch"\s*=>\s*\{\s*vec!\['
    r"\s*GitExecutionCapability::CleanSmudgeFilter\s*\]",
    r'"merge"\s*\|\s*"cherry-pick"\s*\|\s*"rebase"\s*\|\s*"revert"\s*=>\s*vec!\['
    r"\s*GitExecutionCapability::CleanSmudgeFilter,\s*"
    r"GitExecutionCapability::MergeDriver,",
    r'cmd\.arg\("--no-ext-diff"\);',
    r'cmd\.arg\("--no-textconv"\);',
)

# Independent obligations of the trusted read route.
#
# These are asserted separately from the key overrides above, because a key
# override cannot stand in for a route refusal and a route refusal cannot
# stand in for a key override: they defend against different surfaces. An
# override for `core.attributesFile` does not stop `git status` from running
# a `filter.<name>.process` the repository declares, and refusing the
# filter-bearing operations does not stop a repository-configured diff driver
# from running on a read. Each obligation below is therefore a protection in
# its own right, and removing any one of them is a finding.
@dataclass(frozen=True)
class RouteObligation:
    """One protection the trusted read route must carry."""

    name: str
    why: str
    patterns: Tuple[str, ...]
    scope: str = "the trusted Git route"


ROUTE_OBLIGATIONS: Tuple[RouteObligation, ...] = (
    RouteObligation(
        name="external diff drivers",
        why="a repository-configured diff driver must not run on a read",
        patterns=(r'cmd\.arg\("--no-ext-diff"\);',),
    ),
    RouteObligation(
        name="textconv drivers",
        why="a repository-configured textconv driver must not run on a read",
        patterns=(r'cmd\.arg\("--no-textconv"\);',),
    ),
    RouteObligation(
        name="remote operations",
        why=(
            "a repository-configured URL rewrite, per-URL credential helper or "
            "Git proxy is only reachable through a remote operation"
        ),
        patterns=_REMOTE_ROUTE_DENIAL,
    ),
    RouteObligation(
        name="filter-bearing operations",
        why=(
            "a repository-configured clean/smudge filter or merge driver is "
            "consulted by the operations that materialise content"
        ),
        patterns=_ATTRIBUTES_ROUTE_DENIAL[:3],
    ),
    RouteObligation(
        name="replace refs",
        why=(
            "a repository-declared `refs/replace/*` makes Git substitute one "
            "object for another, so a read that honoured it would not "
            "describe the generation the repository holds"
        ),
        patterns=(r'cmd\.env\("GIT_NO_REPLACE_OBJECTS", "1"\.into\(\)\);',),
    ),
)

HOSTILE_GIT_CONFIG_KEYS: Tuple[HostileGitConfigKey, ...] = (
    HostileGitConfigKey(
        key="core.fsmonitor",
        why="the repository's fsmonitor hook must not run",
        override_values=("false",),
    ),
    HostileGitConfigKey(
        key="core.hooksPath",
        why="configured and traditional hooks must not run",
        override_values=("/dev/null",),
    ),
    HostileGitConfigKey(
        key="core.pager",
        why="repository-configured pagers must not run",
        override_values=("cat",),
    ),
    HostileGitConfigKey(
        key="core.editor",
        why="repository-configured editors must not run",
        override_values=("true",),
        env_pins=('cmd.env("GIT_EDITOR", "true".into())',),
    ),
    HostileGitConfigKey(
        key="sequence.editor",
        why="repository-configured sequence editors must not run",
        override_values=("true",),
        env_pins=('cmd.env("GIT_SEQUENCE_EDITOR", "true".into())',),
    ),
    HostileGitConfigKey(
        key="core.sshCommand",
        why="a repository-configured ssh command must not run",
        override_values=("",),
    ),
    HostileGitConfigKey(
        key="core.gitProxy",
        why="a repository-configured git proxy command must not run",
        override_values=("",),
        denial_patterns=_REMOTE_ROUTE_DENIAL,
        denial_why="the trusted route refuses every remote operation to containment",
    ),
    HostileGitConfigKey(
        key="core.askPass",
        why="a repository-configured askpass program must not run",
        override_values=("",),
    ),
    HostileGitConfigKey(
        key="core.attributesFile",
        why="a user-level attributes file must not be consulted",
        override_values=("/dev/null", ""),
        denial_patterns=_ATTRIBUTES_ROUTE_DENIAL,
        denial_why=(
            "attributes select clean/smudge filters and merge drivers, so the trusted "
            "route refuses the filter-bearing operations and disables external "
            "diff/textconv drivers on the read routes"
        ),
    ),
    HostileGitConfigKey(
        key="credential.helper",
        why="credential helpers must not run",
        override_values=("",),
    ),
    HostileGitConfigKey(
        key="diff.external",
        why="external diff drivers must not run",
        override_values=("",),
    ),
    HostileGitConfigKey(
        key="protocol.ext.allow",
        why="`ext::` remote helpers must not run",
        override_values=("never",),
    ),
    HostileGitConfigKey(
        key="core.alternateRefsCommand",
        why="a repository-configured alternate-refs command must not run",
        override_values=("",),
        denial_patterns=_REMOTE_ROUTE_DENIAL,
        denial_why="the trusted route refuses every remote operation to containment",
    ),
    HostileGitConfigKey(
        key="gpg.program",
        why="a repository-configured signing program must not run",
        override_values=("",),
    ),
    HostileGitConfigKey(
        key="commit.gpgSign",
        why="a commit must not be signed with a repository-selected program",
        override_values=("false",),
    ),
    HostileGitConfigKey(
        key="tag.gpgSign",
        why="a tag must not be signed with a repository-selected program",
        override_values=("false",),
    ),
)

# Dangerous configuration families. A key from one of these families must
# never be carried by the trusted override table with a value that can run:
# the route does not select filters, merge drivers, URL rewrites, per-URL
# credential helpers, custom diff drivers, per-command pagers or aliases.
#
# These templates mirror the surfaces the bounded hostile-config detector
# reports (`crates/egggit/src/config_surface.rs`,
# `HostileConfigSurface`). The detector names what a repository declares;
# this guard names what the trusted route may not carry. Keep the two lists
# in sync when a surface is added.
HOSTILE_GIT_CONFIG_FAMILIES: Tuple[Tuple[str, str], ...] = (
    (r"^filter\..+\.(process|clean|smudge)$", "clean/smudge filter commands"),
    (r"^merge\..+\.driver$", "merge drivers"),
    (r"^url\..+\.(insteadOf|pushInsteadOf)$", "URL rewrite rules"),
    (r"^credential\..+\.helper$", "per-URL credential helpers"),
    (r"^diff\..+\.(command|textconv)$", "custom diff drivers"),
    (r"^pager\..+$", "per-command pagers"),
    (r"^alias\..+$", "repository aliases"),
)

_HOSTILE_KEY_NAMES = frozenset(spec.key for spec in HOSTILE_GIT_CONFIG_KEYS)

_OVERRIDE_ENTRY_RE = re.compile(r'\(\s*"((?:[^"\\]|\\.)*)"\s*,\s*"((?:[^"\\]|\\.)*)"\s*\)')
_STRING_LITERAL_RE = re.compile(r'"((?:[^"\\]|\\.)*)"')
_CONFIG_KEY_RE = re.compile(r"^[a-z][a-z0-9-]*(\.[A-Za-z0-9_.-]+)+$")

# The bounded hostile-config detector, and the trusted-route methods that must
# invoke it.
#
# The detector is the mediation that refuses a repository which declares a
# configuration surface the trusted route cannot neutralise. Declaring it is
# not the same as calling it: a route that builds a Git child without ever
# calling the detector builds a child against an unbounded repository
# configuration, and every other check in this file still passes — the
# override table is still pinned, the dangerous keys are still absent from
# it. So the *call* is asserted here, in both child-construction paths.
#
# The pattern matches a call, not a mention: the trailing `(` is required, the
# name may not be part of a longer identifier, and the body it is searched in
# has its line comments stripped first, so a comment that names the detector
# cannot satisfy the obligation.
DETECTOR_NAME = "ensure_repository_config_surface_is_contained"
DETECTOR_CALL_RE = re.compile(rf'(?<![\w"]){re.escape(DETECTOR_NAME)}\s*\(')
DETECTOR_ROUTE_METHODS: Tuple[str, ...] = ("apply", "apply_sync")


def _strip_line_comments(text: str) -> str:
    """Remove `//` comments while leaving string literals intact."""
    out: List[str] = []
    in_string = False
    escaped = False
    index = 0
    while index < len(text):
        char = text[index]
        if in_string:
            out.append(char)
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            index += 1
            continue
        if char == '"':
            in_string = True
            out.append(char)
            index += 1
            continue
        if char == "/" and index + 1 < len(text) and text[index + 1] == "/":
            while index < len(text) and text[index] != "\n":
                index += 1
            continue
        out.append(char)
        index += 1
    return "".join(out)


def _function_body(text: str, name: str) -> Optional[str]:
    """Return the body of a top-level Rust function, or None if it is absent.

    The body opens on the first line that ends with `{` and closes on the
    first line that is exactly `}` at column 0, which is how a top-level
    item ends in this crate. Returning None is a finding, never a pass: a
    route the guard cannot read is a route it cannot vouch for.
    """
    match = re.search(
        rf"^(?:pub(?:\([^)]*\))?\s+)?fn {re.escape(name)}\b", text, re.MULTILINE
    )
    if match is None:
        return None
    position = match.start()
    open_at: Optional[int] = None
    while position < len(text):
        end_of_line = text.find("\n", position)
        if end_of_line == -1:
            end_of_line = len(text)
        if text[position:end_of_line].rstrip().endswith("{"):
            open_at = end_of_line
            break
        position = end_of_line + 1
    if open_at is None:
        return None
    close = re.search(r"^\}$", text[open_at:], re.MULTILINE)
    if close is None:
        return None
    return text[open_at : open_at + close.end()]


def _impl_block(text: str, name: str) -> Optional[Tuple[str, int]]:
    """Return the body of `impl <name> { ... }` and the line it starts on.

    The body opens on the `impl` line and closes on the first line that is
    exactly `}` at column 0, which is how a top-level item ends in this
    crate. Returning None is a finding, never a pass: a route the guard
    cannot read is a route it cannot vouch for.
    """
    match = re.search(rf"^impl {re.escape(name)}\s*\{{", text, re.MULTILINE)
    if match is None:
        return None
    open_at = text.find("\n", match.start())
    if open_at == -1:
        return None
    close = re.search(r"^\}$", text[open_at:], re.MULTILINE)
    if close is None:
        return None
    body = text[match.start() : open_at + close.end()]
    line = text.count("\n", 0, match.start()) + 1
    return body, line


def _method_body(block: str, name: str) -> Optional[Tuple[str, int]]:
    """Return the body of a method inside an impl block, and its line.

    The method is located by its `fn` line, and its body ends on the first
    line that closes it: the same indentation as the `fn` line, holding a
    single `}`. Returning None is a finding, never a pass.
    """
    match = re.search(
        rf"^\s*(?:pub(?:\([^)]*\))?\s+)?fn {re.escape(name)}\s*\(",
        block,
        re.MULTILINE,
    )
    if match is None:
        return None
    line_start = match.start()
    indent_match = re.match(r"[ \t]*", block[line_start:])
    indent = len(indent_match.group(0)) if indent_match else 0
    close_pattern = "^" + re.escape(" " * indent) + r"\}\s*$"
    close = re.search(close_pattern, block[match.end() :], re.MULTILINE)
    if close is None:
        return None
    body = block[line_start : match.end() + close.end()]
    line = block.count("\n", 0, line_start) + 1
    return body, line


def _override_table_span(body: str) -> Optional[Tuple[int, int]]:
    """Locate the `for (key, value) in [ ... ]` override table in a body."""
    match = re.search(r"for\s*\(\s*key\s*,\s*value\s*\)\s*in\s*\[", body)
    if match is None:
        return None
    end = body.find("]", match.end())
    if end == -1:
        return None
    return match.end(), end


def _is_hostile_config_key(literal: str) -> bool:
    """True when a string literal names a dangerous Git configuration key.

    The literal may be a bare key (`core.hooksPath`) or a `key=value`
    override argument (`core.hooksPath=.githooks`).
    """
    key = literal.split("=", 1)[0].strip()
    if key in _HOSTILE_KEY_NAMES:
        return True
    return any(re.match(pattern, key) for pattern, _ in HOSTILE_GIT_CONFIG_FAMILIES)


def check_hostile_git_metadata_authority(root: Optional[Path] = None) -> List[Finding]:
    """Repository-controlled Git metadata must not reach code execution.

    The trusted read route is the only place a CodeGG-owned Git child is
    constructed, so this check asserts four things about it:

      1. every dangerous configuration key is neutralised — by a pinned
         `-c key=value` override, by an environment pin Git prefers over the
         configuration key, or by a route denial for the operations that
         would consult it;
      2. no dangerous key is carried by the trusted override table in a form
         that can execute, no dangerous configuration family is carried at
         all, and no dangerous key is smuggled into the route beside the
         table;
      3. the `GIT_CONFIG_COUNT` injection path carries only the commit
         identity keys, so a dangerous key cannot be smuggled through the
         environment instead of the override table;
      4. the bounded hostile-config detector is actually *called* from every
         trusted child-construction path — the async one and the sync one.
         A route that stops calling it keeps every override and every
         environment pin, so no other check in this file can see the loss.
    """
    root = ROOT if root is None else root
    rel = "crates/egggit/src/process.rs"
    path = root / "crates" / "egggit" / "src" / "process.rs"
    findings: List[Finding] = []

    if not path.is_file():
        return [
            Finding(
                rule="hostile-git-metadata-source-missing",
                file=rel,
                line=1,
                message=(
                    "canonical Git process owner is missing; hostile metadata authority "
                    "cannot be verified"
                ),
            )
        ]
    text = path.read_text()
    if not text.strip():
        return [
            Finding(
                rule="hostile-git-metadata-source-empty",
                file=rel,
                line=1,
                message=(
                    "canonical Git process owner is empty; hostile metadata authority "
                    "cannot be verified"
                ),
            )
        ]

    overrides = _function_body(text, "add_trusted_git_overrides")
    environment = _function_body(text, "apply_environment")
    identity = _function_body(text, "apply_commit_identity")
    classification = _function_body(text, "classify_git_request")
    for name, body in (
        ("add_trusted_git_overrides", overrides),
        ("apply_environment", environment),
        ("apply_commit_identity", identity),
        ("classify_git_request", classification),
    ):
        if body is None:
            findings.append(
                Finding(
                    rule="hostile-git-metadata-route-missing",
                    file=rel,
                    line=1,
                    message=f"trusted Git route function {name}() could not be located",
                )
            )
    if findings:
        return findings

    # Match against code only: a comment that names a dangerous key is
    # documentation, not a route.
    overrides_code = _strip_line_comments(overrides or "")
    environment_code = _strip_line_comments(environment or "")
    identity_code = _strip_line_comments(identity or "")
    classification_code = _strip_line_comments(classification or "")
    route_text = "\n".join((overrides_code, environment_code, classification_code))

    span = _override_table_span(overrides_code)
    entries: List[Tuple[str, str]] = []
    if span is None:
        findings.append(
            Finding(
                rule="hostile-git-metadata-override-table-missing",
                file=rel,
                line=1,
                message="trusted Git override table could not be located",
            )
        )
    else:
        start, end = span
        entries = _OVERRIDE_ENTRY_RE.findall(overrides_code[start:end])

    # 1. Every dangerous key must be neutralised on the trusted read route.
    for spec in HOSTILE_GIT_CONFIG_KEYS:
        carried = [(key, value) for key, value in entries if key == spec.key]
        if carried:
            # Git applies `-c` options in the order they appear, so the LAST
            # occurrence of a key is the one that takes effect. Checking
            # "any occurrence is neutralising" would let a later hostile
            # value silently win over an earlier safe one — which is exactly
            # the shape of an accidental regression.
            effective_key, effective_value = carried[-1]
            if effective_value not in spec.override_values:
                findings.append(
                    Finding(
                        rule="hostile-git-metadata-unsafe-override",
                        file=rel,
                        line=1,
                        message=(
                            f"trusted Git override table carries {spec.key}="
                            f"{', '.join(repr(value) for _, value in carried)} — {spec.why}"
                        ),
                    )
                )
            elif len(carried) > 1:
                # A duplicated key is a finding even when the surviving value
                # is the safe one: the table then depends on the order of
                # two entries that both name the same surface, which is
                # exactly the fragility that lets a later edit reintroduce
                # the dangerous value without changing the check.
                findings.append(
                    Finding(
                        rule="hostile-git-metadata-unsafe-override",
                        file=rel,
                        line=1,
                        message=(
                            f"trusted Git override table names {spec.key} "
                            f"{len(carried)} times "
                            f"({', '.join(repr(value) for _, value in carried)}) — {spec.why}. "
                            "A dangerous key must be named exactly once."
                        ),
                    )
                )
            continue
        if spec.env_pins and any(pin in environment_code for pin in spec.env_pins):
            continue
        if spec.denial_patterns and all(
            re.search(pattern, route_text) for pattern in spec.denial_patterns
        ):
            continue
        findings.append(
            Finding(
                rule="hostile-git-metadata-not-neutralised",
                file=rel,
                line=1,
                message=(
                    f"dangerous Git config key {spec.key} is not neutralised on the trusted read "
                    f"route — {spec.why}"
                ),
            )
        )

    # 2. No dangerous key may be carried in a form that can execute.
    for key, value in entries:
        if key in _HOSTILE_KEY_NAMES:
            continue
        for pattern, description in HOSTILE_GIT_CONFIG_FAMILIES:
            if re.match(pattern, key) and value != "":
                findings.append(
                    Finding(
                        rule="hostile-git-metadata-unsafe-override",
                        file=rel,
                        line=1,
                        message=(
                            f"trusted Git override table carries {key}={value!r} — {description} "
                            "must never be configured by the trusted route"
                        ),
                    )
                )
                break

    # 2b. No dangerous key may be smuggled into the route beside the table.
    for name, body in (
        ("add_trusted_git_overrides", overrides_code),
        ("apply_environment", environment_code),
        ("apply_commit_identity", identity_code),
        ("classify_git_request", classification_code),
    ):
        scanned = body
        if name == "add_trusted_git_overrides" and span is not None:
            start, end = span
            scanned = scanned[:start] + " " * (end - start) + scanned[end:]
        for literal in _STRING_LITERAL_RE.findall(scanned):
            if _is_hostile_config_key(literal):
                findings.append(
                    Finding(
                        rule="hostile-git-metadata-unsafe-literal",
                        file=rel,
                        line=1,
                        message=(
                            f"{name}() names the dangerous Git config key {literal!r} outside the "
                            "pinned override table; a repository-selected surface must not reach "
                            "the trusted route"
                        ),
                    )
                )

    # 3. The GIT_CONFIG_COUNT injection path carries only the commit identity.
    injected = {
        literal
        for literal in _STRING_LITERAL_RE.findall(identity_code)
        if _CONFIG_KEY_RE.match(literal)
    }
    if injected != {"user.name", "user.email"}:
        findings.append(
            Finding(
                rule="hostile-git-metadata-config-injection",
                file=rel,
                line=1,
                message=(
                    "the GIT_CONFIG_COUNT injection path may only carry the commit identity keys "
                    f"['user.email', 'user.name'], found {sorted(injected)}"
                ),
            )
        )

    # 4. Each route obligation holds on its own.
    #
    # A key override cannot stand in for one of these, and one of these
    # cannot stand in for a key override: they defend against different
    # surfaces. So each is asserted separately, and losing any single one is
    # a finding.
    for obligation in ROUTE_OBLIGATIONS:
        missing = [
            pattern
            for pattern in obligation.patterns
            if not re.search(pattern, route_text)
        ]
        if missing:
            findings.append(
                Finding(
                    rule="hostile-git-metadata-route-obligation-missing",
                    file=rel,
                    line=1,
                    message=(
                        f"{obligation.scope} no longer carries the protection for "
                        f"{obligation.name} — {obligation.why} "
                        f"(missing {len(missing)} of {len(obligation.patterns)} required forms)"
                    ),
                )
            )

    # 5. The bounded hostile-config detector is invoked from every trusted
    #    child-construction path.
    #
    #    The detector is the mediation: it refuses a repository that declares
    #    a configuration surface the route cannot neutralise. Every other
    #    check in this file describes what the route *carries*, and a route
    #    that never calls the detector carries all of it correctly while
    #    still building a child against an unbounded repository
    #    configuration. So the call itself is asserted — in the async path
    #    and in the sync path, because a route that mediates only one of
    #    them is half-moderated, not moderated.
    #
    #    The assertion matches a call, not a mention: line comments are
    #    stripped from the body first, and the pattern requires the name to
    #    be followed by `(`.
    policy = _impl_block(text, "GitEnvPolicy")
    if policy is None:
        findings.append(
            Finding(
                rule="hostile-git-metadata-detector-call-missing",
                file=rel,
                line=1,
                message=(
                    "the trusted Git route policy (impl GitEnvPolicy) could not be located; "
                    f"the {DETECTOR_NAME}() call sites cannot be verified"
                ),
            )
        )
    else:
        policy_body, policy_line = policy
        for method in DETECTOR_ROUTE_METHODS:
            found = _method_body(policy_body, method)
            if found is None:
                findings.append(
                    Finding(
                        rule="hostile-git-metadata-detector-call-missing",
                        file=rel,
                        line=policy_line,
                        message=(
                            f"the trusted Git route method {method}() could not be located; "
                            f"the {DETECTOR_NAME}() call site cannot be verified"
                        ),
                    )
                )
                continue
            method_body, method_line = found
            if not DETECTOR_CALL_RE.search(_strip_line_comments(method_body)):
                findings.append(
                    Finding(
                        rule="hostile-git-metadata-detector-call-missing",
                        file=rel,
                        # The method's line is counted from the impl block, so
                        # the file's line is the block's line plus the
                        # method's offset from the block's first line.
                        line=policy_line + method_line - 1,
                        message=(
                            f"the trusted Git route method {method}() no longer calls "
                            f"{DETECTOR_NAME}(); the detector is the mediation that refuses a "
                            "repository-controlled configuration surface, and a comment naming "
                            "it is not a call"
                        ),
                    )
                )

    return findings


# ── Self-test ────────────────────────────────────────────────────────────────
#
# The guard is only evidence if it fails on a broken tree. `--self-test` copies
# the trusted Git route into a temporary directory, injects one defect at a
# time, and asserts that each injected defect is rejected. It also asserts
# that the unmodified route produces no finding, so a guard that rejects
# everything is not mistaken for a guard that discriminates.


def _replace_once(text: str, old: str, new: str) -> str:
    if old not in text:
        raise AssertionError(f"fixture target not found: {old!r}")
    return text.replace(old, new, 1)


def _delete_line(text: str, needle: str) -> str:
    lines = text.splitlines(keepends=True)
    for index, line in enumerate(lines):
        if needle in line:
            del lines[index]
            return "".join(lines)
    raise AssertionError(f"fixture line not found: {needle!r}")


def _delete_nth_line(text: str, needle: str, occurrence: int) -> str:
    """Delete the `occurrence`-th (1-based) line that contains `needle`.

    The detector is called from two places with identical text, so deleting
    "the async call" and deleting "the sync call" are different defects and
    need different fixtures.
    """
    lines = text.splitlines(keepends=True)
    seen = 0
    for index, line in enumerate(lines):
        if needle in line:
            seen += 1
            if seen == occurrence:
                del lines[index]
                return "".join(lines)
    raise AssertionError(
        f"fixture line not found: {needle!r} (occurrence {occurrence}; saw {seen})"
    )


def _delete_all_lines(text: str, needle: str) -> str:
    """Delete every line that contains `needle`."""
    lines = text.splitlines(keepends=True)
    kept = [line for line in lines if needle not in line]
    if len(kept) == len(lines):
        raise AssertionError(f"fixture line not found: {needle!r}")
    return "".join(kept)


def _insert_override(text: str, entry: str) -> str:
    anchor = re.search(r'(?m)^(\s*)\("core\.fsmonitor", "false"\),$', text)
    if anchor is None:
        raise AssertionError("override table anchor not found")
    indent = anchor.group(1)
    return text[: anchor.end()] + f"\n{indent}{entry}," + text[anchor.end() :]


def _remove_override_table(text: str) -> str:
    start = text.find("    for (key, value) in [")
    if start == -1:
        raise AssertionError("override table not found")
    end = text.find("    ] {", start)
    if end == -1:
        raise AssertionError("override table end not found")
    return text[:start] + text[end + len("    ] {") :]


def _insert_config_argument(text: str) -> str:
    anchor = '    cmd.arg(format!("--work-tree={}", worktree.display()));'
    if anchor not in text:
        raise AssertionError("work-tree anchor not found")
    return text.replace(
        anchor,
        '    cmd.arg("-c");\n    cmd.arg("core.hooksPath=.githooks");\n' + anchor,
        1,
    )


def _self_test_cases() -> List[Tuple[str, Callable[[str], Optional[str]], str]]:
    """The injected defects, each with the rule that must reject it."""
    return [
        (
            "the trusted route stops overriding core.hooksPath",
            lambda text: _delete_line(text, '("core.hooksPath", "/dev/null"),'),
            "hostile-git-metadata-not-neutralised",
        ),
        (
            "the trusted route stops overriding core.fsmonitor",
            lambda text: _delete_line(text, '("core.fsmonitor", "false"),'),
            "hostile-git-metadata-not-neutralised",
        ),
        (
            "the trusted route stops overriding diff.external",
            lambda text: _delete_line(text, '("diff.external", ""),'),
            "hostile-git-metadata-not-neutralised",
        ),
        (
            "the trusted route stops overriding credential.helper",
            lambda text: _delete_line(text, '("credential.helper", ""),'),
            "hostile-git-metadata-not-neutralised",
        ),
        (
            "the trusted route stops overriding protocol.ext.allow",
            lambda text: _delete_line(text, '("protocol.ext.allow", "never"),'),
            "hostile-git-metadata-not-neutralised",
        ),
        (
            "the trusted route stops overriding core.sshCommand",
            lambda text: _delete_line(text, '("core.sshCommand", ""),'),
            "hostile-git-metadata-not-neutralised",
        ),
        (
            "the trusted route stops overriding core.pager",
            lambda text: _delete_line(text, '("core.pager", "cat"),'),
            "hostile-git-metadata-not-neutralised",
        ),
        (
            "the trusted route stops overriding core.askPass",
            lambda text: _delete_line(text, '("core.askPass", ""),'),
            "hostile-git-metadata-not-neutralised",
        ),
        (
            "the trusted route points core.hooksPath at a real hook directory",
            lambda text: _replace_once(
                text,
                '("core.hooksPath", "/dev/null"),',
                '("core.hooksPath", ".githooks"),',
            ),
            "hostile-git-metadata-unsafe-override",
        ),
        (
            "the trusted route re-enables the fsmonitor hook",
            lambda text: _replace_once(
                text,
                '("core.fsmonitor", "false"),',
                '("core.fsmonitor", "true"),',
            ),
            "hostile-git-metadata-unsafe-override",
        ),
        (
            "the trusted route lets ext:: remote helpers run",
            lambda text: _replace_once(
                text,
                '("protocol.ext.allow", "never"),',
                '("protocol.ext.allow", "always"),',
            ),
            "hostile-git-metadata-unsafe-override",
        ),
        (
            "the trusted route introduces a filter process override",
            lambda text: _insert_override(
                text, '("filter.probe.process", "/tmp/evil.sh")'
            ),
            "hostile-git-metadata-unsafe-override",
        ),
        (
            "the trusted route introduces a merge driver override",
            lambda text: _insert_override(text, '("merge.probe.driver", "/tmp/evil.sh")'),
            "hostile-git-metadata-unsafe-override",
        ),
        (
            "the trusted route introduces a URL rewrite override",
            lambda text: _insert_override(
                text, '("url.https://github.com/.insteadOf", "https://evil.example/")'
            ),
            "hostile-git-metadata-unsafe-override",
        ),
        (
            "the trusted route introduces a per-URL credential helper override",
            lambda text: _insert_override(
                text, '("credential.https://github.com.helper", "/tmp/evil.sh")'
            ),
            "hostile-git-metadata-unsafe-override",
        ),
        (
            "the trusted route introduces a custom diff driver override",
            lambda text: _insert_override(text, '("diff.probe.command", "/tmp/evil.sh")'),
            "hostile-git-metadata-unsafe-override",
        ),
        (
            "the trusted route introduces a repository alias override",
            lambda text: _insert_override(text, '("alias.probe", "!sh -c evil")'),
            "hostile-git-metadata-unsafe-override",
        ),
        (
            "the trusted route introduces an executable attributes file override",
            lambda text: _insert_override(text, '("core.attributesFile", "/tmp/attrs")'),
            "hostile-git-metadata-unsafe-override",
        ),
        (
            "the trusted route introduces an alternate refs command override",
            lambda text: _insert_override(
                text, '("core.alternateRefsCommand", "/tmp/evil.sh")'
            ),
            "hostile-git-metadata-unsafe-override",
        ),
        (
            "the trusted route stops pinning the editor",
            lambda text: _delete_line(text, 'cmd.env("GIT_EDITOR", "true".into());'),
            "hostile-git-metadata-not-neutralised",
        ),
        (
            "the trusted route stops pinning the sequence editor",
            lambda text: _delete_line(text, 'cmd.env("GIT_SEQUENCE_EDITOR", "true".into());'),
            "hostile-git-metadata-not-neutralised",
        ),
        (
            "the trusted route stops disabling external diff drivers",
            lambda text: _delete_line(text, 'cmd.arg("--no-ext-diff");'),
            "hostile-git-metadata-route-obligation-missing",
        ),
        (
            "the trusted route stops disabling textconv drivers",
            lambda text: _delete_line(text, 'cmd.arg("--no-textconv");'),
            "hostile-git-metadata-route-obligation-missing",
        ),
        (
            "the trusted route stops refusing remote operations",
            lambda text: _delete_line(
                text, '"fetch" | "pull" | "push" | "clone" | "ls-remote"'
            ),
            "hostile-git-metadata-route-obligation-missing",
        ),
        (
            "the trusted route stops refusing filter-bearing checkouts",
            lambda text: _replace_once(
                text,
                '"checkout" | "restore" | "switch" => {',
                '"checkout" => {',
            ),
            "hostile-git-metadata-route-obligation-missing",
        ),
        (
            "the trusted route stops disabling replace refs",
            lambda text: _delete_line(
                text, 'cmd.env("GIT_NO_REPLACE_OBJECTS", "1".into());'
            ),
            "hostile-git-metadata-route-obligation-missing",
        ),
        (
            "the trusted route adds a dangerous key as a bare config argument",
            _insert_config_argument,
            "hostile-git-metadata-unsafe-literal",
        ),
        (
            "the trusted route smuggles a dangerous key through GIT_CONFIG_COUNT",
            lambda text: _replace_once(
                text,
                '["user.name", "user.email"]',
                '["user.name", "user.email", "core.hooksPath"]',
            ),
            "hostile-git-metadata-config-injection",
        ),
        (
            "the trusted override table disappears",
            _remove_override_table,
            "hostile-git-metadata-override-table-missing",
        ),
        (
            "the trusted route loses the system config denial",
            lambda text: _delete_line(text, 'cmd.env("GIT_CONFIG_NOSYSTEM", "1".into());'),
            "trusted-git-policy-incomplete",
        ),
        (
            "the canonical Git process owner disappears",
            lambda text: None,
            "hostile-git-metadata-source-missing",
        ),
        (
            "the canonical Git process owner is empty",
            lambda text: "",
            "hostile-git-metadata-source-empty",
        ),
        (
            "the trusted route stops calling the hostile-config detector in the async path",
            lambda text: _delete_nth_line(
                text, "ensure_repository_config_surface_is_contained(&worktree)?;", 1
            ),
            "hostile-git-metadata-detector-call-missing",
        ),
        (
            "the trusted route stops calling the hostile-config detector in the sync path",
            lambda text: _delete_nth_line(
                text, "ensure_repository_config_surface_is_contained(&worktree)?;", 2
            ),
            "hostile-git-metadata-detector-call-missing",
        ),
        (
            "the trusted route stops calling the hostile-config detector at all",
            lambda text: _delete_all_lines(
                text, "ensure_repository_config_surface_is_contained(&worktree)?;"
            ),
            "hostile-git-metadata-detector-call-missing",
        ),
        (
            "the trusted route only mentions the hostile-config detector in a comment",
            lambda text: _replace_once(
                text,
                "ensure_repository_config_surface_is_contained(&worktree)?;",
                "// ensure_repository_config_surface_is_contained(&worktree)?;",
            ),
            "hostile-git-metadata-detector-call-missing",
        ),
    ]


def _source_path(root: Path) -> Path:
    return root / "crates" / "egggit" / "src" / "process.rs"


def _write_source(text: str, root: Path) -> None:
    target = _source_path(root)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text)


def self_test() -> int:
    """Prove the guard is not vacuous."""
    failures: List[str] = []
    cases = 0
    source = ROOT / "crates" / "egggit" / "src" / "process.rs"
    if not source.is_file():
        print(
            f"self-test failed: {source} is missing, so there is nothing to self-test against",
            file=sys.stderr,
        )
        return 1
    pristine = source.read_text()

    with tempfile.TemporaryDirectory() as tmp:
        tmp_root = Path(tmp)

        baseline_root = tmp_root / "baseline"
        _write_source(pristine, baseline_root)
        baseline = check_hostile_git_metadata_authority(baseline_root)
        baseline += check_trusted_git_execution_policy(baseline_root)
        if baseline:
            failures.append(
                "the guard reports findings on an unmodified trusted Git route, so its checks are "
                f"not measuring what they claim: {baseline}"
            )

        for description, mutate, expected_rule in _self_test_cases():
            cases += 1
            work = tmp_root / re.sub(r"[^a-z0-9]+", "_", description.lower()).strip("_")
            _write_source(pristine, work)
            try:
                mutated = mutate(pristine)
            except AssertionError as exc:
                failures.append(f"{description}: fixture could not be injected: {exc}")
                continue
            if mutated is None:
                _source_path(work).unlink()
            else:
                _write_source(mutated, work)
            reported = check_hostile_git_metadata_authority(work)
            reported += check_trusted_git_execution_policy(work)
            if not reported:
                failures.append(f"{description}: the guard did not reject the injected defect")
                continue
            if expected_rule and not any(
                finding.rule == expected_rule for finding in reported
            ):
                failures.append(
                    f"{description}: the guard rejected the injected defect, but not with the "
                    f"check that owns it (no finding had rule {expected_rule!r}): {reported}"
                )
                continue
            # The self-test names the injection it exercised, so its own
            # output is the evidence that this defect — and not some other
            # one — is the thing the guard rejects.
            print(f"  ok   {description} (rejected by {expected_rule})")

    if failures:
        for failure in failures:
            print(f"self-test failed: {failure}", file=sys.stderr)
        return 1
    print(
        f"git-forbidden-patterns self-test passed: {cases} injected defects were each rejected by "
        "the guard, and the unmodified trusted Git route produced no finding"
    )
    return 0


CHECKS = [
    check_secret_debug_serialize,
    check_expose_secret_callers,
    check_duplicated_env_policy,
    check_direct_git_process_construction,
    check_trusted_git_execution_policy,
    check_hostile_git_metadata_authority,
    check_rerun_argv_construction,
    check_runstore_argv_sanitization,
]


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Forbidden-pattern static checks for the Git agent integration.",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help=(
            "prove the guard is not vacuous: inject defects into a copy of the trusted "
            "Git route and assert that each one is rejected"
        ),
    )
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()

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
    raise SystemExit(main(sys.argv[1:]))
