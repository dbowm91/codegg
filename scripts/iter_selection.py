#!/usr/bin/env python3
"""Select the local iteration test commands for a change set.

Used by scripts/verify.sh iter. Does not follow dependents, and does not
widen a helper or manifest change into the workspace sweep.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

IDENT = re.compile(r"^[A-Za-z0-9_-]+$")
FILTER = re.compile(r"^[A-Za-z0-9_:-]+$")


def workspace_packages(root: Path) -> tuple[str, set[str]]:
    text = (root / "Cargo.toml").read_text(encoding="utf-8")
    members = re.search(r"^members\s*=\s*\[([^\]]+)\]", text, re.M)
    name = re.search(r'^name\s*=\s*"([^"]+)"', text, re.M)
    if not members or not name:
        raise SystemExit("iter: could not read workspace members from Cargo.toml")
    packages = {name.group(1)}
    for raw in members.group(1).split(","):
        path = raw.strip().strip('"')
        if not path or path == ".":
            continue
        packages.add(Path(path).name)
    return name.group(1), packages


def changed_files(base: str) -> list[str]:
    try:
        merge = subprocess.check_output(
            ["git", "merge-base", "HEAD", base], text=True
        ).strip()
    except subprocess.CalledProcessError as exc:
        raise SystemExit(f"iter: could not resolve merge-base with {base}") from exc
    files: set[str] = set()
    for args in (
        ["git", "diff", "--name-only", merge],
        ["git", "diff", "--name-only"],
        ["git", "diff", "--name-only", "--cached"],
        ["git", "ls-files", "--others", "--exclude-standard"],
    ):
        out = subprocess.check_output(args, text=True)
        files.update(line for line in out.splitlines() if line)
    return sorted(files)


class Selection:
    def __init__(self) -> None:
        self.lib: dict[str, set[str] | None] = {}
        self.bins: dict[str, set[str]] = {}
        self.tests: dict[tuple[str, str], set[str] | None] = {}
        self.notes: list[str] = []
        self.uncovered: list[str] = []

    def add_lib(self, package: str, module: str | None) -> None:
        current = self.lib.get(package, set())
        if current is None or module is None:
            self.lib[package] = None
            return
        current.add(module)
        self.lib[package] = current

    def add_test(self, package: str, target: str, member: str | None) -> None:
        key = (package, target)
        current = self.tests.get(key, set())
        if current is None or member is None:
            self.tests[key] = None
            return
        current.add(member)
        self.tests[key] = current


def _check_ident(value: str, what: str) -> str:
    if not IDENT.fullmatch(value):
        raise SystemExit(f"iter: refusing unusual {what}: {value}")
    return value


def select(paths: list[str], root_package: str, packages: set[str]) -> Selection:
    selected = Selection()
    for raw in paths:
        path = raw.replace("\\", "/")
        if path.startswith("./"):
            path = path[2:]
        if not path or path.startswith("../") or "/../" in f"/{path}":
            selected.uncovered.append(f"{raw} is outside the iteration selector")
            continue
        parts = path.split("/")
        if parts[0] == "crates" and len(parts) >= 2:
            package = parts[1]
            if package not in packages:
                selected.uncovered.append(
                    f"{path} is not a workspace package; iter does not guess a target"
                )
                continue
            _check_ident(package, "package")
            rest = parts[2:]
            if not rest or rest == ["Cargo.toml"] or rest == ["Cargo.lock"]:
                selected.add_lib(package, None)
                continue
            if rest[0] == "tests":
                _add_crate_test(selected, package, rest[1:])
                continue
            if rest[0] == "src":
                _add_crate_lib(selected, package, rest[1:])
                continue
            selected.notes.append(f"{path} does not name a test target")
            continue
        if parts[0] == "src":
            _add_root_src(selected, root_package, parts[1:])
            continue
        if parts[0] == "tests":
            _add_root_test(selected, root_package, parts[1:])
            continue
        if path in {"Cargo.toml", "Cargo.lock"}:
            selected.uncovered.append(
                f"{path} can affect every target; iter does not run the sweep"
            )
            continue
        selected.notes.append(f"{path} is outside the product test set")
    return selected


def _add_crate_lib(selected: Selection, package: str, rest: list[str]) -> None:
    if not rest or rest[0] in {"lib.rs", "main.rs", "mod.rs"}:
        selected.add_lib(package, None)
        return
    module = rest[0][:-3] if rest[0].endswith(".rs") else rest[0]
    _check_ident(module, "module")
    selected.add_lib(package, module)


def _add_crate_test(selected: Selection, package: str, rest: list[str]) -> None:
    if not rest:
        selected.uncovered.append(
            f"crates/{package}/tests has no single target; iter does not widen"
        )
        return
    target = rest[0][:-3] if rest[0].endswith(".rs") else rest[0]
    _check_ident(target, "test target")
    member = None
    if len(rest) > 1 and not rest[0].endswith(".rs"):
        leaf = rest[-1]
        if leaf not in {"mod.rs"} and (leaf.endswith(".rs") or leaf.endswith(".inc")):
            member = _check_ident(leaf.rsplit(".", 1)[0], "member")
    selected.add_test(package, target, member)


def _add_root_src(selected: Selection, package: str, rest: list[str]) -> None:
    if not rest:
        selected.add_lib(package, None)
        return
    if rest[0] == "bin" and len(rest) >= 2:
        bin_name = rest[1][:-3] if rest[1].endswith(".rs") else rest[1]
        _check_ident(bin_name, "bin")
        selected.bins.setdefault(package, set()).add(bin_name)
        return
    if rest[0] in {"lib.rs", "main.rs", "mod.rs"}:
        selected.add_lib(package, None)
        return
    module = rest[0][:-3] if rest[0].endswith(".rs") else rest[0]
    _check_ident(module, "module")
    selected.add_lib(package, module)


def _add_root_test(selected: Selection, package: str, rest: list[str]) -> None:
    if not rest:
        selected.uncovered.append("tests/ has no single target; iter does not widen")
        return
    if rest[0] == "common":
        selected.uncovered.append(
            "tests/common/"
            + "/".join(rest[1:])
            + " is a shared helper; iter does not widen to every family"
        )
        return
    if rest[0].endswith(".rs"):
        selected.add_test(package, _check_ident(rest[0][:-3], "test target"), None)
        return
    target = _check_ident(rest[0], "test target")
    member = None
    if len(rest) > 1 and rest[-1] not in {"mod.rs"}:
        leaf = rest[-1]
        if leaf.endswith(".inc") or leaf.endswith(".rs"):
            member = _check_ident(leaf.rsplit(".", 1)[0], "member")
    selected.add_test(package, target, member)


def emit(selected: Selection) -> list[str]:
    lines: list[str] = []
    if (
        not selected.lib
        and not selected.bins
        and not selected.tests
        and not selected.uncovered
    ):
        lines.append("empty\tno product test target in the change set")
        return lines
    for package in sorted(selected.lib):
        modules = selected.lib[package]
        if modules is None:
            lines.append(f"run\t{package}\tlib")
        else:
            filters = ",".join(f"{name}::" for name in sorted(modules))
            for item in filters.split(","):
                if not FILTER.fullmatch(item):
                    raise SystemExit(f"iter: refusing unusual filter: {item}")
            lines.append(f"run\t{package}\tlib\t{filters}")
    for package in sorted(selected.bins):
        for bin_name in sorted(selected.bins[package]):
            lines.append(f"run\t{package}\tbin\t{bin_name}")
    for package, target in sorted(selected.tests):
        members = selected.tests[(package, target)]
        if members is None:
            lines.append(f"run\t{package}\ttest\t{target}")
        else:
            filters = ",".join(f"{name}::" for name in sorted(members))
            lines.append(f"run\t{package}\ttest\t{target}\t{filters}")
    for note in selected.notes:
        lines.append("note\t" + note.replace("\t", " "))
    for item in selected.uncovered:
        lines.append("uncovered\t" + item.replace("\t", " "))
    return lines


def _self_test() -> int:
    packages = {"codegg", "codegg-core", "egglsp"}
    lines = emit(
        select(
            [
                "src/tui/app/mod.rs",
                "src/tool/bash.rs",
                "tests/tui_render/tui_main_buffer.inc",
                "tests/composed_two_agent_race_m011.rs",
                "crates/codegg-core/src/session/mod.rs",
                "crates/codegg-core/tests/writer_permit.rs",
                "src/bin/codegg-sandbox-helper.rs",
                "architecture/testing.md",
            ],
            "codegg",
            packages,
        )
    )
    expected = {
        "run\tcodegg\tlib\ttool::,tui::",
        "run\tcodegg\ttest\ttui_render\ttui_main_buffer::",
        "run\tcodegg\ttest\tcomposed_two_agent_race_m011",
        "run\tcodegg-core\tlib\tsession::",
        "run\tcodegg-core\ttest\twriter_permit",
        "run\tcodegg\tbin\tcodegg-sandbox-helper",
    }
    if expected.difference(lines) or any(line.startswith("uncovered\t") for line in lines):
        print("self-test: mixed selection mismatch", lines, file=sys.stderr)
        return 1
    widened = emit(select(["src/lib.rs", "src/tui/app.rs"], "codegg", packages))
    if widened != ["run\tcodegg\tlib"]:
        print("self-test: lib.rs did not drop the module filter", widened, file=sys.stderr)
        return 1
    common = emit(select(["tests/common/mod.rs"], "codegg", packages))
    if any(line.startswith("run\t") for line in common) or not any(
        line.startswith("uncovered\ttests/common") for line in common
    ):
        print("self-test: common helper was not left uncovered", common, file=sys.stderr)
        return 1
    empty = emit(
        select(["architecture/testing.md", ".github/workflows/ci.yml"], "codegg", packages)
    )
    if empty != ["empty\tno product test target in the change set"]:
        print("self-test: docs-only change was not empty", empty, file=sys.stderr)
        return 1
    manifest = emit(select(["Cargo.toml"], "codegg", packages))
    if any(line.startswith("run\t") for line in manifest) or not any(
        line.startswith("uncovered\t") and "Cargo.toml" in line for line in manifest
    ):
        print("self-test: root manifest was not left uncovered", manifest, file=sys.stderr)
        return 1
    leaf = emit(select(["crates/codegg-core/src/lib.rs"], "codegg", packages))
    if leaf != ["run\tcodegg-core\tlib"]:
        print("self-test: leaf crate widened beyond its own library", leaf, file=sys.stderr)
        return 1
    outside = emit(select(["../secret.rs"], "codegg", packages))
    if any(line.startswith("run\t") for line in outside) or not any(
        line.startswith("uncovered\t") for line in outside
    ):
        print("self-test: path traversal was not left uncovered", outside, file=sys.stderr)
        return 1
    dotted = emit(select([".github/workflows/ci.yml", "src/tool/bash.rs"], "codegg", packages))
    if "note\t.github/workflows/ci.yml is outside the product test set" not in dotted:
        print("self-test: dotted path was rewritten", dotted, file=sys.stderr)
        return 1
    print("iter_selection --self-test: pass")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--base", default="origin/main")
    parser.add_argument("--root", type=Path, default=Path.cwd())
    args = parser.parse_args(argv)
    if args.self_test:
        return _self_test()
    root_package, packages = workspace_packages(args.root)
    for line in emit(select(changed_files(args.base), root_package, packages)):
        print(line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
