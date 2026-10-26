# Exact-HEAD CI E001 — Restore Strict Workspace Clippy Signal

Status: ready
Baseline: `main@957fcbf00e17bf108429f0d75dcbf760736ae511`, CI `40324996955` 2026-10-26, failed Workspace Clippy.
Source: `plans/subsystems/ci-exact-head-signal-corrective-addendum.md`.
Primary class: infrastructure/invariant; no new ADR.

## 1. Objective

Restore the actual repository's CI all-targets lint gate without relaxing `-D warnings` or papering over source changes. The exact-head E001 failure is independent of the Harness Lab's Python suite and must be independently resolved before claiming green mainline CI.

## 2. Current evidence

`cargo clippy --workspace --all-targets --locked -- -D warnings` on Rust 1.99.0 failed with a dozen warnings treated as errors. Recorded error locations/classes:
- `src/eggsact/adapter.rs:306` collapsible_if.
- `src/search_backend/eggsearch.rs:227` manual_clamp.
- `src/test_runner/index.rs:396`; `src/tool/deterministic.rs:438` manual_contains.
- `src/tool/edit.rs:355` type_complexity.
- `src/tool/task.rs:495` question_mark.
- `src/search_backend/framing.rs:614,616,619,623` bool_comparison (test target).
- `src/test_runner/runner.rs:1539` field_reassign_with_default.
- `src/tool/skill.rs:174` items_after_test_module.
First inspect current main and exact GitHub log before patching; these numbers are observed baseline, not guaranteed current. Previous `CI / verify` failure `40224670033` is a different M015 DB-lock/test incident E002.

## 3. Invariants and dependencies

None beyond existing Cargo lock/toolchain and CI workflow contract; may proceed in parallel with E002. No changing Clippy level, skip target, blanket allow, muting tests or removing source behavior. Where suggested idiom might alter error semantics, use equivalence test first. Tooling/toolchain pin changes require independent platform parity evidence; prefer local narrow source fixes.

## 4. Work packages

A. **Reproduce and enumerate** strict workspace/all-targets warnings on pinned hosted-equivalent Rust; capture diagnostic categories, source SHA and check whether any warning is new owing to an updated toolchain vs a source commit. Distinguish CI-only from local toolchain drift.

B. **Minimal equivalent fixes** at each named source site: collapse if without changing conditional priority; clamp only if bounds are constant/ordered; use contains preserving lifetime/equality semantics; name the edit-strategy function-pointer alias instead of suppressing type-complexity; replace let-else with ? preserving None; fix bool comparisons and struct initialization; move test module or helpers while preserving item scopes. No broad formatter refactor.

C. **Failing-first tests / negative controls:** exercise original input/output edge cases for provider parser, Eggsearch bounds, tool-list search, edit strategy selection, task parser and tool skills; verify any existing tests still run rather than being skipped. If a specific Clippy lint is demonstrably incorrect, a narrowly documented exception requires explicit maintainer approval and a standalone regression test.

D. **Exact-head CI:** run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `scripts/verify.sh quick`, targeted Rust tests, then obtain hosted `CI / verify` on the exact implementation SHA. If tests now fail on M015 locking, classify E001 as source-lint complete, **not** fully green CI; route test failure to E002.

## 5. Security / concurrency / compatibility

Pure syntactic fixes shouldn't alter effective permissions, provider API errors, tool registration or file effects. Preserve Clippy in production **and test targets**, because current failure includes `lib test`. No wire/database/config migration. Parallel code changes must avoid overwriting unrelated M015/agent tool patches.

## 6. Acceptance / stop / closure

Accept E001 only on exact all-target Clippy zero errors with original tests still executing, no unapproved blanket allow and documented matched Rust toolchain. Hosted CI may remain failed for **independent E002**, which must be named rather than misrepresented.

Stop for uncontrolled toolchain upgrade or behavior-dependent lints that require a design decision. Record closure at `plans/closure/ci-exact-head-signal/001-status.md`: before/after 12-site map, tests and counts, exact SHA, hosted CI URL/conclusion and E002 status. Update roadmap/registry transactionally.
