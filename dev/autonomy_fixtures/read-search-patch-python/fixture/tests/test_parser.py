"""The provided test for the record parser.

This file is part of the frozen fixture. It is the case's stated failure
condition, so it must not be edited: the independent verifier in `../verifier`
checks its digest.
"""
import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from parser import parse_record  # noqa: E402


class ParserTests(unittest.TestCase):
    def test_a_plain_record_keeps_every_field(self):
        self.assertEqual(parse_record("a,b,c"), ["a", "b", "c"])

    def test_a_trailing_empty_field_is_significant(self):
        self.assertEqual(parse_record("a,b,"), ["a", "b", ""])

    def test_an_empty_record_is_one_empty_field(self):
        self.assertEqual(parse_record(""), [""])

    def test_surrounding_whitespace_is_normalized(self):
        self.assertEqual(parse_record(" a , b "), ["a", "b"])


if __name__ == "__main__":
    unittest.main()
