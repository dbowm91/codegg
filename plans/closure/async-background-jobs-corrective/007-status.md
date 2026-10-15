# Async Background Jobs Corrective C007 — Closure Status

Status: **conditionally closed**

Source implementation plan:

- `plans/implementation/async-background-jobs-corrective/007-mainline-pr119-hosted-qualification-and-merge-readiness.md`

Source subsystem roadmap:

- `plans/subsystems/async-background-jobs-integration-qualification-corrective-addendum.md`

Repository baseline reviewed: main base `f5e70435888a5c4d6e2541f25af5bbf4235ee65c`; exact qualified PR head `0ff0158c5b8d7fa69fe0072a68af7e2c8c5f4eab`.

Implementation commits or pull requests:

- PR [#119](https://github.com/dbowm91/codegg/pull/119) — remains draft and unmerged; current main was integrated and final hosted checks ran on the recorded head.
- `5b9f761` — reconciled the earlier C001 main repair; superseded by later latest-main integration.
- `2cb032a` — added file-backed v72-to-v78 upgrade/restart/idempotence/rollback coverage.
- `8951da6` — corrected C005 test provider setup and C001 provenance.
- `383f5c3` — integrated current main `f5e70435` while retaining unrelated registry work.
- `bc631de` — made the blocked-shell test use an explicit hard-deny fixture.
- `71bcb75` — repaired the long-horizon migration fixture before rewinding its schema marker.
- `d053c97` — made migration v76 tolerate already-present columns while preserving other errors.
- `0ff0158` — removed an arbitrary transient suppression cap from the 50-way wake-race test; exact-one admission and convergence remain asserted.

## 1. Executive finding

C007 completed integration and hosted qualification of PR #119 against current main. The exact merged-tree candidate passed root CI with every workspace test executed, Desktop E2E, the Windows native-probe contract, and the Windows process-launcher workflow. Migration upgrade and rollback coverage is present. The PR is still draft and no independent review has been recorded. The previously proposed SBC-class daemon startup benchmark was expressly removed from scope by the project owner on 2026-10-15 (§13); its absence is not a finding or merge gate. The milestone remains **conditionally closed** pending independent review and any required approvals; no merge-ready or merge authorization is claimed.

Prerequisite closures remain intact: C006 [`006-status.md`](006-status.md) demonstrates model-facing `ci_watch` and scheduler-backed Bash lifecycles; tool-advisor C001 [`001-status.md`](../tool-advisor-m003-receipt-ci-corrective/001-status.md) preserves the historical M003 receipt while restoring hosted CI.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Reconcile PR with current main and preserve unrelated work | PR base `f5e70435888a5c4d6e2541f25af5bbf4235ee65c`; integration commit `383f5c3`; PR API confirms same base and qualified head | pass | Registry conflict retained async and eggsearch rows. |
| Preserve C006 and independent receipt C001 prerequisites | C006 and tool-advisor C001 closure records; C001 fix merged on main | pass | Historical M003 negative receipt and D disposition were not weakened. |
| Verify schema upgrade v72 through v78, restart/idempotence, and rollback | `v72_to_latest_upgrade_survives_restart_and_rolls_back_failed_v78`; legacy remigration fixture test | pass | Hosted root CI also executed the full workspace suite. |
| Verify authenticated idle continuation, event ownership, and scheduler authority | C006 7/7 lifecycle suite; C004/C005/C001/M004 compatibility suites 55/55; callsite, scheduler-bypass, execution-ownership guards | pass | Detailed lifecycle evidence remains in C006 closure. |
| Root CI / verify on exact integration head | [Run 38735365495](https://github.com/dbowm91/codegg/actions/runs/38735365495) | pass | 13,791/13,791 tests passed; 6 configured skips. Formatting, Clippy and guards passed. |
| Desktop build and user trajectory on same head | [Run 38735365185](https://github.com/dbowm91/codegg/actions/runs/38735365185) | pass | Production boundary, renderer, host and built-app WebDriver steps passed. |
| Applicable Windows native probe evidence | [Run 38735363733](https://github.com/dbowm91/codegg/actions/runs/38735363733) | pass | C019 actual-child matrix and timeout/cancellation/descendant coverage passed. |
| Windows process launcher | [Run 38735365742](https://github.com/dbowm91/codegg/actions/runs/38735365742) | pass with skips | Launcher and listed ACL/AppContainer checks passed; synthetic-root feasibility, C044 differential, and c014 negative reproduction were skipped. No `Enforced` claim. |
| Review and branch-protection state | PR API on 2026-10-15: draft, CLEAN, no reviews, no review requests | partial | Independent review and any approval requirement remain outstanding. |
| SBC-class startup benchmark (withdrawn requirement) | No SBC benchmark was run. Original closure treated it as required, before 2026-10-15 owner correction. | **not applicable — out of scope** | No Raspberry Pi/SBC build, startup benchmark, release binary, or distribution setup is required for C007 or PR #119. See §13. |

## 3. Production implementation evidence

This milestone integrates the already-implemented C006 lifecycle and tool-advisor C001 repair with current main. The production idle-resume implementation remains daemon-owned, authenticated, scheduler-backed and lease/receipt fenced. This milestone added migration regression evidence and corrected qualification fixtures exposed by exact-head hosted CI. It did not add a new execution authority or broaden user-facing permissions.

The latest-main integration preserved unrelated main work during registry resolution. PR #119 reports `mergeStateStatus: CLEAN`, is draft, and is unmerged. All four hosted runs qualify head `0ff0158c5b8d7fa69fe0072a68af7e2c8c5f4eab`.

## 4. Verification executed

### Commands and hosted runs

Local evidence on the candidate includes:

```bash
cargo fmt --all -- --check
cargo test -p codegg-core --lib migration_upgrade_tests --locked
cargo test --test long_horizon_trajectory_qualification legacy_session_without_workplan_uses_legacy_behavior
cargo test --test work_orders_m001_foundation daemon_remigration_is_additive_and_empty_by_default
cargo test --test async_background_c005_idle_resume
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 scripts/check_idle_continuation_callsites.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
```

The focused migration, remigration and C005 suites passed; the long-horizon qualification binary passed 27/27; the C005 suite passed 16/16 with provider access disabled in the test environment. The earlier merged candidate passed `verify.sh quick`; final exact-head hosted CI ran formatting, Clippy, all configured guards and workspace tests on the final code head.

Hosted exact-head evidence:

- [CI / verify run 38735365495](https://github.com/dbowm91/codegg/actions/runs/38735365495): 13,791 tests passed, 6 configured skips; Clippy, formatting and guards passed.
- [Desktop E2E run 38735365185](https://github.com/dbowm91/codegg/actions/runs/38735365185): built-app WebDriver trajectory passed.
- [Windows process-launcher run 38735365742](https://github.com/dbowm91/codegg/actions/runs/38735365742): workflow succeeded with the skipped scopes listed above.
- [Windows native-probe run 38735363733](https://github.com/dbowm91/codegg/actions/runs/38735363733): native probe matrix and restricted-child timeout/cancellation/descendant tests passed.

All hosted jobs report the same exact head SHA. Main had not advanced beyond `f5e70435` at final fetch. No hosted result from an earlier candidate is counted as final qualification.

## 5. Invariant review

- One daemon and one JobScheduler remain the production execution authorities; C007 introduced no alternate executor.
- Authenticated host grants remain required for idle continuation; model-facing intent does not create consent.
- The continuation admission receipt/controller lease continues to bound admission; uncertain provider-side admission is not blindly replayed.
- Default background Bash behavior remains notify-only; no default inference or token spend was introduced.
- Event notices remain owner/session scoped, and raw workflow content is not promoted to trusted instructions.
- No forced push to main, unreviewed main mutation, or PR merge occurred.
- Windows workflow success is scoped to its named tests and does not establish general Windows sandbox `Enforced` behavior.

## 6. Failure and recovery review

The exact-head gate exposed and corrected four test/evidence issues before final success: an unsafe shell test expectation was replaced with an explicit blocked-command fixture; a long-horizon fixture now removes the current `ci_watch` table before rewinding its migration marker; migration v76 now handles columns already present in test remigration paths; and the 50-way race no longer fails on an arbitrary transient-suppression count while retaining exact-one-dispatch and convergence assertions. Each correction was pushed and the final hosted matrix passed on the resulting head.

The v72-to-v78 file-backed test exercises restart/idempotence and failed-v78 rollback. Startup interruption and provider uncertainty semantics remain covered by existing suites. No real provider-side exactly-once guarantee is claimed.

## 7. Migration and compatibility review

The final candidate includes a real prior-version SQLite fixture at v72, advances it to v78, restarts to validate idempotence, and verifies rollback when the v78 migration fails after the v77 state. Legacy remigration coverage accounts for an already-present `ci_watch` table when test fixtures lower the marker. Opening a v78 database with v72 code is not a supported rollback strategy. Protocol and event compatibility remain covered by C004/C006 suites and workspace CI.

## 8. Security review

The integration retained authenticated grant creation/revocation/status, server-resolved identity and current capability checks, scheduler-owned execution, owner-only notice delivery, bounded provider admission and fail-closed ambiguity behavior. Static callsite, scheduler-bypass, execution-ownership and hosted security/authorization guards passed. Explicit shell-block test configuration prevents executing the dangerous command. No new credentials or production secrets were added.

The successful Windows jobs qualify only their named tests. The synthetic-root `WRITE_RESTRICTED` probe, C044 token-flag differential, and c014 negative-reproduction job were skipped; no broad Windows enforcement statement is supported.

## 9. Documentation and operations

- Refreshed PR #119 body with accepted ADR-0031/C005/C006/C001 history, current base/head, exact run links and explicit nonclaims; kept PR draft.
- Updated the implementation plan, integration addendum, and `plans/registry.md`.
- Updated final closure evidence here. Earlier C004, C005, C006 and tool-advisor C001 closure records remain unchanged.
- PR review was not requested because no reviewer was specified or authorized for outreach.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Independent review and branch-protection approval state are unresolved; PR has no reviews or review requests. | Merge-readiness is not established. | Arrange an authorized independent review, resolve objections, and recheck exact-head status. |
Owner scope correction: the former low-severity SBC startup item is **withdrawn**, not an unresolved risk or deferred gate. No hardware benchmark was run or is now required (see §13).
| Low | Windows synthetic-root feasibility, C044 differential, and c014 negative reproduction were skipped. | Those specific Windows scopes remain unqualified. | Do not claim these results; run later only if that platform evidence is required. |

No known high or critical finding remains from the completed integration qualification. There is no claim of live paid-provider behavior, public GitHub API service-level behavior, general Windows sandbox enforcement, or universal native macOS portability.

## 11. Roadmap disposition

**Milestone conditionally closed with named operational evidence outstanding.** Implementation, latest-main integration, migration regression, exact-head CI and Desktop E2E are complete. C007 is not declared merge-ready: independent security/concurrency review and applicable branch-protection approvals must be recorded first. SBC benchmarking is explicitly out of scope. PR #119 remains draft and unmerged; this closure does not authorize a merge.

## 12. Registry updates and unblock audit

- Mark C007 conditionally closed and link this record from the integration addendum and registry.
- Preserve the historical C006 closure's statement that C007 was blocked on tool-advisor C001 at the time C006 closed; C001 later closed and C007 advanced.
- No registered downstream async plan depends on C007. No plan became newly eligible or unblocked; `Milestones eligible for fresh planning` remains empty. Existing unrelated blocked rows are unchanged.

## 13. Owner-directed scope correction — SBC benchmark withdrawn (2026-10-15)

This is a prospective scope correction to the C007 merge-readiness checklist, **not** evidence that an SBC benchmark was performed. The original closure recorded a missing SBC-class daemon startup benchmark as a low-severity finding and required it before merge readiness. The project owner explicitly withdrew that requirement: Codegg is not being benchmarked primarily on SBC hardware, native Raspberry Pi compilation could take disproportionate time or run out of memory, and producing precompiled SBC binaries/release architecture is not justified for this feature. **No SBC build, timing run, hardware qualification, release artifact, or associated plan is required for PR #119**, and no failure or postponed work should be inferred from its absence. No production code, test evidence, or historical workflow result changed under this correction. C007 remains conditionally closed **solely** pending independent security/concurrency code review and any applicable branch-protection approvals; maintainers retain the merge decision.
