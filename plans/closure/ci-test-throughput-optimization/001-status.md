# CI/Test Throughput M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/ci-test-throughput-optimization/001-build-link-measurement-and-test-profile-tuning.md`

Source subsystem roadmap:

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md#...`

Repository baseline reviewed: `0c896db32d2325da39b129acde7dd406ce472dc4`

Implementation commits or pull requests:

- `e504751d` — M001 implementation (CARGO_BUILD_JOBS 8→4, diagnostic
  recipe + dispositions in `architecture/testing.md`)
- `3fe57df5` — verify.sh resource-policy pointer clarification; warmed
  the rotated rust-cache key for the second comparable measurement
- Hosted CI runs `36171306439` (green, cold cache), `36173876950`
  attempt 1 (red on a timing flake, build phase completed),
  `36173876950` attempt 2 / rerun (green, warm cache)

## 1. Executive finding

M001 is complete. The routine hosted configuration now has separated
build/link vs execution evidence, a measured final `CARGO_BUILD_JOBS=4`
(equal to runner vCPUs and nextest `ci` slots; the `8` probe language is
gone), an explicitly rejected `CARGO_PROFILE_TEST_DEBUG=0` override, a
declined codegen-units sweep with rationale, and a retained diagnostic
recipe. No test scope, assertion, resource classification, or feature
boundary changed. The new steady-state baseline is hosted run
`36173876950` (rerun): 17m54s total, 14m57s test step, 7m58s build
phase, 416 s execution of 11781 tests, all green.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| WP1 comparable baseline with separated compile/link + execution | Hosted run `36159580594` (warm, JOBS=8): total 18m45s; clippy 2m16s; test step 15m49s; `Finished test profile in 8m50s`; nextest Summary 414 s / 11781 passed | pass | Runner class ubuntu-latest (4 vCPU / 16 GB); rust-cache hit |
| WP2 `CARGO_PROFILE_TEST_DEBUG=0` measured, retained or rejected | Local codegen-isolated A/B (representative large target `session_crud`, `CARGO_INCREMENTAL=0`, JOBS=8): 2m22s baseline vs 2m23s with override (wall parity; CPU 3m44s vs 2m58s). Incremental A/B confounded by cache maturity, discarded. Hosted `Finished ... + debuginfo` confirms generation happens but the parallel phase is link-count dominated | pass (rejected) | No CI-only override; no profile change; diagnostic cost avoided |
| WP3 `CARGO_BUILD_JOBS` measured final value | Warm hosted: 6m46s + 7m58s build phase (JOBS=4) vs 8m50s (JOBS=8); totals 17m54s vs 18m45s; clippy 2m08s vs 2m16s (parity) | pass (retained `4`) | Cold transition run (10m10s) labeled non-comparable; see §4 |
| WP4 codegen-units single-variable experiment if material | Declined: wall parity in WP2 shows codegen is not the wall bottleneck for a representative unit; link-count structure dominates (100 binaries) | pass | Recorded for M003; no sweep performed per plan |
| WP5 reproducible diagnostic instructions | `architecture/testing.md` "Build/link vs execution diagnostics (M001 recipe)" + retained-settings dispositions | pass | No helper script (direct Cargo/Nextest commands preferred per plan) |
| Required verification (fmt, quick, clippy, nextest, hosted) | fmt ✓; `verify.sh quick` ✓; clippy `+1.98.1` ✓ (see local-toolchain note); local nextest 6919 passed / 1 timing failure under extreme load (see note); hosted green `36173876950` rerun | pass | Hosted CI is the acceptance signal; local flakes characterized below |
| Acceptance: no scope/assertion/resource/feature change | `git diff` implementation commits: `.github/workflows/ci.yml` env + comments, `architecture/testing.md`, `scripts/verify.sh` comment only | pass | — |
| Acceptance: no retained setting described as experiment | ci.yml env block rewritten as measured final; testing.md economy item 4 fixed | pass | Stale `~37-minute` concurrency comment also corrected |
| Acceptance: no new CI lane/benchmark/artifact gate | Workflow remains one `verify` job; `cargo --timings` stays local/ephemeral | pass | — |

## 3. Production implementation evidence

Landed changes (no production Rust code; CI/dev-infra only):

- `.github/workflows/ci.yml`: `CARGO_BUILD_JOBS: 8 → 4` with the
  experimental probe comment replaced by the measured-final rationale
  (== vCPUs == nextest slots; rustc threads codegen internally);
  stale `~37-minute` concurrency comment corrected to the current
  steady state.
- `architecture/testing.md`: CI economy item 4 fixed to the final
  value; new "Build/link vs execution diagnostics (M001 recipe)"
  section (hosted phase separation, local representative-unit probe
  with incremental-bias warning, ephemeral `--timings` usage);
  recorded WP2/WP3/WP4 dispositions.
- `scripts/verify.sh`: resource-policy header now points at the hosted
  `CARGO_BUILD_JOBS=4` override and the M001 disposition record.

Behavior deliberately absent: no `CARGO_PROFILE_TEST_DEBUG`
override, no `[profile.test]` change, no codegen-units change, no
sccache, no consolidation, no live-qualification move, no release
profile change.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
scripts/verify.sh quick
cargo +1.98.1 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.98.1 nextest run --workspace --locked --profile ci
cargo +1.98.1 test --test scheduler_cancellation --locked -- --test-threads=1
touch src/lib.rs && time cargo test --locked --no-run --test session_crud            # WP2 incremental A/B
touch src/lib.rs && time CARGO_INCREMENTAL=0 cargo test --locked --no-run --test session_crud  # WP2 clean A/B, both arms
```

### Results

- fmt: pass. quick: pass.
- clippy: pass with the CI-parity toolchain (`1.98.1`, matching hosted
  `stable-x86_64-unknown-linux-gnu rustc 1.98.1`). Note: the local
  default toolchain (`1.89.0`, the MSRV floor) reports four
  pre-existing lint failures (`collapsible_else_if`,
  `overly_complex_bool_expr`, `nonminimal_bool`, `ptr_arg`) in
  `codegg-providers`/`egglsp` on code untouched by this milestone;
  hosted stable is green on the identical tree. Local/production
  conclusion: stale-toolchain lint drift, not an M001 regression.
- Local nextest (1.98.1, profile `ci`): 6919 passed / 1 failed —
  `scheduler_cancellation::cancel_running_job_terminates_process_and_releases_permit`
  ("executor must observe cancellation token") under machine load
  average ~47 (decaying from the build). Serial re-runs of that
  binary on macOS fail 3 timing-sensitive tests deterministically
  while hosted Linux is green on the same code: pre-existing
  platform timing sensitivity (real-`sleep` cancellation
  propagation), owned for re-examination by M002 WP3. M001 changed no
  test-execution semantics (`CARGO_BUILD_JOBS` affects compile
  parallelism only).
- Hosted runs (ubuntu-latest, `Swatinem/rust-cache@v2`,
  `CARGO_INCREMENTAL=0` set by the cache action):
  - `36159580594` baseline (warm, JOBS=8): 18m45s total; clippy
    2m16s; test step 15m49s; build `8m50s`; exec Summary `414s`,
    11781 passed / 1 skipped. Green.
  - `36171306439` M001 impl (JOBS=4): `No cache found` — changing a
    `CARGO_*` value rotates the rust-cache env-hash key, so this run
    rebuilt dependencies from scratch: 21m51s total; build `10m10s`;
    exec `421s`, 11781 passed. Green but **non-comparable** for the
    JOBS decision (cold vs warm); recorded only as the transition
    cost. Operational finding for M005: any `CARGO_*` tuning pays a
    one-time ~3 min cold-cache run.
  - `36173876950` attempt 1 (warm JOBS=4 key): build `6m46s`, then
    fail-fast on `session_control_m004_controller_lease::
    reaper_releases_on_turn_completed_event` (event-timing flake;
    7184 passed before cancel). Red; build phase completed and is
    valid evidence. No code in scope can affect this test's timing.
  - `36173876950` rerun / attempt 2 (warm): **17m54s total**; clippy
    2m08s; test step 14m57s; build `7m58s`; exec Summary `416s`,
    **11781 passed / 1 skipped. Green.**
- Warm build-phase comparison (the JOBS decision): 6m46s + 7m58s
  (JOBS=4) vs 8m50s (JOBS=8) — both samples favorable, mean ≈ −1.5
  min. Total: 17m54s vs 18m45s (−51s). Retained.

## 5. Invariant review

- All tests / routine scope preserved: yes (11781 tests, same count
  across baseline and candidate; 1 pre-existing skip in both).
- mold retained; one routine CI job; Nextest `ci` semantics
  untouched (no `.config/nextest.toml` change); docs-only skip and
  superseded-run cancellation intact.
- Local `verify.sh` resource defaults unchanged in behavior
  (`CARGO_BUILD_JOBS=2` default; comment-only edit).
- No benchmark lane, no sccache, no target consolidation, no live
  move, no release-profile change, no weakened Clippy/fmt, no
  timeout or resource-class change.

## 6. Failure and recovery review

- Attempt-1 timing flake (`reaper_releases_on_turn_completed_event`):
  event-driven lease-reaper window missed under 4-slot parallel
  execution; passed on rerun and in the baseline. No M001-scope
  mechanism (compile-job count cannot affect post-build test
  timing). Recorded as a watch item for M002's execution-tail
  measurement, not Sparrow-corrected here.
- Local `scheduler_cancellation` macOS failures: platform timing
  sensitivity, pre-existing, Linux-green; M002 WP3 owns the
  heavy-test scheduling re-audit.
- Cache-key rotation on `CARGO_*` change: one-time cold rebuild;
  no recovery action needed (cache re-saved under the new key,
  subsequent runs warm).

## 7. Migration and compatibility review

No schema, protocol, config, or public-surface change. Rollback is a
one-line env revert (`CARGO_BUILD_JOBS: 4 → 8`) plus comment; no data
migration involved. Contributor-facing commands unchanged.

## 8. Security review

No authorization, secret, network, or privilege change. CI retains
`contents: read`, no release authority. No new dependency or tool.

## 9. Documentation and operations

- `architecture/testing.md`: retained settings, M001 dispositions,
  diagnostic recipe (see §3).
- `.github/workflows/ci.yml` comments: measured-final rationale, no
  probe language.
- `scripts/verify.sh` header: hosted-override pointer.
- New steady-state baseline for M002: run `36173876950` (17m54s).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Local default-toolchain (1.89.0) clippy drift on untouched files | None on CI (1.98.1 green); may confuse local MSRV-floor runs | None in this workstream; consider toolchain Covid-note in contributing docs if drift recurs (M005 docs pass) |
| low | `scheduler_cancellation` real-sleep tests fail on macOS locally while Linux-green | Local-only signal; no CI impact | M002 WP3 re-audit owns heavy-test determinism |
| low | `reaper_releases_on_turn_completed_event` flaked once hosted under parallel execution | Single occurrence; passed rerun; no scope mechanism | Watch item for M002 WP5 execution-tail comparison; escalate only on repeat |
| low | Any future `CARGO_*` tuning pays a one-time cold-cache run (~+3 min) | Measurement planning only | M005 cache-closure owns the mechanics note |

No high/medium/critical findings. Nothing blocks M002.

## 11. Roadmap disposition

Milestone closed; M002 may proceed. M002's stable baseline is hosted
run `36173876950` (17m54s, JOBS=4, warm cache). Handoff evidence for
M003 recorded: the parallel build phase remains link-count dominated
(~100 test executables; single-unit codegen A/B showed wall parity),
supporting the consolidation hypothesis to be proven or rejected by
the M003 pilot.

## 12. Registry updates

- `plans/registry.md`: M001 `ready → closed`; M002 `blocked → ready`
  (hard dependency satisfied); CI gate text updated.
- `plans/subsystems/ci-test-throughput-optimization-roadmap.md`: M001
  `ready → closed`; M002 `blocked on M001 → ready`.
- `plans/implementation/ci-test-throughput-optimization/001-*.md`:
  `ready → implemented (closed; see closure record)`.
