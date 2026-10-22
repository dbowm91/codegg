"""The independent verifier for the Rust record-parser case.

This verifier sits outside the candidate-editable fixture tree. It does not
trust the fixture's own test file: it re-derives the contract itself with a
hidden test that lives here, and it checks that the frozen files the
candidate was given were not edited.

It FAILS while the task is unsolved. The shipped implementation folds away a
record's trailing empty fields, so the provided test and the hidden contract
test both fail against the baseline. It also fails loudly, never silently,
when the toolchain it needs is missing.
"""
from __future__ import annotations

import hashlib
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from loader import build_and_run, case_root, fixture_root, require_rustc  # noqa: E402


#: Digests of the frozen files the candidate was given and must not edit.
FROZEN_DIGESTS = {
    "fixture/tests/parser_test.rs":
        "fa399ced652bf47b79739f69cbb0e294a8059175fbfb07405544fc169cfb3b77",
}

#: The hidden contract test. It lives beside the verifier, never inside the
#: fixture, so the candidate cannot read it and cannot edit it.
HIDDEN_TEST = Path(__file__).resolve().parent / "hidden_contract_test.rs"


class FrozenFixtureTests(unittest.TestCase):
    def test_the_given_files_were_not_edited(self) -> None:
        root = case_root()
        for relative, expected in FROZEN_DIGESTS.items():
            path = root / relative
            self.assertTrue(path.is_file(), f"the frozen file is missing: {relative}")
            actual = hashlib.sha256(path.read_bytes()).hexdigest()
            self.assertEqual(actual, expected,
                             f"the candidate edited a frozen file: {relative}")


class RustCaseTests(unittest.TestCase):
    """Compile the candidate crate and run the tests against it."""

    def _run(self, test_source: Path) -> str:
        """Compile the fixture and run ``test_source`` against it.

        Returns the test binary's output. Fails the test when the toolchain
        or the crate is unusable and never skips: an unverifiable case is a
        failed case, not a pass.
        """
        try:
            rustc = require_rustc()
        except RuntimeError as exc:
            self.fail(str(exc))
        with tempfile.TemporaryDirectory() as temp:
            ran, error = build_and_run(rustc, fixture_root(), test_source, Path(temp))
        if ran is None:
            self.fail(f"{test_source.name} did not build against the candidate crate:\n{error}")
        self.assertEqual(
            ran.returncode, 0,
            f"{test_source.name} failed against the candidate crate:\n{ran.stdout}\n{ran.stderr}")
        return ran.stdout + ran.stderr

    def test_the_provided_test_passes(self) -> None:
        self._run(fixture_root() / "tests" / "parser_test.rs")

    def test_the_hidden_contract_test_passes(self) -> None:
        self._run(HIDDEN_TEST)


if __name__ == "__main__":
    unittest.main()
