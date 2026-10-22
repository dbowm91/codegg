"""The provided test suite for the reconciler.

This file is part of the frozen fixture. It states the behaviour the modules
must have, so it must not be edited: the independent verifier in
`../verifier` checks the digests of the frozen files.
"""
from __future__ import annotations

from datetime import date
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from ledger import Entry, net_amount, total_by_reference  # noqa: E402
from reconcile import reconcile  # noqa: E402
from textutil import normalize_reference  # noqa: E402


def entry(reference: str, amount: int, day: int = 1) -> Entry:
    """Build one ledger entry."""
    return Entry(booked_on=date(2024, 5, day), reference=reference,
                 amount=amount, description=reference)


class NormalizeReferenceTests(unittest.TestCase):
    def test_the_separators_are_removed(self):
        self.assertEqual(normalize_reference("INV-1042"), "INV1042")

    def test_the_case_is_ignored(self):
        self.assertEqual(normalize_reference("inv_1042"), "INV1042")


class LedgerTests(unittest.TestCase):
    def test_the_net_amount_sums_the_entries(self):
        self.assertEqual(net_amount([entry("A", 500), entry("B", -200)]), 300)

    def test_entries_are_grouped_by_normalized_reference(self):
        totals = total_by_reference([entry("INV-1", 100), entry("inv 1", 50)])
        self.assertEqual(totals, {"INV1": 150})


class ReconcileTests(unittest.TestCase):
    def test_a_matching_reference_reconciles(self):
        result = reconcile([entry("INV-1", 100)], [entry("inv-1", 100)])
        self.assertEqual(len(result.matched), 1)
        self.assertEqual(result.unmatched_bank, [])
        self.assertEqual(result.unmatched_book, [])

    def test_an_amount_mismatch_is_still_reconciled(self):
        result = reconcile([entry("INV-1", 100)], [entry("INV-1", 999)])
        self.assertEqual(len(result.matched), 1)
        self.assertEqual(result.reconciled_amount, 999)

    def test_a_date_mismatch_is_still_reconciled(self):
        result = reconcile([entry("INV-1", 100, day=1)],
                           [entry("INV-1", 100, day=9)])
        self.assertEqual(len(result.matched), 1)

    def test_an_unmatched_bank_line_is_reported(self):
        result = reconcile([], [entry("INV-9", 100)])
        self.assertEqual(result.unmatched_bank[0].reference, "INV-9")

    def test_a_book_entry_nobody_claimed_is_reported(self):
        result = reconcile([entry("INV-1", 100)], [])
        self.assertEqual(result.unmatched_book[0].reference, "INV-1")

    def test_the_first_free_book_entry_with_the_reference_is_used(self):
        result = reconcile([entry("INV-1", 100), entry("INV-1", 200)],
                           [entry("INV-1", 100)])
        self.assertEqual(result.matched[0][0].amount, 100)
        self.assertEqual(result.unmatched_book[0].amount, 200)


if __name__ == "__main__":
    unittest.main()
