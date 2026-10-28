import unittest

from src.parser import parse_count


class ParseCountTests(unittest.TestCase):
    def test_negative_count_is_rejected(self):
        with self.assertRaises(ValueError):
            parse_count("-2")

    def test_nonnegative_decimal_is_returned(self):
        self.assertEqual(parse_count("17"), 17)


if __name__ == "__main__":
    unittest.main()
