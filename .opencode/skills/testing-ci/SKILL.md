---
name: testing-ci
description: Choose focused CodeGG verification commands and interpret the canonical quick and full checks.
---

# Testing and CI

Use the narrowest check that covers the change, then run `scripts/verify.sh quick` for a broad local sanity pass. The `full` mode also runs workspace clippy and tests and needs `cargo-nextest`.

## Test selection

- Rust module behavior: `cargo test -p codegg <module-filter>`.
- Integration behavior: `cargo test --test <target>`; inspect `tests/` and Cargo targets first.
- A feature-gated test must name its feature explicitly, such as `cargo test -p egglsp --features lsp-test-support --test scenario_engine`.
- Use `cargo nextest run --workspace --locked --profile ci` for the capped workspace suite when nextest is available.

Run `cargo fmt --all -- --check` after Rust changes. `scripts/verify.sh quick` is the repository's canonical routine sanity command; `scripts/verify.sh full` adds clippy and broad tests. Do not use `--all-features`: it enables real-server tests that need installed external servers.

Tests that mutate process-global environment should remain isolated by the repository's nextest profile. New async tests default to Tokio's current-thread flavor unless actual parallel execution is part of the behavior under test.

## Evidence

Report the exact command and result. A compile or formatting pass does not establish runtime behavior. See `architecture/testing.md` and `scripts/verify.sh` for current policy.
