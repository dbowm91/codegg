# Shell Output Projection Phase 9: Evaluation Harness and Regression Corpus

## Objective

Create a regression and evaluation harness for command-output projection quality. The goal is to measure both token reduction and correctness preservation across raw/truncated, native structured, RTK-backed, and aggressive projection paths.

Projection is only valuable if it preserves the actionable cause of a command result. This phase should make that measurable.

## Dependency

This phase assumes:

- Generic projectors exist.
- Native Git/Rust projectors exist.
- RTK invocation or at least RTK skeleton/fallback behavior exists.
- Expansion handles exist or are close enough to validate omitted-range recoverability.
- Redaction is implemented or clearly marked as a separate axis in evaluation.

## Evaluation Principles

Do not evaluate only compression ratio. A smaller projection that hides the root cause is worse than raw output. Track both size and preservation.

Required dimensions:

- input bytes
- projected bytes
- approximate input tokens
- approximate projected tokens
- projector selected
- exactness/lossiness
- exit-state preservation
- stderr preservation
- failure-cause preservation
- source-span preservation
- omitted-range recoverability
- redaction behavior
- warnings emitted

## Fixture Corpus

Create a fixture directory, for example:

```text
tests/fixtures/shell_projection/
```

Suggested categories:

### Rust

- successful `cargo check`
- failing `cargo check` with one error
- failing `cargo check` with multiple errors/warnings
- successful `cargo test` with long output
- failing `cargo test` with panic
- failing `cargo test` with assertion diff
- colored cargo output
- JSON message-format diagnostics if supported

### Git

- clean status
- dirty status with staged/unstaged/untracked files
- merge conflict status
- small diff
- large multi-file diff
- binary diff marker
- rename/copy diff
- log output with many commits

### Python

- successful pytest
- failing pytest assertion
- traceback
- ruff/mypy/pyright style diagnostics

### JavaScript/TypeScript

- TypeScript compiler error
- ESLint output
- Jest/Vitest passing and failing output

### Generic shell/search

- long `rg` output
- long `find`/`tree` output
- mixed stdout/stderr
- non-UTF-8 bytes
- ANSI-colored output
- timeout/cancelled/spawn-failed synthetic events

### Security/redaction

- synthetic fake credential-like output
- HTTP header dump with fake sensitive values
- connection-string-like fake output
- false-positive prose fixtures

Do not include real secrets. All sensitive fixtures must use fake values.

## Golden Assertions

Each fixture should declare expected preservation invariants. A simple metadata file can work:

```toml
[fixture]
name = "cargo_test_failure_assertion_diff"
command = "cargo test"
exit_code = 101

[expect]
must_contain = ["FAILED", "test_name", "assertion"]
must_not_contain = []
source_spans = ["src/lib.rs:42"]
failed_tests = ["tests::example_failure"]
max_projected_tokens_safe = 4000
max_projected_tokens_aggressive = 1500
```

The harness should compare projections against these invariants rather than relying on exact string snapshots only. Exact snapshots are useful for small, stable cases, but invariant tests are less brittle.

## Projection Matrix

Run every fixture through:

- raw/off policy
- safe policy
- aggressive policy
- RTK policy with RTK unavailable
- RTK policy with RTK available, if installed
- native-preferred true/false where relevant

When RTK is unavailable, tests should verify safe fallback. When RTK is available, optional tests should record RTK results and compare invariants.

## Metrics Output

Add a developer-facing report command or test helper that prints a table:

```text
fixture                         projector              input_tok  output_tok  reduction  invariants
cargo_test_failure_assertion     native-cargo-test      18320      1420        92.2%      pass
git_diff_large                   native-git-diff        50200      2600        94.8%      pass
unknown_failed_command           error-retention        12000      2100        82.5%      pass
```

This does not need to be user-facing initially. It should help maintainers see regression trends.

## Tests

Add tests for:

1. Fixture loader parses metadata.
2. Every fixture produces a projection without panic.
3. Safe policy preserves required invariants.
4. Aggressive policy preserves minimum invariants.
5. RTK-unavailable policy falls back safely.
6. Native projectors meet source-span/failure preservation expectations.
7. Omitted ranges have corresponding expansion handles.
8. Redaction fixtures redact fake sensitive values.
9. False-positive fixtures remain useful.
10. Token estimates are monotonic enough for budget checks.

## Success Criteria

- A fixture corpus exists across Rust, Git, and generic shell outputs at minimum.
- The harness evaluates both compression and correctness preservation.
- Safe policy passes all critical preservation invariants.
- Aggressive policy has explicit, lower but documented invariants.
- RTK behavior is evaluated when available and skipped cleanly otherwise.
- Metrics can be produced for maintainers.

## Non-Goals

- Do not require RTK in CI.
- Do not build a full benchmarking dashboard.
- Do not make exact string snapshots the only assertion mechanism.
- Do not include real secrets in fixtures.

## Handoff Notes

This phase should become the safety net for all future projection work. Any new projector, RTK mode, redaction rule, or context-budget integration should add fixtures and invariants here.
