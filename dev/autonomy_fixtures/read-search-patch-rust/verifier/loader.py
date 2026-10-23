"""Locate the case's fixture tree and drive the Rust build/test cycle.

The verifier reaches the candidate's crate in two situations. In a standalone
run the case root is the verifier's parent directory. In a live lane the
harness copies ``fixture/`` into the candidate's workspace and copies
``verifier/`` somewhere else entirely, then points the case root at the lane
through ``CODEGG_AUTONOMY_CASE_ROOT``. Every root lookup goes through this
module so both situations resolve the same way.

Nothing here imports the fixture. The verifier never trusts a file the
candidate could have edited: it compiles the crate and re-derives the
contract itself.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess


def case_root() -> Path:
    """Return the case root: the directory that holds fixture/ and verifier/."""
    override = os.environ.get("CODEGG_AUTONOMY_CASE_ROOT")
    if override:
        return Path(override).expanduser().resolve()
    return Path(__file__).resolve().parents[1]


def fixture_root() -> Path:
    """Return the agent-editable tree."""
    return case_root() / "fixture"


def transcript_path() -> Path:
    """Return the harness-written transcript, or None when there is none."""
    override = os.environ.get("CODEGG_AUTONOMY_TRANSCRIPT")
    if override:
        return Path(override).expanduser().resolve()
    candidate = case_root() / "TRANSCRIPT.json"
    return candidate if candidate.is_file() else None


def load_transcript():
    """Return the transcript mapping, or None when no transcript was written."""
    path = transcript_path()
    if path is None or not path.is_file():
        return None
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


#: The compiler used for the fixture crate. The crate has no dependencies, so
#: ``rustc`` alone is enough; cargo would want a registry and a lockfile.
RUSTC = "rustc"

#: The crate name the fixture is compiled under, whatever the candidate calls
#: its own package: the hidden test is written against ``candidate``.
CRATE_NAME = "candidate"


def require_rustc() -> str:
    """Return the path to ``rustc``, or raise when it is not installed.

    The case cannot be judged without a Rust compiler, so this never degrades
    into a skip: an absent toolchain is a loud failure, not a pass.
    """
    found = shutil.which(RUSTC)
    if found is None:
        raise RuntimeError(
            "rustc was not found on PATH. The read-search-patch-rust case "
            "compiles its fixture with rustc, so it cannot be verified "
            "without a Rust toolchain (see https://rustup.rs)."
        )
    return found


def build_and_run(rustc, crate_root: Path, test_source: Path, workdir: Path):
    """Compile the candidate crate and one test file against it, then run it.

    The crate is compiled straight from ``crate_root/src/lib.rs`` into
    ``workdir`` as an rlib named ``candidate``; the test file is then linked
    against it with ``--extern candidate=<rlib>`` and run. Nothing is written
    into the case tree, so a verifier run leaves no artifacts behind.

    Returns ``(completed_process, None)`` when the test ran, and
    ``(None, stderr)`` when the crate or the test failed to build.
    """
    lib = workdir / f"lib{CRATE_NAME}.rlib"
    built = subprocess.run(
        [rustc, "--edition", "2021", "--crate-type", "rlib",
         "--crate-name", CRATE_NAME, str(crate_root / "src" / "lib.rs"),
         "--out-dir", str(workdir)],
        capture_output=True, text=True, timeout=180, check=False)
    if built.returncode != 0:
        return None, built.stderr
    test_bin = workdir / "case_tests"
    compiled = subprocess.run(
        [rustc, "--edition", "2021", "--test",
         "--extern", f"{CRATE_NAME}={lib}", str(test_source),
         "-o", str(test_bin)],
        capture_output=True, text=True, timeout=180, check=False)
    if compiled.returncode != 0:
        return None, compiled.stderr
    ran = subprocess.run([str(test_bin)], capture_output=True, text=True,
                         timeout=180, check=False)
    return ran, None
