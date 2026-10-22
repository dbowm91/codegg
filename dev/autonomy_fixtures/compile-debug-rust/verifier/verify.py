"""Independent verifier for the ``compile-debug-rust`` autonomy case.

The verifier sits outside the agent-editable fixture tree. It does not trust
anything the agent could have rewritten:

* the provided test file is **frozen** -- its SHA-256 digest is embedded here,
  so an edited, weakened or deleted test file fails the case;
* the contract is re-derived by a **hidden** test that lives in this
  directory, never under ``fixture/``, and that checks the same three
  documented contracts with different inputs.

It FAILS while the task is unsolved. The shipped baseline does not compile
at all (a struct field rename was missed at one construction site), so the
compiler diagnostic is printed on stderr and the verifier exits non-zero.

The crate is dependency-free and is compiled with ``rustc`` directly (never
``cargo``): the taxonomy forbids network access and the crate must build
without a registry or a lockfile. Everything is built in a temporary
directory, so a verifier run leaves no artifacts in the case tree.
"""
from __future__ import annotations

import hashlib
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

# Importing the helper must not leave a `__pycache__` behind in the case tree.
sys.dont_write_bytecode = True

sys.path.insert(0, str(Path(__file__).resolve().parent))

from loader import case_root, fixture_root  # noqa: E402

#: The provided test file. The agent may read it but must not delete, weaken
#: or rewrite it, so its digest is pinned here.
PROVIDED_TEST = "fixture/tests/range_contract.rs"
PROVIDED_TEST_SHA256 = "e2a5c0969152041eddfff44449d18033cd35c7579c3d5faef3da015c76bd128a"

#: The hidden contract test. It is a sibling of this script, so it lives
#: outside the agent's workspace and cannot be weakened.
HIDDEN_TEST = Path(__file__).resolve().parent / "hidden_contract_test.rs"

RUSTC = "rustc"
TIMEOUT_SECONDS = 180


def fail(message: str) -> int:
    """Print a failure on stderr and return the non-zero exit code."""
    print(f"FAIL: {message}", file=sys.stderr)
    return 1


def check_frozen_test() -> int:
    """Verify the provided test file was neither edited nor deleted."""
    path = case_root() / PROVIDED_TEST
    if not path.is_file():
        return fail(f"the provided test file is missing: {PROVIDED_TEST}")
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    if actual != PROVIDED_TEST_SHA256:
        return fail(
            f"the provided test file was edited: {PROVIDED_TEST}\n"
            f"  expected sha256 {PROVIDED_TEST_SHA256}\n"
            f"  actual   sha256 {actual}"
        )
    return 0


def build_library(workdir: Path) -> tuple[Path | None, str]:
    """Compile the candidate crate into ``workdir``; return the rlib and stderr."""
    built = subprocess.run(
        [
            RUSTC,
            "--edition",
            "2021",
            "--crate-type",
            "rlib",
            "--crate-name",
            "candidate",
            str(fixture_root() / "src" / "lib.rs"),
            "--out-dir",
            str(workdir),
        ],
        capture_output=True,
        text=True,
        timeout=TIMEOUT_SECONDS,
        check=False,
    )
    if built.returncode != 0:
        return None, built.stderr
    return workdir / "libcandidate.rlib", ""


def compile_and_run(
    lib: Path, test_source: Path, workdir: Path, label: str
) -> tuple[int, str]:
    """Compile one test file against the candidate rlib and run it."""
    test_bin = workdir / f"{label}_tests"
    compiled = subprocess.run(
        [
            RUSTC,
            "--edition",
            "2021",
            "--test",
            "--extern",
            f"candidate={lib}",
            str(test_source),
            "-o",
            str(test_bin),
        ],
        capture_output=True,
        text=True,
        timeout=TIMEOUT_SECONDS,
        check=False,
    )
    if compiled.returncode != 0:
        print(f"FAIL: the {label} test file did not compile:\n{compiled.stderr}",
              file=sys.stderr)
        return 1, ""
    ran = subprocess.run(
        [str(test_bin)],
        capture_output=True,
        text=True,
        timeout=TIMEOUT_SECONDS,
        check=False,
    )
    if ran.stdout:
        print(ran.stdout, end="")
    if ran.stderr:
        print(ran.stderr, end="", file=sys.stderr)
    if ran.returncode != 0:
        return fail(f"the {label} test suite failed."), ""
    return 0, ran.stdout


def main() -> int:
    if shutil.which(RUSTC) is None:
        return fail(
            "`rustc` is not on PATH, so the candidate crate cannot be built "
            "and the case cannot be verified."
        )
    if not HIDDEN_TEST.is_file():
        return fail(f"the hidden contract test is missing: {HIDDEN_TEST}")

    frozen = check_frozen_test()
    if frozen != 0:
        return frozen

    with tempfile.TemporaryDirectory(prefix="compile-debug-rust-") as tmp:
        workdir = Path(tmp)
        lib, stderr = build_library(workdir)
        if lib is None:
            # The compile diagnostic is the whole point of the first half of
            # this case: print it so it is visible to the agent and the log.
            print("FAIL: the candidate crate does not compile:", file=sys.stderr)
            print(stderr, end="", file=sys.stderr)
            return 1

        code, _ = compile_and_run(
            lib, case_root() / PROVIDED_TEST, workdir, "provided"
        )
        if code != 0:
            return code

        code, _ = compile_and_run(lib, HIDDEN_TEST, workdir, "hidden")
        if code != 0:
            return code

    print("PASS: the candidate crate compiles and honours the documented contract.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
