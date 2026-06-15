# LSP Phase 2 Final Packaging Hygiene: Test-Support Isolation and Package Self-Containment

## Purpose

Complete the final packaging-hygiene work after:

```text
4f72a64a5f777e8682188edc97d2ebae63947595
99d8b11bfbeae6dd21a8b32783a1694200b8709c
```

The Phase 2 runtime, semantic, security, hunk-navigation, preview, diagnostics, node-limit, and depth-limit work is complete. The remaining issue is isolation of the scripted fake-server implementation and its tests from ordinary `egglsp` builds and packaged artifacts.

The current repository has the right single-source design:

```text
crates/egglsp/src/test_support.rs
    -> crates/egglsp/src/bin/egglsp-test-server.rs
    -> crates/egglsp-test-server/src/main.rs
```

Both binary wrappers are thin and call:

```rust
egglsp::test_support::run_or_exit();
```

However, `egglsp::test_support` is currently compiled and exposed unconditionally, even when the `lsp-test-support` feature is disabled. In addition, `crates/egglsp/tests/scenario_engine.rs` still includes a test source file from outside the `egglsp` package root.

This final pass should make the test-support code truly feature-gated, package-contained, and absent from ordinary production builds.

## Target State

At completion:

1. `egglsp::test_support` is compiled only when `lsp-test-support` is enabled.
2. The root `lsp-test-support` feature forwards to `egglsp/lsp-test-support`.
3. Both fake-server binaries remain thin package-local wrappers.
4. All scenario-engine tests referenced by `egglsp` live inside `crates/egglsp`.
5. `cargo package -p egglsp` produces a self-contained archive.
6. The unpacked packaged crate can run its feature-enabled scenario-engine and stdio tests where Cargo permits packaged integration testing.
7. Ordinary `cargo build -p egglsp` does not compile the scenario engine or expose `egglsp::test_support`.
8. Phase 2 documentation accurately describes the feature-gated test-support boundary.

## Scope

Primary files:

```text
Cargo.toml
crates/egglsp/Cargo.toml
crates/egglsp/src/lib.rs
crates/egglsp/src/test_support.rs
crates/egglsp/src/bin/egglsp-test-server.rs
crates/egglsp/tests/scenario_engine.rs
crates/egglsp-test-server/src/main.rs
crates/egglsp-test-server/tests/scenario_engine.rs
```

Documentation:

```text
README.md
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

## Non-Goals

Do not change:

- the fake-server scenario language;
- protocol framing behavior;
- production `LspClient` or `LspService` behavior;
- security-context or hunk-context logic;
- existing test semantics;
- captured-ID behavior;
- integration-test counts except where tests are moved or renamed;
- runtime dependencies unrelated to test-support isolation.

# Phase 1 — Gate the `test_support` Module

## Current Problem

`crates/egglsp/src/lib.rs` currently exposes:

```rust
pub mod test_support;
```

unconditionally.

This causes the entire fake-server scenario engine to be compiled into ordinary `egglsp` library builds and included in the public module surface even when `lsp-test-support` is disabled.

## Required Change

Change the declaration to:

```rust
#[cfg(feature = "lsp-test-support")]
#[doc(hidden)]
pub mod test_support;
```

Do not use `cfg(test)` alone because integration-test binaries and package-local fake-server binaries compile the library as a normal dependency. The Cargo feature is the authoritative gate.

## Expected Behavior

Without the feature:

```bash
cargo check -p egglsp
```

must compile without parsing or type-checking `src/test_support.rs`.

With the feature:

```bash
cargo check -p egglsp --features lsp-test-support --all-targets
```

must compile the module, binary wrapper, and integration tests.

## Public API Check

Add a compile-fail or documentation-level verification only if already supported by the repository tooling. Do not add a new compile-test framework solely for this.

A practical manual verification is sufficient:

```bash
cargo doc -p egglsp --no-deps
```

Confirm `test_support` is absent from ordinary generated docs.

Then:

```bash
cargo doc -p egglsp --features lsp-test-support --no-deps
```

The module may exist but should remain hidden due to `#[doc(hidden)]`.

## Acceptance Criteria

- `test_support` is not compiled without the feature.
- `test_support` is not part of ordinary generated public documentation.
- Feature-enabled binaries and tests still compile.

# Phase 2 — Forward the Root Feature to `egglsp`

## Current Problem

The root package currently defines:

```toml
lsp-test-support = []
```

The root fake-server binary calls `egglsp::test_support::run_or_exit()`. Once the module is feature-gated, enabling the root feature alone will not enable the corresponding dependency feature unless it is explicitly forwarded.

## Required Change

In the root `Cargo.toml`, change:

```toml
lsp-test-support = []
```

to:

```toml
lsp-test-support = ["egglsp/lsp-test-support"]
```

Do not add a second feature with a different name.

## Verify Root Binary

The existing root binary target should remain:

```toml
[[bin]]
name = "codegg-lsp-test-server"
path = "crates/egglsp-test-server/src/main.rs"
required-features = ["lsp-test-support"]
```

The wrapper should continue to call:

```rust
egglsp::test_support::run_or_exit();
```

## Verify Feature Isolation

Run:

```bash
cargo check --bin codegg
```

This should not enable `egglsp/lsp-test-support`.

Then run:

```bash
cargo check --features lsp-test-support --bin codegg-lsp-test-server
```

This should compile successfully.

## Acceptance Criteria

- Root feature activation enables the dependency feature automatically.
- Ordinary root builds do not enable `egglsp` test support.
- Root composite tests continue to receive `CARGO_BIN_EXE_codegg-lsp-test-server`.

# Phase 3 — Move Scenario-Engine Tests Inside the `egglsp` Package

## Current Problem

`crates/egglsp/tests/scenario_engine.rs` currently contains:

```rust
#![allow(clippy::all)]
include!("../../egglsp-test-server/tests/scenario_engine.rs");
```

The included file is outside the `crates/egglsp` package root. A published `egglsp` archive is not self-contained when feature-enabled tests attempt to compile this target.

## Preferred Structure

Move the scenario-engine test implementation to a package-contained file:

```text
crates/egglsp/tests/scenario_engine_cases.rs
```

Then make the test target file contain either:

```rust
mod scenario_engine_cases;
```

or move the test code directly into:

```text
crates/egglsp/tests/scenario_engine.rs
```

The simplest preferred result is:

```text
crates/egglsp/tests/scenario_engine.rs
```

containing the actual tests with no `include!` from outside the package.

## Single-Source Test Requirement

After moving the tests, remove or replace:

```text
crates/egglsp-test-server/tests/scenario_engine.rs
```

Do not keep two full copies of the same test suite.

Acceptable options:

### Option A — `egglsp` owns the tests

- Move all test cases into `crates/egglsp/tests/scenario_engine.rs`.
- Delete `crates/egglsp-test-server/tests/scenario_engine.rs`.
- Keep the external directory only for the thin root binary wrapper source if still needed.

This is preferred.

### Option B — Unit tests next to the implementation

- Move pure scenario-engine tests into `crates/egglsp/src/test_support.rs` under:

```rust
#[cfg(test)]
mod tests;
```

- Keep only binary/process integration tests under `crates/egglsp/tests/scenario_engine.rs`.

Use this only if the existing suite naturally separates pure logic from process-level tests.

## Clippy Policy

Remove blanket:

```rust
#![allow(clippy::all)]
```

Fix warnings or add narrow, justified allowances for specific lints.

Examples:

```rust
#[allow(clippy::too_many_lines)]
```

or:

```rust
#[allow(clippy::unwrap_used)]
```

only if the repository permits such test-specific allowances.

Do not suppress all Clippy lints for the entire test target.

## Path References

Audit moved tests for assumptions based on the old source location:

```text
CARGO_MANIFEST_DIR
relative fixture paths
include! paths
transcript paths
binary paths
```

All paths must resolve from `crates/egglsp` or use Cargo-provided binary environment variables.

## Acceptance Criteria

- No `include!` in `crates/egglsp/tests` references a path outside `crates/egglsp`.
- Scenario-engine tests have one authoritative source.
- Blanket `clippy::all` suppression is removed.
- Test behavior and scenario coverage remain unchanged.

# Phase 4 — Keep the Binary Wrappers Thin and Feature-Safe

## `egglsp` Binary Wrapper

Retain:

```text
crates/egglsp/src/bin/egglsp-test-server.rs
```

with only:

```rust
fn main() {
    egglsp::test_support::run_or_exit();
}
```

Do not duplicate scenario-engine code into the binary target.

## Root Binary Wrapper

Retain:

```text
crates/egglsp-test-server/src/main.rs
```

with only:

```rust
fn main() {
    egglsp::test_support::run_or_exit();
}
```

If the directory name is misleading after its tests are removed, leave renaming for a separate cleanup unless it can be done with minimal churn. Do not rename it during this pass if that risks breaking Cargo binary paths or documentation broadly.

## Compile Guards

No wrapper needs a source-level `compile_error!` because Cargo `required-features` already prevents invalid target selection. Keep gating in the manifests.

## Acceptance Criteria

- Both wrappers remain three-line entry points.
- Scenario-engine implementation exists only in `egglsp/src/test_support.rs`.
- No implementation duplication returns.

# Phase 5 — Verify Ordinary Dependency Isolation

## Dependency Review

The scenario engine uses:

```text
base64
libc
serde
serde_json
filesystem and thread APIs
```

Some of these are already production dependencies of `egglsp`, so this pass does not require optionalizing all dependencies.

However, verify whether any dependency exists solely for `test_support`.

For each dependency in `crates/egglsp/Cargo.toml`:

1. Search production modules excluding `test_support.rs`.
2. If used only by test support, make it optional and attach it to `lsp-test-support`.
3. If used by production code, leave it unchanged.

Example:

```toml
[features]
lsp-test-support = ["dep:base64"]

[dependencies]
base64 = { version = "0.22", optional = true }
```

Only do this when the dependency is genuinely test-support-only.

Do not over-refactor dependencies or change versions.

## Build Graph Verification

Use Cargo metadata or build output to confirm ordinary builds do not select optional test-only dependencies.

Suggested commands:

```bash
cargo tree -p egglsp -e features
cargo tree -p egglsp -e features --features lsp-test-support
```

Record meaningful differences in the implementation notes only if dependencies are changed.

## Acceptance Criteria

- No dependency is made optional incorrectly.
- Any true test-support-only dependency is gated by the feature.
- Ordinary `egglsp` behavior is unchanged.

# Phase 6 — Validate the Published Crate Archive

## Package Creation

Run:

```bash
cargo package -p egglsp --allow-dirty
```

This must succeed.

## Inspect Archive Contents

Locate the generated `.crate` archive under:

```text
target/package/
```

List its contents and verify it includes:

```text
src/lib.rs
src/test_support.rs
src/bin/egglsp-test-server.rs
tests/scenario_engine.rs
other production integration tests
Cargo.toml
```

It is acceptable for `test_support.rs` to be present in the source archive; the requirement is that it is feature-gated and not compiled into ordinary builds.

Verify the archive does not rely on:

```text
../../egglsp-test-server/...
../outside-package/...
```

## Unpacked Archive Verification

Unpack the generated crate into a temporary directory and run, from the unpacked package where practical:

```bash
cargo check
cargo check --features lsp-test-support --all-targets
cargo test --features lsp-test-support --test scenario_engine
```

If Cargo's normalized packaged manifest or missing workspace context prevents a specific command, record the exact limitation. At minimum:

```bash
cargo check
cargo check --features lsp-test-support --all-targets
```

must succeed in the unpacked archive.

## Package Verification Without Feature

Confirm ordinary package verification does not compile the fixture binary:

```bash
cargo package -p egglsp --allow-dirty --no-verify
```

followed by archive inspection is not enough by itself; retain the normal verified package command as the authoritative check.

## Acceptance Criteria

- Packaged archive is self-contained.
- Feature-disabled unpacked check succeeds.
- Feature-enabled unpacked all-targets check succeeds.
- Scenario-engine test source exists inside the archive.

# Phase 7 — Re-run Clean-Checkout Integration Tests

## Root Integration

```bash
cargo clean
cargo test --features lsp-test-support \
  --test lsp_composite_stdio \
  security_context_tool_filters_and_preserves_diagnostic_evidence
```

## `egglsp` Integration

```bash
cargo clean
cargo test -p egglsp --features lsp-test-support \
  --test production_protocol_stdio \
  initialization_handshake
```

## Scenario Engine

```bash
cargo test -p egglsp --features lsp-test-support \
  --test scenario_engine
```

## Parallel Suites

```bash
cargo test --features lsp-test-support \
  --test lsp_composite_stdio -- --test-threads=1

cargo test --features lsp-test-support \
  --test lsp_composite_stdio -- --test-threads=8

cargo test -p egglsp --features lsp-test-support \
  --tests -- --test-threads=1

cargo test -p egglsp --features lsp-test-support \
  --tests -- --test-threads=8
```

## Acceptance Criteria

- Clean target directories do not break binary discovery.
- Feature forwarding works for root tests.
- Package-local binary discovery works for `egglsp` tests.
- Moving scenario tests does not reduce coverage.

# Phase 8 — Documentation Corrections

## Required Documentation Statement

Document the final test-support architecture accurately:

```text
egglsp::test_support is hidden and compiled only with lsp-test-support.
The root lsp-test-support feature forwards to egglsp/lsp-test-support.
Both fake-server binaries are thin package-local wrappers.
Scenario-engine tests are contained inside crates/egglsp.
```

## Correct Existing Wording

Audit statements saying the scenario engine “lives in `egglsp::test_support`” and add that it is feature-gated.

Audit any documentation that still references:

```text
crates/egglsp-test-server/tests/scenario_engine.rs
```

Update it to the new package-contained test location.

## Phase 2 Closure

After the verification matrix passes, mark Phase 2 complete without a packaging caveat.

Do not increase test-count claims unless the move changes the actual number of tests.

# Exact Implementation Order for a Smaller Model

Execute these steps in order:

1. Change `pub mod test_support` to a feature-gated, hidden module.
2. Forward root `lsp-test-support` to `egglsp/lsp-test-support`.
3. Run `cargo check -p egglsp` and root feature-enabled binary check.
4. Copy/move scenario-engine tests into `crates/egglsp/tests/scenario_engine.rs`.
5. Remove the external `include!` and blanket Clippy suppression.
6. Delete the old external scenario-engine test file after confirming no other target references it.
7. Verify both fake-server wrappers remain thin.
8. Audit `egglsp` dependencies for test-support-only usage; gate only dependencies proven exclusive.
9. Run `cargo package -p egglsp --allow-dirty`.
10. Inspect and unpack the `.crate` archive; run feature-disabled and feature-enabled checks.
11. Run clean-checkout root, `egglsp`, and scenario-engine tests.
12. Run single-thread and eight-thread suites.
13. Update documentation.
14. Run full formatting, check, test, and Clippy validation.

# Verification Commands

## Feature isolation

```bash
cargo check -p egglsp
cargo check -p egglsp --features lsp-test-support --all-targets
cargo check --bin codegg
cargo check --features lsp-test-support --bin codegg-lsp-test-server
```

## Package integrity

```bash
cargo package -p egglsp --allow-dirty
```

Then inspect and unpack the archive and run:

```bash
cargo check
cargo check --features lsp-test-support --all-targets
cargo test --features lsp-test-support --test scenario_engine
```

## Integration tests

```bash
cargo test --features lsp-test-support --test lsp_composite_stdio
cargo test -p egglsp --features lsp-test-support --tests
```

## Full workspace validation

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo check --workspace --all-targets --all-features
cargo test --workspace
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If unrelated optional features fail, record exact diagnostics and separately prove all changed targets with `lsp-test-support` enabled.

# Review Checklist

## Module isolation

- [ ] `test_support` has `#[cfg(feature = "lsp-test-support")]`.
- [ ] `test_support` has `#[doc(hidden)]`.
- [ ] Ordinary `egglsp` docs do not expose the module.
- [ ] Ordinary builds do not compile the module.

## Feature forwarding

- [ ] Root `lsp-test-support` forwards to `egglsp/lsp-test-support`.
- [ ] Root fixture binary builds with one feature flag.
- [ ] Ordinary root builds do not enable test support.

## Test containment

- [ ] No `egglsp` test includes files outside the package root.
- [ ] Scenario-engine tests have one source of truth.
- [ ] Blanket `clippy::all` suppression is removed.
- [ ] Old external test file is removed or no longer authoritative.

## Binary wrappers

- [ ] `egglsp-test-server` wrapper remains thin.
- [ ] `codegg-lsp-test-server` wrapper remains thin.
- [ ] Scenario engine exists only in `egglsp/src/test_support.rs`.

## Packaging

- [ ] `cargo package -p egglsp` succeeds.
- [ ] Packaged archive contains all referenced source/test files.
- [ ] Unpacked crate checks without the feature.
- [ ] Unpacked crate checks with `lsp-test-support --all-targets`.
- [ ] No package path escapes the crate root.

## Regression tests

- [ ] Root composite tests pass from a clean target directory.
- [ ] `egglsp` production stdio tests pass from a clean target directory.
- [ ] Scenario-engine tests pass after relocation.
- [ ] Single-thread and multi-thread runs pass.

## Documentation

- [ ] Feature-gated scenario engine is documented.
- [ ] Scenario-engine test location is accurate.
- [ ] Phase 2 packaging caveat is removed only after archive verification.

# Completion Criteria

This final hygiene pass is complete when:

1. `egglsp::test_support` is absent from ordinary builds and public docs.
2. Root test-support activation correctly enables the dependency feature.
3. Scenario-engine tests are fully contained inside `crates/egglsp`.
4. Both fake-server binaries remain thin wrappers over one implementation.
5. `cargo package -p egglsp` creates a self-contained crate archive.
6. The unpacked archive compiles with and without `lsp-test-support`.
7. Clean-checkout integration and scenario-engine tests pass.
8. No blanket Clippy suppression or external test include remains.
9. Documentation accurately records the final architecture.
10. LSP Phase 2 is complete without runtime, semantic, or packaging qualification.

## Handoff Result

After this pass, the scripted LSP harness will remain fully available for development and CI while disappearing cleanly from ordinary `egglsp` builds and public API. The repository can then move to Phase 3 real-server compatibility and lifecycle supervision without further Phase 2 packaging cleanup.
