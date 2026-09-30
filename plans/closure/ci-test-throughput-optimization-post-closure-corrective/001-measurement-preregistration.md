# C001 Hosted Measurement Preregistration

Status: preregistered before candidate configuration

Repository baseline: `4f7e508976e70fbec0e645bd530b63fe3c8393c1`

## Control and candidate

Control: current single-job workflow with `Swatinem/rust-cache@v2`, no
`RUSTC_WRAPPER`, `CARGO_BUILD_JOBS=4`, and `CARGO_INCREMENTAL=0`.
The incremental setting is explicit because the current upstream sccache
Rust guide says Rust incremental compilation must be disabled.

Candidate: same workflow and settings, plus
`mozilla-actions/sccache-action@v0.0.11`, `SCCACHE_GHA_ENABLED=true`, and
`RUSTC_WRAPPER=sccache`. The action's post-step reports statistics; an
explicit `sccache --show-stats` step captures them before job completion.
The GitHub Actions runtime supplies cache credentials. No secret is added.

## Comparable sequence

1. Stabilized non-sccache control with the same toolchain, profile, build
   jobs, Nextest scope/resource rules, and incremental setting.
2. Candidate seed run.
3. Candidate warm run.
4. Candidate run after an ordinary Rust source comment change in
   `crates/egggit/src/lib.rs`; this changes one crate while leaving most
   workspace crate sources untouched.
5. If needed to resolve runner variance, one more matched control/candidate
   pair. Existing M005 run `36196068239` is contextual only because it did
   not explicitly set `CARGO_INCREMENTAL=0`.

The cache candidate is retained only if at least two comparable candidate
observations show both >=45 seconds reduction in warm Workspace tests build
time and >=30 seconds reduction in total warm job time versus control,
without setup/save overhead erasing the gain, cache errors, new flakes,
stale output, or any test-scope change. Otherwise revert the candidate.

For every run capture run/attempt/SHA/event/cache state, setup and action
overhead, Clippy, fixture prebuild, Workspace tests step, Cargo `Finished`
build duration, Nextest summary duration/count, total job wall time, and
`sccache --show-stats` when candidate. A cancelled or superseded run is
not evidence.

Control reference selected before candidate activation: prior stabilized
main/live run `36196068239` attempt 3 (17m17s total, Clippy 2m08s,
Workspace tests 13m57s including 7m51s build and 362.537s execution,
11,726 passed / 1 skipped). A dedicated same-incremental-mode control was
also run as `36211049535` attempt 1 because the prior run predates the
explicit cache-compatible setting. Final results and disposition are in
`plans/closure/ci-test-throughput-optimization-post-closure-corrective/001-status.md`.
