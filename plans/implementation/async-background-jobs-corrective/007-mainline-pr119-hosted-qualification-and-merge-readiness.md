# Async Background Jobs Corrective C007 — Mainline Reconciliation, PR #119 Qualification and Merge-Readiness Decision

Status: **conditionally closed** — integration and exact-head qualification are complete. Independent review and an SBC-class startup benchmark remain outstanding, so merge-ready status is deferred. No merge is authorized by this plan.
Planning baseline: PR #119 `247edec5f37851ca3f67db5943675c896d19e9d9`; final reviewed `main` base `f5e70435888a5c4d6e2541f25af5bbf4235ee65c`; exact qualified PR head `0ff0158c5b8d7fa69fe0072a68af7e2c8c5f4eab`.
Integration addendum (conditionally closed): `plans/subsystems/async-background-jobs-integration-qualification-corrective-addendum.md`.
Historical closure: C005 `plans/closure/async-background-jobs-corrective/005-status.md`, accepted ADR-0031, C004 delivery closure and M001–M004/C001–C003 closures retained unchanged.

## 1. Integration question and present facts

PR [#119](https://github.com/dbowm91/codegg/pull/119) is **open, draft and unmerged**. The final exact-head check on 2026-10-15 reports base `f5e70435888a5c4d6e2541f25af5bbf4235ee65c`, head `0ff0158c5b8d7fa69fe0072a68af7e2c8c5f4eab`, and `mergeStateStatus: CLEAN`. There are no reviews or review requests. Its body was refreshed with current scope and evidence; draft status is preserved. Mergeability is not approval or merge permission.

**Final current-head hosted evidence** (all jobs tested exact head `0ff0158c5b8d7fa69fe0072a68af7e2c8c5f4eab`, base `f5e70435888a5c4d6e2541f25af5bbf4235ee65c`):

| Check | Workflow run | Current result | Interpretation |
|---|---|---|---|
| CI / verify | [38735365495](https://github.com/dbowm91/codegg/actions/runs/38735365495) | success | Formatting, Clippy, guards and all **13,791/13,791** workspace tests passed; 6 configured tests skipped |
| Desktop E2E | [38735365185](https://github.com/dbowm91/codegg/actions/runs/38735365185) | success | Production boundary, renderer, host and built-app WebDriver trajectory passed |
| Windows process launcher | [38735365742](https://github.com/dbowm91/codegg/actions/runs/38735365742) | success, with scoped skips | Launcher/AppContainer/ACL and failure-injection checks passed. Synthetic-root `WRITE_RESTRICTED` feasibility, C044 token-flag differential and `c014-negative-reproduction` were skipped; no Windows `Enforced` claim follows. |
| Windows native probe contract | [38735363733](https://github.com/dbowm91/codegg/actions/runs/38735363733) | success | C019 actual-child probe matrix and restricted-child timeout/cancellation/descendant coverage passed |

The original receipt mismatch was corrected under `plans/implementation/tool-advisor-m003-receipt-ci-corrective/001-receipt-identity-reproduction-and-hosted-gate-restoration.md` without weakening the historical M003 negative result. Final root CI is green on the integrated head. Earlier exact-head failures in test fixtures and the race stress test were corrected before this final evidence.

The async component's model-facing tool lifecycle is covered by C006 and its closure. C007 integrated the qualification and bounded corrections before final CI. Review and SBC startup evidence remain conditions for a merge-readiness decision.

## 2. Strict dependency graph and ownership

```text
C005 implementation + C004 delivery + accepted ADR-0031 [historically closed]
       |
       +-- C006 model-facing tool -> terminal -> idle real turn  [ready]
       |          |
       |          +----\
       |                +--> C007 PR #119 integration / hosted gate [BLOCKED]
       |          +----/
       +-- Tool-advisor M003 receipt C001 (independent) [ready]
```

C006 work changes only async-jobs/runtime/tests; tool-advisor C001 changes only its owner subsystem and should land independently on main as appropriate; C007 is the **reconciliation/merge decision**, not a substitute for either. If the tool-advisor line receives a new distinct CI fix before C007, independently verify it meets the same gate before marking this prerequisite satisfied. No broad new architectural milestone authorized.

## 3. Ordered work packages

### WP-A — Baseline and controlled reconciliation

On implementation handoff, fetch latest `main`, PR #119 actual head SHA, accepted ADR-0031, C005 closure and the two successor closures. Verify C006's strict proof and tool-advisor C001 CI restoration with real artifacts. Compare `main...PR` merge base and changed files, identify overlapping ownership areas: `src/core/{daemon,daemon_construct,daemon_bootstrap,daemon_continuation,daemon_turns,daemon_shutdown}.rs`, `crates/codegg-{core,protocol}`, `src/agent/turn_runtime.rs`, `src/scheduler/*`, `src/tool/*`, `src/tui/app/mod.rs`, projection/auth/audit/approval/OS sandbox, schema v73–v78, `plans/registry.md`, workflow guards. Distinguish true merge conflict from merely being behind; `mergeable: true` on Oct 15 means conflict not then reported but is not contemporaneous with later main commits.

Completed: a dedicated integration commit merged latest main after C006 and the independent tool-advisor repair. Registry conflict resolution retained async history and unrelated eggsearch rows. No forced push to main or Cargo.lock regeneration occurred. The integration was checked for unrelated registry loss, local artifacts, secrets, new unsafe permissions, and broad unrelated refactors.

### WP-B — Authorization/compatibility/migration and diff audit

Audited accepted ADR-0031 delegated admission at the exact merge head: `CoreRequest::ContinuationGrantCreate/Revoke/Status` uses the canonical authenticated gate; no arbitrary model-issued grants; `submit_idle_continuation_turn` has only the two attested daemon callers (guard `scripts/check_idle_continuation_callsites.py`); server-resolved principal and current team capability, model/connection/sandbox and approval ceilings, no fabricated client transport, exact job/notice ownership, one controller lease/receipt, and one SQLite CAS are retained. The intentionally `pub` test API has no transport or plugin route. Ambiguous provider admissions remain actionable `NeedsAttention`; no default token spend is introduced.

Validated schema/migration **72 -> 78** on a file-backed prior-version fixture, restart/idempotence, and rollback after failed v78 migration. Legacy remigration coverage also exercises marker repair. Opening a v78 database with v72 code is not a supported rollback strategy. An SBC-class daemon startup benchmark was unavailable and remains an explicit condition. C001 watch bounds, permit ownership, event compatibility and origin-scoped hints are covered by existing guards and qualification suites.

### WP-C — CI/evidence gate and PR metadata

Run on **the exact integrated SHA** (complete; final hosted evidence is listed above):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
python3 scripts/check_idle_continuation_callsites.py
# Plus existing scheduler-bypass / execution-ownership / authorization-matrix
# / audit-coverage / projection and sandbox guards invoked by quick/CI.
cargo test --test async_background_c005_idle_resume
cargo test --test async_background_c004_live_attention
cargo test --test ci_watch_m004_completion
cargo test --test ci_watch_c001_idle_driver
cargo test --test async_background_c006_tool_to_idle_ci
cargo test --test async_background_c006_tool_to_job
```

Completed on exact head `0ff0158c5b8d7fa69fe0072a68af7e2c8c5f4eab`: hosted `CI / verify` executed and passed the full 13,791-test workspace suite with six configured skips; Desktop E2E, the Windows native-probe contract, and process-launcher workflow completed successfully with the explicit skips recorded above. No prior-head result is counted as final evidence. Exact run IDs are linked in the evidence table.

PR #119 title/body now identify accepted ADR-0031, historical C005 closure, C004 details, C006 lifecycle evidence, current mainline baseline, exact hosted run links, fail-closed guarantees and nonclaims. Preserve draft until independent review and startup evidence are resolved; do **not** merge or relabel as release-ready.

### WP-D — Review, sign-off and closure

Independent review was not requested because no reviewer was specified or authorized for outreach. PR review state is empty and branch-protection approval requirements remain unresolved. No auto-merge or merge action is authorized. C007 is conditionally closed for implementation/integration qualification, but is **not marked merge-ready**. A maintainer can decide merge readiness only after an independent review and SBC-class startup evidence are recorded and all checks remain green on the reviewed SHA.

Closure: `plans/closure/async-background-jobs-corrective/007-status.md` records exact base and tested head, integration commits, C006 and tool-advisor C001 closures, local/hosted matrix, PR #119 state, executed and unrun evidence, residuals and unblock audit. Earlier C004/C005 closure records remain unchanged.

## 4. Pass/fail acceptance (strict)

- **Blocked until:** C006 demonstrates non-synthetic tool-to-continuation E2E (or transparently reports an uncovered seam; a partial C006 is not enough), **and** tool-advisor receipt C001 has a substantive fix with a green root CI on the repaired integration SHA.
- **Pass merge qualification:** current-main integration clean, correct storage upgrade, async/security/authorization/ownership guards green, root CI/verify green, Desktop E2E green on the same SHA, no open high/critical issue, accurate PR description, explicit review state, and startup benchmark recorded. Review and startup evidence remain outstanding, so this criterion is not met.
- **Failure:** red or incomplete root CI, unreviewed integration divergence, manual coordinator/test-only injection masquerading as full trajectory, weakened frozen benchmark test, grant identity spoofing, notification cross-session leakage, or unqualified sandbox assertion. Keep draft; write corrective with new evidence. Never claim tests not run.
- **Scope:** qualified feature **merge candidate**, not end-user product release. A green merge still does not prove native Windows sandbox enforcement, live paid provider, genuine public GitHub API SLA, or universal macOS portability. Those need separate evidence if later required.

**No final merge action is authorized by this planning document.**

## 5. Final disposition (2026-10-15)

The exact-head implementation and hosted qualification gates are complete. PR #119 remains draft, unmerged and mergeable against current main. Closure is **conditional** because independent review and an SBC-class startup benchmark remain outstanding; these are external evidence requirements, not known correctness defects. No registered downstream async plan depends on C007, and no plan became newly eligible or unblocked.
