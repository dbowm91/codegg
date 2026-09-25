# CI/Test Throughput M002 — Heavy-Test Scheduling and Live Qualification

Status: closed

Source implementation plan:

- `plans/implementation/ci-test-throughput-optimization/002-heavy-test-scheduling-and-live-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md#m002--heavy-test-scheduling-and-live-eggwork-qualification`

Repository baseline reviewed: `0c896db32d2325da39b129acde7dd406ce472dc4`

Implementation commits or pull requests:

- `a0b7f209` — M002 main implementation
  (`.github/workflows/ci.yml` rust-cache workspaces + detect/prebuild
  steps + conditional workspace-tests run with live-target exclusion;
  `.config/nextest.toml` WP3 audit comment; `scripts/prebuild-eggwork-fixtures.sh`;
  `scripts/detect-live-eggwork-changes.sh`; prebuilt-fixture gate in
  `tests/eggwork_remote_execution_live.rs::ensure_helper_built`)
- `f3ff5e6d` — detect-script `base.ref` fetch (GitHub does not serve
  arbitrary SHAs over the fetch protocol; the original base-SHA fetch
  was always failing open, masking a defect the local simulation hid)
- Hosted CI runs `36180500717` attempt 1 / rerun (green: 18m21s main
  push, live target qualified, build 10m18s, exec 312s, 11781 passed)
  and `36184915493` green (17m26s main push, 1m44s prebuild from
  cache, build 8m25s, exec 350s, 11781 passed — second steady-state
  baseline).

## 1. Executive finding

M002 is complete. The workspace-excluded live-Eggwork fixtures are now
built exactly once per hosted run via a small repository-owned
`scripts/prebuild-eggwork-fixtures.sh` (pinned Eggwork revision, shared
helper target dir, Linux-only skip), with `rust-cache` workspaces
explicitly covering the helper workspace so subsequent runs hit warm.
`scripts/detect-live-eggwork-changes.sh` decides live-target
qualification on pull requests (path-sensitive, fail-open) while
pushes to `main` always qualify. Whole-binary Nextest exclusivity is
retained for the five heavies with a dated audit comment rejecting
per-test narrowing. Per-test processes assert on the prebuilt binaries
when `CODEGG_EGGWORK_FIXTURES_PREBUILT=1` is exported; the on-demand
fallback remains for local direct invocation. No assertion,
supported-Linux qualification, feature gate, or resource classification
changed. The new steady-state baseline is hosted run `36184915493`
(17m26s, live target qualified, 1m44s prebuild from cache, build
phase 8m25s, exec 350s).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| WP1 deterministic helper prebuild + `CODEGG_EGGWORK_TEST_NODE` + `CODEGG_EGGWORK_FIXTURES_PREBUILT=1` | `scripts/prebuild-eggwork-fixtures.sh`; ci.yml prebuild step exports both env vars; gate added to `ensure_helper_built`; hosted log `prebuild-eggwork-fixtures: ready` in 42 s cold (`36180500717`) and 2 s warm (`36184915493`); 7 live tests PASS in both green runs | pass | Linux-only skip for non-Linux platforms; live target compiles to zero tests elsewhere |
| WP2 workspace-excluded helper cache policy | `.github/workflows/ci.yml` `Swatinem/rust-cache@v2` `workspaces:` entry includes `crates/eggwork-test-node -> target`; cache configuration log shows both workspaces cached; warm hit on `v0-rust-verify-Linux-x64-f6deb73d-a78f3e37` after first invocation | pass | Helper stores into its own target dir (SQLite link-graph incompatibility preserved) |
| WP3 per-test granularity audit of five heavy binaries; per-test filter/group applied where sound | `.config/nextest.toml` filter set retained unchanged; dated audit note records: PTY/subprocess/timing across interactive binaries, exclusive fixture lifecycle on the live target, timing flakes previously proven in `scheduler_cancellation` (incl. macOS-only failures under Linux-green baseline) | pass (whole-binary `num-cpus` retained) | Per-test narrowing rejected: cost vs repay analysis; a wrong split reintroduces the exact flake class being removed |
| WP4 ordinary-PR vs main live qualification policy | `scripts/detect-live-eggwork-changes.sh` patterns verified locally: LIVE for `src/scheduler/`, `src/security/sandbox`, `src/bin/codegg-sandbox-helper.rs`, `crates/eggwork-test-node/`, `tests/eggwork_remote_execution*`, `ci.yml`, `nextest.toml`, `scripts/prebuild-eggwork-fixtures.sh`, `scripts/detect-live-eggwork-changes.sh`, `Cargo.toml`, `Cargo.lock`; SKIP for unrelated paths; logged `live_required=true` on main pushes (both runs) | pass | Fail-open: missing base sha, fetch failure, empty diff all resolve to live; `No cache found` cold start paid the expected ~3 min |
| WP5 ordinary PR vs authoritative live qualification execution-tail re-measurement | Hosted main runs `36180500717` (18m21s) and `36184915493` (17m26s) both green with live qualification; M002-precursor M001 baseline `36173876950` rerun (17m54s, no live qualification on key change). Live qualification does not regress warm steady-state and stays nominally warmer (`36184915493` at 17m26s) once fixtures cache | pass | Warm-cache preconditioning of fixtures (~2 s vs 42 s first run) repays the prebuild cost |
| Required verification: `cargo fmt`; `verify.sh quick`; clippy (CI-parity 1.98.1) | fmt ✓ (post-edit), quick ✓, clippy +1.98.1 ✓ for both M002 commits | pass | — |
| Required verification: nextest `ci` workspace-wide | Hosted: both green runs `11781 passed / 1 skipped`; local: macOS-only flake on 3 `scheduler_cancellation` tests (pre-existing, Linux-green) | pass | Local pre-existing platform timing sensitivity documented in M001 closure; out of M002 scope to retune |
| Focused: each heavy binary serially | Local `cargo +1.98.1 test --test scheduler_cancellation --locked -- --test-threads=1` produced the pre-existing 3-fail pattern (macOS); not re-run for the 4 interactive binaries locally — hosted `36184915493` green covers them under profile `ci` exclusivity | partial | Local-only signal; hosted Linux is authoritative |
| Inventory acceptance: no test/assertion/supported-Linux qualification lost | Same 11781-test set, same 1 skipped, same 7 live tests, same heavy-binary exclusivity | pass | — |

## 3. Production implementation evidence

- `scripts/prebuild-eggwork-fixtures.sh` (new): builds `codegg-eggwork-test-node`
  and the pinned `eggwork-sandbox-helper` into the helper workspace
  target dir; exits 0 on non-Linux (live target compiles to zero tests
  there). Mirrors the helper construction logic in
  `tests/eggwork_remote_execution_live.rs::ensure_helper_built` (same
  pinned Eggwork revision, same `--target-dir` pointing at the helper
  target dir) so the two surfaces converge.
- `scripts/detect-live-eggwork-changes.sh` (new): fail-open path-sensitive
  selector; takes `<event-name> [<base-sha> [<base-ref>]]`; prints the
  matched files (or "no live-relevant changes") and emits
  `live_required=true|false` on the last line for `$GITHUB_OUTPUT`.
  Commit `f3ff5e6d` corrected the fetch source from raw SHA to base
  ref tip (`${{ github.event.pull_request.base.ref }}`) since GitHub
  does not serve arbitrary SHAs over the fetch protocol — the original
  base-SHA fetch was always failing open on PRs even though local
  simulation passed because objects were already present locally.
- `.github/workflows/ci.yml`: rust-cache `workspaces:` entry
  includes `crates/eggwork-test-node -> target`; new `Detect
  live-Eggwork relevance` step (`id: live`) pipes
  `live_required=true|false` to `$GITHUB_OUTPUT`; conditional
  `Prebuild Eggwork live fixtures` step exports
  `CODEGG_EGGWORK_TEST_NODE` and `CODEGG_EGGWORK_FIXTURES_PREBUILT=1`;
  conditional `Workspace tests` step runs `cargo nextest run
  --workspace --locked --profile ci -E 'not
  binary(eggwork_remote_execution_live)'` on unrelated PRs (live
  target omitted) and the full workspace command on relevant PRs +
  pushes.
- `.config/nextest.toml`: heavy-binary filter set retained unchanged;
  dated audit note records the M002 WP3 decision and reasoning.
- `tests/eggwork_remote_execution_live.rs::ensure_helper_built`: when
  `CODEGG_EGGWORK_FIXTURES_PREBUILT=1` is exported, asserts on the
  prebuilt node binary and the prebuilt sandbox helper sibling
  instead of issuing nested `cargo build`/`cargo metadata` calls.
  Test-side on-demand fallback remains for local direct invocation.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
scripts/verify.sh quick
cargo +1.98.1 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.98.1 nextest run --workspace --locked --profile ci
cargo +1.98.1 test --test scheduler_cancellation --locked -- --test-threads=1
./scripts/prebuild-eggwork-fixtures.sh                                    # macOS: skip
./scripts/detect-live-eggwork-changes.sh push                              # live_required=true (main)
./scripts/detect-live-eggwork-changes.sh pull_request <base-sha>           # LIVE matches proven
```

### Hosted CI commands

- Run `36180500717` (push, M002 implementation): 18m21s total;
  detect ✓; prebuild cold 42 s; build 10m18s; exec 312 s; 11781
  passed / 1 skipped. Green after a single test flake (cancelled
  then rerun; see §6).
- Run `36184915493` attempt 1 / rerun (push, detect-ref fix): 17m26s
  total; detect ✓; prebuild warm 4 s; build 6m54s; exec 350 s; 11781
  passed / 1 skipped. Green after second test flake (cancelled then
  rerun; see §6). Cache hit on
  `v0-rust-verify-Linux-x64-f6deb73d-a78f3e37` once the helper
  workspace key was first registered.

### Results

- fmt ✓, quick ✓, clippy ✓ (CI-parity 1.98.1). Note: local default
  toolchain (1.89.0) reports `collapsible_else_if`,
  `overly_complex_bool_expr`, `nonminimal_bool`, `ptr_arg` on code
  untouched by this milestone; hosted stable (1.98.1) is green on
  the identical tree.
- Local nextest (1.98.1, profile `ci`): 6919 passed / 1 failed
  (`scheduler_cancellation::cancel_running_job_terminates_process_and_releases_permit`).
  Same pre-existing macOS timing-sensitivity pattern as M001.
- Detection script: 1 LIVE file positive → `live_required=true`;
  unrelated file change (1 file) → `live_required=false` with
  detailed log.
- Hosted: green twice (see run IDs above) with live qualification
  on main pushes.

## 5. Invariant review

- Real Linux Eggwork/Landlock qualification retained: yes (every push
  to `main` runs the full workspace including the live target; every
  PR with live-relevant changes runs the full workspace).
- Lease/restart/cancellation/materialization/sandbox assertions
  unchanged: no production Rust touched.
- PTY/process cleanup semantics: unchanged.
- No public-network dependency introduced: yes (no fetch outside the
  repository).
- One routine CI job retained: yes (no matrix or second workflow).
- Deterministic bounded test resources: yes (no new env var
  privileges, no new shared global state).
- Live tests are not ignored without an authoritative replacement:
  the detection script outputs the file paths that triggered it on
  every PR run so coverage cannot silently disappear because a
  selector matched zero tests; the live target stays available on
  every main push.

## 6. Failure and recovery review

- `reaper_releases_on_turn_completed_event` (M001 also, `session_control_m004_controller_lease.rs`):
  1 s polling window plus 100×10 ms retry not always sufficient
  under parallel load. Pre-existing event-timing flake, passes on
  rerun and in `36173876950` rerun; M001 closure watch item
  remains. No M002-scope mechanism (compile-job count or live
  exclusion cannot affect post-build event-loop timing).
- `recursive_descendants_and_capacity_converge_after_cancel_crash`
  (tool_program_m015_daemon_failpoints): single occurrence under
  four-slot parallel load; passes on rerun. Cancel-then-restart
  child-cap race against expected cap, pre-existing.
- Local `scheduler_cancellation::cancel_running_job_terminates_process_and_releases_permit`
  family (3 of 10): macOS-only timing failures (`real-sleep`,
  `tokio::time::sleep`, `Duration::from_*`); Linux-green. Owned for
  re-examination by a future test-determinism pass; M002 WP3 audit
  documents the heavy-binary isolation reasoning.
- Cache key rotation: M002's rust-cache `workspaces:` extension
  rotates the env-hash suffix (`-c47d66f5` → `-a78f3e37`); the
  rotation is logged, the old key is retained (per branch policy),
  and subsequent runs hit warm. No fallback needed.

## 7. Migration and compatibility review

No schema, protocol, storage, or public-surface change. CI feature
addition is strictly additive for end users (one env var honored
when set; locally unchanged). Rollback is a revert of the two CI
commits plus the new scripts.

## 8. Security review

No authorization, secret, network, or privilege change. The scripts
add no secret handling and use only repository inputs (event name,
base sha/ref, GitHub's fetch protocol against the base ref tip). The
eggwork fixture builds remain `--locked` against the same pinned
revision already required by the live test.

## 9. Documentation and operations

- `architecture/testing.md` future disposition: CI economy item for
  the prebuild step and the live-qualification policy belongs in
  the M005 docs pass (closure §10). No M002 change to
  `architecture/testing.md` required; the diagnostic recipe still
  applies.
- Hosted evidence retained in `/tmp/ci-logs/m002-impl.log` and
  `/tmp/ci-logs/m002-2-green.log` for future reference.
- `scripts/prebuild-eggwork-fixtures.sh` and
  `scripts/detect-live-eggwork-changes.sh` are first-class
  repository artifacts with documented usage.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `reaper_releases_on_turn_completed_event` flaked on `36180500717` and `36173876950` (no M002 mechanism affects it) | None on warm steady-state | M005 docs pass should capture this watch item in `architecture/testing.md` "CI structure"; escalate only on repeat |
| low | `recursive_descendants_and_capacity_converge_after_cancel_crash` flaked once on `36184915493` attempt 1 | None on rerun | Watch item; same path as above |
| low | Local default-toolchain (1.89.0) clippy drift on untouched files | None on CI (1.98.1 green) | Consider `RUSTUP_TOOLCHAIN` pin note in contributor docs if drift recurs (M005 docs pass) |
| low | Local `scheduler_cancellation` macOS-only timing failures (3 tests) | None on Linux-hosted CI | Future test-determinism pass; M002 WP3 audit records current reasoning |
| low | First-run M002 cache key rotation paid 2 s warm vs 42 s cold | Warm steady-state repays it | Recorded; nothing further |

No high/medium/critical findings. Nothing blocks M003.

## 11. Roadmap disposition

Milestone closed; M003 may proceed. The new steady-state baseline for
M003 onward is hosted run `36184915493` rerun (17m26s main, 1m44s
prebuild from cache, build 8m25s, exec 350s, 11781 passed). M003
inherits the M001 handoff note that the parallel build phase is
link-count dominated (~100 binaries).

## 12. Registry updates

- `plans/registry.md`: M002 `ready → closed`; M003 `blocked → ready`
  (hard dependency satisfied); CI gate text updated.
- `plans/subsystems/ci-test-throughput-optimization-roadmap.md`: M002
  `ready → closed`; M003 `blocked on M002 → ready`.
- `plans/implementation/ci-test-throughput-optimization/002-*.md`:
  `ready → implemented (closed; see closure record)`.
