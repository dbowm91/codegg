#!/usr/bin/env python3
"""Static guard for bounded, metadata-only project discovery.

The discovery module is intentionally a core-only boundary.  This guard keeps
future changes from turning it into an activation path or an unbounded crawler.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODULE = ROOT / "crates/codegg-core/src/project_discovery.rs"
LIB = ROOT / "crates/codegg-core/src/lib.rs"


def production_source() -> str:
    source = MODULE.read_text(encoding="utf-8")
    # Tests may create fixture directories; the production guard deliberately
    # excludes that section.
    return source.split("#[cfg(test)]", 1)[0]


def check_module_and_export() -> bool:
    if not MODULE.is_file():
        print("  FAIL: project_discovery.rs is missing")
        return False
    if "pub mod project_discovery" not in LIB.read_text(encoding="utf-8"):
        print("  FAIL: codegg-core does not export project_discovery")
        return False
    return True


def check_explicit_bounds() -> bool:
    source = production_source()
    required = ("MAX_", "max_depth", "max_entries", "max_candidates", "max_elapsed", "CancellationToken")
    missing = [item for item in required if item not in source]
    if missing:
        print(f"  FAIL: discovery bounds/cancellation markers missing: {', '.join(missing)}")
        return False
    return True


def check_no_activation_imports() -> bool:
    source = production_source()
    forbidden = (
        r"use\s+.*(?:egglsp|provider|agent|workspace_services)",
        r"(?:Lsp|Indexer|Provider|AgentLoop|WorkspaceServices)",
        r"(?:Command::new|tokio::process|std::process)",
    )
    for pattern in forbidden:
        if re.search(pattern, source, re.IGNORECASE):
            print(f"  FAIL: activation/process dependency pattern found: {pattern}")
            return False
    return True


def check_no_candidate_writes_or_cwd() -> bool:
    source = production_source()
    forbidden = (
        r"std::env::current_dir",
        r"(?:create|remove|rename|write)_dir",
        r"fs::write",
        r"File::create",
        r"OpenOptions",
    )
    for pattern in forbidden:
        if re.search(pattern, source):
            print(f"  FAIL: candidate write/process-cwd pattern found: {pattern}")
            return False
    return True


def check_remote_locators_not_scanned() -> bool:
    source = production_source()
    for line in source.splitlines():
        if re.search(r"(?:Ssh|LinkedNode|remote)", line, re.IGNORECASE) and re.search(
            r"(?:PathBuf::from|canonicalize|scan)", line
        ):
            print("  FAIL: remote locator data is converted into a local scan path")
            return False
    return True


def main() -> int:
    checks = [
        ("module and export", check_module_and_export),
        ("explicit finite bounds", check_explicit_bounds),
        ("no activation/process imports", check_no_activation_imports),
        ("no candidate writes or cwd inference", check_no_candidate_writes_or_cwd),
        ("remote locators remain inert", check_remote_locators_not_scanned),
    ]
    results = [(name, check()) for name, check in checks]
    for name, ok in results:
        print(f"  [{'PASS' if ok else 'FAIL'}] {name}")
    passed = sum(ok for _, ok in results)
    print(f"{passed}/{len(results)} discovery invariant checks passed.")
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
