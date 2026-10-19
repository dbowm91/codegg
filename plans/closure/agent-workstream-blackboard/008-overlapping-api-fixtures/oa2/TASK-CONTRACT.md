# Task contract: safe project-summary source revisions

Starting source baseline: the accepted SI-1/S0 candidate commit
`9260fda525284b821beda2782de23f6888d37b56`. The frozen evaluator adds
malformed-revision and Markdown code-span cases to the Rust and Python suites
plus a cross-language CLI check.

Enforce the same `source_revision` rule in the Rust producer and Python
consumer: it must be a string of 1–128 Unicode scalar values, contain no
Unicode control characters, and have no leading or trailing whitespace. Do not
trim or rewrite accepted revisions. Reject invalid values in both languages
with actionable errors. Keep existing v1 summaries that meet this rule valid.

Render source revisions as safe Markdown code spans in the report. Use a
backtick delimiter one longer than the longest consecutive backtick run in the
revision. The value may include backticks, pipes, backslashes, or angle
brackets; the Markdown renderer must not interpret any of them as markup. Keep
all other v1 output bytes unchanged. The frozen expected report uses revision
`rev`|<x>`.

Source changes are restricted to `rust/src/` and `python/summary_report/`.
The evaluator files under `rust/tests/`, `python/tests/`, `fixtures/`, and
`scripts/` are immutable.

Acceptance from the fixture root:

```text
cargo test --locked --workspace
python3 -m unittest discover -s python/tests -v
python3 scripts/check_source_revision.py
```
