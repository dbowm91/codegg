# Task contract: deterministic project summary producer and consumer

Implement a two-language audit-summary pipeline. Keep code changes confined to
`rust/` and `python/`; `interface/`, `fixtures/`, `tests/`, and the verification
script are frozen evaluator inputs.

## Rust producer

Implement `summary-producer` as a library plus a small CLI. It reads newline-
delimited JSON events and emits one compact JSON document conforming to
`interface/project-summary-v1.schema.json`.

Each input event has `id`, `occurred_at`, `kind`, and optional `path`,
`insertions`, and `deletions` fields. `kind` is one of `file_changed`,
`test_passed`, or `test_failed`. File-change events require a relative,
normalized `path` and nonnegative integer insertion/deletion counts. Test
events do not accept file paths or line counts. Reject unknown fields, invalid
timestamps, malformed JSON, empty identifiers, unsafe paths (absolute,
parent-traversing, empty, or backslash-containing), and invalid kind-specific
fields with an actionable error that identifies the input line.

Normalize timestamps to UTC RFC 3339 with seconds precision. Sort output
events by normalized timestamp and then `id`. Exact duplicate IDs with
semantically identical normalized event data are idempotent; the same ID with
different data is a conflict and must fail. Aggregate `totals.events`,
`totals.files_changed`, `totals.insertions`, and `totals.deletions`, plus
`by_kind` counts. Use checked arithmetic and reject overflow. Serialize
deterministically.

CLI contract:

```text
summary-producer --source-revision <nonempty-revision> [< input.jsonl]
```

Emit the summary on stdout and diagnostics on stderr. An empty input is a
valid zero-event summary. Invalid input exits nonzero and must not emit a
partial JSON document.

## Python consumer

Implement `summary_report` as a standard-library-only package and CLI. It
loads a summary document and validates the full v1 contract before rendering:
the exact schema marker, field types, allowed event kinds, UTC timestamps,
safe relative paths, sorted unique event IDs/order, and all totals and
`by_kind` aggregates must agree with the event list. Reject unknown fields,
unsupported versions, booleans where integers are required, negative counts,
and inconsistent totals with actionable errors.

Render a stable Markdown report containing the source revision, event count,
file-change count, insertion/deletion totals, counts by kind in the frozen
kind order, and one row per file-change event. Escape Markdown table syntax in
paths. Output must be byte-for-byte deterministic for equivalent valid input.
The CLI accepts an input file path or `-` for stdin and writes only the report
to stdout; errors go to stderr with a nonzero exit status.

## Shared interface and completion

The producer output must be accepted unchanged by the Python consumer. Keep
the schema marker, required fields, meanings, normalization, and ordering
identical on both sides. Do not weaken validation to make the golden fixture
pass. The frozen golden test invokes the Rust binary, parses its JSON, feeds
that exact JSON to Python, and checks the expected Markdown bytes.

The task is complete when Rust unit/CLI tests, Python unit/CLI tests, and the
cross-language golden test all pass from a clean fixture checkout. Report
remaining limitations rather than relaxing the contract.
