#!/usr/bin/env python3
"""A self-re-exec test must name its test by a path that exists.

Some integration tests re-execute their own test binary to model a second
process: a fixture child, an interrupted publisher, a daemon that crashes
mid-publication. They select the one test they want with `--exact <name>`.

`--exact` matches the **full test path**. That path is not a constant — it is
the path the test has *inside its own cargo target*:

| the file | the path cargo knows the test by |
|---|---|
| standalone target `tests/foo.rs` | `<test_fn>` |
| family member `tests/<family>/<member>.inc` | `<member>::<test_fn>` |

The root package's integration tests are **families**: one cargo test target
declared in the root `Cargo.toml` as `[[test]] name = "NAME" path =
"tests/NAME/mod.rs"`, whose members live at `tests/NAME/<member>.inc` and are
bound with `#[path = "<member>.inc"] pub mod <member>;`.

So the moment a file moves into a family, the bare name that used to be the
whole path becomes **half** of it — and a bare name matches **nothing**. Cargo
prints `0 passed; 0 failed; ...; N filtered out` and exits **0**. The child
therefore does no work at all and exits *successfully*, so the parent reads an
empty success as evidence.

This repository has paid for that lesson three times:

1. `tests/command_git/git_fixture_isolation.inc` — the child printed nothing,
   so the parent read an empty child report as a failure to derive a fixture
   path.
2. `tests/security_permission/windows_process_launcher.inc` — two sites. The
   fixture child selected no test, exited 0 without publishing its PID, and
   three hosted Windows tests failed on *"managed process exited before
   publishing its PID"* with a child stdout of *"running 0 tests"*.
3. `tests/linux_git_test_support/linux_m004_process_ipc.inc` — quieter, and
   worse. The recovery worker selected no test, exited 0 having done no work,
   and never created the interrupted publisher's `index.lock`. The parent
   failed on *"displaced authorized gitdir lost the interrupted publisher
   lock"* — the recovery scenario the test exists to prove was never
   exercised at all.

Site 2 broke a lane. Site 3 broke a lane *and* silently removed the coverage
it was written to provide. A consolidation that moves a file into a family
changes that file's test path, and nothing else in the build notices.

This control closes the class. It resolves the family structure from the root
`Cargo.toml`, derives the module prefix each test file's path carries, and
requires every `--exact` argument to be the full path of a test function
**defined in that same file**. A name that does not exist is rejected too —
including a name that exists in a *different* file, which is the copy-paste
trap.

The scope is every integration-test source in the repository: the root
package's `tests/` tree (families and standalone targets alike) and each
workspace crate's `tests/*.rs`, which are standalone targets because those
packages have no families.

The scan is Rust-aware: it uses `scripts/rust_test_scan.py`, which knows about
raw strings, byte strings, character literals and comments. A regex scan would
misread a lifetime as a character literal and miss the very literals it is
looking for.

Modes:
    python3 scripts/check_test_self_reexec_paths.py             # static guard
    python3 scripts/check_test_self_reexec_paths.py --self-test # non-vacuity

`--self-test` copies the tree, injects each defect class into the copy, and
requires the guard to reject every one of them, while accepting the unmodified
copy.
"""

from __future__ import annotations

import argparse
import re
import shutil
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import rust_test_scan as rts  # noqa: E402

# `#[path = "member.inc"] pub mod member;` — the literal carries the file and
# the module name follows it.
_MODULE_AFTER_PATH = re.compile(r"(?:pub\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;")

# A macro invocation between a `--exact` literal and the literal that is its
# argument means the argument is computed — `format!`, `concat!`, `stringify!`,
# `env!`. A plain conversion such as `OsString::from("…")` is fine: the
# literal is still the argument. Anything else the gap contains is caught by
# the rule that the name must be a test function in this file.
_ARGUMENT_GAP_MACRO = "!"


@dataclass(frozen=True)
class TestFile:
    """One test source file and the module prefix its test path carries."""

    path: Path
    rel: str
    kind: str  # "family" | "standalone"
    prefix: str  # the module name, or "" for a standalone target
    target: str


def _declared_targets(root: Path) -> list[tuple[str, Path]]:
    """Every `[[test]]` target the root package declares."""
    manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    targets: list[tuple[str, Path]] = []
    for entry in manifest.get("test", []):
        name = entry.get("name")
        path = entry.get("path")
        if not isinstance(name, str) or not isinstance(path, str):
            continue
        targets.append((name, (root / path).resolve()))
    return targets


def _family_members(root: Path, family_root: Path) -> dict[Path, str]:
    """The member files a family's `mod.rs` binds, and their module names."""
    text = family_root.read_text(encoding="utf-8")
    scan = rts.scan_source(text)
    code = scan.code()
    members: dict[Path, str] = {}
    for literal in scan.literals:
        if literal.kind != "string":
            continue
        value = literal.content
        if not (value.endswith(".inc") or value.endswith(".rs")):
            continue
        match = _MODULE_AFTER_PATH.search(code[literal.end : literal.end + 240])
        if match is None:
            continue
        members[(family_root.parent / value).resolve()] = match.group(1)
    return members


def _test_files(root: Path) -> list[TestFile]:
    """Every file that holds a test, with the prefix its test path carries."""
    root = root.resolve()
    tests_dir = root / "tests"
    files: dict[Path, TestFile] = {}
    for name, path in _declared_targets(root):
        if path.name == "mod.rs" and path.parent.parent == tests_dir.resolve():
            for member, module in _family_members(root, path).items():
                files[member] = TestFile(
                    path=member,
                    rel=member.relative_to(root).as_posix(),
                    kind="family",
                    prefix=module,
                    target=name,
                )
            files[path] = TestFile(
                path=path,
                rel=path.relative_to(root).as_posix(),
                kind="standalone",
                prefix="",
                target=name,
            )
            continue
        if not path.exists():
            continue
        files[path] = TestFile(
            path=path,
            rel=path.relative_to(root).as_posix(),
            kind="standalone",
            prefix="",
            target=name,
        )
    # Cargo also auto-discovers `tests/*.rs`, which are standalone targets
    # unless the manifest claims them.
    if tests_dir.is_dir():
        for path in sorted(tests_dir.glob("*.rs")):
            resolved = path.resolve()
            if resolved in files:
                continue
            files[resolved] = TestFile(
                path=resolved,
                rel=resolved.relative_to(root).as_posix(),
                kind="standalone",
                prefix="",
                target=path.stem,
            )
    # The workspace crates carry their own integration tests. Those packages
    # have no families, so each of their `tests/*.rs` files is a standalone
    # target whose test paths carry no module prefix.
    for crate_dir in sorted((root / "crates").glob("*/tests")):
        if not crate_dir.is_dir():
            continue
        for path in sorted(crate_dir.glob("*.rs")):
            resolved = path.resolve()
            if resolved in files:
                continue
            files[resolved] = TestFile(
                path=resolved,
                rel=resolved.relative_to(root).as_posix(),
                kind="standalone",
                prefix="",
                target=path.stem,
            )
    return sorted(files.values(), key=lambda item: item.rel)


def _test_names(text: str, path: Path, rel: str) -> set[str]:
    """The test functions defined in one file."""
    return {fn.name for fn in rts.find_test_functions(text, path, rel, "tests")}


def check_tree(root: Path) -> list[str]:
    """Every `--exact` argument in the tree that names no real test path."""
    violations: list[str] = []
    root = root.resolve()
    for test_file in _test_files(root):
        if not test_file.path.exists():
            continue
        text = test_file.path.read_text(encoding="utf-8")
        scan = rts.scan_source(text)
        code = scan.code()
        literals = sorted(scan.literals, key=lambda literal: literal.start)
        names = _test_names(text, test_file.path, test_file.rel)
        for index, literal in enumerate(literals):
            if literal.kind != "string" or literal.content != "--exact":
                continue
            line = text.count("\n", 0, literal.start) + 1
            where = f"{test_file.rel}:{line}"
            following = literals[index + 1] if index + 1 < len(literals) else None
            if following is None:
                violations.append(
                    f"{where}: `--exact` has no argument; the child would select "
                    "no test and exit 0"
                )
                continue
            gap = code[literal.end : following.start]
            if _ARGUMENT_GAP_MACRO in gap:
                violations.append(
                    f"{where}: the `--exact` argument is computed by a macro, "
                    "not written down as a string literal; the guard cannot "
                    "prove which test it selects"
                )
                continue
            if following.kind != "string":
                violations.append(
                    f"{where}: the `--exact` argument is not a string literal"
                )
                continue
            value = following.content
            if test_file.kind == "family":
                expected = f"{test_file.prefix}::"
                if not value.startswith(expected):
                    violations.append(
                        f"{where}: `--exact {value}` is not module-qualified; "
                        f"as a member of `{test_file.target}` its test path "
                        f"starts with `{expected}`"
                    )
                    continue
                selected = value[len(expected) :]
            else:
                if "::" in value:
                    violations.append(
                        f"{where}: `--exact {value}` is module-qualified, but "
                        f"`{test_file.target}` is a standalone target whose test "
                        "paths carry no module prefix"
                    )
                    continue
                selected = value
            if selected not in names:
                violations.append(
                    f"{where}: `--exact {value}` names no test function defined "
                    f"in this file; cargo would select 0 tests and exit 0, and "
                    "the parent would read that empty success as evidence"
                )
    return violations


def _copy_tree(root: Path, destination: Path) -> Path:
    """Copy the parts of the tree the guard reads into a scratch directory."""
    shutil.copy2(root / "Cargo.toml", destination / "Cargo.toml")
    shutil.copytree(root / "tests", destination / "tests")
    for crate_dir in sorted((root / "crates").glob("*/tests")):
        if not crate_dir.is_dir():
            continue
        target = destination / "crates" / crate_dir.parent.name / "tests"
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(crate_dir, target)
    return destination


def _inject(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"self-test could not find the anchor text in {path}: {old!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def _self_test() -> int:
    """Prove the guard rejects each defect class and accepts the clean tree."""
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="codegg-self-reexec-") as raw:
        scratch = Path(raw)
        copy = _copy_tree(ROOT, scratch)
        if check_tree(copy):
            failures.append("the unmodified copy must pass the guard")
            _report(failures)
            return 1

        cases: list[tuple[str, str, str, str]] = [
            (
                "a bare test name in a family member",
                "tests/command_git/git_fixture_isolation.inc",
                '"git_fixture_isolation::child_prints_its_own_process_unique_fixture_path"',
                '"child_prints_its_own_process_unique_fixture_path"',
            ),
            (
                "a sibling member's module name",
                "tests/command_git/git_fixture_isolation.inc",
                '"git_fixture_isolation::child_prints_its_own_process_unique_fixture_path"',
                '"command_routing_adversarial::child_prints_its_own_process_unique_fixture_path"',
            ),
            (
                "a test that does not exist",
                "tests/command_git/git_fixture_isolation.inc",
                '"git_fixture_isolation::child_prints_its_own_process_unique_fixture_path"',
                '"git_fixture_isolation::no_such_test_exists_here"',
            ),
            (
                "a computed `--exact` argument",
                "tests/command_git/git_fixture_isolation.inc",
                '"git_fixture_isolation::child_prints_its_own_process_unique_fixture_path"',
                'format!("git_fixture_isolation::{}", "child_prints_its_own_process_unique_fixture_path")',
            ),
        ]

        for label, rel, old, new in cases:
            with tempfile.TemporaryDirectory(prefix="codegg-self-reexec-case-") as raw_case:
                case_copy = _copy_tree(ROOT, Path(raw_case))
                _inject(case_copy / rel, old, new)
                violations = check_tree(case_copy)
                if not violations:
                    failures.append(f"the guard did not reject {label}")
                else:
                    print(f"  rejected {label}: {violations[0]}")

        # A standalone target must reject a module-qualified name.
        with tempfile.TemporaryDirectory(prefix="codegg-self-reexec-case-") as raw_case:
            case_copy = _copy_tree(ROOT, Path(raw_case))
            standalone = case_copy / "tests/contained_worktree_c008.rs"
            text = standalone.read_text(encoding="utf-8")
            if "#[test]" not in text:
                failures.append("the standalone-target self-test needs a #[test] in the file")
            else:
                _inject(
                    standalone,
                    "#[test]",
                    '#[test]\nfn self_reexec_probe_placeholder() {\n    let _ = "--exact";\n    let _ = "contained_worktree_c008::self_reexec_probe_placeholder";\n}\n#[test]',
                )
                violations = check_tree(case_copy)
                if not violations:
                    failures.append(
                        "the guard did not reject a module-qualified name in a standalone target"
                    )
                else:
                    print(
                        "  rejected a module-qualified name in a standalone target: "
                        f"{violations[0]}"
                    )

    if failures:
        _report(failures)
        return 1
    print("self-reexec path guard self-test passed")
    return 0


def _report(failures: list[str]) -> None:
    for failure in failures:
        print(f"ERROR: {failure}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects each defect class it is meant to catch",
    )
    args = parser.parse_args()
    if args.self_test:
        return _self_test()
    violations = check_tree(ROOT)
    if violations:
        for violation in violations:
            print(f"ERROR: {violation}", file=sys.stderr)
        print(f"{len(violations)} self-re-exec path violation(s)", file=sys.stderr)
        return 1
    print("self-re-exec path guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
