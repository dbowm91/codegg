# ledger-reconcile

A small accounting helper. It reconciles a **book** (what the ledger recorded)
against a **bank** statement (what the money actually did) and reports the
lines that did not match.

## Layout

- `src/textutil.py` — the normalization helpers shared by the other modules.
- `src/ledger.py` — the `Entry` record and the totals over a list of entries.
- `src/reconcile.py` — the matching rules.
- `tests/test_reconcile.py` — the test suite.

## Running the tests

    python3 -m unittest discover -s fixture/tests -v

The suite shows what the matcher actually does; `src/reconcile.py` states the
rules it encodes.
