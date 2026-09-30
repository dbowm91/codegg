# C001 Hosted Measurement Preregistration (post-C002)

Status: preregistered before candidate configuration

Repository baseline: `0fae5c55051c895061b26b6eb5b0d5d90cd64ebc`
C002 closure: `plans/closure/ci-test-throughput-optimization-post-closure-corrective/002-status.md`
(implementation `696ec282`; hosted `36748429660` attempts 1–3 green).

## Control and candidate

Control A (stabilized non-sccache, post-C002 identity):

- current single-job workflow;
- `Swatinem/rust-cache@v2` (workspaces `. -> target`,
  `crates/eggwork-test-node -> target`);
- no `RUSTC_WRAPPER`;
- `CARGO_BUILD_JOBS=4`;
- `CARGO_INCREMENTAL=0` made explicit for cache comparability (removes
  incremental-cache-maturity bias per M001 recipe);
- current Nextest `ci` policy (4 slots, 5 heavy binaries run-alone);
- same Rust toolchain (`dtolnay/rust-toolchain@stable`), same Cargo
  profile, same test scope.

Candidate B (identical plus compiler-result cache):

- preserve `Swatinem/rust-cache` for dependency/target reuse;
- add `mozilla-actions/sccache-action@v0.0.11` with `version: v0.18.0`
  (supported upstream, same as Sep-26 probe; pin per repo convention
  only if retained);
- `SCCACHE_GHA_ENABLED=true`, `RUSTC_WRAPPER=sccache` via
  `$GITHUB_ENV`;
- backend is the supported GitHub Actions cache with Actions-provided
  runtime credentials; no user-managed/external secret;
- same toolchain, profile, build jobs, test scope, Nextest policy;
- `CARGO_INCREMENTAL=0` explicit and identical to control;
- explicit `sccache --show-stats` capture step (`if: always()`).

No workflow permission change (`contents: read` retained). Cache
candidate is removable in one revert and never required for
correctness (Cargo compiles normally if sccache fails open on
performance only).

## Comparable sequence (WP4)

1. Control/reference: dedicated post-C002 warm control on the
   stabilized non-sccache configuration with `CARGO_INCREMENTAL=0`
   explicit, OR the recent post-C002 warm mains
   `36748429660` (C002 tree `696ec282`, 17m39s, live included) and
   `36760308368` (tree `3c7438c7`, 17m45s, 11,830 pass, live included)
   if toolchain/workflow identity is unchanged for the comparison.
   A dedicated same-incremental control is preferred because the
   historical `36196068239` (17m17s, 7m51s build, 362.537s exec,
   11,726/1) predates the explicit incremental setting and the C002
   lifecycle fix.
2. Candidate seed/cold run (first sccache population).
3. Candidate warm run (same branch, cache restored).
4. Second candidate run after a small ordinary Rust source change that
   leaves most workspace crates unchanged (e.g. one-line comment in a
   leaf crate), so compiler-output reuse can be observed.
5. If runner variance makes the result ambiguous, one additional
   control/candidate comparison.

Do not compare a cold control to a warm candidate as proof. A
cancelled/superseded run is not evidence.

## Retention threshold (preregistered)

Retain sccache only if ALL hold:

- warm workspace-test build phase reduction >= 45s vs comparable
  control;
- total warm job wall-time reduction >= 30s vs comparable control;
- improvement appears in at least two comparable candidate
  observations (not one outlier);
- cache/setup/save overhead does not erase the gain;
- no new flake, stale-build behavior, cache error, or test-scope
  change.

Otherwise fully revert the candidate and close with a measured
negative disposition. Threshold is modest vs the 7m51s build phase
but exceeds ordinary tens-of-seconds runner noise.

## Required statistics per run

- run ID, attempt, SHA, event, cache state (cold/warm/unknown);
- `sccache --show-stats` (compile requests, hits/misses,
  non-cacheable, read/write errors, size/overhead where available);
- Workspace Clippy wall time;
- workspace-test build phase (`Finished 'test' profile ... in Xs`);
- Nextest execution (`Summary [Xs] N tests run`, count);
- fixture-prebuild state, detector result (`live_required`);
- total job wall time.

## Unrelated-PR fast path (WP5)

One green PR run where `scripts/detect-live-eggwork-changes.sh`
reports `live_required=false` and `eggwork_remote_execution_live` is
omitted via a controlled non-live-relevant source change (not
docs-only, since `paths-ignore` skips CI). Record detector output,
prebuild skipped/included, Clippy, build, exec, total, count, cache
state, and whether sccache was control or retained candidate. This
becomes the ordinary unrelated-PR baseline; main/live remains
authoritative.

## Stop conditions

Stop and report rather than widen scope if sccache needs an external
service/credential, requires profile/topology/job changes to win,
produces stale output, the selector omits live where required, another
plan depends on stale M004/M005 status, hosted failures show
product/test defects, or coverage must drop to meet the threshold.
