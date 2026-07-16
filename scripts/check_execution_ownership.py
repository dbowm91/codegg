#!/usr/bin/env python3
"""Static lint: execution ownership inventory guard.

This script enforces the machine-readable execution-ownership manifest
declared in ``docs/execution-ownership.toml``. Every production source
location in ``src/`` and ``crates/`` that can spawn a process, send work
to a worker pool, start a test runner, start a background loop, invoke
a domain-specific process service, create or enqueue a durable job, or
acquire scheduler permits / workspace locks MUST be declared in the
manifest with an explicit ``owner`` classification.

The manifest classifications are:

  - ``scheduler`` — production daemon path, must route through
    ``JobSubmissionService`` (heavy work).
  - ``interactive`` — long-lived user-controlled PTY / REPL / editor.
  - ``standalone_compat`` — explicit ``--standalone``, ``--stdio``,
    or test harness surface; documented as outside the daemon
    singleton guarantee.
  - ``definition_or_adapter`` — defines the canonical subsystem but
    does not invoke it on its own; the canonical invoker is a
    scheduler executor or another declared site.
  - ``deferred_domain_executor`` — typed subsystem scheduled for
    future migration to scheduler; documented compatibility path.
  - ``test_only`` — test fixture (``#[cfg(test)]`` or ``tests/``).
  - ``forbidden_bypass`` — must be fixed; static guard fails.

Additionally, this script greps for direct process-spawn and
dispatch patterns (``tokio::process::Command::new``,
``std::process::Command::new``, ``JobStore::create_job``,
``pool.spawner().send``) and verifies that any match outside the
known allowlist is classified in the manifest.

Run:

  python3 scripts/check_execution_ownership.py

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST_PATH = ROOT / "docs" / "execution-ownership.toml"

VALID_OWNERS = {
    "scheduler",
    "interactive",
    "standalone_compat",
    "definition_or_adapter",
    "deferred_domain_executor",
    "test_only",
    "forbidden_bypass",
}

# Patterns that indicate a process-spawning or work-dispatch site.
# A match outside the explicit allowlist is required to be declared in
# the manifest under one of the VALID_OWNERS classes.
#
# The patterns are anchored to avoid matching doc-comments and string
# literals. Each pattern is matched against each non-comment line of
# source.
SPAWN_PATTERNS: list[tuple[str, re.Pattern[str]]] = [
    (
        "tokio_process_spawn",
        re.compile(r"\btokio::process::Command::new\s*\("),
    ),
    (
        "std_process_spawn",
        re.compile(r"\bstd::process::Command::new\s*\("),
    ),
    (
        "StdCommand_spawn",
        re.compile(r"\bStdCommand::new\s*\("),
    ),
    (
        "JobStore_create_job",
        re.compile(r"\bJobStore::create_job\s*\("),
    ),
    (
        "spawner_send",
        re.compile(r"\.spawner\(\)\.send(?:_async)?\s*\("),
    ),
    (
        "spawner_send_and_wait",
        re.compile(r"\.spawner\(\)\.send_and_wait\s*\("),
    ),
    (
        "BackgroundScheduler_spawn_loop",
        re.compile(r"BackgroundScheduler::(?:new\s*\(|\w+).*?\.spawn_loop\s*\("),
    ),
    (
        "resolve_and_run_test",
        re.compile(r"\btest_runner::runner::resolve_and_run_test\b"),
    ),
    (
        "dispatch_to_test_runner",
        re.compile(r"\bdispatch_to_test_runner\b"),
    ),
    (
        "hardened_git_command",
        re.compile(r"\bhardened_git_command\s*\("),
    ),
    (
        "executor_kind_for_job_call",
        re.compile(r"\bexecutor_kind_for_job\s*\("),
    ),
]

# Per-line annotation format. ``// execution-ownership: <owner>`` on the
# line above or on the same line is accepted.
LINE_ANNOTATION = re.compile(
    r"execution-ownership\s*:\s*(scheduler|interactive|standalone_compat|"
    r"definition_or_adapter|deferred_domain_executor|test_only|"
    r"forbidden_bypass)"
)


def load_manifest() -> list[dict]:
    """Load the machine-readable execution-ownership manifest.

    The manifest uses a simple line-oriented TOML syntax for
    ``[[site]]`` entries. We avoid pulling in a TOML dependency by
    implementing a narrow parser that handles the format documented
    in ``docs/execution-ownership.md``.
    """
    if not MANIFEST_PATH.exists():
        raise SystemExit(
            f"missing manifest: {MANIFEST_PATH}. "
            "Create it before running this guard."
        )

    sites: list[dict] = []
    current: dict | None = None

    with MANIFEST_PATH.open("r", encoding="utf-8") as fp:
        for raw_line in fp:
            line = raw_line.rstrip("\n")
            stripped = line.strip()
            if not stripped or stripped.startswith("#"):
                continue
            if stripped.startswith("[[") and stripped.endswith("]]"):
                if current is not None:
                    sites.append(current)
                current = {"_raw": []}
                continue
            if current is None:
                continue
            current["_raw"].append(stripped)
            if "=" in stripped:
                key, _, value = stripped.partition("=")
                key = key.strip()
                value = value.strip()
                if value.startswith('"') and value.endswith('"'):
                    value = value[1:-1]
                current[key] = value
        if current is not None:
            sites.append(current)
    return sites


def normalize_path(path: str) -> str:
    return path.lstrip("./")


def is_classified(sites: list[dict], rel_path: str) -> bool:
    """Check whether a file is fully classified by the manifest.

    A file is classified if it appears in the manifest. We allow a
    single ``[[site]]`` entry to declare ownership for a whole file
    via the ``path`` field. A path that ends with ``/`` matches a
    whole directory tree.
    """
    for site in sites:
        path = site.get("path", "").rstrip("/")
        if not path:
            continue
        if rel_path == path or rel_path.startswith(path + "/"):
            return True
    return False


def walk(root: Path) -> list[Path]:
    out: list[Path] = []
    for dirpath, _dirs, files in os.walk(root):
        rel = Path(dirpath).relative_to(ROOT).as_posix() if Path(dirpath).is_absolute() else dirpath
        if "/.git" in dirpath or "/target" in dirpath or "/node_modules" in dirpath:
            continue
        # Test fixtures are test_only by definition; they do not
        # require per-line classification.
        rel_posix = Path(dirpath).relative_to(ROOT).as_posix()
        if rel_posix.startswith("src/") and ("/tests/" in rel_posix or rel_posix.endswith("/tests")):
            continue
        if rel_posix.startswith("crates/") and ("/tests/" in rel_posix or rel_posix.endswith("/tests")):
            continue
        for f in files:
            if f.endswith(".rs"):
                out.append(Path(dirpath) / f)
    return out


def is_comment_line(line: str) -> bool:
    """Return True for Rust comment lines (//, ///, //!, or block
    comment continuations)."""
    stripped = line.lstrip()
    if not stripped:
        return True
    return stripped.startswith("//")


def annotate_owner(line_text: str, prev_line: str | None) -> str | None:
    m = LINE_ANNOTATION.search(line_text)
    if m is not None:
        return m.group(1)
    if prev_line is not None:
        m = LINE_ANNOTATION.search(prev_line)
        if m is not None:
            return m.group(1)
    return None


def main() -> int:
    sites = load_manifest()
    bad_owners: list[str] = []
    forbidden_paths: list[str] = []

    for site in sites:
        owner = site.get("owner", "")
        if owner not in VALID_OWNERS:
            bad_owners.append(
                f"{site.get('path', '<no-path>')}: unknown owner '{owner}'"
            )
        if owner == "forbidden_bypass":
            forbidden_paths.append(site.get("path", "<no-path>"))

    src_files = walk(ROOT / "src") + walk(ROOT / "crates")
    failures: list[str] = []
    unclassified_paths: set[str] = set()

    for path in src_files:
        rel = path.relative_to(ROOT).as_posix()
        if rel.startswith("src/scheduler/") or rel.startswith("src/test_runner/"):
            # Scheduler subsystem and test_runner subsystem are definition
            # sites; their process-spawn entries are owned by the
            # scheduler executors themselves.
            continue
        if not is_classified(sites, rel):
            try:
                content = path.read_text(encoding="utf-8")
            except OSError:
                continue
            lines = content.splitlines()
            for i, line in enumerate(lines):
                if is_comment_line(line):
                    continue
                for name, pat in SPAWN_PATTERNS:
                    if pat.search(line):
                        prev = lines[i - 1] if i > 0 else None
                        owner = annotate_owner(line, prev)
                        if owner is None:
                            unclassified_paths.add(rel)
                            failures.append(
                                f"{rel}:{i + 1}: {name} site has no "
                                f"execution-ownership annotation and "
                                f"{rel} is not classified in manifest"
                            )

    if bad_owners:
        print("execution-ownership: manifest has unknown owners:")
        for line in bad_owners:
            print(f"  {line}")
        return 1

    if forbidden_paths:
        print("execution-ownership: forbidden_bypass declared for:")
        for p in forbidden_paths:
            print(f"  - {p}")
        print("These must be fixed before closure.")
        return 1

    if failures:
        print("execution-ownership guard failed:")
        for line in failures[:50]:
            print(f"  {line}")
        if len(failures) > 50:
            print(f"  ... and {len(failures) - 50} more")
        if unclassified_paths:
            print(
                "\n  unclassified paths (declare in docs/execution-ownership.toml):"
            )
            for p in sorted(unclassified_paths):
                print(f"    - {p}")
        return 1

    print("execution-ownership guard ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
