import unittest

from retry_policy import retry_delay_ms


class RetryDelayTests(unittest.TestCase):
    def test_exponential_growth_is_capped(self):
        self.assertEqual(retry_delay_ms(1, 100, 450), 100)
        self.assertEqual(retry_delay_ms(2, 100, 450), 200)
        self.assertEqual(retry_delay_ms(3, 100, 450), 400)
        self.assertEqual(retry_delay_ms(4, 100, 450), 450)

    def test_retry_after_is_a_lower_bound_before_the_cap(self):
        self.assertEqual(retry_delay_ms(1, 100, 450, 250), 250)
        self.assertEqual(retry_delay_ms(3, 100, 450, 250), 400)
        self.assertEqual(retry_delay_ms(1, 100, 450, 900), 450)

    def test_cap_below_base_and_zero_cap_are_valid(self):
        self.assertEqual(retry_delay_ms(1, 100, 50), 50)
        self.assertEqual(retry_delay_ms(2, 100, 0, 900), 0)

    def test_invalid_inputs_raise_value_error(self):
        for values in ((0, 100, 450, None), (1, -1, 450, None),
                       (1, 1, -1, None), (1, 1, 10, -1),
                       (True, 1, 10, None), (1, True, 10, None),
                       (1, 1, False, None), (1, 1, 10, True)):
            with self.subTest(values=values):
                with self.assertRaises(ValueError):
                    retry_delay_ms(*values)

    def test_large_attempt_saturates_without_huge_integer_work(self):
        self.assertEqual(retry_delay_ms(10**9, 1, 900), 900)


if __name__ == "__main__":
    unittest.main()
