import unittest
from ranges import merge_intervals, intersect_intervals, subtract_intervals

class RangeTests(unittest.TestCase):
    def test_merge_sorts_and_merges_overlap_and_touch(self):
        self.assertEqual(merge_intervals([(8, 10), (1, 3), (3, 5), (4, 9), (12, 13)]), [(1, 10), (12, 13)])
    def test_merge_empty_and_single(self):
        self.assertEqual(merge_intervals([]), [])
        self.assertEqual(merge_intervals([(2, 4)]), [(2, 4)])
    def test_intersection(self):
        self.assertEqual(intersect_intervals([(1, 5), (8, 12)], [(3, 9)]), [(3, 5), (8, 9)])
    def test_intersection_empty(self):
        self.assertEqual(intersect_intervals([], [(1, 3)]), [])
    def test_subtract_middle(self):
        self.assertEqual(subtract_intervals([(1, 10)], [(4, 6)]), [(1, 4), (6, 10)])
    def test_subtract_multiple_overlapping_blockers(self):
        self.assertEqual(subtract_intervals([(0, 20)], [(3, 7), (5, 12), (15, 30)]), [(0, 3), (12, 15)])
    def test_subtract_blocker_outside_source(self):
        self.assertEqual(subtract_intervals([(5, 10)], [(0, 4), (10, 12)]), [(5, 10)])
    def test_input_not_mutated(self):
        source = [(5, 9), (1, 3)]
        before = source.copy()
        merge_intervals(source)
        self.assertEqual(source, before)
    def test_invalid_shapes_rejected(self):
        for intervals in ([(1,)], [(1, 2, 3)], [1], ["1,2"]):
            with self.subTest(intervals=intervals), self.assertRaises(ValueError):
                merge_intervals(intervals)
    def test_invalid_bounds_rejected(self):
        for interval in ((-1, 2), (2, 2), (4, 1), (True, 3), (1, False)):
            with self.subTest(interval=interval), self.assertRaises(ValueError):
                merge_intervals([interval])
    def test_intersection_and_subtraction_share_validation(self):
        with self.assertRaises(ValueError):
            intersect_intervals([(1, 3)], [(0, 0)])
        with self.assertRaises(ValueError):
            subtract_intervals([(1, 3)], [(0, -1)])

if __name__ == "__main__":
    unittest.main()
