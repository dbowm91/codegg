# Event ledger contract

Implement the workspace in dependency order. Input rows are UTF-8 text in the form
`timestamp|id|kind|amount|source`, one record per line. Timestamps and amounts are signed
integers; IDs are non-empty; kinds are `open`, `close`, or `transfer`. `src` and `source`
are aliases for the same source. Identical IDs deduplicate; conflicting IDs are errors.

Summaries use UTC-independent integer windows of 60 timestamp units and are sorted by window,
then ID. Journal entries are versioned rows with a checksum. Replaying twice is idempotent;
a malformed final line is treated as a torn write, while malformed interior lines are errors.

The CLI accepts `ingest <journal> <shard>...`, `summarize <shard>...`, `inspect <journal>`, and
`export <input> <output>`. Ingest and summarize merge every supplied shard. Errors go to stderr and return non-zero. Do not add dependencies;
the locked offline build is part of the task.

`ledger-index/tests/high_volume.rs` is an additional task-owned stress regression. It performs 24
deterministic merges of 500,000 records each, including an identical duplicate shard, and checks
ordering and aggregate counts. Keep this test unchanged. It is intended to provide real background
validation time while the dependent CLI work continues; do not use sleeps or idle waits to extend it.
