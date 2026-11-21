#!/usr/bin/env python3
"""Guard the compile-time dependency direction between the service and TUI."""

from __future__ import annotations

import re
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ROOT_MANIFEST = "Cargo.toml"
ROOT_LIB = "src/lib.rs"
TUI_MANIFEST = "crates/codegg-tui/Cargo.toml"
DEPENDENCY_SECTIONS = ("dependencies", "dev-dependencies", "build-dependencies")
PUBLIC_TUI_EXPORT = re.compile(
    r"(?m)^\s*pub\s+(?:use\s+(?:codegg_tui|(?:crate|self)::tui|tui)\b|mod\s+tui\b)"
)


def _manifest(root: Path, relative: str, errors: list[str]) -> dict:
    try:
        with (root / relative).open("rb") as handle:
            return tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as error:
        errors.append(f"cannot read {relative}: {error}")
        return {}


def _depends_on(manifest: dict, package_name: str) -> bool:
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


def violations(root: Path) -> list[str]:
    errors: list[str] = []
    root_manifest = _manifest(root, ROOT_MANIFEST, errors)
    tui_manifest = _manifest(root, TUI_MANIFEST, errors)

    root_package = root_manifest.get("package", {}).get("name")
    if root_package != "codegg":
        errors.append(f"root package must be codegg, found {root_package!r}")
    tui_package = tui_manifest.get("package", {}).get("name")
    if tui_package != "codegg-tui":
        errors.append(f"TUI package must be codegg-tui, found {tui_package!r}")
    members = root_manifest.get("workspace", {}).get("members", []) or []
    if "crates/codegg-tui" not in members:
        errors.append("workspace does not include crates/codegg-tui")
    if _depends_on(root_manifest, "codegg-tui"):
        errors.append("root package must not depend on codegg-tui")
    if not _depends_on(tui_manifest, "codegg"):
        errors.append("codegg-tui must depend on the codegg service package")

    try:
        source = (root / ROOT_LIB).read_text(encoding="utf-8")
    except OSError as error:
        source = ""
        errors.append(f"cannot read {ROOT_LIB}: {error}")
    if PUBLIC_TUI_EXPORT.search(source):
        errors.append("root library must not publicly re-export or declare the TUI")
    return errors


def _fixture(root: Path) -> None:
    files = {
        ROOT_MANIFEST: '''[workspace]
members = [".", "crates/codegg-tui"]

[package]
name = "codegg"
version = "0.1.0"

[dependencies]
''',
        TUI_MANIFEST: '''[package]
name = "codegg-tui"
version = "0.1.0"

[dependencies]
codegg = { path = "../.." }
''',
        ROOT_LIB: "pub mod service;\n",
    }
    for relative, content in files.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")


def _self_test() -> int:
    failures: list[str] = []

    def rejects(label: str, mutate) -> None:
        with tempfile.TemporaryDirectory(prefix="tui-dependency-direction-") as temporary:
            fixture = Path(temporary)
            _fixture(fixture)
            mutate(fixture)
            found = violations(fixture)
            if found:
                print(f"  ok: injected {label} rejected")
            else:
                failures.append(label)
                print(f"  FAIL: injected {label} was accepted")

    def public_reexport(root: Path) -> None:
        path = root / ROOT_LIB
        path.write_text(path.read_text(encoding="utf-8") + "pub use codegg_tui as tui;\n", encoding="utf-8")

    def reverse_dependency(root: Path) -> None:
        path = root / ROOT_MANIFEST
        source = path.read_text(encoding="utf-8")
        path.write_text(source.replace("[dependencies]\n", "[dependencies]\ncodegg-tui = { path = \"crates/codegg-tui\" }\n", 1), encoding="utf-8")

    def missing_service_dependency(root: Path) -> None:
        path = root / TUI_MANIFEST
        path.write_text(path.read_text(encoding="utf-8").replace("codegg = { path = \"../..\" }\n", "", 1), encoding="utf-8")

    rejects("public `pub use` of the TUI", public_reexport)
    rejects("reverse root-to-TUI package dependency", reverse_dependency)
    rejects("missing TUI-to-service dependency", missing_service_dependency)

    with tempfile.TemporaryDirectory(prefix="tui-dependency-direction-baseline-") as temporary:
        fixture = Path(temporary)
        _fixture(fixture)
        if violations(fixture):
            failures.append("unmodified fixture")
            print("  FAIL: the unmodified fixture did not pass")
        else:
            print("  ok: unmodified fixture passes")

    if failures:
        print(f"check_tui_dependency_direction --self-test: {len(failures)} failing", file=sys.stderr)
        return 1
    print("check_tui_dependency_direction --self-test passed")
    return 0


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return _self_test()
    errors = violations(ROOT)
    if errors:
        print("TUI dependency-direction guard failed:", file=sys.stderr)
        for error in errors:
            print(f"  FAIL: {error}", file=sys.stderr)
        return 1
    print("TUI dependency-direction guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
