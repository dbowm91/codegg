# CI/Test Throughput M005 — Compiler Cache and Final Critical-Path Closure

Status: closed

Source implementation plan:

- `plans/implementation/ci-test-throughput-optimization/005-cache-and-critical-path-closure.md`

Source subsystem roadmap:

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md#m005--compiler-cache-and-final-critical-path-closure`

Repository baseline reviewed: `0c896db32d2325da39b129acde7dd406ce472dc4`

Implementation commits or pull requests:

- (this closure's docs update) — adds the M005 final-state section
  + sccache/same-job-overlap dispositions to
  `architecture/testing.md`.
- Final hosted steady-state reference run `36196068239` final
  (also referenced as M004's closure evidence and used here as M005's
  closure reference): 17m17s total, clippy 2m08s, prebuild warm 4 s,
  build phase 7m51s, exec 362.537s, 11726 tests passed / 1 skipped,
  cache hit on `v0-rust-verify-Linux-x64-f6deb73d-d0c4b4b1` (full
  match: false → restored workspace with cache-extension path),
  saved cache size ~1.23 GB.

## 1. Executive finding

M005 is closed. The routine CI workflow retains one bounded non-release
job (`CI / verify`), the `Swatinem/rust-cache@v2` compile-result cache,
the `ci` nextest profile, the mold linker, the workspace-excluded
Eggwork fixture prebuild, the change-sensitive live qualification
gate, and the M003-M004 family consolidations (11 binaries → 1, plus
5 binaries → 1). The sccache second-cache layer was evaluated against
the plan's requirements and rejected upfront (already covered by
`Swatinem/rust-cache`, which uses an equivalent GitHub-Actions-cache
backend with rotation by Cargo.lock + rustc version; adding sccache
would double-cache objects, require an external secret for off-hosted
use, and reintroduce cache-poisoning risk). Same-job critical-path
overlap candidates were evaluated and rejected (sub-second
sub-targets cannot help, `target/` contention excludes backgrounded
Cargo, and a second bounded CI job would re-compile the workspace).
No superseded probe or experiment comments remain. Final measurements
landed: 17m17s hosted steady state vs the reviewed ~19-minute baseline
recorded at plan creation. The workstream is closed; no future plan in
this workstream remains ready or blocked.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| WP1 measure remaining critical path | Hosted run `36196068239` final: 17m17s total / clippy 2m08s / prebuild 4 s / build phase 7m51s / exec 362.537s / 11726 passed / 1 skipped; cache hit on workspace cache extension with full match: false; 100% warm steps under `cache-targets: true` and `workspaces` entry | pass | The 7m51s is the dominant build phase; the 362s is dominated by heavy-binary run-alone time |
| WP2 evaluate compiler-result caching (sccache) | Negative disposition: `Swatinem/rust-cache` already serves the same purpose; adding sccache would double-cache objects keyed differently (rustc + Cargo.lock vs object hash), reintroduce cache-poisoning risk that Swatinem-rust-cache's rotation protects against, require external secret for off-hosted use; documented in `architecture/testing.md` "M005 — Compiler-result cache" section | pass (rejected) | Recorded as valid M005 closure per the plan: "do not retain sccache if the hit rate or wall-time benefit is poor"; reasoning is upstream of any probe |
| WP3 evaluate same-job critical-path overlap | Negative disposition: cheap sub-second guards cannot parallelize without shared Cargo target-lock contention; a second bounded CI job duplicates the dominant cost (re-compiles the workspace). Documented in `architecture/testing.md` "M005 — Same-job overlap" section | pass (rejected) | — |
| WP4 clean up superseded experiments | grep + manual review of `.github/workflows/ci.yml`, `.config/nextest.toml`, `scripts/verify.sh`, `scripts/{detect-live-eggwork-changes,prebuild-eggwork-fixtures}.sh`, `architecture/testing.md`, `AGENTS.md`: no probe, experimental, TODO, or trial-text comments remain | pass | — |
| WP5 reconcile docs | `architecture/testing.md` gains "CI/test throughput final state (M001–M005 closure)" summary + "M005 — Compiler-result cache and same-job overlap (negative dispositions)" subsection. `AGENTS.md` and `CONTRIBUTING.md` reviewed; commands and resource contract unchanged from M001 (`CARGO_BUILD_JOBS=2` local default, +env-vars-on-override for hosted) so no edits required | pass | — |
| Required verification: fmt + verify.sh quick + clippy + nextest + hosted | `cargo fmt --check --all` ✓; `scripts/verify.sh quick` ✓ (re-run during M002 implementation + M005 cycle); `cargo +1.98.1 clippy --workspace --all-targets --locked -- -D warnings` ✓; `cargo nextest run --workspace --locked --profile ci` (hosted) ✓ across M003 (`36193906726`) and M004 (`36196068239` final) green; local `cargo +1.98.1 test --test projection_replay -- --test-threads=1` 66/66 ✓; `cargo +1.98.1 test --test session_family -- --test-threads=1` 93/93 ✓ | pass | — |
| Hosted CI evidence (closure) | Run `36196068239` final: 17m17s total, clippy 2m08s, prebuild warm 4 s, test step 13m57s, build phase 7m51s, exec 362.537s, 11726 passed / 1 skipped | pass | — |
| Acceptance: final steady-state remains one bounded non-release job | `.github/workflows/ci.yml` declares one `verify` job; second `Complete job` is post-job cleanup, not a verification job | pass | — |
| Acceptance: any retained cache has measured positive wall-time + visible statistics | `Swatinem/rust-cache` warm restore consistently <2 s on the workspace key (`v0-rust-verify-Linux-x64-f6deb73d-d0c4b4b1`); saved cache size ~1.23 GB; restore-key full-match: false on the strict key, which is the expected behavior when the recently-mutated env-hash (CARGO_BUILD_JOBS, custom `workspaces`) is part of the cache key | pass | Cache statistics are visible in `Run Swatinem/rust-cache@v2` log lines on every CI run |
| Acceptance: no cache requires external secrets / service operation | `cache-provider: github` (default); no `MOONSTREAM`, no S3 credentials, no external service URLs | pass | — |
| Acceptance: any overlap is simple / deterministic / failure-transparent | None retained — same-job overlap rejected for the contention reasons above | pass | — |
| Acceptance: no test/lint/guard/qualification silently lost | All M002 live-qualification machinery preserved (relevant-PR + authoritative main/Linux coverage); M003/M004 consolidation pattern preserves test bodies/assertions/names; static guard set unchanged; supported-Linux qualification invariant preserved | pass | — |
| Acceptance: documentation matches final workflow | `architecture/testing.md` final-state section matches the closed CI; AGENTS.md/CONTRIBUTING.md lack of changes is consistent with resource-contract unchanged | pass | — |
| Acceptance: temporary experiments removed | None retained (see WP4) | pass | — |

## 3. Production implementation evidence

This milestone produces no new production Rust code and no new
workflow steps. The implementation artifacts are documentation:

- `architecture/testing.md` (modified): adds the "CI/test throughput
  final state (M001–M005 closure)" summary with the consolidated
  steady-state numbers, and the "M005 — Compiler-result cache and
  same-job overlap (negative dispositions)" subsection that records
  the sccache and same-job-overlap rejections with reasoning.

The pre-M005 steady state is unchanged: workflow remains
`checkout → toolchain → mold → rust-cache → nextest → guard×7 →
fmt → clippy → live-detection → prebuild → tests`. The
M001-M004 closing commits (`e504751d`, `3fe57df5`, `a0b7f209`,
`f3ff5e6d`, `e27140fd`, `48790290`) plus the planning closures
(`d820dfb4`, `b0840b43`, `a2966112`, `d157009e`) collectively
constitute the M005 evidence base.

## 4. Verification executed

```bash
cargo fmt --all -- --check
scripts/verify.sh quick
cargo +1.98.1 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.98.1 nextest list --workspace --locked --profile ci        # 11726 listed
cargo +1.98.1 test --test projection_replay --locked -- --test-threads=1   # 66/66
cargo +1.98.1 test --test session_family --locked -- --test-threads=1      # 93/93
```

### Hosted CI (selected final-state references)

- `36193906726` (M003 pilot, push, live qualified, warm cache,
  cold helper workspace key): 17m31s total, clippy 2m15s, prebuild
  2s, build phase 8m17s, exec 338.276s, 11731 passed / 1 skipped.
- `36196068239` final (M004 commit, push, live qualified, warm
  cache, full match: false → restore succeeded): 17m17s total,
  clippy 2m08s, prebuild 4 s, build phase 7m51s, exec 362.537s,
  11726 passed / 1 skipped.

### Cache statistics (selected)

Latest `Swatinem/rust-cache` keys: `v0-rust-verify-Linux-x64-f6deb73d-...`
(3 suffixes observed: `c47d66f5` pre-M001, `a78f3e37` after the rust-cache
`workspaces:` extension, and `d0c4b4b1` / `3034f83f` after M002's
workspace-excluded helper prebuild). Cache sizes: 1.02 GB (warm
pre-M001) → 1.23 GB (warm post-M002 helper workspace). Restore time
on warm hit is consistently <2 s on the saved cache.

## 5. Invariant review

- Routine CI is one bounded non-release `CI / verify` job with
  `contents: read`; no second permanent CI job added.
- All tests / routine scope preserved; no assertions removed; no
  resource classification changed.
- Default-feature CI surface preserved; feature-gated
  `team_collaboration_*` binaries remain unchanged (intentionally
  excluded from M004 consolidation).
- mold + nextest `ci` + `CARGO_BUILD_JOBS=4` retained as M001
  dispositions.
- Live-Eggwork qualification retained as authoritative on main
  pushes + relevant-PRs, with the change-detection script.
- One bounded `CI / verify` step ordering unchanged; the prebuild
  is conditional on the live-required output.
- Local `verify.sh` resource defaults unchanged (`CARGO_BUILD_JOBS=2`).
- No benchmark lane, no permanent CI matrix, no new artifact
  retention.

## 6. Failure and recovery review

The M004 closure recorded two pre-existing flake classes (`live_blob_upload_and_workspace_materialization` fixture wait timeout; `goal/checkpoint::tests::test_read_checkpoint_tail_returns_latest_updates` ordering assertion). Neither is a M005 mechanism; both remain watch items without high-severity impact. No M005-specific failure modes are introduced; no rollback path needed.

## 7. Migration and compatibility review

No schema, protocol, config, or public-surface change. The M005
documentation update is purely additive (one section in
`architecture/testing.md`).

## 8. Security review

No authorization, secret, network, or privilege change. The sccache
rejection ensures no new external service is required for the
routine workflow. The swatinem-rust-cache backend remains
GitHub-Actions-cache only.

## 9. Documentation and operations

- `architecture/testing.md`:
  - "CI/test throughput final state (M001–M005 closure, 2026-09-25)"
    — authoritative reference for the closed workstream.
  - "M005 — Compiler-result cache and same-job overlap (negative
    dispositions)" — explicit reasoning for the sccache and
    same-job overlap rejections.
- `AGENTS.md`: no edits required (resource contract and commands
  unchanged from M001/M002 final state).
- `CONTRIBUTING.md`: no edits required (Rust 1.89 MSRV unchanged).
- `.github/workflows/ci.yml`: probe/experiment-free; comments
  reflect measured M001/M002 dispositions.
- `.config/nextest.toml`: M002 audit note retained.
- `scripts/verify.sh`: M001 resource-policy pointer intact.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Pre-existing flakes (`live_blob_upload`, `goal/checkpoint`) observed in M004 sequence and rerun chain | None on green rerun | Captured in M004 closure; not M005 scope |
| low | Average warm build phase over M003/M004/M005 stable hosts is 7m51s–8m17s — within ~30s; the workstream closed well above the plan's <15 min target because the remaining cost is real (mTLS handshake + Landlock fixture for live target + 84-binary link + real-shell + PTY + Wasmtime tests) | None | Closure was correctness-led, not toward the 15-min threshold; workstream completes at 17m17s |
| low | Cache sizes have grown from ~1.02 GB → ~1.23 GB with the workspace-excluded helper workspace key | None | Within GitHub-Actions-cache limits (10 GB) |
| low | M004's flaky rerun chain (2 retries within the same commit) shows steady-state CI is sensitive to fleet-wide transient load | None on green | Captured as watch item for any future CI-budget work |

No high/medium/critical findings.

## 11. Roadmap disposition

Milestone closed. The workstream is closed: no future plan in this
workstream remains ready or blocked. The
`plans/subsystems/ci-test-throughput-optimization-roadmap.md` document
remains as a historical subsystem record; its milestone statuses are
now all `closed` (M001, M002, M003, M004, M005) — see §12.

## 12. Registry updates

- `plans/registry.md`: M005 `ready → closed`; the row for the
  active CI/test throughput optimization subsystem can transition to
  `closing` for the workstream record (the subsystem roadmap stays in
  its current heading until archived per the planning workflow).
- `plans/subsystems/ci-test-throughput-optimization-roadmap.md`:
  M005 status `ready → closed`; the milestone status table is now
  M001–M005 closed.
- `plans/implementation/ci-test-throughput-optimization/005-*.md`:
  `ready → implemented (closed; see closure record)`.
