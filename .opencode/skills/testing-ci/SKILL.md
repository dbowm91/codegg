---
name: testing-ci
description: Select focused CodeGG verification and report CI evidence accurately
version: 1.0.0
tags: [testing, ci, verification]
---

# Testing and CI

Use this guide when choosing verification for a CodeGG change. The repository
verification entry point is `scripts/verify.sh`: `quick` runs formatting,
boundary and contract guards plus a workspace check; `full` adds Clippy and
the workspace test suites; `scoped` runs only the tests a change can affect.
Read `AGENTS.md` for the current exact command definitions because the
verifier evolves with the workspace.

## Choose evidence by change

- Start with the narrowest test target covering changed behavior.
- Run `scripts/verify.sh quick` for routine repository sanity.
- Run `scripts/verify.sh scoped [base-ref]` when a change touches one or two
  subsystems and the full suite is not warranted. It computes the affected
  members and test targets from `docs/test-ownership.toml` and runs only
  those. The default base is `origin/main`; it compares against the merge
  base, and it includes uncommitted work. It is **additive** to the guards —
  every quick guard still runs — and it never runs less than the manifest
  declares for an affected member: a changed file with no mapping widens the
  affected member, a changed file outside every member (a script, a doc,
  a workflow) widens the scope to the whole workspace, and a changed file in
  a crate that **another crate depends on** widens every transitive dependent
  to its full surface. That last rule is the one people are surprised by:
  the root package depends on all eleven other workspace crates and re-exports
  several of them, so a change to a leaf crate reaches the root package's
  tests through the root's own source — a path rule cannot see it, and the
  manifest's `[[dependencies]]` section is what closes the gap. It prints the
  scope it selected and why, including any cargo features the selected
  targets are gated on. The hosted CI job's scope is unaffected by it.
- Run `scripts/verify.sh full` for broad handoff qualification when the change
  affects shared contracts or when the owning plan requires it.
- Run change-triggered guards named in `AGENTS.md`; a green CI subset does not
  imply every quick guard ran.
- Separate local command results from hosted CI results. Report the command,
  exit status, and any skipped or unavailable evidence.

Do not claim a test passed because a build succeeded. Do not run
`--all-features` for workspace sweeps: real-server tests need separately
installed servers. Use the feature set documented in `AGENTS.md`.

## Test targets are families

The root package's integration tests are **families**: one cargo executable
per subject area, under `tests/<family>/`. `cargo test --test <member>` does
not select a single member any more — the unit of selection is the family.
Use `cargo test --test <family> -- <member>` to narrow to one member, and
remember that a test's full path is now `family::member::test_name`, which is
what `--exact` matches against. `scripts/check_test_target_references.py`
fails if a nextest or CI filter names a target that does not exist.

## Source of truth

- `scripts/verify.sh` defines quick, full, and scoped verification.
- `docs/test-ownership.toml` maps source paths to the test targets that
  cover them; `scripts/check_test_ownership.py` keeps it honest.
- `architecture/testing.md` describes test classes and execution policy.
- `AGENTS.md` lists feature and change-triggered guard requirements.

## Test bodies must assert something

Two guards enforce the *quality* of a test body, not just its presence.
Both run in `verify.sh quick` and in CI, and both are built on
`scripts/rust_test_scan.py`:

- `scripts/check_test_assertion_quality.py` — a body that asserts nothing
  (no panic-capable construct, and no delegation to a helper that asserts)
  must be recorded in `docs/test-assertion-quality.toml` with the reason it
  asserts nothing. A test that asserts nothing costs a compile-and-run cycle
  to prove nothing; a test that asserts the *wrong* thing is worse, because
  it reports green.
- `scripts/check_test_duplication.py` — two test bodies with the same
  normalised content must not both survive, unless
  `docs/test-duplication.toml` records which one is the survivor and why.
  The duplicate detector is **content-preserving**: a literal is replaced by
  a marker derived from its content, so two calls that differ only in a
  string argument stay distinct. Blanking literals instead reports 456
  groups where the true figure is 89.

When you add a test, run `python3 scripts/rust_test_scan.py --validate` for
the ground-truth function counts. **Never scan Rust with a regex**: a regex
scan reads `&'a T` as a character literal and undercounts `tests/` by 76%.
