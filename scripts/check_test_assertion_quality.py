#!/usr/bin/env python3
"""Static guard: every test asserts something checkable.

A test that cannot fail reports a pass it did not earn. This guard
finds the four shapes in which that happens and refuses them:

- **inert-cfg-test** — a `cfg(test)` gate in a file that is *only* ever
  compiled as a test target. The gate is always true there, so it decides
  nothing; it only reads as if it did.
- **ignored-repository-writer** — an `#[ignore]`d test whose body writes
  inside the repository tree. A routine `cargo test -- --ignored` would
  rewrite a tracked file. The write must be gated behind an explicit
  opt-in.
- **no-assertion** — a test body with no panic-capable construct. It
  asserts nothing, and can never fail.
- **skip-before-assertion** — a test body that reaches a skip marker and
  returns before any assertion. The runner cannot see a runtime skip, so
  the test reports *passed* having done nothing.

The triage of every surviving hit is recorded in
`docs/test-assertion-quality.toml`, and each entry there names the helper
or construct that carries the property. An entry that names neither is a
failure, not an exemption.

The scanner this guard uses is `scripts/rust_test_scan.py`, the shared
Rust-aware scanner. It is imported, not reimplemented: a second
implementation would reproduce the failures the scanner's own fixtures
pin (lifetimes read as character literals, raw strings matched at a
closing quote, `#[tokio::test]` bodies missed, and multi-line literals
shifting every line number after them).

Run with `--self-test` to prove the guard rejects an injected violation
of each class.

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
MANIFEST = Path("docs") / "test-assertion-quality.toml"

# A file that is only ever compiled as a test target. `cfg(test)` is
# always true there.
TEST_TARGET_PREFIXES = ("tests/",)

# A filesystem write.
_WRITE_CALL = re.compile(
    r"(?:\bfs::write\b"
    r"|\bfs::write_all\b"
    r"|\bfs::create_dir(?:_all)?\b"
    r"|\bfs::copy\b"
    r"|\bfs::remove_file\b"
    r"|\bFile::create\b"
    r"|\bwrite_derived_view_atomic\b"
    r"|\.write_all\s*\()"
)

# `env!("CARGO_MANIFEST_DIR")` names the crate root, so a path built
# from it lands inside the repository tree.
_MANIFEST_DIR = re.compile(r"CARGO_MANIFEST_DIR")

# A call to a local function.
_CALL = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\(")

NO_ASSERTION_CATEGORIES = ("named-helper", "compile-time-construct", "no-panic-property")
SKIP_CLASSES = ("opt-in-gated", "environment-absent")


def _is_test_target(rel: str) -> bool:
    """True when the file is only ever compiled as a test target."""
    if rel.startswith(TEST_TARGET_PREFIXES):
        return True
    return bool(re.match(r"^crates/[^/]+/tests/", rel))


class Manifest:
    """The parsed `docs/test-assertion-quality.toml`.

    Entries are keyed by `(file, name)`, **not** by line number. A line
    number moves whenever the file is reformatted or a test above it is
    added or removed, and a guard that fails on a reformat is a guard that
    gets disabled. The `line` field is kept in the manifest as a pointer
    for a human, and the guard reports a *shifted* line as information
    rather than as a violation — what it checks is that the entry still
    names a live test.
    """

    def __init__(self, data: dict) -> None:
        self.schema_version = data.get("schema-version")
        self.repository_writer_opt_in = data.get("repository-writer-opt-in")
        self.no_assertion: dict[tuple[str, str], dict] = {}
        for entry in data.get("no-assertion-exceptions", []):
            key = (entry["file"], entry["name"])
            self.no_assertion[key] = entry
        self.skip: dict[tuple[str, str], dict] = {}
        for entry in data.get("skip-exceptions", []):
            key = (entry["file"], entry["name"])
            self.skip[key] = entry

    @classmethod
    def load(cls, path: Path) -> "Manifest":
        with path.open("rb") as handle:
            return cls(tomllib.load(handle))


def _transitive_helper(local: dict, body: str, seen: set[str] | None = None) -> str | None:
    """The name of a local function the body delegates to that asserts.

    The search follows the file's own call graph, so a body that calls a
    helper which calls an asserting helper is resolved too.
    """
    seen = seen or set()
    for name in set(_CALL.findall(body)):
        if name in seen:
            continue
        seen.add(name)
        function = local.get(name)
        if function is None:
            continue
        if function.has_panic_construct:
            return name
        deeper = _transitive_helper(local, function.body_text, seen)
        if deeper:
            return name
    return None


def check_inert_cfg_test(root: Path = REPO_ROOT) -> list[str]:
    """A `cfg(test)` gate in a test-target-only file decides nothing."""
    violations: list[str] = []
    for source in rust_test_scan.iter_source_files(root):
        if not _is_test_target(source.rel):
            continue
        for match in re.finditer(r"^#!\[cfg\(test\)\]", source.text, re.M):
            line = source.text[: match.start()].count("\n") + 1
            violations.append(
                f"{source.rel}:{line} has an inner `#![cfg(test)]` gate, but the "
                f"file is only ever compiled as a test target: the gate is "
                f"always true and decides nothing"
            )
        for match in re.finditer(
            r"^[ \t]*#\[cfg\(test\)\][ \t]*\n[ \t]*mod\s", source.text, re.M
        ):
            line = source.text[: match.start()].count("\n") + 1
            violations.append(
                f"{source.rel}:{line} has a `#[cfg(test)]` gate on a module, but "
                f"the file is only ever compiled as a test target: the gate is "
                f"always true and decides nothing"
            )
    return violations


def check_ignored_repository_writer(
    root: Path = REPO_ROOT, manifest: Manifest | None = None
) -> list[str]:
    """An `#[ignore]`d test must not write into the repository tree
    unless the write is gated behind an explicit opt-in."""
    opt_in = manifest.repository_writer_opt_in if manifest else "CODEGG_REGENERATE_ASSETS"
    violations: list[str] = []
    for function in rust_test_scan.collect_test_functions(root):
        if not function.is_ignored:
            continue
        code = rust_test_scan.scan_source(function.body_text).code()
        if not _WRITE_CALL.search(code):
            continue
        if not _MANIFEST_DIR.search(code):
            # The write target is not built from the repository root.
            continue
        opt_in_at = code.find(opt_in)
        write_at = min(
            (match.start() for match in _WRITE_CALL.finditer(code)),
            default=-1,
        )
        if opt_in_at >= 0 and 0 <= write_at and opt_in_at < write_at:
            # The opt-in is checked before the write.
            continue
        violations.append(
            f"{function.location} {function.name} writes inside the repository "
            f"tree without checking the `{opt_in}` opt-in first: a routine "
            f"`cargo test -- --ignored` would rewrite a tracked file"
        )
    return violations


def check_no_assertion(
    root: Path = REPO_ROOT, manifest: Manifest | None = None
) -> list[str]:
    """Every test body must assert something, or be listed with a named
    helper or construct."""
    violations: list[str] = []
    functions = rust_test_scan.collect_test_functions(root)
    local_map = rust_test_scan.local_function_map(root)
    for function in functions:
        if function.has_panic_construct or function.has_skip_marker:
            continue
        local = local_map.get(function.rel, {})
        if _transitive_helper(local, function.body_text):
            continue
        key = (function.rel, function.name)
        entry = manifest.no_assertion.get(key) if manifest else None
        if entry is None:
            violations.append(
                f"{function.location} {function.name} asserts nothing: its body "
                f"has no panic-capable construct and delegates to no local helper "
                f"that asserts. Add an assertion, delegate to a named helper, "
                f"or record the triage in {MANIFEST.as_posix()}."
            )
            continue
        category = entry.get("category")
        if category not in NO_ASSERTION_CATEGORIES:
            violations.append(
                f"{function.location} {function.name} has an unknown "
                f"`no-assertion-exceptions` category {category!r}; expected one "
                f"of {', '.join(NO_ASSERTION_CATEGORIES)}"
            )
        if not entry.get("helper") and not entry.get("construct"):
            violations.append(
                f"{function.location} {function.name} has a "
                f"`no-assertion-exceptions` entry that names neither a helper nor "
                f"a construct: \"this test is fine\" is not a reason"
            )
    return violations


def check_skip_before_assertion(
    root: Path = REPO_ROOT, manifest: Manifest | None = None
) -> list[str]:
    """Every body that skips before asserting must be listed with the
    workflow that satisfies it."""
    violations: list[str] = []
    for function in rust_test_scan.collect_test_functions(root):
        if not function.skips_before_asserting:
            continue
        key = (function.rel, function.name)
        entry = manifest.skip.get(key) if manifest else None
        if entry is None:
            violations.append(
                f"{function.location} {function.name} reaches a skip marker and "
                f"returns before any assertion: it reports *passed* having done "
                f"nothing. Record it in {MANIFEST.as_posix()} with the workflow "
                f"that satisfies it, or make the skip a static `#[ignore]`."
            )
            continue
        if entry.get("class") not in SKIP_CLASSES:
            violations.append(
                f"{function.location} {function.name} has an unknown "
                f"`skip-exceptions` class {entry.get('class')!r}; expected one of "
                f"{', '.join(SKIP_CLASSES)}"
            )
        if not entry.get("satisfied-by"):
            violations.append(
                f"{function.location} {function.name} has a `skip-exceptions` "
                f"entry that names no satisfying workflow: silence is not a reason"
            )
    return violations


def check_stale_entries(
    root: Path = REPO_ROOT, manifest: Manifest | None = None
) -> list[str]:
    """A manifest entry that no longer names a live test is stale."""
    violations: list[str] = []
    live = {
        (fn.rel, fn.name)
        for fn in rust_test_scan.collect_test_functions(root)
    }
    for key in sorted(manifest.no_assertion):
        if key not in live:
            violations.append(
                f"{key[0]} {key[1]} has a `no-assertion-exceptions` entry "
                f"but no such test: the entry is stale"
            )
    for key in sorted(manifest.skip):
        if key not in live:
            violations.append(
                f"{key[0]} {key[1]} has a `skip-exceptions` entry but no "
                f"such test: the entry is stale"
            )
    return violations


def check(root: Path = REPO_ROOT) -> list[str]:
    manifest = Manifest.load(root / MANIFEST)
    if manifest.schema_version != 1:
        return [
            f"{MANIFEST.as_posix()} has schema-version {manifest.schema_version!r}; "
            f"this guard understands version 1"
        ]
    violations: list[str] = []
    violations += check_inert_cfg_test(root)
    violations += check_ignored_repository_writer(root, manifest)
    violations += check_no_assertion(root, manifest)
    violations += check_skip_before_assertion(root, manifest)
    violations += check_stale_entries(root, manifest)
    return violations


def _fixture_tree(root: Path) -> None:
    """A minimal tree the self-test can inject violations into."""
    module = root / "tests" / "family"
    module.mkdir(parents=True, exist_ok=True)
    (module / "member.inc").write_text(
        "mod tests {\n"
        "    #[test]\n"
        "    fn good() { assert!(true); }\n"
        "}\n",
        encoding="utf-8",
    )
    (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")


def _self_test() -> int:
    """Prove the guard rejects one injected violation of each class."""
    failures: list[str] = []

    def expect_rejected(name: str, violations: list[str], needle: str) -> None:
        if any(needle in violation for violation in violations):
            print(f"  ok: injected {name} rejected")
        else:
            print(f"  FAIL: injected {name} and it passed")
            failures.append(name)

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        _fixture_tree(root)

        manifest_path = root / MANIFEST
        manifest_path.parent.mkdir(parents=True, exist_ok=True)
        manifest_path.write_text(
            'schema-version = 1\n'
            'repository-writer-opt-in = "CODEGG_REGENERATE_ASSETS"\n',
            encoding="utf-8",
        )
        manifest = Manifest.load(manifest_path)

        # A clean tree has no violations.
        baseline = (
            check_inert_cfg_test(root)
            + check_ignored_repository_writer(root, manifest)
            + check_no_assertion(root, manifest)
            + check_skip_before_assertion(root, manifest)
        )
        if baseline:
            print("  FAIL: a clean tree has violations")
            for violation in baseline:
                print(f"    {violation}")
            failures.append("clean tree")
        else:
            print("  ok: a clean tree passes")

        member = root / "tests" / "family" / "member.inc"

        # Violation 1: an inert cfg(test) gate.
        member.write_text(
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn good() { assert!(true); }\n}\n",
            encoding="utf-8",
        )
        expect_rejected(
            "inert-cfg-test", check_inert_cfg_test(root), "always true and decides nothing"
        )
        member.write_text(
            "mod tests {\n    #[test]\n    fn good() { assert!(true); }\n}\n",
            encoding="utf-8",
        )

        # Violation 2: a test with no assertion.
        member.write_text(
            "mod tests {\n    #[test]\n    fn nothing() { let _ = 1 + 1; }\n}\n",
            encoding="utf-8",
        )
        expect_rejected(
            "no-assertion", check_no_assertion(root, manifest), "asserts nothing"
        )

        # Violation 3: a test that skips before asserting.
        member.write_text(
            "mod tests {\n"
            "    #[test]\n"
            "    fn skipping() { eprintln!(\"skipped: absent\"); return; }\n"
            "}\n",
            encoding="utf-8",
        )
        expect_rejected(
            "skip-before-assertion",
            check_skip_before_assertion(root, manifest),
            "reports *passed* having done nothing",
        )

        # A listed skip is accepted, and an entry that names no workflow
        # is not.
        member.write_text(
            "mod tests {\n"
            "    #[test]\n"
            "    fn skipping() { eprintln!(\"skipped: absent\"); return; }\n"
            "}\n",
            encoding="utf-8",
        )
        manifest_path.write_text(
            'schema-version = 1\n'
            'repository-writer-opt-in = "CODEGG_REGENERATE_ASSETS"\n'
            "\n"
            "[[skip-exceptions]]\n"
            'file = "tests/family/member.inc"\n'
            "line = 3\n"
            'name = "skipping"\n'
            'class = "environment-absent"\n'
            'condition = "the tool is absent"\n'
            'satisfied-by = ""\n',
            encoding="utf-8",
        )
        manifest = Manifest.load(manifest_path)
        expect_rejected(
            "skip-exception with no satisfying workflow",
            check_skip_before_assertion(root, manifest),
            "names no satisfying workflow",
        )

        # Violation 4: an `#[ignore]`d test that writes into the
        # repository tree without an opt-in.
        member.write_text(
            "mod tests {\n"
            "    #[test]\n"
            "    #[ignore]\n"
            "    fn writer() {\n"
            "        let p = std::path::PathBuf::from(env!(\"CARGO_MANIFEST_DIR\"));\n"
            "        std::fs::write(p.join(\"x.json\"), b\"x\").unwrap();\n"
            "    }\n"
            "}\n",
            encoding="utf-8",
        )
        expect_rejected(
            "ignored-repository-writer",
            check_ignored_repository_writer(root, manifest),
            "writes inside the repository tree",
        )

        # The same writer, gated behind the opt-in, is accepted.
        member.write_text(
            "mod tests {\n"
            "    #[test]\n"
            "    #[ignore]\n"
            "    fn writer() {\n"
            "        if std::env::var(\"CODEGG_REGENERATE_ASSETS\").as_deref() != Ok(\"1\") {\n"
            "            return;\n"
            "        }\n"
            "        let p = std::path::PathBuf::from(env!(\"CARGO_MANIFEST_DIR\"));\n"
            "        std::fs::write(p.join(\"x.json\"), b\"x\").unwrap();\n"
            "    }\n"
            "}\n",
            encoding="utf-8",
        )
        gated = check_ignored_repository_writer(root, manifest)
        if gated:
            print("  FAIL: an opt-in-gated repository writer was reported")
            for violation in gated:
                print(f"    {violation}")
            failures.append("opt-in-gated repository writer")
        else:
            print("  ok: an opt-in-gated repository writer is accepted")

        # A manifest entry that names a test which no longer exists is
        # stale: the entry was a triage decision about a test that is gone.
        member.write_text(
            "mod tests {\n    #[test]\n    fn good() { assert!(true); }\n}\n",
            encoding="utf-8",
        )
        manifest_path.write_text(
            'schema-version = 1\n'
            'repository-writer-opt-in = "CODEGG_REGENERATE_ASSETS"\n'
            "\n"
            "[[no-assertion-exceptions]]\n"
            'file = "tests/family/member.inc"\n'
            "line = 3\n"
            'name = "deleted_test"\n'
            'category = "named-helper"\n'
            'helper = "some_helper"\n',
            encoding="utf-8",
        )
        manifest = Manifest.load(manifest_path)
        expect_rejected(
            "stale manifest entry",
            check_stale_entries(root, manifest),
            "the entry is stale",
        )

        # An entry whose line number has shifted is NOT stale: the entry
        # still names a live test. A guard that fails on a reformat is a
        # guard that gets disabled.
        manifest_path.write_text(
            'schema-version = 1\n'
            'repository-writer-opt-in = "CODEGG_REGENERATE_ASSETS"\n'
            "\n"
            "[[no-assertion-exceptions]]\n"
            'file = "tests/family/member.inc"\n'
            "line = 999\n"
            'name = "good"\n'
            'category = "named-helper"\n'
            'helper = "some_helper"\n',
            encoding="utf-8",
        )
        manifest = Manifest.load(manifest_path)
        shifted = check_stale_entries(root, manifest)
        if shifted:
            print("  FAIL: a shifted line number was reported as stale")
            for violation in shifted:
                print(f"    {violation}")
            failures.append("shifted line number")
        else:
            print("  ok: a shifted line number is not reported as stale")

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
        print(f"check_test_assertion_quality --self-test: {len(failures)} failing")
        return 1
    print("check_test_assertion_quality --self-test: all checks pass")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        print("check_test_assertion_quality --self-test")
        return _self_test()

    violations = check()
    for violation in violations:
        print(f"  FAIL: {violation}")
    if violations:
        print(f"check_test_assertion_quality: {len(violations)} failing")
        return 1
    print("check_test_assertion_quality: all checks pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
