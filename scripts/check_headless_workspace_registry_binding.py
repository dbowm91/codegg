#!/usr/bin/env python3
"""Guard that headless CLI entry points bind tools and broker policy to one root."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CONSTRUCTOR = "with_config_and_search_runtime_for_workspace"
POLICY_SETTER = "set_workspace_path_policy"
IDENTITY_SETTER = "set_workspace_id"


def findings(exec_source: str, run_source: str) -> list[str]:
    errors: list[str] = []
    for label, source in (("exec", exec_source), ("run", run_source)):
        if CONSTRUCTOR not in source:
            errors.append(f"{label} does not bind its workspace into ToolRegistry construction")
        if POLICY_SETTER not in source:
            errors.append(f"{label} does not install the broker workspace path policy")
        if IDENTITY_SETTER not in source:
            errors.append(f"{label} does not install an ephemeral workspace identity")
    return errors


def self_test() -> bool:
    clean_source = (
        f"ToolRegistry::{CONSTRUCTOR}(config, runtime, root); "
        f"loop.{IDENTITY_SETTER}(id); loop.{POLICY_SETTER}(root)"
    )
    clean = findings(clean_source, clean_source)
    injected = findings(
        "ToolRegistry::with_config_and_search_runtime(config, runtime)",
        f"ToolRegistry::{CONSTRUCTOR}(config, runtime, root); "
        f"loop.{IDENTITY_SETTER}(id); loop.{POLICY_SETTER}(root)",
    )
    policy_missing = findings(
        clean_source,
        f"ToolRegistry::{CONSTRUCTOR}(config, runtime, root)",
    )
    if clean or not any("exec" in error for error in injected) or not any(
        "path policy" in error for error in policy_missing
    ):
        print("headless workspace binding guard self-test failed", file=sys.stderr)
        return False
    print("headless workspace binding guard self-test passed")
    return True


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return 0 if self_test() else 2
    exec_source = (ROOT / "src/exec.rs").read_text()
    run_source = (ROOT / "src/main.rs").read_text()
    errors = findings(exec_source, run_source)
    if errors:
        for error in errors:
            print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print("headless workspace registry binding passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
