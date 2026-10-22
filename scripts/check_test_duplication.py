#!/usr/bin/env python3
"""Static guard: no two test functions assert the same thing twice.

A duplicated test body is a test that costs a full compile-and-run cycle
to prove something another test already proved. The detector is
**content-preserving**: a literal is replaced by a marker derived from its
*content*, so two calls that differ only in a string argument stay
distinct while two identical calls stay equal.

The three classes:

- **cross-file-duplicate** — two test bodies in different files are
  identical after normalisation, and the pair is not recorded in
  `docs/test-duplication.toml`.
- **same-file-duplicate** — two test bodies in the same file are
  identical after normalisation, and the pair is not recorded.
- **shared-helper-assertion** — a helper module under `tests/common/` or
  `crates/*/tests/common/` carries its own `#[test]` functions, so its
  assertions are compiled and run into every family that declares it.

The normalisation is: strip comments, replace every literal with a
content-derived marker, collapse whitespace, and drop the function's own
name **as a whole identifier**. Dropping the name as a substring would
turn `foo` and `foo_impl` into the same text and collapse seventeen
distinct tests into one false group.

The `--self-test` injects one violation of each class and proves the
guard rejects it, and includes the **negative** case: two bodies that
differ only in a string argument must **not** be reported.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys
import tempfile
import tomllib

from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import rust_test_scan  # noqa: E402  (path is set above)

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = Path("docs") / "test-duplication.toml"

# Helper directories whose assertions fan out into every family that
# declares them. `crates/*/tests/common/` is the same shape in a crate
# outside the root package's family topology.
SHARED_HELPER_DIRS = ("tests/common/",)

# A family root that declares the shared helper module.
FAMILY_DECLARES_COMMON = re.compile(
    r"^\s*(?:#\[path\s*=\s*\"[^\"]*\"\]\s*)?mod\s+common\s*;",
    re.M,
)


def _is_shared_helper(rel: str) -> bool:
    """True for a file that lives in a shared integration-test helper tree."""
    if rel.startswith(SHARED_HELPER_DIRS):
        return True
    return bool(re.match(r"^crates/[^/]+/tests/common/", rel))


def _allowed_pairs(manifest_path: Path) -> set[tuple[str, str]]:
    """The pairs the decision table records as allowed."""
    with manifest_path.open("rb") as handle:
        data = tomllib.load(handle)
    allowed: set[tuple[str, str]] = set()
    for group in data.get("groups", []):
        files = sorted({member["file"] for member in group["members"]})
        for index, first in enumerate(files):
            for second in files[index + 1 :]:
                allowed.add((first, second))
    return allowed


def _allowed_same_file(manifest_path: Path) -> set[tuple[str, str, str]]:
    """The same-file pairs the decision table records as allowed."""
    with manifest_path.open("rb") as handle:
        data = tomllib.load(handle)
    allowed: set[tuple[str, str, str]] = set()
    for group in data.get("groups", []):
        members = group.get("members", [])
        for index, first in enumerate(members):
            for second in members[index + 1 :]:
                if first["file"] != second["file"]:
                    continue
                allowed.add((first["file"], first["name"], second["name"]))
    return allowed


def check_duplicates(root: Path = REPO_ROOT) -> list[str]:
    """Every identical body must be recorded in the decision table."""
    violations: list[str] = []
    manifest = root / MANIFEST
    if not manifest.is_file():
        return [
            f"{MANIFEST.as_posix()} is missing: the guard cannot tell an "
            f"intentional repeat from an accidental duplicate without it"
        ]
    allowed_pairs = _allowed_pairs(manifest)
    allowed_same = _allowed_same_file(manifest)

    for group in rust_test_scan.duplicate_groups(root=root):
        files = sorted({fn.rel for fn in group})
        cross_file = len(files) > 1
        if cross_file:
            for index, first in enumerate(files):
                for second in files[index + 1 :]:
                    if (first, second) in allowed_pairs:
                        continue
                    violations.append(
                        f"{first} and {second} carry byte-identical test bodies "
                        f"after normalisation, and the pair is not recorded in "
                        f"{MANIFEST.as_posix()}: "
                        + ", ".join(f"{fn.rel}:{fn.line} {fn.name}" for fn in group)
                    )
        else:
            for index, first in enumerate(group):
                for second in group[index + 1 :]:
                    if (first.rel, first.name, second.name) in allowed_same:
                        continue
                    violations.append(
                        f"{first.rel} carries two byte-identical test bodies: "
                        f"{first.name} (line {first.line}) and {second.name} "
                        f"(line {second.line}), and the pair is not recorded in "
                        f"{MANIFEST.as_posix()}"
                    )
    return violations


def check_shared_helper_assertions(root: Path = REPO_ROOT) -> list[str]:
    """A shared helper must not carry its own assertions."""
    violations: list[str] = []
    for source in rust_test_scan.iter_source_files(root):
        if not _is_shared_helper(source.rel):
            continue
        for function in rust_test_scan.find_test_functions(
            source.text, source.path, source.rel, source.tree
        ):
            violations.append(
                f"{function.location} {function.name} is an assertion in the "
                f"shared helper {source.rel}: it is compiled into every family "
                f"that declares the helper. Move it into the one family that "
                f"uses it."
            )
    return violations


def check_unused_helper_declarations(root: Path = REPO_ROOT) -> list[str]:
    """A family must not declare the shared helper when it never uses it."""
    violations: list[str] = []
    tests_dir = root / "tests"
    if not tests_dir.is_dir():
        return violations
    for family_root in sorted(tests_dir.glob("*/mod.rs")):
        text = family_root.read_text(encoding="utf-8")
        if not FAMILY_DECLARES_COMMON.search(text):
            continue
        family = family_root.parent
        used_by = None
        for member in sorted(family.glob("*.inc")) + sorted(family.glob("*.rs")):
            if member == family_root:
                continue
            body = member.read_text(encoding="utf-8")
            if "common::" in body or "super::common" in body:
                used_by = member.relative_to(root).as_posix()
                break
        if used_by is None:
            violations.append(
                f"{family_root.relative_to(root).as_posix()} declares the shared "
                f"helper module `common`, but no member of that family "
                f"references it: the helper and every assertion it carries are "
                f"compiled into that target for nothing. Remove the "
                f"declaration, or make the family use the helper."
            )
    return violations


def check(root: Path = REPO_ROOT) -> list[str]:
    violations: list[str] = []
    violations += check_duplicates(root)
    violations += check_shared_helper_assertions(root)
    violations += check_unused_helper_declarations(root)
    return violations


def _fixture(root: Path) -> None:
    (root / "tests" / "family").mkdir(parents=True, exist_ok=True)
    (root / "tests" / "family" / "member.inc").write_text(
        "mod tests {\n"
        "    #[test]\n"
        "    fn good() { assert!(true); }\n"
        "}\n",
        encoding="utf-8",
    )
    (root / "docs").mkdir(parents=True, exist_ok=True)
    (root / MANIFEST).write_text("schema-version = 1\n", encoding="utf-8")


def _self_test() -> int:
    """Prove the guard rejects one injected violation of each class, and
    that the negative case is not reported."""
    failures: list[str] = []

    def expect_rejected(name: str, violations: list[str], needle: str) -> None:
        if any(needle in violation for violation in violations):
            print(f"  ok: injected {name} rejected")
        else:
            print(f"  FAIL: injected {name} and it passed")
            failures.append(name)

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        _fixture(root)
        first = root / "tests" / "family" / "member.inc"
        second = root / "tests" / "other" / "member.inc"
        second.parent.mkdir(parents=True, exist_ok=True)

        baseline = check(root)
        if baseline:
            print("  FAIL: a clean tree has violations")
            for violation in baseline:
                print(f"    {violation}")
            failures.append("clean tree")
        else:
            print("  ok: a clean tree passes")

        # Violation 1: a cross-file duplicate.
        first.write_text(
            "mod tests {\n"
            "    #[test]\n"
            "    fn one() { assert_eq!(1 + 1, 2); }\n"
            "}\n",
            encoding="utf-8",
        )
        second.write_text(
            "mod tests {\n"
            "    #[test]\n"
            "    fn two() { assert_eq!(1 + 1, 2); }\n"
            "}\n",
            encoding="utf-8",
        )
        expect_rejected(
            "cross-file-duplicate", check(root), "byte-identical test bodies"
        )

        # Violation 2: a same-file duplicate.
        second.write_text(
            "mod tests {\n"
            "    #[test]\n"
            "    fn unrelated() { assert!(true); }\n"
            "}\n",
            encoding="utf-8",
        )
        first.write_text(
            "mod tests {\n"
            "    #[test]\n"
            "    fn one() { assert_eq!(1 + 1, 2); }\n"
            "    #[test]\n"
            "    fn two() { assert_eq!(1 + 1, 2); }\n"
            "}\n",
            encoding="utf-8",
        )
        expect_rejected(
            "same-file-duplicate", check(root), "two byte-identical test bodies"
        )

        # Violation 3: a shared helper that carries its own assertion.
        first.write_text(
            "mod tests {\n"
            "    #[test]\n"
            "    fn good() { assert!(true); }\n"
            "}\n",
            encoding="utf-8",
        )
        helper = root / "tests" / "common" / "helper.rs"
        helper.parent.mkdir(parents=True, exist_ok=True)
        helper.write_text(
            "#[cfg(test)]\nmod tests {\n"
            "    #[test]\n"
            "    fn helper_assertion() { assert!(true); }\n"
            "}\n",
            encoding="utf-8",
        )
        expect_rejected(
            "shared-helper-assertion",
            check(root),
            "is an assertion in the shared helper",
        )
        helper.unlink()

        # Violation 4: a family that declares the shared helper and never
        # uses it.
        family_root = root / "tests" / "family" / "mod.rs"
        family_root.write_text(
            '#[path = "../common/mod.rs"]\nmod common;\n', encoding="utf-8"
        )
        (root / "tests" / "common" / "mod.rs").write_text(
            "pub mod helper;\n", encoding="utf-8"
        )
        expect_rejected(
            "unused-helper-declaration",
            check(root),
            "declares the shared helper module `common`, but no member",
        )
        family_root.unlink()

        # The negative case: two bodies that differ only in a string
        # argument must NOT be reported.
        first.write_text(
            "mod tests {\n"
            "    #[test]\n"
            "    fn one() { classify(\"find\"); }\n"
            "}\n",
            encoding="utf-8",
        )
        second.write_text(
            "mod tests {\n"
            "    #[test]\n"
            "    fn two() { classify(\"brew\"); }\n"
            "}\n",
            encoding="utf-8",
        )
        negative = check(root)
        if negative:
            print("  FAIL: bodies differing only in a string argument were reported")
            for violation in negative:
                print(f"    {violation}")
            failures.append("negative case")
        else:
            print("  ok: bodies differing only in a string argument are not reported")

        # The real tree must still pass.
        real = check()
        if real:
            print("  FAIL: the real tree has violations")
            for violation in real:
                print(f"    {violation}")
            failures.append("real tree")
        else:
            print("  ok: the real tree passes")

    if failures:
        print(f"check_test_duplication --self-test: {len(failures)} failing")
        return 1
    print("check_test_duplication --self-test: all checks pass")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        print("check_test_duplication --self-test")
        return _self_test()

    violations = check()
    for violation in violations:
        print(f"  FAIL: {violation}")
    if violations:
        print(f"check_test_duplication: {len(violations)} failing")
        return 1
    print("check_test_duplication: all checks pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
