# Task 1: Worklog JSONL utility

Implement `worklog.py` from this specification. Keep it dependency-free and compatible with Python 3.11+. Run `python3 -m unittest -v` until the supplied tests pass.

`Worklog(path)` stores one JSON object per line. Each event has `id` (positive integer, assigned monotonically by append), `author` (non-empty string), `text` (non-empty string), and `created_at` (UTC RFC3339 string). `append(text, author, created_at=None)` validates inputs, uses current UTC time when omitted, appends safely, returns the created event, and creates parent directories. Explicit timestamps must be timezone-aware and serialized in UTC with a trailing `Z`.

`events(author=None, newest_first=False)` reads and validates the file, returns dictionaries, filters by exact author when provided, and reverses order only when requested. Missing files produce an empty list. Malformed rows raise `ValueError` with the line number.

`summarize()` returns exactly `{"count": N, "authors": {author: count, ...}, "first_at": timestamp-or-null, "last_at": timestamp-or-null}` using append order and sorted author keys.
