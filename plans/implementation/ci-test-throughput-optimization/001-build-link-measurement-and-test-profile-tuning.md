# CI/Test Throughput M001 — Build/Link Measurement and Test-Profile Tuning

Status: ready

Repository baseline reviewed: `0c896db32d2325da39b129acde7dd406ce472dc4`

Source roadmap:

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md`

Primary class: polish / development infrastructure.

## 1. Objective

Establish trustworthy compiler/link timing evidence for the current ~19-minute hosted CI configuration and land only low-risk test-profile/build-job changes that measurably shorten the routine critical path.

The principal hypothesis is that CodeGG is paying for unnecessary test debug-information generation and/or suboptimal Cargo build concurrency while linking a large number of test executables.

## 2. Current evidence

At the baseline:

- `.github/workflows/ci.yml` uses mold, `Swatinem/rust-cache@v2`, Nextest, and `CARGO_BUILD_JOBS=8`;
- `architecture/testing.md` records ~2 minutes Workspace Clippy and ~16 minutes workspace tests, with ~7 minutes actual test execution;
- roughly ~9 minutes therefore remain in test build/link/setup;
- `[profile.test]` has `opt-level = 0`, `codegen-units = 16`, and `strip = "debuginfo"`, but no explicit `debug = 0`;
- 8-way Nextest execution was already shown to oversubscribe the 4-vCPU runner and induce timing flakes; this milestone must not infer that Cargo build parallelism has the same optimum without measurement.

## 3. Invariants and non-goals

Preserve:

- all tests and current routine scope;
- mold;
- one routine CI job;
- Nextest `ci` semantics;
- docs-only skip and superseded-run cancellation;
- local `verify.sh` resource defaults unless a local change is separately justified.

Do not:

- add a permanent benchmark CI lane;
- add sccache yet;
- consolidate test targets yet;
- move live qualification yet;
- change release profiles;
- weaken Clippy or formatting;
- change test timeouts or resource classification merely to improve timing.

## 4. Work packages

### WP1 — Capture a comparable baseline

Use the current hosted configuration to record:

- total job duration;
- setup/guards/fmt duration;
- Workspace Clippy duration;
- workspace-test step duration;
- Nextest execution duration/test count;
- inferred or directly measured compile/link duration.

Use Cargo timing output for at least one representative test build (`cargo test --workspace --locked --no-run --timings` or an equivalent Cargo build invoked by the implementation agent). Timing output may remain local/ephemeral; do not add a permanent artifact gate.

Record cache state and runner CPU count where observable.

### WP2 — A/B test test debuginfo generation

Measure the exact routine build/test surface with:

```bash
CARGO_PROFILE_TEST_DEBUG=0 ...
```

against the current profile.

If the override materially improves compile/link wall time without reducing failure diagnostics needed by hosted CI, retain it as a CI-only environment setting or encode the equivalent test-profile policy if repository-wide behavior is explicitly preferred and justified.

Do not treat `strip = "debuginfo"` as proof that debug generation is already disabled.

If the improvement is within ordinary runner variance, do not retain the change solely on theory.

### WP3 — Re-measure Cargo build-job concurrency

Compare bounded candidate values appropriate to the hosted 4-vCPU runner, at minimum the current 8 and 4, after the debuginfo decision is fixed.

Use equivalent cache states where possible. Reject a faster setting if it causes:

- OOM;
- substantial CPU starvation of subprocess tests;
- reproducible timing flakes;
- worse total wall time.

Do not test unbounded/default concurrency as the production candidate unless explicitly justified.

### WP4 — Evaluate test codegen units only if still material

Only if Cargo timing evidence shows code generation remains a dominant contributor after WP2/WP3, measure the current `codegen-units = 16` against one bounded alternative.

Do not perform a wide tuning sweep. This is a single-variable experiment, not profile micro-optimization.

### WP5 — Leave reproducible diagnostic instructions

Update `architecture/testing.md` with the retained settings and a short diagnostic recipe for separating build/link from execution time.

If a helper script is added, it must be optional, small, local-only, and not create retained CI artifacts or a new framework. Prefer documenting direct Cargo/Nextest commands when sufficient.

## 5. Required verification

At minimum:

```bash
cargo fmt --all -- --check
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked --profile ci
```

Hosted evidence is required for the final chosen CI settings because local hardware is not representative of GitHub-hosted runner concurrency.

The closure record must distinguish cold/warm/unknown cache state.

## 6. Acceptance criteria

- A baseline with separated compile/link and execution evidence exists.
- The `CARGO_PROFILE_TEST_DEBUG=0` hypothesis is explicitly measured and either retained or rejected.
- `CARGO_BUILD_JOBS` has a measured final value rather than an experimental comment.
- No test scope, assertion, resource classification, or feature boundary changed.
- Retained changes reduce comparable hosted wall time or are reverted.
- Active CI/testing documentation no longer describes a retained setting as an experiment.
- No new permanent CI lane, benchmark gate, or artifact pipeline.

## 7. Stop conditions

Stop rather than widening scope if:

- a candidate requires deleting or skipping tests;
- improved timing depends on a flaky/failed run;
- the result depends primarily on cache-state mismatch;
- profile changes materially degrade usable panic/backtrace diagnostics without an alternative;
- a new dependency/tool is required solely for timing measurement;
- Cargo timing evidence points primarily to specific integration-test executable repetition — record that evidence for M003 rather than beginning consolidation here.

## 8. Closure evidence

Create `plans/closure/ci-test-throughput-optimization/001-status.md` recording:

- baseline revision and runner;
- before/after step timings;
- cache-state notes;
- Cargo timing summary;
- debuginfo disposition;
- build-job disposition;
- any codegen-units experiment;
- exact retained CI/profile changes;
- verification results;
- unresolved findings;
- whether M002 is unblocked.
