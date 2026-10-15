# Async Background Jobs Corrective C007 — Mainline Reconciliation, PR #119 Qualification and Merge-Readiness Decision

Status: **active** — C006 and tool-advisor C001 are closed; latest-main integration and exact-head qualification are in progress. No merge is authorized by this plan.
Planning baseline: PR #119 `247edec5f37851ca3f67db5943675c896d19e9d9`; latest `main` repair merge `9de8a5452dbf4070b15aa78801bb0e703eec2447`. Refresh before final hosted qualification.
Active addendum: `plans/subsystems/async-background-jobs-integration-qualification-corrective-addendum.md`.
Historical closure: C005 `plans/closure/async-background-jobs-corrective/005-status.md`, accepted ADR-0031, C004 delivery closure and M001–M004/C001–C003 closures retained unchanged.

## 1. Integration question and present facts

PR [#119](https://github.com/dbowm91/codegg/pull/119) is **open, draft and unmerged**. On 2026-10-15 its API returned `mergeable: true` (NOT permission to merge), head `247edec5` and base recorded as `f73afcec`. Comparing its branch with current `main` reports **44 ahead, 21 behind**; the next current-main reconciliation has not been exercised as a new exact-head merge candidate. PR title/body are stale: they still label ADR-0031 "proposed", grant/executor gated and C005 unclosed, while ADR-0031 is accepted and C005 implementation/closure landed. Do not assume old PR text reflects current implementation.

**Current-head hosted evidence** (explicit, noninterchangeable):

| Check | Workflow run | Current result | Interpretation |
|---|---|---|---|
| Desktop E2E | [38650792103](https://github.com/dbowm91/codegg/actions/runs/38650792103) | success | Desktop qualified on `247edec5`; does not imply root CI green |
| Windows process launcher | [38650792129](https://github.com/dbowm91/codegg/actions/runs/38650792129) | success | Specific launcher check only; not Windows sandbox `Enforced` |
| CI / verify | [38650792114](https://github.com/dbowm91/codegg/actions/runs/38650792114) | **failure** | Workspace test `tool_advisor::causal_frontier::tests::m003_checked_in_receipt_matches_recomputation`; 4,267 passed, 1 failed, 9,522 not executed because nextest fail-fast |

The red receipt test is owned by `plans/implementation/tool-advisor-m003-receipt-ci-corrective/001-receipt-identity-reproduction-and-hosted-gate-restoration.md` and must be fixed without weakening the historical M003 negative result. A red root CI is a **hard no-merge gate** even if one can prove the failing test is unrelated to async-jobs. C005's local 16/16, C004 delivery and Desktop E2E do not waive it.

The async component separately lacks a single complete `ci_watch` / background Bash-Test *model-facing tool* -> actual scheduler/watcher -> durable terminal -> live owner event -> authenticated idle turn trajectory; C006 owns that qualification. C007 must integrate its tests and any bounded necessary implementation correction before final CI.

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

Use a dedicated **integration commit/head** on the async implementation branch (or a reviewed integration PR) to merge/rebase latest main after C006 and independent tool-advisor repair; preserve commit authorship and unrelated main changes. No forced push to `main`, no silent conflicted-resolver dropping other workstreams' registry/ADRs/closure records. Regenerate Cargo.lock only if necessary and audit dependency changes. Confirm no accidental inclusion of local worktree artifacts, secrets, new unsafe permissions, or broad unrelated refactors.

### WP-B — Authorization/compatibility/migration and diff audit

Review accepted ADR-0031 delegated admission at the exact merge head: `CoreRequest::ContinuationGrantCreate/Revoke/Status` uses canonical authenticated gate; no arbitrary model-issued grants, `submit_idle_continuation_turn` has only the two attested daemon callers (guard `scripts/check_idle_continuation_callsites.py`), server-resolved principal and *current* team capability, model/connection/sandbox and approval ceilings, no fabricated client transport, job/notice exact ownership, one controller lease/receipt, one SQLite CAS. Confirm intentionally `pub` test API still has no transport or plugin route; if widened inadvertently, narrow/guard it. All ambiguous provider admissions stay actionable `NeedsAttention`; no default token spend.

Validate **schema/migration 72 -> 78** on a real prior-version SQLite fixture, idempotent second startup, interrupted upgrade, rollback behavior (do not instruct users to open a v78 database with v72 code as if compatible), and bounded daemon startup on SBC-class hardware. Verify C001 watch polls remain finite, no process permits held while waiting; `BackgroundJobCompleted` event type/version compatibility with TUI/desktop/remote replay and origin-scoped live hints.

### WP-C — CI/evidence gate and PR metadata

Before any "ready for review" transition, run on **the exact integrated SHA**:

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

Adapt filenames to C006's recorded actual artifacts and list exact invocations. Do not rely on a former branch's pass for this SHA. Capture hosted exact-head `CI / verify` with **all workspace tests actually executed and passing**, Desktop E2E (renderer/host/WebDriver), any applicable Windows/macOS compile/native checks and static guards. Record actual workflow ID, attempt number, head SHA, pass/fail/skip counts, plus tests unavailable. A rerun of the **same** unchanged SHA may be a flake diagnostic but an unexplained red test is not green acceptance. If CI fails due to a separate tool-advisor or infrastructure issue, register/route it to its owner, keep the PR draft and C007 blocked, do not skip check, mark success or disable enforcement.

Refresh PR #119 title/body after evidence: accepted ADR-0031, closed historical C005, C004 delivery details, C006 real tool-lifecycle evidence and names, C007 current gate status, current mainline baseline, exact hosted run links, negative/fail-closed guarantees and known nonclaims. Preserve draft until all gates; do **not** merge, mark ready, or relabel as release-ready merely because the doc says to prepare.

### WP-D — Review, sign-off and closure

Ask for a review of merge diff with priority to grants/authorization, scheduler/daemon ownership, frontend event isolation, storage migrations and native shell/sandbox compatibility. Reconfirm PR checks after review resolution; record branch-protection approval status. No auto-merge in this handoff plan. C007 can be marked **merge-ready for maintainer decision** only if all gates are green on an exact SHA and review objections resolved. The maintainer separately decides whether/when to merge; Codegg as a whole remains far from release.

Closure: `plans/closure/async-background-jobs-corrective/007-status.md` with precise merge base/latest main, integration commit(s), reviewed diff summary, C006 and tool-advisor C001 closure references, full local+hosted matrix, PR #119 updated status, tests run and not run, severity-ranked residuals and unblock audit. Do not retroactively revise C004/C005 closure records; update only the new integration addendum and registry.

## 4. Pass/fail acceptance (strict)

- **Blocked until:** C006 demonstrates non-synthetic tool-to-continuation E2E (or transparently reports an uncovered seam; a partial C006 is not enough), **and** tool-advisor receipt C001 has a substantive fix with a green root CI on the repaired integration SHA.
- **Pass merge qualification:** current-main integration clean (including active planning registry), correct storage upgrade, all async regression and security/authorization/ownership guards green, root CI/verify green with no unexplained skips, Desktop E2E green on same SHA, no open high/critical issue, PR description accurate, review state explicit.
- **Failure:** red or incomplete root CI, unreviewed integration divergence, manual coordinator/test-only injection masquerading as full trajectory, weakened frozen benchmark test, grant identity spoofing, notification cross-session leakage, or unqualified sandbox assertion. Keep draft; write corrective with new evidence. Never claim tests not run.
- **Scope:** qualified feature **merge candidate**, not end-user product release. A green merge still does not prove native Windows sandbox enforcement, live paid provider, genuine public GitHub API SLA, or universal macOS portability. Those need separate evidence if later required.

**No final merge action is authorized by this planning document.**
