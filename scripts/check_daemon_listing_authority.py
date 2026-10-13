#!/usr/bin/env python3
"""Static guard for daemon-owned directory listing authority (C005/ADR-0022).

The TUI must never enumerate its own filesystem as a directory-listing
authority: a same-spelled local path can describe a different machine's
directory. All production TUI listing traffic goes through the
daemon-owned `WorkspaceDirectoryList` request; the single shared walker
lives in `src/directory_listing.rs` (used by the daemon handler and, via
delegation, the presentation types in `src/tui/file_tree.rs`).

This script rejects direct filesystem enumeration for listing purposes
outside the two walker homes. Test fixtures that *create* files with
`std::fs::write`/`create_dir_all` are fine; only enumeration entry
points are rejected.

Exit code 1 if violations are found.

Usage::

    python3 scripts/check_daemon_listing_authority.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SRC = REPO_ROOT / "src"

# Files that may enumerate directories for listing purposes: the single
# shared walker and the presentation module that delegates to it (plus
# its refusal helper and conversion code).
WALKER_HOMES = {
    "directory_listing.rs",
    "tui/file_tree.rs",
}

# The TUI file-listing path that must not enumerate. Deliberately a file
# list, not a surface glob: the background `@`-mention file indexer in
# `tui/app/mod.rs` (`index_files_sync`) is a separate client-side index
# that the C005 plan explicitly defers ("no ... separate client-side file
# index" in §3 non-goals). It is recorded as a residual limitation in the
# C005 closure, not silently covered by this guard.
PROTECTED_FILES: list[str] = [
    "tui/commands/file_tree.rs",
    "tui/components/dialogs/files.rs",
    "tui/components/file_tree.rs",
    "tui/app/state/file_tree.rs",
    "tui/app/commands.rs",
    "tui/runtime/command_dispatch.rs",
]

# Enumeration entry points. `read_dir` is the raw syscall wrapper;
# `build_tree`/`build_tree_with_cancel` are the walker entry points;
# `local_walk_root` is the retired client-side refusal gate (kept only in
# its home module for historical tests, never called on a listing path).
PATTERNS: list[re.Pattern] = [
    re.compile(r"\bread_dir\s*\("),
    re.compile(r"\bbuild_tree_with_cancel\s*\("),
    re.compile(r"(?<![_:a-zA-Z])build_tree\s*\("),
    re.compile(r"\blocal_walk_root\s*\("),
]


def _strip_test_modules(text: str) -> list[tuple[int, str]]:
    """Return `(lineno, line)` pairs outside `#[cfg(test)] mod tests` regions.

    Test fixtures build temp dirs through the public walker API; only
    production code is bound by the single-walker rule.
    """
    lines = text.splitlines()
    kept: list[tuple[int, str]] = []
    index = 0
    pending_cfg_test = False
    while index < len(lines):
        line = lines[index]
        stripped = line.strip()
        if stripped == "#[cfg(test)]":
            pending_cfg_test = True
            index += 1
            continue
        if pending_cfg_test and re.match(r"(pub\s+)?mod\s+tests\s*\{", stripped):
            pending_cfg_test = False
            depth = line.count("{") - line.count("}")
            index += 1
            while index < len(lines) and depth > 0:
                depth += lines[index].count("{") - lines[index].count("}")
                index += 1
            continue
        pending_cfg_test = False
        kept.append((index + 1, line))
        index += 1
    return kept


def main() -> int:
    failures: list[str] = []
    for rel in PROTECTED_FILES:
        path = SRC / rel
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            continue
        for lineno, line in _strip_test_modules(text):
            for regex in PATTERNS:
                if regex.search(line):
                    failures.append(f"{rel}:{lineno}: {line.strip()}")
                    break
    if failures:
        print("FAIL: TUI production code enumerates directories outside the")
        print("single shared walker (src/directory_listing.rs):")
        for failure in failures:
            print(f"  {failure}")
        return 1
    print("OK: no TUI directory enumeration outside the shared walker")
    return 0


if __name__ == "__main__":
    sys.exit(main())
