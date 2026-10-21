# Task 2: Integer range operations

Implement `ranges.py` from this specification. Keep it dependency-free and compatible with Python 3.11+. Run `python3 -m unittest -v` until the supplied tests pass.

All intervals are half-open `(start, end)` pairs of non-negative integers; booleans are invalid despite being Python integers, and `start < end` is required. Invalid inputs raise `ValueError`.

`merge_intervals(intervals)` returns a sorted list of disjoint intervals, merging both overlaps and touching boundaries. `intersect_intervals(left, right)` returns the intersection of two interval sets as sorted disjoint intervals. `subtract_intervals(source, blockers)` returns the portions of source not covered by blockers. Inputs may be unsorted and overlapping. Functions must not mutate their inputs and should handle empty lists.
