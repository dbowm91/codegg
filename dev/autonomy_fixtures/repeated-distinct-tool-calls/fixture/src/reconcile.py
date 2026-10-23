"""The reconciliation rules.

`reconcile` matches the bank lines against the book entries and returns the
matched pairs plus whatever was left over on either side.
"""
from __future__ import annotations

from dataclasses import dataclass, field

from ledger import Entry


@dataclass
class ReconcileResult:
    """The outcome of one reconciliation."""

    matched: list[tuple[Entry, Entry]] = field(default_factory=list)
    unmatched_bank: list[Entry] = field(default_factory=list)
    unmatched_book: list[Entry] = field(default_factory=list)

    @property
    def reconciled_amount(self) -> int:
        """The sum of the *bank* amounts of the matched pairs."""
        return sum(bank.amount for _, bank in self.matched)


def reconcile(book: list[Entry], bank: list[Entry]) -> ReconcileResult:
    """Match every bank line to at most one book entry.

    The rules, in full:

    * a bank line matches a book entry when their references normalize to the
      same string (see `textutil.normalize_reference`);
    * the amount and the date are **not** compared. A bank line that carries a
      different amount than the book entry is still reconciled, and the bank
      amount is the one the result reports;
    * the first still-unmatched book entry with a matching reference is used,
      in the order the book entries were given;
    * a bank line that matches nothing stays in `unmatched_bank`, and a book
      entry that no bank line claimed stays in `unmatched_book`.
    """
    result = ReconcileResult()
    used = set()
    for bank_entry in bank:
        for position, book_entry in enumerate(book):
            if position in used:
                continue
            if book_entry.key == bank_entry.key:
                used.add(position)
                result.matched.append((book_entry, bank_entry))
                break
        else:
            result.unmatched_bank.append(bank_entry)
    result.unmatched_book = [entry for position, entry in enumerate(book)
                             if position not in used]
    return result
