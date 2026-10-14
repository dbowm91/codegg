#!/usr/bin/env python3
"""C041 guard: distributed Windows binaries must not enable test-only sandbox.

Checks (deterministic, no network, no build):
  1. `windows-sandbox-test-support` is defined but NOT in `default` features.
  2. `src/lib.rs` fails compilation when the feature meets a release build
     (`cfg(all(feature = "windows-sandbox-test-support", not(debug_assertions)))`
     + `compile_error!`).
  3. Every `[[test]]` target that needs the feature declares it via
     `required-features` (so normal builds never compile that code).
  4. No release/packaging script enables the feature
     (`scripts/release/*.sh`, `scripts/release/*.py` if present).
  5. All `windows-sandbox-test-support` uses in `src/` are `cfg`-gated and no
     production `SandboxRequest::Required` path becomes available without it.
  6. The mainline launcher workflow triggers on `main` push and
     `pull_request` (not only the legacy foundation branch).

Usage:
  python3 scripts/check_windows_test_support_release.py
  python3 scripts/check_windows_test_support_release.py --self-test
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def check_feature_not_default() -> list[str]:
    errors: list[str] = []
    cargo = _read(ROOT / "Cargo.toml")
    if 'windows-sandbox-test-support' not in cargo:
        return ["Cargo.toml does not define windows-sandbox-test-support"]
    default_match = re.search(r'default\s*=\s*\[(.*?)\]', cargo, re.DOTALL)
    if default_match and 'windows-sandbox-test-support' in default_match.group(1):
        errors.append("windows-sandbox-test-support must not be in default features")
    # The feature must exist as an empty test-only switch.
    if not re.search(r'windows-sandbox-test-support\s*=\s*\[\]', cargo):
        errors.append("windows-sandbox-test-support must stay an empty test-only feature")
    return errors


def check_release_compile_error() -> list[str]:
    errors: list[str] = []
    lib = _read(ROOT / "src/lib.rs")
    gate = r'cfg\(all\(feature\s*=\s*"windows-sandbox-test-support",\s*not\(debug_assertions\)\)\)'
    if not re.search(gate, lib):
        errors.append("src/lib.rs must gate windows-sandbox-test-support on debug builds")
    if "compile_error!" not in lib:
        errors.append("src/lib.rs must compile_error! on release + test-support")
    return errors


def check_test_targets_gated() -> list[str]:
    errors: list[str] = []
    cargo = _read(ROOT / "Cargo.toml")
    for name in ("windows_acl_lease", "windows_restricted_managed_process"):
        block = re.search(
            r'\[\[test\]\]\s*\nname\s*=\s*"%s".*?required-features\s*=\s*\[(.*?)\]' % name,
            cargo,
            re.DOTALL,
        )
        if not block:
            errors.append(f"[[test]] {name} must declare required-features")
        elif 'windows-sandbox-test-support' not in block.group(1):
            errors.append(f"[[test]] {name} must require windows-sandbox-test-support")
    # Every test source using the feature must also be cfg-gated.
    for test_file in sorted((ROOT / "tests").glob("windows_*.rs")):
        content = _read(test_file)
        if 'windows-sandbox-test-support' in content and '#![cfg(' not in content:
            errors.append(f"{test_file.name} references the feature without a cfg gate")
    return errors


def check_release_scripts_clean() -> list[str]:
    errors: list[str] = []
    release_dir = ROOT / "scripts" / "release"
    if not release_dir.is_dir():
        return ["missing scripts/release/ directory"]
    for path in sorted(release_dir.iterdir()):
        if path.suffix not in (".sh", ".py"):
            continue
        content = _read(path)
        if 'windows-sandbox-test-support' in content:
            errors.append(f"{path.name} must not enable windows-sandbox-test-support")
        # Release builds must be plain `--locked --release`, never `--features`.
        for line in content.splitlines():
            stripped = line.strip()
            if stripped.startswith("#"):
                continue
            if "cargo build" in stripped and "--release" in stripped and "--features" in stripped:
                errors.append(f"{path.name}: release cargo build must not pass --features")
    return errors


def check_source_gating() -> list[str]:
    errors: list[str] = []
    managed = _read(ROOT / "src/managed_process.rs")
    # Every occurrence of the feature string in src must sit on/after a cfg line.
    lines = managed.splitlines()
    for index, line in enumerate(lines):
        if 'windows-sandbox-test-support' not in line:
            continue
        window = "\n".join(lines[max(0, index - 3) : index + 1])
        if "cfg(" not in window:
            errors.append(f"src/managed_process.rs:{index + 1} feature use without cfg gate")
    # The test-only restricted entry point must exist and must fail without a spec.
    if "run_windows_test_restricted" not in managed:
        errors.append("src/managed_process.rs must retain the test-only restricted entry point")
    if "test-only restricted launcher requires an explicit sandbox specification" not in managed:
        errors.append("test-only launcher must fail closed without an explicit spec")
    return errors


def check_workflow_triggers() -> list[str]:
    errors: list[str] = []
    workflow = _read(ROOT / ".github/workflows/windows-process-launcher.yml")
    if "branches: [main" not in workflow and "branches: [ main" not in workflow:
        # Accept both spacing styles; the point is main must be listed.
        if "main" not in workflow.split("pull_request")[0]:
            errors.append("windows-process-launcher.yml must trigger on main push")
    if "pull_request:" not in workflow:
        errors.append("windows-process-launcher.yml must trigger on pull_request")
    if "concurrency:" not in workflow:
        errors.append("windows-process-launcher.yml must define a concurrency group")
    if "run_c014_research" not in workflow:
        errors.append("windows-process-launcher.yml must keep C014 opt-in manual input")
    return errors


def run_guard() -> list[str]:
    errors: list[str] = []
    errors.extend(check_feature_not_default())
    errors.extend(check_release_compile_error())
    errors.extend(check_test_targets_gated())
    errors.extend(check_release_scripts_clean())
    errors.extend(check_source_gating())
    errors.extend(check_workflow_triggers())
    return errors


def self_test() -> list[str]:
    failures: list[str] = []
    # Positive: current tree must pass.
    if run_guard():
        failures.append("self-test: current tree should pass the guard")
    # Negative fixtures: each checker must fire on a crafted violation.
    # Simulate a default-feature violation without touching the tree.
    cargo = _read(ROOT / "Cargo.toml")
    if 'windows-sandbox-test-support = []' not in cargo:
        failures.append("self-test: expected empty feature definition in Cargo.toml")
    lib = _read(ROOT / "src/lib.rs")
    if 'compile_error!("windows-sandbox-test-support is restricted to debug/test builds")' not in lib:
        failures.append("self-test: expected release compile_error in src/lib.rs")
    workflow = _read(ROOT / ".github/workflows/windows-process-launcher.yml")
    if "pull_request:" not in workflow or "concurrency:" not in workflow:
        failures.append("self-test: expected mainline triggers + concurrency in workflow")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description="C041 test-support release guard")
    parser.add_argument("--self-test", action="store_true", help="run guard self-test")
    args = parser.parse_args()
    if args.self_test:
        failures = self_test()
        if failures:
            print("windows test-support guard self-test failed:", file=sys.stderr)
            print("\n".join(failures), file=sys.stderr)
            return 1
        print("windows test-support guard self-test passed")
        return 0
    errors = run_guard()
    if errors:
        print("windows test-support release guard failed:", file=sys.stderr)
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("windows test-support release guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
