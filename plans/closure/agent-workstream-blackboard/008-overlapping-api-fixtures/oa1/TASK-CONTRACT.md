# Task contract: overlapping project-summary actor API

Starting source baseline: the accepted SI-1/S0 candidate commit
`9260fda525284b821beda2782de23f6888d37b56`. The frozen evaluator adds actor
vectors to the Rust and Python suites plus a cross-language CLI check.

Extend the project-summary contract with an optional `actor` event field.
Normalize actor values by trimming leading and trailing ASCII whitespace while
preserving case. A normalized actor must contain 1–64 ASCII bytes and match
`[A-Za-z0-9][A-Za-z0-9._@-]{0,63}`. Reject empty, non-ASCII, internal
whitespace, and other punctuation. The field may appear on any event kind.

Preserve v1 behavior exactly when no input event has an actor: emit schema
`codegg.project-summary.v1`, omit actor fields, and continue accepting frozen
v1 summaries. If at least one event has an actor, emit schema
`codegg.project-summary.v2`; actor is optional on each event in v2. The
consumer must validate v1 and v2, reject actor fields in v1, validate and
normalize v2 actors according to this contract, and render an Actor column for
file-change rows only in v2 reports. The v2 JSON schema and expected summary and
report bytes are frozen in this fixture. Do not weaken v1 validation or modify
any evaluator file.

Source changes are restricted to `rust/src/`, `python/summary_report/`, and
`interface/project-summary-v2.schema.json`. The evaluator files under
`rust/tests/`, `python/tests/`, `fixtures/`, and `scripts/` are immutable.

Acceptance from the fixture root:

```text
cargo test --locked --workspace
python3 -m unittest discover -s python/tests -v
python3 scripts/check_actor_v2.py
```
