# C001 Closure — Closure Evidence, Cache Qualification, and Documentation Reconciliation

Status: conditionally closed

Implementation plan: `plans/implementation/ci-test-throughput-optimization-post-closure-corrective/001-closure-evidence-cache-qualification-and-doc-reconciliation.md`

Repository: CodeGG CI/test throughput optimization

## 1. Closure finding

C001 reconciled the predecessor roadmap, registry, and testing documentation; corrected the unsupported reasoning behind M005's compiler-cache disposition; measured the `sccache` candidate under the registered decision rule; and captured a green hosted unrelated-PR fast-path run. The candidate did not meet the required build-time gain, so all sccache workflow configuration was removed. M001-M005 remain closed, with only M005's cache disposition superseded by this measured result. Same-job overlap remains rejected.

The plan is conditionally closed because the requested local verification exposed a repeatable `scheduler_cancellation` test failure and local Clippy findings while the hosted stable-toolchain suite and Clippy were green. These are outside C001's allowed product/test scope and need independent triage. The hosted live suite also had helper-readiness timeouts on two non-qualifying attempts; later full live runs passed. No implementation or test changes were made to mask those results.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Reconcile predecessor status without rewriting immutable closures | Roadmap, registry and addendum reconciled; `plans/closure/ci-test-throughput-optimization/001-status.md` through `005-status.md` unchanged | Complete |
| Correct active CI documentation and sequence | `architecture/testing.md` now documents both hosted paths, settings, test totals, and workflow order | Complete |
| Preregister cache comparison | `plans/closure/ci-test-throughput-optimization-post-closure-corrective/001-measurement-preregistration.md`, committed before the candidate | Complete |
| Measure comparable control and candidate runs, including cache statistics | Control run `36211049535`; candidate runs `36212091132`, `36217114209`, and `36218047329` | Complete; negative decision |
| Measure a green unrelated-PR fast path | PR #83, run `36218924520`, head `9df35f13c31f682cdd4d82cbb9a96709e5d6e010` | Complete; live target omitted |
| Revert unqualified cache configuration | Final workflow tree has no sccache action, environment, wrapper, or stats step; `CARGO_INCREMENTAL=0` candidate-only setting removed | Complete |
| Close and audit dependencies | Registry and blocked-work audit updated; no CI-throughput dependent plan was ready or blocked on C001 | Complete; nothing newly unblocked |
| Required local broad verification | `scripts/verify.sh quick` passed; standalone Clippy and Nextest had the failures recorded below | Conditional |

## 3. Implementation and production effect

No product code, runtime behavior, test assertion, test count, feature gate, or test-target topology changed. No test was removed or edited. The temporary source comment and all candidate sccache settings were reverted before finalization.

The only final workflow text change replaces the stale intermediate `~19 min` description with the final 17m17s main/live measurement. A separate correctness defect was found in the live-change detector's three-dot diff against a shallow synthetic PR checkout. It was fixed in selector-only PR #82 and merged as `9c88b9b80852022b39daa80a6b9fb7797fcaa824`; this minimal correction uses a two-tree diff. The corrected detector returned `live_required=false` for the controlled unrelated change and `live_required=true` for workflow-file changes.

## 4. Verification and hosted measurements

### Repository-local commands

- `rtk scripts/verify.sh quick` — passed.
- `rtk cargo fmt --all -- --check` — passed through quick verification.
- `rtk cargo clippy --workspace --all-targets --locked -- -D warnings` — failed on four findings under installed Rust 1.89: `nonminimal_bool` in `crates/egglsp/src/cache.rs`, `ptr_arg` in `crates/egglsp/src/evidence_collector.rs`, `collapsible_else_if` in `crates/codegg-providers/src/sse_parser.rs`, and `overly_complex_bool_expr` in `crates/codegg-providers/src/openai_compatible.rs`.
- `rtk cargo nextest run --workspace --locked --profile ci` — failed at 6,870/11,707 after 6,869 passed / 1 failed / 1 skipped. `codegg::scheduler_cancellation::cancel_running_job_terminates_process_and_releases_permit` failed because the executor did not observe the cancellation token.
- The isolated rerun `rtk cargo nextest run -p codegg --test scheduler_cancellation --locked --profile ci -E 'test(cancel_running_job_terminates_process_and_releases_permit)'` reproduced that test failure.
- `rtk cargo test --workspace --locked --no-run` — passed.

The four Clippy findings and the scheduler-cancellation assertion are outside C001's scope. The final hosted stable-toolchain checks for Clippy and the complete workspace suite passed. The repository remains conditionally closed pending separate resolution/triage of these local verification failures; this record does not claim the local broad checks passed.

### Hosted run evidence

| Run | Role and path | Clippy | Test build | Nextest | Tests | Total job |
|---|---|---:|---:|---:|---|---:|
| `36211049535` attempt 1 | Matched no-sccache control, main/relevant live path, `CARGO_INCREMENTAL=0` | 2m25s | 8m29s | Control baseline | 11,726 passed / 1 skipped | ~18m12s |
| `36212091132` attempt 1 | Candidate seed/cold run; attempt 2 failed and is not accepted as a full run | 2m12s | 8m30s | Green on attempt 1 | 11,726 passed / 1 skipped | ~17m14s |
| `36212091132` attempt 2 | Candidate warm run | — | 6m54s to failure | Stopped at 5,608 run; helper readiness timeout | 5,607 passed / 1 failed / 1 skipped; 6,118 not run | Not comparable |
| `36217114209` attempt 1 | Candidate after a Rust source change; workflow diff required live path | 2m19s | 8m20s | 366.400s | 11,726 passed / 1 skipped | 17m40s |
| `36218047329` attempt 1 | Candidate warm/source-change observation; workflow diff required live path | 2m18s | 7m53s | 365.635s | 11,726 passed / 1 skipped | 17m13s |
| `36218924520` attempt 1 | PR #83 unrelated-PR fast path; detector omitted live target and fixture prebuild | 1m48s | 6m43s | 309.068s | 11,714 passed / 1 skipped | 14m34s |

Candidate `sccache --show-stats` observations had 485 requests, 475 non-cacheable calls, 10 executed/cacheable requests, zero cache read/write/errors. The seed had 0 hits / 10 misses; the warm failed attempt had 10 hits / 0 misses but did not complete the suite; each full source-change observation had 6 hits / 4 misses. Cache size was unavailable from the GHA backend. Non-cacheable reasons were primarily multiple input files (237) and crate type (229); cacheable-call hits did not translate into a qualifying full build reduction.

Against the 8m29s control build, the two full source-change observations saved 9s and 36s, respectively. Neither reached the preregistered 45s minimum, and therefore the requirement for two qualifying observations failed. Total job reductions were approximately 32s and 59s, but that does not override the failed build threshold. The candidate is rejected and fully reverted. The candidate warm failure also means the no-new-flake gate was not established.

The unrelated-PR hosted run's detector output was `live_required=false` for one changed Rust source file. Fixture prebuild was skipped, and the resulting 11,714 tests reflect omission of the single live Eggwork binary. Main and relevant PR changes continue to require live qualification.

The corrected selector PR #82 had a green full live run on attempt 2, `36213918571`. Main run `36215541015` attempt 1 failed in `session_family::control_m004_controller_lease::reaper_releases_on_turn_completed_event`; attempt 2 failed when `live_blob_upload_and_workspace_materialization` timed out after 60 seconds waiting for its helper. Candidate runs `36217114209` and `36218047329` later passed the same live suite. These failures are recorded; no test harness changes were made in this plan.

## 5. Invariant review

- One bounded `verify` job remains; permissions, release authority and local quick/full command semantics are unchanged.
- Hosted `CARGO_BUILD_JOBS=4`, Nextest four-slot policy, heavy-binary exclusivity and consolidated targets are unchanged.
- No test target, test assertion, suite feature scope, supported platform boundary, or main live-qualification requirement changed.
- The unrelated PR omits only `eggwork_remote_execution_live`; the measured result was 11,714 tests vs 11,726 on live paths.
- No candidate cache is required for correctness.

## 6. Failure, recovery and unresolved conditions

- An unrelated comment-only PR exposed detector fail-open behavior because a shallow synthetic checkout lacked the merge base required by the three-dot diff. PR #82 fixed the selector with a two-tree comparison and merged as `9c88b9b`; the corrected fast-path and relevant-path detector behavior was observed in hosted runs.
- The candidate warm run and main retry had helper-readiness timeouts. Subsequent candidate full live runs and the selector-fix PR retry passed; this corrective did not widen into live-fixture test changes.
- The candidate failed its preregistered throughput threshold and was removed. The successful candidate runs do not justify retaining a cache that saves less than 45 seconds of build time.
- Local `scheduler_cancellation` failed both in the workspace run and in isolation. Local Rust 1.89 Clippy found four diagnostics while hosted stable Clippy passed. These findings require independent triage; C001 did not edit source or test behavior.
- Recovery is the normal `Swatinem/rust-cache` workflow with no `RUSTC_WRAPPER`, sccache action, GHA sccache environment, or incremental override.

## 7. Compatibility

The final workflow retains the M001-M005 settings and the baseline toolchain behavior. `CARGO_INCREMENTAL=0` existed only in the controlled cache comparison and was removed with the candidate. No cache service, external secret, new dependency, or persistent benchmark lane remains.

## 8. Security and authority

The experiment used GitHub Actions-provided runtime cache credentials and added no user-managed secret. The workflow continues to use read-only repository permissions and has no release or publication authority. No secret value was emitted in logs.

## 9. Documentation and operations

- `architecture/testing.md` records the main/live and unrelated-PR hosted baselines, selector/prebuild order, measured cache rejection and same-job-overlap disposition.
- The predecessor roadmap is historically closed; duplicate stale M004/M005 blocked fragments are removed.
- The C001 addendum, implementation plan and registry reflect conditional closure. Predecessor closure records 001-005 remain byte-for-byte unchanged.
- The C001 text audit of `JOBS=8|~19 min|11781|11,781|~100 test binaries|M005.*ready|M004.*blocked` found only explicit history or unrelated workstreams after reconciliation. In `architecture/testing.md`, 11,781/11,781-test values and roughly 100 root binaries are labeled historical or M001 baseline; current totals are 11,726 live-path tests, 11,714 unrelated-PR tests, 84 root integration binaries and 224 workspace binaries. The original CI roadmap labels pre-M003/M004 counts historical and records current 84/224 totals. The addendum's old-state findings are explicitly marked “At discovery.” CI registry M003/M004 rows are closed; remaining matching blocked/ready M004/M005 entries belong to other subsystems. `AGENTS.md`, `CONTRIBUTING.md`, `.github`, `.config` and `scripts` contain no active stale CI-throughput setting among those search terms.
- The candidate PR #80 is the implementation/evidence review surface; temporary fast-path PR #83 is retained only as run evidence and is closed after this measurement.

## 10. Findings

| Severity | Finding | Disposition |
|---|---|---|
| Medium | Local scheduler-cancellation test consistently fails to observe its token, including isolated rerun | Outside C001 scope; requires separate test/scheduler triage |
| Medium | Four local Rust 1.89 Clippy findings conflict with hosted stable Clippy success | Outside C001 scope; requires toolchain/source baseline triage |
| Low | Hosted live fixture helper readiness timed out in a candidate warm attempt and a main retry | Later full live runs passed; report and monitor; no C001 harness change |
| Informational | sccache produced hits, but did not meet the measured build-time threshold | Rejected and reverted |

## 11. Roadmap and dependency disposition

M001-M004 implementation and topology conclusions remain accepted. M005's same-job-overlap rejection remains accepted; its unmeasured cache rejection is superseded by this measured negative disposition. The original CI/test throughput roadmap remains closed. C001 is conditionally closed because local broad verification has unresolved failures, not because the cache or CI path is left implemented.

The dependency-ready, active and blocked plan tables were audited. No registered future plan depends on this corrective, and no CI/test-throughput plan is waiting on C001. **Nothing is newly unblocked.** No successor plan is registered from these measurements; the local verification failures should be triaged under their owning scheduler/toolchain scope.

## 12. Registry updates

`plans/registry.md` marks the post-closure corrective C001 conditionally closed, moves it out of active closure work, and points the recently closed/conditionally closed table at this record. The original workstream stays closed, and no ready/blocked CI-throughput handoff remains. The addendum and implementation plan carry the same conditional status.
