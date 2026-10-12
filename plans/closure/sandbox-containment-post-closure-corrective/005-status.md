# Sandbox Containment Post-Closure Corrective C005 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/sandbox-containment-post-closure-corrective/005-integration-branch-ownership-and-base-reconciliation.md`

Source subsystem roadmap:

- `plans/subsystems/sandbox-containment-post-closure-corrective-roadmap.md#7-milestones-and-useroperator-exits`

Repository baseline reviewed: `origin/main` at `7317a67ad604fe39fd5282ddca07496255faa4d1`; preserved mixed branch at `8cd12a9c6ec3ebac01b1165b846efb127973133f`.

Implementation commits and pull requests:

- `c6a63a46e0efdb0e1b68b5258e03206db91e5dcb` — sandbox-only C001–C004 candidate on PR [#112](https://github.com/dbowm91/codegg/pull/112), created from current `main` without rewriting the shared branch.
- `e9ea2078718130229ef639af0658cab96fed191c` — Linux-gated Landlock policy import and semantics-preserving Clippy wildcard cleanup.
- `cbed1e5aeff6507cb542a6ab44209da7908c6bff` — shared sandbox boundary now rejects missing required read/write grant paths before backend normalization or target launch; pushed as current PR #112 head.
- [PR #111](https://github.com/dbowm91/codegg/pull/111), head `3f8453bab75966a4086bcfc009edccbfd6901bfe` — separate owner review containing human-shell, LSP/editor, desktop and Windows-native work; hosted CI and Desktop E2E passed. Windows process-launcher run [38278824470](https://github.com/dbowm91/codegg/actions/runs/38278824470) failed the C013 existing-object ACL/alias probe with `Access is denied`; that finding remains in the sibling owner scope.
- [PR #110](https://github.com/dbowm91/codegg/pull/110), head `8cd12a9c6ec3ebac01b1165b846efb127973133f` — preserved mixed provenance branch; not force-rebased or closed.

## 1. Executive finding

The mixed branch is no longer the sandbox review candidate. PR #112 is based on current `main` and its diff contains sandbox implementation, tests, documentation and C001–C004 closure material, plus two narrowly scoped CI repairs described below. Human-shell/editor/desktop changes and their accepted records remain reachable on the unchanged source branch and are present in the separately reviewable PR #111. No sandbox policy semantics were dropped or weakened during the transplant.

C005 remains conditionally closed. The local focused `cargo test` still cannot link on this configured x86_64 macOS toolchain. C006's Linux hosted run confirms the required-grant regression passed, but the full hosted workspace job failed on unrelated tests on both its first attempt and one permitted retry; C006 therefore did not establish a clean full-suite qualification. The local limitation is not treated as a test pass or as a sandbox invariant failure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Current base and final sandbox candidate are identified | `origin/main` `7317a67`; PR #112 head `cbed1e5`; merge base `7317a67` | Pass | Branch was created from current `main`; the final candidate is 0 behind and 22 ahead. |
| Sandbox candidate has bounded ownership | `gh pr diff 112 --name-only`; `git diff --name-status origin/main...sandbox/c001-c004-review` | Pass | Sandbox implementation/tests/docs and its own plans/closures only, plus `src/security/sandbox/landlock.rs` target gating and one existing test-pattern Clippy cleanup. |
| Human-shell and editor work remain reachable and reviewable | Preserved branch/PR #110; separate PR #111 diff contains `src/core/daemon_turns.rs`, `src/core/daemon_lsp.rs`, `src/tui/commands/editor.rs`, desktop host sources and their plan/closure files | Pass | PR #111 hosted CI and Desktop E2E passed. Its Windows process-launcher check failed C013's ACL/alias probe; that qualification remains unresolved in the sibling owner scope. |
| Four known red-CI code paths are not attributed to sandbox | Compare PR #112 diff to the exact diagnostics in plans C005/C006 | Pass | Five `daemon_turns.rs` diagnostics, three LSP/editor diagnostics, and two desktop duplicate-field diagnostics are absent from #112 and owned by #111. |
| Focused static/security ownership guards pass | Commands in §4 | Pass | Sandbox contract self-test and contract, policy-wiring, execution-ownership, and scheduler-bypass guards all passed. |
| Local canonical quick verification passes | `scripts/verify.sh quick` | Pass | Passed on `e9ea207` and reran successfully on final source head `cbed1e5`; the final run included `cargo check --workspace --all-targets --locked`. |
| Focused Rust tests run locally | `cargo test -p codegg --lib security::sandbox` | Environment blocked | Link failed with unresolved `_crc32`, zlib, iconv and lzma symbols; Rust compiler-builtins were built for macOS 26 while the local link target was macOS 10.12. Hosted CI on `cbed1e5` ran the exact required-grant regression successfully, but full workspace tests failed on unrelated tests. |
| Rebase/transplant preserves mixed history | Original branch remains at `8cd12a9`; new branch begins at `7317a67`; no force push | Pass | Sandbox commits were cherry-picked onto a new branch. Registry conflicts during historical plan commits were resolved by retaining current `main`'s registry on #112; this planning branch retains the full registrations and records. |

## 3. Production implementation evidence

No new sandbox behavior was introduced by C005. The sandbox candidate carries the accepted C001–C004 source: required fail-closed launches, Landlock ABI/truncate rights, effective deny precedence, scoped private scratch, Python mode enforcement, and the measured Seatbelt contract. The transplant retained the sandbox-only changes in shared `src/managed_process.rs` and `src/python_script/executor.rs`; human-shell environment and dispatch changes were not copied.

The C006 gate found two narrow lint issues on the candidate. `BackendPolicy` is now imported only on Linux because it is used only by the Linux Landlock implementation. The existing `ProjectionEvent::TurnCompleted` test pattern now uses `..` instead of two redundant wildcard fields; test behavior is unchanged. Focused Clippy on the session-family target passed after these changes.

## 4. Verification executed

### Commands run

```bash
git rebase origin/main
git diff --check origin/main...HEAD
git rev-list --left-right --count origin/main...HEAD
python3 scripts/check_sandbox_contract.py --self-test
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
cargo fmt --all -- --check
cargo test -p codegg --lib security::sandbox
scripts/verify.sh quick
cargo clippy -p codegg --test session_family --locked -- -D warnings
```

### Results

- Rebase reported current branch up to date; base/merge base was `7317a67ad604fe39fd5282ddca07496255faa4d1`.
- Static guards, `git diff --check`, and formatting passed.
- `scripts/verify.sh quick` passed its schema, boundary, security, ownership, TUI, provider, skills and workspace check stages. It emitted a target-specific unused-import warning before the C006 import-gating correction; the final focused Clippy command passed after that correction.
- The final candidate `cbed1e5` was checked with `scripts/verify.sh quick`; it passed, including `cargo check --workspace --all-targets --locked`.
- Local `cargo test -p codegg --lib security::sandbox` exited 101 at link. The host reports unresolved zlib (`_crc32`/deflate), iconv and lzma symbols, along with compiler-builtins targeting macOS 26 against a macOS 10.12 deployment target. No test result is claimed from this command.
- The first hosted CI run on `c6a63a46` passed formatting and all static guards, then failed Clippy 1.99 on a redundant wildcard in `tests/session_family/projection_consumer.inc`; workspace tests were skipped. This was fixed without changing test behavior at `e9ea207`.
- Hosted CI on final head `cbed1e5` is run `38281750403`. Both attempts passed formatting, all static guards, and Workspace Clippy. Attempt 1 stopped after 9,896 passing tests and one unchanged `codegg-core::goal::checkpoint::tests::test_append_checkpoint_update` failure; the permitted retry passed that test, reached 11,554 passing tests, then received SIGKILL in unchanged `egggit::process::tests::run_command_times_out_instead_of_waiting_forever`. The exact sandbox regression `sandbox_landlock::required_rule_failure_stops_before_target_execution` passed on the retry. The full hosted suite remains failed; no more same-SHA retry is authorized.
- Desktop E2E is path-gated out for #112; the sibling PR #111 E2E run `38278829963` passed at head `3f8453b`. PR #111's Windows launcher run `38278824470` failed its separate C013 fixture and is not used as sandbox evidence.

## 5. Invariant review and ownership matrix

| Source area or diagnostic path | Owner | Disposition in focused candidate |
|---|---|---|
| `src/security/sandbox.rs`, `src/security/sandbox/{landlock,policy,seatbelt}.rs`, `src/python_script/{sandbox,executor,projection,types}.rs`, `src/tool/bash/process.rs`, sandbox tests/guards, security docs/skill, C001–C004 plans and closures | Sandbox C001–C004 | Included; source changes are the accepted qualified implementation. |
| Sandbox-only hunks in `src/managed_process.rs` | Sandbox C003/C004 | Included: private scratch, deny preflight and lifecycle qualification. Human-shell-only environment policy and dispatch hunks are excluded. |
| `src/core/daemon_turns.rs` (five prior Clippy diagnostics) | Human-shell | Excluded from #112; retained on source branch and present in #111. |
| `src/core/daemon_lsp.rs` and `src/tui/commands/editor.rs` (three prior Clippy diagnostics) | LSP/editor | Excluded from #112; retained on source branch and present in #111. |
| `apps/desktop/src-tauri/src/present.rs` (two prior duplicate-field errors) | Desktop presentation | Excluded from #112; retained on source branch and present in #111. |
| `tests/session_family/projection_consumer.inc` (current Clippy wildcard) | Session-family test fixture / C006 integration gate | One-line semantics-preserving fix included so the current workspace gate can run. |
| `plans/registry.md` | Shared planning control surface | Not copied onto #112 to avoid importing unrelated registry history; this branch records C005/C006 lifecycle and registry changes. |

Sandbox invariants, accepted test provenance, and C004 limitations remain unchanged. No hard human-shell or editor dependency was found for the sandbox code.

## 6. Failure and recovery review

The source branch and PR #110 remain untouched and provide a recovery point for the mixed work. The new branch can be abandoned independently without deleting accepted commits. Cherry-pick conflicts were confined to historical plan files and `plans/registry.md`: plan/roadmap versions associated with sandbox C001–C003 were retained, while the focused candidate kept `main`'s registry to avoid importing concurrent workstream statuses. Sandbox source cherry-picks applied without semantic conflicts. No conflict resolution changed authority or process behavior.

## 7. Migration and compatibility review

The branch separation introduces no schema, protocol, configuration, storage or migration change. The unrelated human-shell v71 migration remains on its independently reviewable branch/PR. Existing sandbox behavior and documented compatibility limits are unchanged.

## 8. Security review

The new PR does not claim stronger Seatbelt confidentiality, network isolation, inherited-descriptor revocation, XPC/Mach containment, Apple Events/Launch Services blocking, setuid protection, or Linux/macOS parity. C004 remains conditionally closed with the existing named limitations. No accepted Linux ABI 4 or native Apple Silicon test evidence was invalidated by the transplant; final hosted CI is still required to verify the integrated candidate.

## 9. Documentation and operations

PR #112 links C001–C004 records and states the C004 limitations. PR #111 carries the separate human-shell/editor/desktop ownership surface and has green hosted CI and Desktop E2E. PR #110 remains available as mixed provenance. The planning branch retains recovered C005/C006 plans, roadmap and registry references.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Medium | Exact final-head full hosted CI did not pass after its one permitted same-SHA retry | Strict full-workspace integration qualification is not established, despite the focused sandbox regression passing | Keep C005/C006 conditional; do not start another retry-only loop. Revisit when the unrelated checkpoint/process test failures are resolved or a later normal candidate run is green. |
| Low | Local x86_64 macOS test linking fails due compiler/linker target and system-library mismatch | Focused tests cannot run locally in this environment | Record the limitation; the focused Linux grant regression passed in hosted CI, but do not claim the full suite passed. |
| Medium | PR #111 Windows process-launcher run `38278824470` failed C013's existing-object ACL/alias fixture with `Access is denied` | Sibling Windows qualification remains open | Leave with the separate PR #111 owner; do not copy the Windows work into sandbox PR #112. |
| Medium | C004 Apple Events/Launch Services, XPC/Mach and setuid probes remain unqualified; Seatbelt has broad reads, unrestricted network and inherited FDs | No stronger macOS confidentiality/IPC/privilege guarantee is supported | Preserve C004's conditional status and limitations; no new probe is required by C005/C006. |

No high-severity ownership or sandbox enforcement defect was found in the branch split.

## 11. Roadmap disposition

C005 remains conditionally closed: branch ownership and preservation are established, and the candidate is reviewable, while local test execution is unavailable and the final full hosted suite has an unrelated test failure after the bounded retry. C006 is conditionally closed with the exact sandbox test passing and the workspace-suite failure recorded. No other plan in this line is dependency-ready; C004 remains conditionally closed.

## 12. Registry updates

- Record C005 and C006 as conditionally closed and remove both from dependency-ready work.
- No downstream plan is unblocked by this closure; no other plan in this line is currently eligible.
- Keep the subsystem conditionally closed pending clean hosted full-suite evidence; retain C004's conditional status and preserve C001–C004 history.
- Record PR #112 as the focused sandbox candidate, PR #111 as sibling ownership, and PR #110 as preserved provenance.
