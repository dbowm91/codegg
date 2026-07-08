# Testing Resource Optimization Polish Pass Plan

## Purpose

The testing resource optimization work is now functionally in good shape: the full workspace suite remains serial, duplicate shell-projection reruns are gone, plugin-focused tests are explicitly serial, real-LSP behavior is documented, and CI now guards against new bare `#[tokio::test]` annotations.

This polish pass is for closing the remaining edge cases and making the policy more robust without changing the conservative execution model. The work should remain narrow. Do not split CI lanes or replace the main test runner unless timing data proves that is worth doing.

## Current State

Implemented and retained:

- `cargo test --workspace --all-features -- --test-threads=1` remains the safe full-suite gate.
- Plugin-focused tests now explicitly pass `-- --test-threads=1`.
- `scripts/check-tokio-test-flavors.py` is wired into CI.
- `scripts/audit_tokio_tests.py` exists as an advisory scan for `current_thread` tests with concurrency-sensitive patterns.
- `.config/nextest.toml` exists for timing and optional future usage.
- `architecture/testing.md` documents the current conservative CI decision.
- Real-LSP tests are documented as outside routine PR validation and skip unless server binaries are installed.

Remaining polish items:

- The real-server smoke tests are still allowlisted for bare `#[tokio::test]`; prefer explicit Tokio runtime flavor if possible.
- The Tokio flavor guard only catches the exact line `#[tokio::test]`; it should catch obvious whitespace variants and simple multiline attributes.
- The advisory Tokio audit exits `1` when candidates are found, which is appropriate for manual review but awkward for documentation snippets and future optional CI usage.
- Nextest timing commands are documented but baseline capture is not standardized.
- Process-heavy comments exist on several files, but the policy could be made easier to maintain with a lightweight resource-class manifest or checklist.

## Phase 1: Remove the Real-Server Bare Tokio Exception if Safe

### 1.1 Inspect `crates/egglsp/tests/real_server_smoke.rs`

Find all bare Tokio annotations:

```bash
rg '#\s*\[\s*tokio::test' crates/egglsp/tests/real_server_smoke.rs
```

For each test, determine whether it needs a multi-thread runtime. These tests launch real language-server subprocesses and perform async protocol I/O, so the most likely explicit replacement is:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
```

Do not use `current_thread` for real-server smoke tests unless validated across all supported servers. Real servers and async stdio handling are better treated as process-heavy/multi-threaded, but with bounded workers.

### 1.2 Convert allowlisted bare tests

Replace bare annotations in `real_server_smoke.rs` with explicit bounded multi-thread annotations.

Then remove `crates/egglsp/tests/real_server_smoke.rs` from the default allowlist inside `scripts/check-tokio-test-flavors.py` if no bare annotations remain.

Acceptance criteria:

- `python3 scripts/check-tokio-test-flavors.py` passes with an empty default allowlist or a smaller documented allowlist.
- `real_server_smoke.rs` uses explicit Tokio runtime flavor.
- The real-LSP workflow semantics are unchanged.
- Real-server smoke tests still skip cleanly when server binaries are missing.

Validation:

```bash
python3 scripts/check-tokio-test-flavors.py
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- --list
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- rust_analyzer --nocapture
```

The final command may skip if `rust-analyzer` is not installed; that is acceptable for local validation.

## Phase 2: Harden the Tokio Flavor Guard

### 2.1 Match whitespace variants

The current guard catches only the exact stripped line:

```rust
#[tokio::test]
```

Update it to catch common variants:

```rust
#[ tokio::test ]
#[tokio :: test]
#[tokio::test ]
#[ tokio :: test]
```

A regex such as the following is sufficient:

```python
BARE_TOKIO_TEST_RE = re.compile(r"^#\[\s*tokio\s*::\s*test\s*\]$")
```

Keep the script simple; this is a policy guard, not a full Rust parser.

### 2.2 Catch simple multiline bare attributes

Also catch straightforward multiline forms:

```rust
#[
    tokio::test
]
```

A simple state machine is enough:

- When a line begins with `#[`, collect until a line containing `]`.
- Normalize whitespace.
- If the normalized attribute equals `#[tokio::test]`, flag it.
- If it contains `flavor`, do not flag it.

Avoid overengineering. Do not parse nested attributes or macro-generated test attributes.

### 2.3 Add self-tests for the script

Add a small Python unittest or table-driven self-test mode, for example:

```bash
python3 scripts/check-tokio-test-flavors.py --self-test
```

Self-test cases should include:

- exact bare annotation,
- whitespace variants,
- multiline bare annotation,
- `current_thread` annotation,
- `multi_thread(worker_threads = 2)` annotation,
- allowlisted file behavior.

Acceptance criteria:

- The script catches the obvious formatting variants.
- The script still does not flag explicit flavor annotations.
- The CI invocation remains `python3 scripts/check-tokio-test-flavors.py`.
- The script has a low false-positive rate.

Validation:

```bash
python3 scripts/check-tokio-test-flavors.py --self-test
python3 scripts/check-tokio-test-flavors.py
```

## Phase 3: Make the Advisory Tokio Audit Easier to Use

### 3.1 Add a non-failing mode

`audit_tokio_tests.py` currently exits `1` when candidates are found. That is semantically useful, but inconvenient for local reporting and optional CI artifact generation.

Add:

```bash
python3 scripts/audit_tokio_tests.py --no-fail
```

Behavior:

- Print the same report.
- Exit `0` even if candidates are found.
- Preserve current default behavior for users who want the command to signal candidates.

### 3.2 Add concise summary output

Add an optional summary mode:

```bash
python3 scripts/audit_tokio_tests.py --summary
```

It should print:

- number of candidate tests,
- top files by candidate count,
- top pattern classes,
- reminder that findings are review candidates, not automatic failures.

Acceptance criteria:

- `--no-fail` works.
- `--summary` gives a useful high-level view.
- `--json` remains supported.
- Existing behavior does not change unless flags are used.

Validation:

```bash
python3 scripts/audit_tokio_tests.py --no-fail
python3 scripts/audit_tokio_tests.py --summary --no-fail
python3 scripts/audit_tokio_tests.py --json > /tmp/tokio-audit.json
```

## Phase 4: Standardize Nextest Timing Capture

### 4.1 Add a helper script for timing runs

Rather than embedding brittle shell pipelines in docs, add a helper script:

```bash
scripts/capture-nextest-timing.sh
```

Suggested behavior:

```bash
scripts/capture-nextest-timing.sh [profile] [output-dir]
```

Defaults:

- profile: `ci-heavy`
- output dir: `target/test-metrics/nextest-YYYYMMDD-HHMMSS`

The script should:

- verify `cargo nextest` is installed,
- run `cargo nextest run --workspace --profile <profile> --all-features`,
- save raw output,
- save a short summary file,
- print where metrics were written.

If nextest JSON output is too version-sensitive, do not parse it aggressively. Start by preserving raw output and a few greppable summary lines.

### 4.2 Update docs to reference the helper

Update `architecture/testing.md` and `AGENTS.md` to prefer:

```bash
scripts/capture-nextest-timing.sh
```

Keep raw nextest commands as fallback examples.

Acceptance criteria:

- Maintainers have one stable command to capture timing data.
- Metrics are stored under `target/`, not committed.
- Docs do not rely on fragile inline Python/JSON assumptions unless verified.

Validation:

```bash
bash scripts/capture-nextest-timing.sh ci-heavy /tmp/codegg-nextest-metrics
```

## Phase 5: Add a Lightweight Test Resource Class Checklist

### 5.1 Create a small manifest or checklist

Add one of the following:

Option A: `docs/test-resource-classes.md`

Option B: `architecture/test-resource-classes.toml`

Option C: a section in `architecture/testing.md`

The intent is to make it easy to classify new heavy tests without reading the full architecture page.

Suggested content:

```toml
[[class]]
name = "fast"
serial = false
examples = ["parser", "registry", "config"]

[[class]]
name = "storage"
serial = true
examples = ["session_crud", "snapshot"]

[[class]]
name = "process-heavy"
serial = true
examples = ["lsp_composite_stdio", "supervisor_restart_stdio"]

[[class]]
name = "plugin-heavy"
serial = true
examples = ["plugin::install", "plugin::management"]

[[class]]
name = "real-lsp"
serial = true
manual_or_scheduled = true
examples = ["real_server_smoke"]
```

Keep this simple. The purpose is maintainability, not machine enforcement.

### 5.2 Link it from process-heavy file headers

If a manifest/checklist is added, process-heavy file headers should point to it:

```rust
//! See architecture/testing.md for resource-class policy.
```

Acceptance criteria:

- Test resource classes are easy to find.
- Heavy test files have a clear pointer to the policy.
- No complex new enforcement is introduced.

## Phase 6: Final Documentation Consistency Pass

Review and align these files:

- `architecture/testing.md`
- `AGENTS.md`
- `.codegg/skills/testing/SKILL.md`, if present
- `.github/workflows/ci.yml`
- `.github/workflows/lsp-real-server.yml`
- `plans/testing_resource_optimization_handoff.md`
- `plans/testing_resource_optimization_closure_pass.md`

Ensure wording is consistent on:

- full serial suite remains intentional,
- plugin-focused tests are duplicate-by-design unless later changed,
- real-LSP tests are not routine PR validation,
- `--all-features` compiles real-server smoke tests but they skip without binaries,
- bare Tokio tests are disallowed unless explicitly allowlisted,
- advisory Tokio audit is not a hard CI gate,
- nextest is optional measurement tooling.

Acceptance criteria:

- No doc says real-LSP is only manual/weekly if push-to-main path triggers remain.
- No doc recommends bare `#[tokio::test]`.
- No doc implies nextest is required for default CI if it remains optional.
- All commands use explicit `-- --test-threads=1` where they refer to plugin/process-heavy/real-LSP tests.

## Validation Commands

Minimum validation for the polish pass:

```bash
python3 scripts/check-tokio-test-flavors.py --self-test
python3 scripts/check-tokio-test-flavors.py
python3 scripts/audit_tokio_tests.py --summary --no-fail
cargo fmt --check --all
cargo check --workspace --all-features --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

Targeted validation:

```bash
cargo test -p codegg --lib plugin::install --all-features -- --test-threads=1
cargo test -p codegg --lib plugin::management --all-features -- --test-threads=1
cargo test -p codegg --lib plugin::registry --all-features -- --test-threads=1
cargo test -p codegg --lib tui::commands::plugin_management --all-features -- --test-threads=1
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- --list
```

Optional timing validation:

```bash
scripts/capture-nextest-timing.sh ci-heavy /tmp/codegg-nextest-metrics
```

## Definition of Done

The polish pass is complete when:

- `real_server_smoke.rs` no longer needs a bare Tokio allowlist, or the allowlist contains a clear rationale that cannot be removed yet.
- The Tokio flavor guard catches exact, whitespace, and simple multiline bare attributes.
- The Tokio flavor guard has self-tests.
- The advisory Tokio audit supports non-failing summary/report mode.
- Nextest timing capture has a stable helper or the docs clearly explain why manual commands are sufficient.
- Test resource-class documentation is consistent and easy to discover.
- CI remains conservative and resource-safe.

The intended end state is not a more aggressive test runner. It is a clearer, harder-to-regress testing resource policy around the existing safe execution model.
