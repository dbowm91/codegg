# CI and Test Throughput Optimization Roadmap

Status: closed; post-closure corrective C001 registered

Repository planning baseline: `0c896db32d2325da39b129acde7dd406ce472dc4`

Post-closure note: M001-M005 are historical closed work. C001 in `plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md` reconciles the historical status/documentation drift and supplies the measured compiler-cache disposition. See its closure record at `plans/closure/ci-test-throughput-optimization-post-closure-corrective/001-status.md`.

Primary class: polish / development infrastructure.

Canonical references:

- `plans/000-long-term-specification.md` §4.7, correctness before transparent magic;
- `plans/003-planning-process.md` §2.3–2.4, subsystem roadmaps and executable milestone plans;
- `plans/subsystems/development-verification-release-roadmap.md` — closed predecessor defining the minimal verification contract;
- `architecture/testing.md` — current test-resource and routine-CI policy;
- `.github/workflows/ci.yml`, `.config/nextest.toml`, `scripts/verify.sh` — current executable verification surfaces.

Related ADRs: none required. This roadmap changes development/CI mechanics only; it does not alter product authority, protocol, storage, release ownership, or runtime architecture.

## 1. Purpose

Routine hosted CI has improved materially but remains too slow for the expected development loop. At the reviewed baseline, the repository records:

- historical routine CI around 37 minutes;
- current tuned steady-state around 19 minutes green;
- Workspace Clippy around 2 minutes;
- workspace test step around 16 minutes;
- actual execution of 11,781 tests around 7 minutes;
- therefore roughly 9 minutes of the current test step still attributable to compile/link/setup rather than test bodies;
- approximately 189 top-level integration-test source files, with the default workspace run linking on the order of 100 test executables.

The objective is to reduce the ordinary PR feedback loop substantially while preserving the same correctness boundary. The workstream is measurement-led: cheap compiler/profile changes land first, heavyweight qualification is separated from ordinary feedback only with explicit coverage retention, and integration-test topology is consolidated only after a bounded pilot proves that repeated linking is a real remaining bottleneck.

## 2. Durable invariants

This roadmap MUST preserve:

- one bounded ordinary GitHub Actions `CI / verify` job; no new permanent CI matrix or lane proliferation;
- `contents: read` and no release/publication authority;
- the canonical `scripts/verify.sh quick` / `full` local contract;
- default-feature routine CI unless a milestone explicitly and narrowly documents a feature-qualified exception;
- all production tests and assertions unless a test is proven redundant and removal is separately justified outside this optimization workstream;
- supported-Linux qualification for Landlock/Eggwork behavior;
- deterministic resource bounds for subprocess, PTY, scheduler, SQLite, and environment-mutating tests;
- process-global environment safety: ordinary libtest runs remain serial where required, and Nextest resource rules must not reintroduce intra-process races;
- no live-provider/public-network CI;
- no permanent benchmark, coverage, size, or performance threshold gate;
- no automatic `cargo clean`, target deletion, or cache ownership outside Cargo/tool-supported cache semantics.

A faster dashboard is not sufficient evidence. Each retained optimization must have before/after timing evidence and a correctness-equivalent verification result.

## 3. Current-state findings

### 3.1 Test profile may generate debug information that CI immediately discards

The root `[profile.test]` sets `strip = "debuginfo"` but does not explicitly set `debug = 0`. M001 must measure whether a CI-only `CARGO_PROFILE_TEST_DEBUG=0` materially reduces compilation/link time before making it permanent.

### 3.2 Integration-test executable count is structurally high

Cargo creates a separate executable for each top-level integration-test target. The repository records 189 root integration-test files and the current CI timing note attributes a large portion of the workspace-test phase to compile/link of roughly 100 binaries. M003/M004 own a bounded consolidation experiment and, only if positive, broader family-based consolidation.

### 3.3 Live Eggwork qualification is expensive and performs nested builds

`tests/eggwork_remote_execution_live.rs` is Linux-only and historical hosted evidence records roughly 144 seconds for that target. It also builds the workspace-excluded `crates/eggwork-test-node` and the pinned Eggwork sandbox helper on demand when no prebuilt fixture path is supplied.

That target directory is outside the ordinary root `target/` tree. M002 owns explicit prebuild/cache integration and a change-sensitive ordinary-PR qualification policy while retaining authoritative Linux evidence.

### 3.4 Current Nextest exclusivity is coarse

The `ci` profile currently marks five whole binaries as requiring all CPU slots:

- `eggwork_remote_execution_live`;
- `interactive_process_attach_resume`;
- `interactive_process_sessions`;
- `interactive_terminal_tui`;
- `scheduler_cancellation`.

Those files contain 53 tests in aggregate. M002 must distinguish tests that genuinely require machine-wide exclusivity from tests that are merely colocated in a heavy binary.

### 3.5 Existing cache is dependency-oriented

`Swatinem/rust-cache` is already used. M005 may evaluate compiler-result caching and final critical-path overlap, but only after M001–M004 establish the new build/test topology. Cache complexity must not mask a structural link problem.

## 4. Dependency graph

```text
M001 measurement + low-risk compiler/profile tuning
                    |
                    v
M002 heavy/live qualification scheduling and fixture build/cache
                    |
                    v
M003 integration-harness consolidation pilot
             | positive
             v
M004 bounded family consolidation
             |
             v
M005 cache and final critical-path closure
```

- M001 is closed with the measured hosted baseline (run `36173876950`, 17m54s, JOBS=4).
- M003 is closed with the measured hosted baseline (run `36193906726`, 17m31s, live target qualified, build 8m17s, exec 338s, positive pilot closed).
- M004 is closed with the bounded consolidation (run `36196068239` final, 17m17s, build 7m51s, exec 362s, 5 `session_*` binaries → 1 `session_family`).
- M005 is ready against the stabilized topology (hosted run `36196068239`).

## 5. Milestones

### M001 — Build/link measurement and low-risk test-profile tuning

Status: closed (`plans/closure/ci-test-throughput-optimization/001-status.md`;
steady-state baseline hosted run `36173876950`, 17m54s, JOBS=4).

Implementation plan:

- `plans/implementation/ci-test-throughput-optimization/001-build-link-measurement-and-test-profile-tuning.md`

Capture reproducible Cargo/Nextest timing evidence, A/B test CI-only test debuginfo generation and current Cargo build-job settings, retain only measured improvements, and leave a compact diagnostic method for later milestones.

### M002 — Heavy-test scheduling and live Eggwork qualification

Status: closed (`plans/closure/ci-test-throughput-optimization/002-status.md`;
implementations `a0b7f209` + `f3ff5e6d`; hosted run `36184915493` rerun,
17m26s, prebuild 4 s warm, build 8m25s, exec 350 s, live target
qualified).

### M003 — Integration-harness consolidation pilot

Status: closed (`plans/closure/ci-test-throughput-optimization/003-status.md`;
implementation `e27140fd`; hosted run `36193906726`, 17m31s, build 8m17s,
exec 338s, 11 projection_replay_* binaries → 1 `projection_replay`).

Implementation plan:

- `plans/implementation/ci-test-throughput-optimization/003-integration-harness-consolidation-pilot.md`

Select one representative high-fragmentation, non-heavy integration-test family and consolidate it into a smaller number of Cargo test targets without deleting test logic. Measure compile/link and execution effects. A negative result is valid closure.

### M004 — Bounded integration-test family consolidation

Status: closed (`plans/closure/ci-test-throughput-optimization/004-status.md`;
implementation `48790290`; hosted run `36196068239` final, 17m17s,
build 7m51s, exec 362s, 5 default-feature `session_*` binaries → 1
`session_family`).

Implementation plan:

- `plans/implementation/ci-test-throughput-optimization/004-bounded-integration-test-family-consolidation.md`

Apply the proven harness pattern to additional compatible families, preserving feature/resource boundaries and human navigability. Stop before heavy/live/special-platform tests where consolidation would weaken isolation or diagnostics.

### M005 — Compiler cache and final critical-path closure

Status: closed (`plans/closure/ci-test-throughput-optimization/005-status.md`;
sccache + same-job-overlap negative dispositions recorded;
documentation reconciled; final steady-state 17m17s hosted, run
`36196068239` final).

Implementation plan:

- `plans/implementation/ci-test-throughput-optimization/005-cache-and-critical-path-closure.md`

Evaluate compiler-result caching and safe same-job critical-path overlap against the stabilized topology. Retain only wins that are reproducible, bounded, and simpler than the time they save. Reconcile active testing documentation with the final measured routine-CI contract.

## 6. Measurement policy

Every milestone that changes CI timing must record:

- exact repository revision;
- hosted runner class when known;
- cache state: cold, warm, or unknown;
- step wall times from Actions;
- test execution count and duration where applicable;
- build/link duration separated from execution when possible;
- whether the run was superseded, retried, or affected by a flake;
- correctness result.

Do not compare a cold baseline to a warm candidate as proof of improvement. Prefer at least two comparable green measurements when runner variance could dominate the claimed gain.

Temporary timing output/artifacts may be used during implementation, but the final routine workflow must not gain a permanent performance gate or artifact-retention apparatus.

## 7. Exit conditions

The workstream closes when:

- routine CI retains equivalent correctness authority for ordinary changes;
- supported-Linux/live qualification remains explicitly owned and runnable;
- the final routine workflow remains one bounded non-release job;
- retained optimizations have measured positive effect;
- test/resource flakes do not increase;
- active docs accurately describe which tests are routine, change-sensitive, main-only, or manual;
- no permanent benchmark/matrix/scanner framework was introduced;
- final hosted evidence demonstrates a material reduction from the reviewed ~19-minute steady-state baseline, or the closure record explains why further reduction was not justified without weakening correctness.

A target below 15 minutes is desirable but is not a correctness gate. A negative optimization experiment may close if it is reverted and documented.

## 8. Non-goals

This roadmap does not authorize:

- deleting tests merely to improve timing;
- replacing Cargo/Nextest with a custom test runner;
- introducing self-hosted runners;
- changing product behavior or feature defaults;
- adding a generalized build farm or remote execution dependency;
- making releases depend on CI performance thresholds;
- broad workspace dependency refactoring unrelated to measured compile cost;
- raising parallelism past demonstrated runner/resource limits without new evidence.
