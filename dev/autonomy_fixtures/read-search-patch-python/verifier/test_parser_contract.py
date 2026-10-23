"""The independent verifier for the record-parser case.

This verifier sits outside the candidate-editable fixture tree. It does not
trust the fixture's own test file: it re-derives the contract itself and
checks that the frozen files the candidate was given were not edited.

It FAILS while the task is unsolved. The base implementation drops trailing
empty fields, so the contract assertions below fail against the frozen
baseline.
"""
from __future__ import annotations

import hashlib
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from loader import case_root, load_parser  # noqa: E402


#: Digests of the frozen files the candidate was given and must not edit.
FROZEN_DIGESTS = {
    "fixture/tests/test_parser.py":
        "ac6e7ac8e8a2ac7602d2b23871ca13c25bec2f768228b5ab499d9083e47d251a",
    "fixture/src/tokenizer.py":
        "bd97f81c75c21898466bc17f3bf7db4ffcc46cbe00a9c23e04ae8beb293aed5d",
}


class FrozenFixtureTests(unittest.TestCase):
    def test_the_given_files_were_not_edited(self) -> None:
        root = case_root()
        for relative, expected in FROZEN_DIGESTS.items():
            path = root / relative
            self.assertTrue(path.is_file(), f"the frozen file is missing: {relative}")
            actual = hashlib.sha256(path.read_bytes()).hexdigest()
            self.assertEqual(actual, expected,
                             f"the candidate edited a frozen file: {relative}")


class ParserContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.parser = load_parser()

    def test_a_plain_record_keeps_every_field(self) -> None:
        self.assertEqual(self.parser.parse_record("a,b,c"), ["a", "b", "c"])

    def test_a_trailing_empty_field_is_significant(self) -> None:
        self.assertEqual(self.parser.parse_record("a,b,"), ["a", "b", ""])

    def test_an_empty_record_is_one_empty_field(self) -> None:
        self.assertEqual(self.parser.parse_record(""), [""])

    def test_surrounding_whitespace_is_normalized(self) -> None:
        self.assertEqual(self.parser.parse_record(" a , b "), ["a", "b"])

    def test_an_interior_empty_field_is_kept(self) -> None:
        self.assertEqual(self.parser.parse_record("a,,b"), ["a", "", "b"])


if __name__ == "__main__":
    unittest.main()
