"""Normalization helpers shared by the ledger and the reconciler.

The two systems that produce the statements disagree about punctuation: one
writes ``INV-1042`` and the other writes ``inv 1042``. `normalize_reference`
is the single place that decides what "the same reference" means.
"""
from __future__ import annotations

#: The separators the two source systems use differently.
SEPARATORS = ("-", "_", " ", ".")


def normalize_reference(value: str) -> str:
    """Return the comparison form of a reference.

    The reference is upper-cased and every separator the two systems use
    differently is removed, so ``INV-1042``, ``inv_1042`` and ``inv 1042`` all
    normalize to ``INV1042``.
    """
    cleaned = value.strip().upper()
    for separator in SEPARATORS:
        cleaned = cleaned.replace(separator, "")
    return cleaned
