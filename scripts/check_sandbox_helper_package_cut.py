#!/usr/bin/env python3
"""Guard the root-compile boundary for the sandbox helper package cut."""

from __future__ import annotations

import re
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HELPER = "codegg-sandbox-helper"
ROOT_MANIFEST = "Cargo.toml"
HELPER_MANIFEST = "crates/codegg-sandbox-helper/Cargo.toml"
SANDBOX_MANIFEST = "crates/codegg-sandbox/Cargo.toml"
CORE_MANIFEST = "crates/codegg-core/Cargo.toml"
SANDBOX_LIB = "crates/codegg-sandbox/src/lib.rs"
OLD_ROOT_BIN = "src/bin/codegg-sandbox-helper.rs"
DEPENDENCY_SECTIONS = ("dependencies", "dev-dependencies", "build-dependencies")


def _manifest(root: Path, relative: str, errors: list[str]) -> dict:
    path = root / relative
    try:
        with path.open("rb") as handle:
            return tomllib.load(handle)
    except FileNotFoundError:
        errors.append(f"missing manifest: {relative}")
    except (OSError, tomllib.TOMLDecodeError) as error:
        errors.append(f"cannot read {relative}: {error}")
    return {}


def _dependency_on(manifest: dict, package_name: str) -> bool:
    """Include normal, dev, build, target-specific, and renamed dependencies."""
    tables = [manifest.get(section, {}) or {} for section in DEPENDENCY_SECTIONS]
    for target in (manifest.get("target", {}) or {}).values():
        tables.extend(target.get(section, {}) or {} for section in DEPENDENCY_SECTIONS)
    for table in tables:
        for name, dependency in table.items():
            actual = dependency.get("package", name) if isinstance(dependency, dict) else name
            if actual == package_name:
                return True
    return False


def _bin_names(manifest: dict, package_name: str) -> set[str]:
    targets = manifest.get("bin", []) or []
    if isinstance(targets, dict):
        targets = [targets]
    return {target.get("name", package_name) for target in targets if isinstance(target, dict)}


def violations(root: Path) -> list[str]:
    errors: list[str] = []
    root_manifest = _manifest(root, ROOT_MANIFEST, errors)
    helper_manifest = _manifest(root, HELPER_MANIFEST, errors)
    sandbox_manifest = _manifest(root, SANDBOX_MANIFEST, errors)
    core_manifest = _manifest(root, CORE_MANIFEST, errors)

    root_package = root_manifest.get("package", {}).get("name")
    if root_package != "codegg":
        errors.append(f"root package must be codegg, found {root_package!r}")
    if HELPER in _bin_names(root_manifest, root_package or "codegg"):
        errors.append("root package still owns the codegg-sandbox-helper bin")
    if (root / OLD_ROOT_BIN).exists():
        errors.append(f"root helper source remains at {OLD_ROOT_BIN}")

    helper_package = helper_manifest.get("package", {}).get("name")
    helper_bins = _bin_names(helper_manifest, helper_package or "")
    if HELPER not in helper_bins:
        errors.append(f"{HELPER_MANIFEST} does not own bin {HELPER!r}")
    if helper_package == "codegg-core":
        errors.append("the helper package must not be codegg-core")
    if _dependency_on(helper_manifest, "codegg"):
        errors.append("the helper package must not depend on package codegg")
    if helper_manifest.get("lints", {}).get("workspace") is True:
        errors.append("the helper package must not inherit workspace lints")

    sandbox_package = sandbox_manifest.get("package", {}).get("name")
    if sandbox_package != "codegg-sandbox":
        errors.append(f"sandbox library package must be codegg-sandbox, found {sandbox_package!r}")
    if _dependency_on(sandbox_manifest, "codegg"):
        errors.append("the sandbox library must not depend on package codegg")
    if sandbox_manifest.get("lints", {}).get("workspace") is not True:
        errors.append("the sandbox library must inherit workspace lints")
    sandbox_source = root / SANDBOX_LIB
    try:
        source = sandbox_source.read_text(encoding="utf-8")
    except OSError as error:
        source = ""
        errors.append(f"cannot read {SANDBOX_LIB}: {error}")
    if not re.search(r"(?m)^\s*#!\s*\[\s*deny\s*\(\s*unsafe_code\s*\)\s*\]", source):
        errors.append("sandbox library lacks #![deny(unsafe_code)]")
    if re.search(r"(?m)^\s*#!\s*\[\s*allow\s*\([^]]*\bunsafe_code\b[^]]*\)\s*\]", source):
        errors.append("sandbox library has a crate-wide #![allow(unsafe_code)]")
    unsafe_lint = sandbox_manifest.get("lints", {}).get("rust", {}).get("unsafe_code")
    if unsafe_lint == "allow" or (isinstance(unsafe_lint, dict) and unsafe_lint.get("level") == "allow"):
        errors.append("sandbox library manifest allows unsafe_code crate-wide")

    if _dependency_on(core_manifest, "codegg-sandbox"):
        errors.append("codegg-core must not depend on codegg-sandbox")
    members = root_manifest.get("workspace", {}).get("members", []) or []
    for member in ("crates/codegg-sandbox", "crates/codegg-sandbox-helper"):
        if member not in members:
            errors.append(f"workspace does not include {member}")
    return errors


def _fixture(root: Path) -> None:
    files = {
        ROOT_MANIFEST: '''[workspace]
members = [".", "crates/codegg-sandbox", "crates/codegg-sandbox-helper"]

[package]
name = "codegg"
''',
        HELPER_MANIFEST: '''[package]
name = "codegg-sandbox-helper"

[[bin]]
name = "codegg-sandbox-helper"
path = "src/main.rs"

[dependencies]
codegg-sandbox = { path = "../codegg-sandbox" }
''',
        SANDBOX_MANIFEST: '''[package]
name = "codegg-sandbox"

[lints]
workspace = true

[dependencies]
codegg-core = { path = "../codegg-core" }
''',
        CORE_MANIFEST: '''[package]
name = "codegg-core"
''',
        SANDBOX_LIB: "#![deny(unsafe_code)]\n",
        "crates/codegg-sandbox-helper/src/main.rs": "fn main() {}\n",
    }
    for relative, content in files.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")


def _self_test() -> int:
    failures: list[str] = []

    def rejects(label: str, mutate) -> None:
        with tempfile.TemporaryDirectory(prefix="sandbox-helper-cut-") as temporary:
            fixture = Path(temporary)
            _fixture(fixture)
            mutate(fixture)
            found = violations(fixture)
            if found:
                print(f"  ok: injected {label} rejected")
            else:
                failures.append(label)
                print(f"  FAIL: injected {label} was accepted")

    def append(relative: str, text: str) -> None:
        path = Path(relative)
        def mutation(root: Path) -> None:
            target = root / path
            target.write_text(target.read_text(encoding="utf-8") + text, encoding="utf-8")
        return mutation

    rejects("optional normal dependency on codegg", append(HELPER_MANIFEST, '\ncodegg = { version = "0.1", optional = true }\n'))
    rejects("dev dependency on codegg", append(HELPER_MANIFEST, '\n[dev-dependencies]\ncodegg = "0.1"\n'))
    rejects("build dependency on codegg", append(HELPER_MANIFEST, '\n[build-dependencies]\ncodegg = "0.1"\n'))
    rejects("root helper bin target", append(ROOT_MANIFEST, '\n[[bin]]\nname = "codegg-sandbox-helper"\npath = "src/bin/codegg-sandbox-helper.rs"\n'))
    rejects("root helper source shim", lambda root: (root / OLD_ROOT_BIN).parent.mkdir(parents=True, exist_ok=True) or (root / OLD_ROOT_BIN).write_text("fn main() {}\n", encoding="utf-8"))
    rejects("helper package named codegg-core", lambda root: (root / HELPER_MANIFEST).write_text((root / HELPER_MANIFEST).read_text(encoding="utf-8").replace('name = "codegg-sandbox-helper"', 'name = "codegg-core"', 1), encoding="utf-8"))
    rejects("missing library unsafe deny", lambda root: (root / SANDBOX_LIB).write_text("// missing deny\n", encoding="utf-8"))
    rejects("crate-wide unsafe allow", lambda root: (root / SANDBOX_LIB).write_text("#![deny(unsafe_code)]\n#![allow(unsafe_code)]\n", encoding="utf-8"))
    rejects("helper inheriting workspace lints", append(HELPER_MANIFEST, '\n[lints]\nworkspace = true\n'))
    rejects("sandbox library depending on codegg", append(SANDBOX_MANIFEST, '\ncodegg = { path = "../..", optional = true }\n'))
    rejects("codegg-core depending on codegg-sandbox", append(CORE_MANIFEST, '\n[dependencies]\ncodegg-sandbox = { path = "../codegg-sandbox" }\n'))

    with tempfile.TemporaryDirectory(prefix="sandbox-helper-cut-baseline-") as temporary:
        fixture = Path(temporary)
        _fixture(fixture)
        if violations(fixture):
            failures.append("unmodified fixture")
            print("  FAIL: the unmodified fixture did not pass")
        else:
            print("  ok: unmodified fixture passes")

    if failures:
        print(f"check_sandbox_helper_package_cut --self-test: {len(failures)} failing", file=sys.stderr)
        return 1
    print("check_sandbox_helper_package_cut --self-test passed")
    return 0


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return _self_test()
    errors = violations(ROOT)
    if errors:
        print("sandbox helper package-cut guard failed:", file=sys.stderr)
        for error in errors:
            print(f"  FAIL: {error}", file=sys.stderr)
        return 1
    print("sandbox helper package-cut guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
