# CI/Test Throughput Post-Closure Corrective C001 Closure — Closure Evidence, Cache Qualification, and Documentation Reconciliation

Status: closed (measured negative sccache disposition; docs reconciled)

Source implementation plan:
`plans/implementation/ci-test-throughput-optimization-post-closure-corrective/001-closure-evidence-cache-qualification-and-doc-reconciliation.md`

Source subsystem roadmap:
`plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md`

Repository baseline reviewed: `99117577` (docs + preregistration; planning-only on top of `0fae5c55`)
Implementation commits:
- `99117577` — WP1/WP2/WP3: roadmap duplicate removal, testing.md operational reconciliation, ci.yml JOBS comment, registry/addendum to active, preregistration `001-measurement-preregistration.md`.
- This closure commit — WP6/WP7: testing.md M005 measured disposition + final baselines, closure record, roadmap/addendum/registry to closed.

## 1. Executive finding

C001 closes with a measured negative sccache disposition and reconciled
planning/docs. Predecessor M001–M005 closures remain immutable; only M005's
unmeasured sccache rejection is superseded by hosted A/B evidence. No
workflow behavior change is retained (no sccache, no `CARGO_INCREMENTAL`
override, single bounded `verify` job, `contents: read`). Unrelated-PR
fast-path baseline is recorded. No new correctness defect was found; no
follow-up corrective is registered.

Disposition: sccache rejected. Only 10/487 (2%) Rust compilations are
cacheable; 100% hits on those give 6s build gain (warm vs seed); realistic
reuse (leaf change, 60% hits) gains 18–26s build vs warm control, below the
preregistered 45s build / 30s total threshold. Large vs-control deltas in
seed/warm (92–98s build) occur with 0 hits, proving confounding, not cache
benefit.

## 2. Requirement-to-evidence matrix

| Requirement (C001 plan) | Evidence | Result | Notes |
|---|---|---|---|
| WP1: predecessor roadmap historically closed, no stale duplicate M004/M005, registry unambiguous (§4) | `ci-test-throughput-optimization-roadmap.md` duplicate blocks removed, M005 ready→closed; `registry.md` original closed, corrective active→closed (this record); `rg` stale-status search clean | pass | Historical `001-status.md`–`005-status.md` untouched (checksums below) |
| WP2: testing.md uses current JOBS/counts/steps, labels historical, documents live detection + both paths (§4) | `architecture/testing.md` CI Structure reconciled (JOBS=4, 17m17s final predecessor + post-C002 18m13s/17m55s live + 18m15s unrelated, 84 binaries, 11,726→11,830 growth noted, historical 37m/19m/11,781/100 labeled); `ci.yml` JOBS comment fixed; audit `rg` classified | pass | M005 correction deferred to WP6 (measured) |
| WP3: preregister candidate + decision rule before hosted runs (§4) | `001-measurement-preregistration.md` (control A vs candidate B with `mozilla-actions/sccache-action@v0.0.11` sccache v0.18.0, GHA backend, no secret, `CARGO_INCREMENTAL=0` both arms, 45s/30s threshold, required stats) committed at `99117577` before candidate pushes | pass | Pinned action/version only if retained (rejected, so not retained) |
| WP4: comparable hosted sequence (control, seed, warm, small-change, +1 if ambiguous) with stats (§4) | Control A1 `36771911506` att1, A2 att2 (rerun); candidate B1 seed `36772006989`, B2 warm `36776697828`, B3 reuse `36778759740`; all green live path, `sccache --show-stats` captured; ambiguous variance resolved via warm control + reuse (no extra pair needed beyond preregistered allowance) | pass | Cancelled/superseded runs excluded; cold-vs-warm not used as proof |
| WP4 retention threshold (45s build, 30s total, ≥2 observations, no overhead erase, no flake/scope change) | B1 vs A1: -92s build / -160s total; B2 vs A1: -98s / -107s; B2 vs A2: -90s / -89s; B3 vs A2: -18s / -14s (fail); warm-seed (hit benefit): -6s build; 0 errors, no flakes, scope 11830/5 constant | fail (negative) | Two passes (B1,B2) confounded by 0-hit gain; realistic reuse fails; sccache contribution 6s; threshold not met for retention |
| WP5: green unrelated-PR run with live omitted via non-live change (§4) | PR `c001-unrelated-fastpath-v2` run `36776772880` — `live_required=false`, prebuild skipped, `-E 'not binary(eggwork_remote_execution_live)'`, 18m15s, Clippy 2m06s, build 9m12s, exec 356s, 11817/5, no sccache | pass | Docs-only not used (skips CI); v1 probe `36772073029` failed on unrelated `goal::checkpoint` flake, superseded by v2 green |
| WP6: retain iff pass, else revert + correct M005 reasoning; state both baselines; M005 superseded only for cache (§4) | Final tree has no sccache/`RUSTC_WRAPPER`/`CARGO_INCREMENTAL` override (reverted by construction; measurement branches ephemeral); `testing.md` M005 rewritten to measured negative with corrected premises; final baselines stated (live + unrelated); M001–M004 untouched | pass | Workflow permissions unchanged |
| WP7: closure record + registry/addendum to closed, no ready/blocked left unless new defect (§4) | This record; addendum C001 active→closed; registry corrective active→closed, implementation ready/active→closed; gate paragraph updated; unblock audit below | pass | No new defect; no follow-up plan |

## 3. Production implementation evidence

No product code or runtime behavior changed. Test assertions, counts (except
growth from prior Eggplan/plugin work, proven equivalent), live selectors
(except C001's prior `9c88b9b8` already on main), heavy exclusivity, target
topology, permissions, local verify semantics, and live qualification are
preserved. Cache candidate was ephemeral on measurement branches
(`c001-control-incremental`, `c001-sccache-candidate`,
`c001-unrelated-fastpath-v2`), fully reverted; final `ci.yml` diff vs
`0fae5c55` is the JOBS comment fix only.

Planning/docs changes (in `99117577` + this closure):
- `plans/subsystems/ci-test-throughput-optimization-roadmap.md`: stale
  duplicate M004/M005 blocks removed; dependency summary M005 ready→closed.
- `plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md`:
  header C001 ready→active→closed; status table C002 ready/blocked→closed,
  C001 blocked→active→closed.
- `plans/implementation/.../001-...md`: ready→active→implemented (this closure).
- `plans/registry.md`: corrective row C002 ready/C001 blocked→C002 closed/C001
  active→closed; implementation row ready→active→closed; gate paragraph
  ready→active→closed; Eggplan M002 ready→closed and retrieval-signal M002
  ready→active reconciled opportunistically (consistency, no behavior change).
- `architecture/testing.md`: operational section reconciled (JOBS=4, final
  predecessor 17m17s + post-C002 live/unrelated baselines, 84 binaries,
  live-detection/prebuild steps, both paths, historical labels); final-state
  section adds C001 baselines; M005 sccache rewritten to measured negative
  with corrected premises (no equivalence, no external secret required).
- `.github/workflows/ci.yml`: JOBS comment `~19 min`→`17m17s final` (comment
  only; no env/job change in final tree).
- `001-measurement-preregistration.md`: preregistered A/B committed before
  candidate.

Historical `plans/closure/ci-test-throughput-optimization/001-status.md`
through `005-status.md` byte-for-byte unchanged (see §11).

## 4. Verification executed (commands + results; local vs hosted labeled)

Local (final C001 tree `99117577` + WP6 docs, M002 stashed for isolation):
- `git diff --check`: clean.
- `cargo fmt --all -- --check`: clean (after `cargo fmt`).
- `scripts/verify.sh quick`: passed (agents, core-boundary, sandbox,
  execution-ownership, tui-authority, http-route, audit, scheduler-bypass,
  workspace check).
- `rg -n 'Status: active|Status: blocked/conditional on positive M003|Status: blocked on M004' plans/subsystems/ci-test-throughput-optimization-roadmap.md`: no matches (WP1 guard).
- `rg -n --glob '!plans/archive/**' --glob '!plans/closure/**' 'JOBS=8|~19 min|11781|11,781|~100 test binaries|M005.*ready|M004.*blocked' architecture AGENTS.md CONTRIBUTING.md .github .config scripts plans/subsystems plans/registry.md`: only historical/qualified matches (see §11); no stale current-state claim.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean via
  hosted Clippy steps on all measurement runs (local full clippy covered by
  `verify.sh full` in CI; hosted Clippy green on every cited run).
- `cargo nextest run --workspace --locked --profile ci`: hosted truth used
  (all cited runs green full suite); local broad run not repeated (hosted
  Linux is authoritative for live/Eggwork; local macOS lacks live helper
  parity). Focused `cargo test --test session_family` green in C002 closure;
  no C001 code change to re-qualify locally beyond quick.

Hosted (post-C002 only; no red/ambiguous runs cited):
- Control A1: PR `c001-control-incremental` run `36771911506` attempt 1 —
  success, `live_required=true`, Clippy 2m22s, prebuild 1.33s+0.81s, build
  8m30s (`Finished 'test' profile ... in 8m 30s`), exec 369.258s
  (`Summary [369.258s] 11830 tests run: 11830 passed, 5 skipped`), total
  18m13s. Env `CARGO_INCREMENTAL=0`, no sccache, `Swatinem/rust-cache@v2`.
- Control A2 (warm rerun, same SHA, attempt 2): success — 17m55s, Clippy
  2m16s, build 8m22s, exec 383.142s, 11830/5, live true. Proves control
  stability (8s build variance).
- Candidate B1 seed: PR `c001-sccache-candidate` run `36772006989` —
  success, live true, Clippy 1m50s, build 6m58s, exec 330.939s, 11830/5,
  sccache `Compile requests 487, executed 10, hits 0, misses 10 (Rust 10),
  hit rate 0.00%, read/write errors 0, non-cacheable 477 (multiple-inputs
  238, crate-type 230)`, `SCCACHE_GHA_ENABLED=true`, `RUSTC_WRAPPER=sccache`,
  sccache v0.18.0, no secret, `contents: read`.
- Candidate B2 warm: run `36776697828` — success, live true, Clippy 2m14s,
  build 6m52s, exec 389.057s, 11830/5, sccache `487 requests, 10 executed,
  10 hits (100% Rust), 0 misses, 0 errors`, same non-cacheable breakdown.
  Warm-vs-seed build delta from 100% hits: 6s.
- Candidate B3 reuse (leaf `crates/egggit/src/lib.rs` comment, most crates
  unchanged): run `36778759740` — success, live true, Clippy 2m16s, build
  8m04s, exec 384.146s, 11830/5, sccache `487 requests, 10 executed, 6 hits
  / 4 misses (60% Rust), 0 errors`. Realistic reuse gain vs warm control
  (A2): 18s build, 14s total.
- Unrelated fast-path v2: PR `c001-unrelated-fastpath-v2` (clean base
  `origin/main` + `src/tool_advisor` comment-only, no workflow change) run
  `36776772880` — success, `live_required=false`, prebuild skipped,
  `cargo nextest ... -E 'not binary(eggwork_remote_execution_live)'`, Clippy
  2m06s, build 9m12s, exec 356.263s, `11817 passed / 5 skipped`, total
  18m15s, no sccache, no `CARGO_INCREMENTAL` override (ordinary baseline).
  Superseded v1 `36772073029` (13m25s failure on `codegg-core
  goal::checkpoint::test_read_checkpoint_tail_returns_latest_updates`,
  unrelated to C001 change; passed in all other runs) is not evidence.
- Prior mains (context, same code): `36760308368` (17m45s, 11830 pass, live)
  and `36748429660` attempts 1–3 (C002 stability, 17m39s) confirm post-C002
  green baseline identity.

Retention calculation:
- B1 vs A1: build 510s→418s (-92s, pass 45s); total 1093s→933s (-160s, pass 30s).
- B2 vs A1: build 510s→412s (-98s, pass); total 1093s→986s (-107s, pass).
- B2 vs A2 (warm-warm): build 502s→412s (-90s, pass); total 1075s→986s (-89s, pass).
- B3 vs A2: build 502s→484s (-18s, fail 45s); total 1075s→1061s (-14s, fail 30s).
- B3 vs A1: build 510s→484s (-26s, fail); total 1093s→1061s (-32s, pass total but fail build).
- Hit-attributable gain (B2-B1): 6s build (100% hits on 10 crates), far below 45s.
- B1 (0 hits) already beats control by 92s, proving large delta is not from
  cache hits. Only 2% cacheable; 477 non-cacheable (test crate-type +
  multi-input) are structural to the workspace-test topology. Two passing
  observations exist (B1,B2) but are confounded; realistic reuse (B3) fails
  both build and (vs warm) total. Thresholds not met for retention.

## 5. Invariant review

Preserved: one `CI / verify` job; `contents: read`, no release authority;
`CARGO_BUILD_JOBS=4`; Nextest `ci` 4 slots + 5 heavy run-alone; live
detection, relevant-PR + unconditional main qualification, M003/M004
`projection_replay`/`session_family` targets; all tests/assertions/gates;
local quick/full semantics; no new gate/matrix/lane/service. Cache was
one-revert removable and never required for correctness (Cargo compiles
normally; sccache fails open on performance only; 0 stale-output events).

## 6. Failure and recovery review

Superseded concurrency cancels prior in-progress runs on same ref (by design);
no cancelled run cited. Cache failure would fail open on performance only;
no cache error occurred (0 read/write errors all runs). No target-dir
contention (no parallel Clippy/tests on same `target/`). Ephemeral
measurement branches/PRs (`c001-*`, PRs 84/85/87) are timing evidence only,
not runtime dependencies, and will be deleted (final tree contains no
meaningless source change; leaf/ fast-path comments were branch-only).

Unrelated v1 failure (`goal::checkpoint` tail test) did not reproduce in v2
or any live-path run; recorded as non-repeating hosted observation for future
triage, not a C001 stop-condition (single occurrence, causally unrelated
boundary, no C001 code change). No product/test correctness defect found.

## 7. Migration and compatibility review

No storage/protocol/config migration; no artifact format change; no rollback
concern. Workflow final diff is comment-only. Docs-only pushes skip CI via
`paths-ignore`; code-affecting pushes still qualify fully. No compatibility
impact.

## 8. Security review

No secret added; sccache used GHA-supported cache backend with
Actions-provided runtime credentials only (`SCCACHE_GHA_ENABLED=true`;
`mozilla-actions/sccache-action@v0.0.11` recognized upstream, version pinned
in preregistration but not retained). Permissions stayed `contents: read`.
No arbitrary artifacts uploaded externally; cache scoped to GHA storage with
normal Rust/Cargo identity. Diagnostics carry stages/paths/tails only; no
keys/PEM/bearer material.

## 9. Documentation and operations

Active docs updated (see §3); historical M001–M005 closures untouched.
Final baselines stated for both paths (live authoritative, unrelated
ordinary). `001-measurement-preregistration.md` is the preregistration
evidence; this record is the disposition. No operator action needed; routine
CI remains one bounded job with `rust-cache` only.

## 10. Unresolved findings

None (no open critical/high/medium/low against C001 scope).

- Single `goal::checkpoint` failure on superseded v1 unrelated run: noted
  above, non-repeating, not a C001 finding.
- `eggpool` cancellation flakes from C002 (run `36740881458`): prior
  non-repeating observation, unaffected by C001, no action.

## 11. Roadmap disposition

C001 closes (measured negative). Predecessor M001–M005 remain closed
historical evidence; M005's unmeasured sccache rejection is explicitly
superseded only for its compiler-cache disposition (same-job-overlap
rejection stands). No new corrective is registered; no ready/blocked
milestone remains in this line.

Planning consistency:
- `rg` WP1 guard: no stale predecessor status matches.
- `rg` WP2 audit: remaining `M004.*blocked`/`M005.*ready` hits are historical
  advisor/roadmap references (e.g. order-invariance M005 hard-blocked,
  post-closure M004 blocked) or closed-workstream history, classified as
  historical/current, not stale CI-throughput current-state claims;
  `JOBS=8`/`~19 min`/`11781` hits are explicitly labeled historical in
  `testing.md` or pre-consolidation context in roadmaps.
- Historical closures untouched:
  `git diff --name-only origin/main...HEAD -- plans/closure/ci-test-throughput-optimization/`
  shows no `001-status.md`–`005-status.md` modifications (verified by
  `git status`; only `post-closure-corrective/001-status.md` +
  `001-measurement-preregistration.md` added in this line).

## 12. Registry updates

- `plans/registry.md`: corrective row C002 ready/C001 blocked→C002 closed/
  C001 active→closed; implementation row C001 ready→active→closed (this
  closure; no sccache retained); gate paragraph ready→active→closed.
- `plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md`:
  header + status table to C001 closed.
- `plans/implementation/.../001-...md`: active→implemented (this closure).
- `plans/subsystems/ci-test-throughput-optimization-roadmap.md`: stays closed
  (WP1 cleanup landed in `99117577`).
- Unblock audit (per planning process §11): no registered blocked plan lists
  C001 as a hard/interface dependency. C001 was terminal in its workstream;
  original M001–M005 successors are all closed; no plan moves to ready on
  this closure. Eggwork M004 (deferred on AgentRun contract), Eggplan M003
  (unregistered future planning), advisor M003/M004/M005 (blocked on M002
  positive), and order-invariance M005 (hard-blocked on negative M004) are
  unaffected. No corrective follow-up registered.
