# Sandbox Containment Post-Closure Corrective C006 — Hosted Integration Qualification and PR Closure

Status: blocked (hard prerequisite C005; recovered from earlier lost planning branch, final CI red on current mixed head)

Repository baseline: `planning/sandbox-containment-post-closure-corrective` at `e6950f5088dc1bb855c4c3c27ac7507b67fa07fd`; `main` at `7317a67ad604fe39fd5282ddca07496255faa4d1`, with the branch 37 ahead / 0 behind (observed 2026-10-12, before recovered plan commits). Implementation must use C005's final candidate head, not these pre-handoff SHAs.

Source roadmap:
- `plans/subsystems/sandbox-containment-post-closure-corrective-roadmap.md#7-milestones-and-useroperator-exits`

Long-term requirements:
- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Applicable ADR: `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`.

Predecessor plans/closures:
- C001-C003 strict closure records under `plans/closure/sandbox-containment-post-closure-corrective/{001,002,003}-status.md`.
- C004 conditional closure: `plans/closure/sandbox-containment-post-closure-corrective/004-status.md`.
- C005 integration/ownership corrective (hard): `plans/implementation/sandbox-containment-post-closure-corrective/005-integration-branch-ownership-and-base-reconciliation.md`, followed by its required `005-status.md`.
- Existing native Apple Silicon real-helper run: https://github.com/dbowm91/codegg/actions/runs/38166386985 ; existing real Linux 6.8/ABI 4 qualification recorded in C002/C003.
- PR #110: https://github.com/dbowm91/codegg/pull/110 (may be superseded in C005; do not assume it remains the final PR).

Primary class: invariant (security qualification and truthful closure); infrastructure (release/merge handoff).

## 1. Objective

Qualify the **final ownership-correct, reconciled sandbox PR head** through the existing normal CI and the minimum necessary focused host security tests, repair any regression introduced by integration, and produce a truthful final review/merge handoff. Do not equate earlier successful native Seatbelt tests or local `verify.sh quick` with passing CI on a new combined head. Maintain C004's named macOS residual limitations, without an unnecessary IPC redesign or another general test loop.

## 2. Why blocked / opening criteria

C001-C003 enforcement fixes are complete, C004 has a real Apple Silicon run, and Linux ABI 4 was qualified. Those results establish the security baseline, **not** the integrity of whatever Git history and source tree emerges from C005's necessary workstream separation and current-main reconciliation. The last checked mixed work branch **did** have commit-associated Actions runs for its head, and both were red. PR #110 still carries obsolete planning-only metadata. C005 must first deliver an integration candidate plus ownership/compatibility evidence and establish which PR will merge. Only then is this milestone dependency-ready.

## 3. Current findings requiring this corrective

- `main...planning/sandbox-containment-post-closure-corrective` now shows `ahead` at 37 ahead / 0 behind after history reconciliation; this is insufficient because unrelated human-shell/editor work and stale PR metadata remain. Recalculate against the final chosen integration branch.
- PR #110 has a misleading title/body that describes the initial **documentation-only** plan and the obsolete ready/blocked statuses of C001-C004; the current accepted evidence closes C001-C003 and conditionally closes C004.
- The source diff contained unrelated daemon-owned human shell and TUI LSP editor changes with separate closure evidence. C005 owns isolation of that scope before the CI conclusion is attributable to the intended review.
- Hosted native run `38166386985` on `98f96a99b9f9703b7cc3d585e0401f3b44764f05`: macOS `15.7.9` (ARM64) Seatbelt real helper 15/15, Python executor 22/22, private scratch cleanup, timeout/cancellation, status fail-closed and hardlink preflight passed. Linux real helper reported kernel `6.8.0-146-generic`, effective Landlock ABI 4, with real read/write/truncate/credential/rename/link behavior documented. Later changes to source or merge-conflict resolution must be checked for evidence invalidation.
- C004 is **conditionally closed**: broad Seatbelt read permissions, unrestricted network, pre-opened descriptors, Apple Events/Launch Services and unqualified XPC/Mach/setuid behavior are not protected by a full Linux-equivalent boundary. No network/IPC/privilege security claim may be introduced in PR metadata.

## 3A. Exact currently failing CI gates (2026-10-11; no false green conclusion)

The CI evidence on the pre-recovery head `e6950f5088dc1bb855c4c3c27ac7507b67fa07fd` is **not** "not run"; it is definitively **failed**:

| Workflow | Run/job | Failure | Owner scope |
|---|---|---|---|
| CI / verify | [38177378810](https://github.com/dbowm91/codegg/actions/runs/38177378810) / `114582507146` | Clippy `cargo clippy --workspace --all-targets --locked -- -D warnings`; eight errors; all workspace tests skipped | Human-shell and LSP editor |
| Desktop E2E | [38177378815](https://github.com/dbowm91/codegg/actions/runs/38177378815) / `114582507440` | Rust E0062 duplicate struct fields in desktop host test; WebDriver skipped | Desktop/test ownership |

**Exact Clippy diagnostics to resolve in the owning change set**:

1. `src/core/daemon_turns.rs:73` and `:1603`: `clippy::result_large_err` from `Result<..., CoreResponse>`; use a narrow typed error / `Box<CoreResponse>` with propagation adapted at the existing authoritative request boundary. Do not globally box/wire-change `CoreResponse` or suppress lint without a reasoned disposition.
2. `src/core/daemon_turns.rs:866` and `:869`: `clippy::needless_return`; preserve branch return semantics.
3. `src/core/daemon_turns.rs:512`: `clippy::unnecessary_to_owned`; avoid a temporary `workspace_id.to_string()` when borrowing is sufficient.
4. `src/tui/commands/editor.rs:1168` and `:1170`: `clippy::useless_conversion` on `&str` string literals; keep the intended parameter types and behavior.
5. `src/core/daemon_lsp.rs:488`: `clippy::items_after_test_module` because the diagnostics test module precedes production constants/functions; move tests after production items without changing visibility/behavior.
6. `apps/desktop/src-tauri/src/present.rs:462–479`: duplicate `input_tokens` / `output_tokens` fields in a test initializer; retain the fixture's intended token values and single assignment to each.

Treat these as **eight Clippy errors plus two compiler diagnostics**, not ten sandbox defects. The first-line remediation is to *separate the unrelated workstream from the sandbox review branch*, then resolve the errors on the **relevant owning branches** (and verify there). If the final sandbox PR intentionally retains some shared code, that code must pass these gates on the final SHA; do not borrow unrelated green CI evidence.

Do not downgrade the failures to "CI flake" or claim the tests passed: the Clippy job did not reach `Workspace tests`, and Desktop E2E never reached `Built-app WebDriver trajectory`. The earlier native sandbox helper run remains valid only for unchanged sandbox code.

## 4. Invariants and non-goals

**Must preserve:** required sandbox failure is terminal before target exec; Linux Landlock ABI>=3 `TRUNCATE` rights, `no_new_privs` and credential carve; Seatbelt final deny ordering; no shared writable global cache; private per-launch scratch with lifecycle cleanup; Python three-mode execution; correct backend status; no raw-target fallback; existing daemon authority and managed-process cancellation. Existing human shell and LSP changes must remain with their independent owners.

**Must not:** re-open C001-C003 just to rerun the same tests; assert macOS/Linux parity; claim C004's IPC/privilege probes were performed; add a permanent macOS CI lane, a new verification matrix, additional task automation, a sandbox rewrite, new migration, or a release. Do not merge an unattributed multi-workstream diff under a sandbox label to get green CI.

## 5. Required work packages

### A. Freeze the C005 candidate and evidence applicability

Record:
- accepted C005 closure, chosen final PR URL/number, base and head SHA, merge base, and diff file inventory;
- precise changes to sandbox source, tests, helper, policy, Python, and shared execution infrastructure **since** native Linux/macOS qualification;
- whether accepted host evidence remains directly applicable (byte-identical sandbox implementation, or only non-semantic docs/registry changes), vs changes demanding targeted real-host retest.

Create a small evidence matrix with rows for fail-closed setup, Linux ABI/truncation, overlapping credential denies, Seatbelt rule order, Python modes, scoped scratch + cancellation, compatibility, and status reporting. For each: linked source SHA, host/cmd, actual outcome, invalidated yes/no, and remediation if required. Avoid deriving test success merely from a source comment or fixture skip.

### B. Run only the required focused tests for changed boundaries

On final candidate, verify:
```bash
git diff --check
cargo fmt --all -- --check
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
cargo test -p codegg --lib security::sandbox
cargo test -p codegg --lib python_script::sandbox
cargo test -p codegg --test sandbox_containment -- --test-threads=1
scripts/verify.sh quick
```

Use the actual repository feature/target names and avoid repeating a full local workspace suite already executed by the standard hosted workflow. Run a focused Linux ABI>=3 real-helper fixture on a supported host **only when prior C002/C003 evidence cannot be traced to unchanged resulting implementation**, recording kernel/ABI and no-skip status. Do the analogous native macOS Seatbelt/Python check **only when C005 changes invalidate the previously qualified semantics**. The previously accepted host runs are not silently extended across unrelated source changes.

### B2. Repair or isolate pre-existing CI blockers with explicit file ownership

After C005 maps the changed hunks to owner workstreams, confirm the exact ten diagnostics above are **absent from the final sandbox diff or fixed at their legitimate upstream owner**. Build minimal reproductions with `cargo +stable clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib --no-run` (or the exact Desktop host command in the workflow). Any code fix must preserve daemon authorization, LSP read-only navigation, test intent and protocol semantics, and have a targeted unit regression if behavior could have changed. A one-line lint-only fix needs no new domain test if it preserves semantics, but the failing command must pass.

If the errors are only in sibling workstreams that do not belong to the focused sandbox PR, remove those hunks safely **rather than importing cross-cutting feature changes into a sandbox branch**. Keep their code and closure documents reachable on separate owned branches. Do not edit C001-C004 accepted closure evidence to pretend these unrelated failures do not exist.

### C. Obtain normal hosted verification on the final PR head

Use the existing PR-triggered `CI / verify` workflow at the *exact final head SHA*. Run no extra permanent lane. Record run ID/URL, triggering event/PR, commit SHA, job and step outcomes, actual conclusion, and whether required format/check/Clippy/static guard/test phases completed.

Failure rules:
- a failing test, compiler error, Clippy/security guard, or inability to run a required check is a **real blocker**, not a passing CI record;
- an unrelated flaky or runner-provisioning failure may receive one normal retry on the same SHA with the cause and outcome recorded; do not create more plans simply to re-run the same gate;
- a source/merge edit after a successful run requires a new run on the new head for strict integration qualification;
- a run associated with the old mixed branch or a prior native qualification commit **cannot** substitute for normal CI on the final review candidate;
- absence of native security enforcement evidence cannot be disguised as passed by a GitHub-hosted Linux test that skipped the backend.

If required checks cannot run because of Github policy/permissions, record the exact restriction and mark conditionally closed or blocked; do not create an empty "green" evidence claim.

### D. Produce reviewable PR metadata and final closure

Update the active sandbox integration PR (either #110 or C005's successor) so it states:
- C001-C003 source implementation and qualified Linux/macOS evidence;
- C004 conditionally closed, exact unqualified Apple Events/Launch Services, XPC/Mach and setuid probes, broad reads, unrestricted network, inherited descriptors;
- scoped file/diff, branch history and review ownership;
- test run URLs, final PR head, CI conclusion, residual issues/severity, compatibility effects (including global cache write changes);
- nothing suggesting "planning only" or generic kernel/IPC/network equivalence.

Link a superseded PR rather than leaving competing active submissions with inconsistent status. Request review via existing repo policy and report mergeability *after* checks. Merge only when ordinary user/repo review authority permits it; do not self-approve or infer approval from green checks. PR may remain open but ready for maintainer merge under a positive C006 closure.

### E. Reconcile documentation/status and successor decision

Create `plans/closure/sandbox-containment-post-closure-corrective/006-status.md`, update `plans/registry.md`, roadmap and `005-status.md` link. Preserve prior C001-C004 closure records and standalone runtime-safety conditional evidence. C004 remains **conditionally closed** until a separately authorized bounded IPC/privilege qualification exists; no new C007 is required solely to repeat those residual probes in this integration pass. Only if new critical/high defects are encountered should another corrective line be considered.

## 6. Failure, cancellation, restart and migration semantics

This is integration work, not a new runtime state machine. Confirm existing regression evidence for child cancellation/reap/private scratch and fail-closed pre-exec setup; do not change it to ease test execution. Do not alter durable storage, migrations, IPC, project/workspace/session identity, or approval rules. The integration candidate may be abandoned without changing or removing accepted independent branches; revert transient test-only configuration safely. If `main` advances during checks, compare actual changed hunks and refresh/requalify as warranted rather than mechanically restarting full qualification from scratch.

## 7. Acceptance and stop conditions

C006 is strictly closed **for integration** only when:
1. C005's ownership matrix and final candidate/PR are accepted, no foreign uncontrolled work remains in the sandbox submission, and the final branch is current with its review base;
2. required focused and existing hosted checks pass on the **final** candidate, with exact SHA/run identity recorded;
3. prior native Linux/macOS sandbox evidence remains applicable or a targeted qualified native rerun replaces it where implementation changed;
4. no critical/high enforcement, provenance, authority or data-integrity finding remains;
5. PR summary and review status accurately describe implemented behavior, scoped changes, tests and residual limitations;
6. `plans/registry.md` and roadmap accurately reflect C005/C006 outcome and keep C004 **conditionally closed** with its documented unresolved medium limitations;
7. there is no added CI lane, no public wire/storage migration, and no unauthorized merge.

Stop on a real security/owner boundary regression, unresolved mix of independently-owned changes, failing required hosted test, missing platform evidence after substantive sandbox changes, or unapproved PR merge. If review approval is the only remaining item, recommend merge but do not claim it was performed. If CI is inaccessible, mark exact external blocker and avoid a repeated verification-only planning loop.

## 8. Closure evidence and handoff

Required record `plans/closure/sandbox-containment-post-closure-corrective/006-status.md`: C005 closure reference; final branch and PR URL; main/head/merge-base SHA and diffs; source evidence matrix; targeted commands pass/fail/skips; actual supported-Linux/macOS runs used and why applicable; final hosted CI run/job/check outcomes; observed PR mergeability and review status; unrelated workstream disposition; security/recovery/compatibility review; unresolved severity-ranked findings; exact recommendation (ready for review/merge, conditionally closed with explicit evidence blocker, or blocked). Link the final merged SHA only if a merge actually occurs.
