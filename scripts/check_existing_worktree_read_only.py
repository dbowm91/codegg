#!/usr/bin/env python3
"""Guard the fail-closed tool profile for existing managed read-only runs."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

REQUIRED = {
    "src/scheduler/executors.rs": [
        "placement.existing_worktree_id()",
        "EXISTING_WORKTREE_READ_ONLY_SENTINEL.into()",
        "child_workspace_id = target_workspace_id",
    ],
    "src/agent/worker.rs": [
        "tool == EXISTING_WORKTREE_READ_ONLY_SENTINEL",
        "if is_read_only_agent(agent) || existing_worktree_read_only",
        'tool_registry.retain_only(&["read", "list", "glob"]);',
        "&& !existing_worktree_read_only",
    ],
    "src/tool/mod.rs": [
        "pub fn retain_only(&mut self, allowed_tools: &[&str])",
        "allowed_tools.iter().any(|allowed| *allowed == tool.name())",
    ],
}


def inspect(sources: dict[str, str]) -> list[str]:
    failures = []
    for path, fragments in REQUIRED.items():
        content = sources.get(path, "")
        for fragment in fragments:
            if fragment not in content:
                failures.append(f"{path}: missing read-only placement invariant {fragment!r}")
    return failures


def self_test() -> int:
    sources = {
        path: "\n".join(fragments)
        for path, fragments in REQUIRED.items()
    }
    if inspect(sources):
        print("read-only placement guard fixture is invalid")
        return 1
    sources["src/agent/worker.rs"] = sources["src/agent/worker.rs"].replace(
        "&& !existing_worktree_read_only", "&& true"
    )
    failures = inspect(sources)
    if not failures:
        print("read-only placement guard failed to detect task delegation re-enabled")
        return 1
    print("read-only placement guard self-test ok (unsafe delegation is rejected)")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        return self_test()
    sources = {}
    for path in REQUIRED:
        try:
            sources[path] = (ROOT / path).read_text(encoding="utf-8")
        except OSError as error:
            print(f"read-only placement guard: cannot read {path}: {error}")
            return 1
    failures = inspect(sources)
    if failures:
        print("read-only placement guard failed:")
        for failure in failures:
            print(f"  {failure}")
        return 1
    print("read-only placement guard ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
