#!/usr/bin/env python3
"""A binder of the shared integration-test helper tree must bind it publicly.

The root package's integration tests share one helper tree,
`tests/common/mod.rs`. Every file that binds that tree — a family's
`mod.rs`, or a standalone test target at the root of `tests/` — names it
with a `common` module.

**The binding must be `pub mod common;`, not `mod common;`.** Everything in
the tree is `pub`. Most files in the tree carry their own per-file
`#[allow(dead_code)]`, but `tests/common/secret_scan.rs` deliberately
carries none: that suppression was removed on purpose, so that the lint
keeps its teeth in a file whose whole purpose is to be checked. A
*private* `mod common;` makes those `pub` items unreachable from the
binding crate root, and rustc reports every one of them as dead code —
which is a **hosted clippy failure** under `-D warnings`, and one that
only a Linux runner can see, because the macOS build of this tree fails
earlier on unrelated findings.

That is not a hypothetical. The root families bind the tree publicly for
exactly this reason — `tests/common/secret_scan.rs` says so in its own
header — and `origin/main` added two standalone targets that bind it
privately, which is harmless on `main` (its copy of the tree still carries
the blanket suppression) and a hosted clippy failure here.

The rule this control enforces is therefore narrow and mechanical: **every
binder of the root package's shared helper tree binds it publicly.**

The workspace crates' own `tests/common/` trees are out of scope. They are
separate trees with their own lint posture — `crates/egglsp/tests/common/`
carries per-item `#[allow(dead_code)]` — and this control says nothing about
them.

Modes:
    python3 scripts/check_test_common_binding.py             # static guard
    python3 scripts/check_test_common_binding.py --self-test # non-vacuity

`--self-test` copies the tree, injects a private binding into each kind of
binder, and requires the guard to reject every one of them while accepting
the unmodified copy.
"""

from __future__ import annotations

import argparse
import re
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import rust_test_scan as rts  # noqa: E402

# A family root binds the shared tree by an explicit `#[path]`, because the
# tree is one directory up.
_FAMILY_BINDING = re.compile(
    r'#\s*\[\s*path\s*=\s*"\.\./common/mod\.rs"\s*\]\s*(pub\s+)?mod\s+common\s*;'
)
# A standalone target at the root of `tests/` binds it by the conventional
# module name: `tests/common/mod.rs` is `mod common;`'s resolution.
_STANDALONE_BINDING = re.compile(r"^[ \t]*(pub\s+)?mod\s+common\s*;", re.MULTILINE)


def _binders(root: Path) -> list[tuple[Path, str, bool]]:
    """Every file that binds the root package's shared helper tree.

    Returns `(path, rel, is_public)` for each binder found.
    """
    tests_dir = root / "tests"
    found: list[tuple[Path, str, bool]] = []
    if not tests_dir.is_dir():
        return found
    for family_root in sorted(tests_dir.glob("*/mod.rs")):
        if family_root.parent.name == "common":
            continue
        text = family_root.read_text(encoding="utf-8")
        code = rts.scan_source(text).code()
        for match in _FAMILY_BINDING.finditer(code):
            found.append((family_root, family_root.relative_to(root).as_posix(), bool(match.group(1))))
    for target in sorted(tests_dir.glob("*.rs")):
        text = target.read_text(encoding="utf-8")
        code = rts.scan_source(text).code()
        for match in _STANDALONE_BINDING.finditer(code):
            found.append((target, target.relative_to(root).as_posix(), bool(match.group(1))))
    return found


def check_tree(root: Path) -> list[str]:
    """Every binder of the shared tree that binds it privately."""
    violations: list[str] = []
    for path, rel, is_public in _binders(root):
        if is_public:
            continue
        line = 0
        for index, candidate in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            if "mod common" in candidate:
                line = index
                break
        violations.append(
            f"{rel}:{line}: binds the shared helper tree with a private "
            "`mod common;`. The tree's items are `pub`, and "
            "`tests/common/secret_scan.rs` carries no `allow(dead_code)` of "
            "its own, so a private binding makes its items unreachable from "
            "this crate root and the dead-code lint reports every one of "
            "them -- a hosted clippy failure under `-D warnings`. "
            "Bind it with `pub mod common;`."
        )
    return violations


def _copy_tree(root: Path, destination: Path) -> Path:
    """Copy the parts of the tree the guard reads into a scratch directory."""
    shutil.copy2(root / "Cargo.toml", destination / "Cargo.toml")
    shutil.copytree(root / "tests", destination / "tests")
    return destination


def _inject(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"self-test could not find the anchor text in {path}: {old!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def _self_test() -> int:
    """Prove the guard rejects a private binding in either kind of binder."""
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="codegg-common-binding-") as raw:
        copy = _copy_tree(ROOT, Path(raw))
        if check_tree(copy):
            failures.append("the unmodified copy must pass the guard")
            _report(failures)
            return 1

        cases: list[tuple[str, str, str, str]] = [
            (
                "a private binding in a family root",
                "tests/agent_loop/mod.rs",
                '#[path = "../common/mod.rs"]\npub mod common;',
                '#[path = "../common/mod.rs"]\nmod common;',
            ),
            (
                "a private binding in a standalone target",
                "tests/composed_two_agent_race_m011.rs",
                "pub mod common;",
                "mod common;",
            ),
        ]

        for label, rel, old, new in cases:
            with tempfile.TemporaryDirectory(prefix="codegg-common-binding-case-") as raw_case:
                case_copy = _copy_tree(ROOT, Path(raw_case))
                _inject(case_copy / rel, old, new)
                violations = check_tree(case_copy)
                if not violations:
                    failures.append(f"the guard did not reject {label}")
                else:
                    print(f"  rejected {label}: {violations[0]}")

    if failures:
        _report(failures)
        return 1
    print("shared helper-tree binding guard self-test passed")
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
        print(f"{len(violations)} shared helper-tree binding violation(s)", file=sys.stderr)
        return 1
    print("shared helper-tree binding guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
