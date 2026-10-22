#!/usr/bin/env python3
"""Static guard: the Tool Program pipeline stays parse-only.

The Tool Program module in `crates/codegg-core/src/tool_program/` compiles
a declarative program into an IR and interprets it. It never executes
user-supplied source, and in particular it never reaches a Python
interpreter.

That invariant used to be "documented" by three test functions in
`crates/codegg-core/src/tool_program/guards.rs`:

- `parse_only_marker_compiles` called a macro whose expansion is a
  `const _: () = { ... }` block that asserts nothing at run time;
- `cpython_guard_function_exists` called a documentation-only function
  with an empty body;
- `no_cpython_import_in_module` had a body of nothing but comments.

None of the three could fail. A test that cannot fail reports a pass it
did not earn, and the invariant it claimed to enforce was enforced
nowhere. This guard is the enforcement: it is a static check of a
source-text property, which is what a source-text property deserves.

The rules:

- `pyo3-dependency` — no workspace manifest declares a `pyo3`
  dependency, and no lockfile entry pulls one in;
- `pyo3-import` — the Tool Program module does not import `pyo3`;
- `process-spawn` — the Tool Program module does not spawn a process, so
  it cannot spawn a Python interpreter;
- `python-ffi` — the Tool Program module does not link against a Python
  runtime.

Comments and string literals are stripped before the source is searched.
That is not a nicety: the module's own documentation names
`std::process::Command` and `pyo3`, and a text search over the raw
source would report the documentation as a violation.

Run with `--self-test` to prove the guard rejects an injected violation
of each rule.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys
import tempfile

from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import rust_test_scan  # noqa: E402  (path is set above)

REPO_ROOT = Path(__file__).resolve().parent.parent

# The module the invariant is about.
TOOL_PROGRAM_DIR = Path("crates") / "codegg-core" / "src" / "tool_program"

# `use pyo3::...` or `pyo3::...` in the module.
PYO3_IMPORT = re.compile(r"\bpyo3\b")

# `std::process::Command`, `process::Command`, `Command::new`.
PROCESS_SPAWN = re.compile(r"\bprocess::Command\b|\bCommand::new\b")

# A link against a Python runtime.
PYTHON_FFI = re.compile(r"link\s*\(\s*name\s*=\s*\"python|\blibpython\b")


def _manifest_files(root: Path) -> list[Path]:
    return sorted(root.glob("Cargo.toml")) + sorted(root.glob("crates/*/Cargo.toml")) + sorted(
        root.glob("apps/*/*/Cargo.toml")
    )


def _module_sources(root: Path) -> list[Path]:
    module = root / TOOL_PROGRAM_DIR
    if not module.is_dir():
        return []
    return sorted(module.rglob("*.rs"))


def _masked(path: Path) -> str:
    """The file's source with comments and string literals blanked."""
    text = path.read_text(encoding="utf-8")
    return rust_test_scan.scan_source(text).mask()


def _code(path: Path) -> str:
    """The file's source with comments blanked and literals kept."""
    text = path.read_text(encoding="utf-8")
    return rust_test_scan.scan_source(text).code()


def check_pyo3_dependency(root: Path = REPO_ROOT) -> list[str]:
    """No workspace manifest or lockfile may pull in `pyo3`."""
    violations: list[str] = []
    for manifest in _manifest_files(root):
        for number, line in enumerate(
            manifest.read_text(encoding="utf-8").splitlines(), 1
        ):
            if "pyo3" in line:
                violations.append(
                    f"{manifest.relative_to(root)}:{number} declares a `pyo3` "
                    f"dependency: {line.strip()}"
                )
    lock = root / "Cargo.lock"
    if lock.is_file():
        for number, line in enumerate(lock.read_text(encoding="utf-8").splitlines(), 1):
            if line.strip().startswith('name = "pyo3"'):
                violations.append(
                    f"Cargo.lock:{number} pins a `pyo3` package: {line.strip()}"
                )
    return violations


def check_module(root: Path = REPO_ROOT) -> list[str]:
    """The Tool Program module may not reach a Python interpreter."""
    violations: list[str] = []
    sources = _module_sources(root)
    if not sources:
        return [
            f"{TOOL_PROGRAM_DIR.as_posix()} not found. The guard must not pass "
            f"because the module it protects is missing."
        ]
    for source in sources:
        rel = source.relative_to(root).as_posix()
        masked = _masked(source)
        code = _code(source)
        for number, line in enumerate(masked.splitlines(), 1):
            if PYO3_IMPORT.search(line):
                violations.append(
                    f"{rel}:{number} imports `pyo3`: {line.strip()}"
                )
            if PROCESS_SPAWN.search(line):
                violations.append(
                    f"{rel}:{number} spawns a process: {line.strip()}"
                )
        # The FFI rule is about what a literal says, so it reads the
        # comment-free source rather than the fully masked one.
        for number, line in enumerate(code.splitlines(), 1):
            if PYTHON_FFI.search(line):
                violations.append(
                    f"{rel}:{number} links against a Python runtime: {line.strip()}"
                )
    return violations


def check(root: Path = REPO_ROOT) -> list[str]:
    return check_pyo3_dependency(root) + check_module(root)


def _write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def _self_test() -> int:
    """Prove the guard rejects one injected violation of each rule."""
    failures: list[str] = []

    def check_rejected(name: str, violations: list[str], needle: str) -> None:
        if any(needle in violation for violation in violations):
            print(f"  ok: injected {name} rejected")
        else:
            print(f"  FAIL: injected {name} and it passed")
            failures.append(name)

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        module = root / TOOL_PROGRAM_DIR
        _write(module / "clean.rs", "pub fn parse_only() {}\n")
        _write(root / "Cargo.toml", "[workspace]\n")
        _write(root / "Cargo.lock", "version = 3\n")

        baseline = check(root)
        if baseline:
            print("  FAIL: a clean tree has violations")
            for violation in baseline:
                print(f"    {violation}")
            failures.append("clean tree")
        else:
            print("  ok: a clean tree passes")

        # Violation 1: a manifest declares pyo3.
        _write(root / "Cargo.toml", "[workspace]\n\n[dependencies]\npyo3 = \"0.22\"\n")
        check_rejected(
            "pyo3-dependency (manifest)",
            check(root),
            "declares a `pyo3` dependency",
        )
        _write(root / "Cargo.toml", "[workspace]\n")

        # Violation 2: the lockfile pins pyo3.
        _write(root / "Cargo.lock", "version = 3\n\n[[package]]\nname = \"pyo3\"\n")
        check_rejected(
            "pyo3-dependency (lockfile)",
            check(root),
            "pins a `pyo3` package",
        )
        _write(root / "Cargo.lock", "version = 3\n")

        # Violation 3: the module imports pyo3.
        _write(module / "clean.rs", "pub fn parse_only() {}\n")
        _write(module / "imports.rs", "use pyo3::prelude::*;\n")
        check_rejected("pyo3-import", check(root), "imports `pyo3`")
        (module / "imports.rs").unlink()

        # Violation 4: the module spawns a process.
        _write(module / "spawn.rs", "fn go() { let _ = std::process::Command::new(\"python3\"); }\n")
        check_rejected("process-spawn", check(root), "spawns a process")
        (module / "spawn.rs").unlink()

        # Violation 5: the module links against a Python runtime.
        _write(module / "ffi.rs", "#[link(name = \"python3.11\")]\nextern \"C\" {}\n")
        check_rejected("python-ffi", check(root), "links against a Python runtime")
        (module / "ffi.rs").unlink()

        # A documentation mention must not be a violation: the module's
        # own doc comments name `pyo3` and `std::process::Command`.
        _write(
            module / "documented.rs",
            "//! The module must never depend on `std::process::Command`\n"
            "//! or import `pyo3`.\n"
            "pub fn parse_only() {}\n",
        )
        documented = check(root)
        if documented:
            print("  FAIL: documentation naming `pyo3` was reported as a violation")
            for violation in documented:
                print(f"    {violation}")
            failures.append("documentation is not a violation")
        else:
            print("  ok: documentation naming `pyo3` is not a violation")

        # And the real tree must still pass.
        real = check()
        if real:
            print("  FAIL: the real tree has violations")
            for violation in real:
                print(f"    {violation}")
            failures.append("real tree")
        else:
            print("  ok: the real tree passes")

    if failures:
        print(f"check_tool_program_parse_only --self-test: {len(failures)} failing")
        return 1
    print("check_tool_program_parse_only --self-test: all checks pass")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        print("check_tool_program_parse_only --self-test")
        return _self_test()

    violations = check()
    for violation in violations:
        print(f"  FAIL: {violation}")
    if violations:
        print(f"check_tool_program_parse_only: {len(violations)} failing")
        return 1
    print("check_tool_program_parse_only: all checks pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
