# M008 shared-interface coding task (frozen fixture v1)

This fixture supplies one provider-backed Rust/Python workload for M008.
The Rust `summary-producer` crate consumes event JSONL and emits the frozen
`codegg.project-summary.v1` document. The Python `summary_report` package
validates that exact document and renders a deterministic Markdown report.
The JSON Schema in `interface/` is the shared interface and is immutable for
this task version.

## Task input

Implement the Rust producer and Python consumer described by
`TASK-CONTRACT.md`. Keep the Rust and Python implementations in their own
directories. Do not change the interface schema, fixture inputs, or tests.
The integrated result must pass both language test suites and the frozen
cross-language golden test. Rust and Python work are coupled through the
versioned summary contract: the producer's serialized output is the consumer's
input.

For S0, one provider-backed agent receives the whole task. For S1/S2/S3, two
provider-backed agents receive the same frozen contract, with one assigned the
Rust producer and one the Python consumer. Both child tasks must finish and
integrate into the same parent revision before evaluation. S1 has no peer
board; S2 adds factual peer observations; S3 adds observations and atomic
claims. Never use a shared checkout for these trials.

## Frozen verification

From this directory:

```bash
cargo test --locked --workspace
python3 -m unittest discover -s python/tests -v
python3 scripts/check_cross_language_golden.py
```

The initial tests intentionally fail because the implementation is incomplete.
Record the exact frozen fixture tree digest in the M008 trial record before
the first agent sees a copy. Do not modify this fixture after a trial starts;
make corrections as a separately versioned task fixture.
