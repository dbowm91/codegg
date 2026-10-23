"""The ledger record and the totals computed over a list of records.

Amounts are integers in minor units (cents). Nothing here rounds or converts:
a ledger entry carries exactly the amount it was created with.
"""
from __future__ import annotations

from dataclasses import dataclass
from datetime import date

from textutil import normalize_reference


@dataclass(frozen=True)
class Entry:
    """One line of a statement."""

    booked_on: date
    reference: str
    amount: int
    description: str = ""

    @property
    def key(self) -> str:
        """The comparison form of this entry's reference."""
        return normalize_reference(self.reference)


def net_amount(entries) -> int:
    """Return the signed sum of ``entries``."""
    return sum(entry.amount for entry in entries)


def total_by_reference(entries) -> dict:
    """Return the net amount per normalized reference."""
    totals = {}
    for entry in entries:
        totals[entry.key] = totals.get(entry.key, 0) + entry.amount
    return totals
