# Managed Agent Scratch M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/managed-agent-scratch/004-private-scratch-reclamation.md`

Source subsystem roadmap:

- `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`

Repository baseline reviewed: `61cbe9222c442c443cc7dfb3bd3a5b7d99ecd649` (`origin/main`, after ADR-0033 numbering and project/workspace identity reconciliation)

Implementation commits or pull requests:

- `a0a9e44b` — owner-validated terminal private-run and deleted-session reclamation, scheduler lifecycle wiring, and busy/restored-session tests.
- `ab5874d5` — restart reconciliation for deleted-session cleanup.
- `5905a2bb` — Windows safe shared-scratch reclamation and native reparse-point coverage.
- `a92e40167d3a4223c3336b4b49985f4f2b1c60fc` — integrated implementation head after rebasing onto current main, ADR numbering and project/workspace identity reconciliation, canonical temporary-path fixture corrections, and the v83 schema assertion fix; PR [#128](https://github.com/dbowm91/codegg/pull/128).

## 1. Executive finding

M004 is complete. Terminal delegated-run scratch is reclaimed only after the
supervised child returns, and only that run's root is removed. A soft-deleted
session's complete scratch tree is reclaimed after all durable runs and jobs
are terminal; active work defers cleanup, restoration prevents deletion, and
daemon startup resumes pending cleanup. Durable transcripts, run results, and
managed Git worktrees remain separately stored. M003's native sandbox
qualification conditions remain separate.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Reclaim only the terminal run's private root | `scratch::tests::terminal_run_cleanup_removes_only_its_private_root`; scheduler calls cleanup after terminal store transition and child return | pass | Sibling run roots remain present; duplicate cleanup is idempotent. |
| Reclaim a deleted session's nested private/shared tree safely | `scratch::tests::deleted_session_cleanup_reclaims_nested_private_and_shared_roots` | pass | Uses typed session identity and the existing owner-validated tombstone reclaimer. |
| Keep active work; stop cleanup after restoration | `daemon_sessions::scratch_cleanup_tests::deleted_session_cleanup_waits_for_jobs_and_preserves_restored_sessions` | pass | Covers an active durable job, restored session, and idle deleted session. |
| Resume deleted-session cleanup after daemon restart | `CoreDaemon::reconcile_deleted_session_scratch` keyset scan, page size 256; invoked during startup recovery | pass | Re-arms the existing durable-state-checked cleanup watcher. Query errors defer cleanup. |
| Preserve workspace/scratch default-Ask correction and explicit permission precedence | CI workspace permission suite, including `global_ask_default_does_not_prompt_for_authenticated_file_mutations` and explicit path/tool Ask/Deny regression tests | pass | Only the implicit general Ask default is bypassed for authenticated workspace and authorized scratch writes. |
| Platform cleanup path | Exact integrated-head Windows process launcher [39258980807](https://github.com/dbowm91/codegg/actions/runs/39258980807); Linux workspace CI [39258980673](https://github.com/dbowm91/codegg/actions/runs/39258980673) | Linux workspace suite passed 14,803/14,803 (6 skipped). Windows workflow passed; process launcher 8/8 and managed scratch lifecycle 11/11 passed. | Windows validates reparse-point handling; no Windows Required sandbox support claim is made here. |

## 3. Production implementation evidence

`src/scratch.rs` reconstructs private paths from host-owned session/run IDs and
uses owner-validated tombstones. `SubagentJobExecutor` reclaims one run's root
after durable terminal transition and child completion. Session deletion starts
a deferred cleanup watcher, which serializes against restore through the
SQLite writer transaction and checks canonical durable run/job state. Query
errors and nonterminal work defer deletion. Startup scans deleted sessions in
bounded keyset pages and re-arms those watchers. Session deletion covers nested
private and shared scratch; durable results and worktrees are outside the tree.
No new migration or protocol field was added. Schema version remains 83 after
main reconciliation.

## 4. Verification executed

### Commands run

```bash
gh run view 39234481292                # integrated-head hosted CI workflow
gh run view 39234481307                # integrated-head Windows process/scratch qualification
gh run view 39234481322                # integrated-head Desktop E2E
gh run view 39258980673                # reconciled exact-head hosted CI workflow
gh run view 39258980807                # reconciled exact-head Windows qualification
gh run view 39258980688                # reconciled exact-head Desktop E2E
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 rustup run 1.89-aarch64-apple-darwin bash scripts/verify.sh full
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 rustup run 1.89-aarch64-apple-darwin cargo test -p codegg --test workspace_isolation --test subagent --test managed_scratch_job_admission --test tui_project_picker --test worktree --locked
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 rustup run 1.89-aarch64-apple-darwin cargo test -p codegg --test tool_surface_minimization --locked
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 rustup run 1.89-aarch64-apple-darwin cargo test -p codegg-core --test user_prompt_queue user_prompt_queue_v80_migration_creates_table_index_and_layout --locked
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 rustup run 1.89-aarch64-apple-darwin cargo test -p codegg --lib session_prompt_submit_resolves_selection_and_submits --locked
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 rustup run 1.89-aarch64-apple-darwin cargo test -p codegg --lib scratch --locked
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 rustup run 1.89-aarch64-apple-darwin cargo nextest run -p codegg --lib --locked --profile ci -E 'test(same_scope_requests_coalesce_to_one_publication)'
rtk cargo fmt --all -- --check
rtk git diff --check
```

### Results

- Exact-head hosted CI run 39227777524: failed during nextest after 5,323/14,803 tests ran (5,322 passed, 1 timed out, 6 skipped); the known load-sensitive `same_scope_requests_coalesce_to_one_publication` timeout stopped the fail-fast sweep. A test-specific runner reservation was added to the CI profile; the isolated test then passed 1/1 locally under the CI profile and in the complete hosted sweep.
- Hosted CI run 39231726989 on `9dc27b4f`: the reservation prevented the timeout; 12,593 tests ran (12,592 passed, 1 failed, 6 skipped). `user_prompt_queue_v81_migration_creates_the_owner_table` still expected v82 after the managed-scratch v83 migration. Its assertion and explanatory comment were corrected in `0bef221a`.
- Exact-head hosted CI run 39234481292 on `0bef221a`: passed all 14,803 tests; 6 intentionally skipped and 1 classified slow. Formatting, repository guards, Clippy and live Eggwork fixtures also passed.
- Reconciled exact-head hosted CI run [39258980673](https://github.com/dbowm91/codegg/actions/runs/39258980673) on `a92e40167d3a4223c3336b4b49985f4f2b1c60fc`: passed all 14,803 tests (6 skipped, 1 classified slow); formatting, repository guards, Clippy and live Eggwork fixtures passed.
- Reconciled exact-head Windows process launcher run [39258980807](https://github.com/dbowm91/codegg/actions/runs/39258980807) on `a92e40167d3a4223c3336b4b49985f4f2b1c60fc`: passed. Process launcher 8/8; managed scratch lifecycle 11/11; all executed failure-injection, ACL, AppContainer, and native policy checks passed. Environment-conditional probes are reported by the workflow as skipped.
- Reconciled exact-head Desktop E2E run [39258980688](https://github.com/dbowm91/codegg/actions/runs/39258980688) on `a92e40167d3a4223c3336b4b49985f4f2b1c60fc`: passed, including the built-app WebDriver trajectory.
- Integrated-head Windows process launcher run 39234481307: passed. Process launcher 8/8; managed scratch lifecycle 11/11; remaining executed qualification steps succeeded. Two environment-conditional WRITE_RESTRICTED probes were skipped, as reported by the workflow.
- Exact-head Desktop E2E run 39234481322 on `0bef221a`: passed after rerunning the two transiently failing renderer tests.
- Exact-head Windows run 39234481307 on `0bef221a`: passed. Process launcher 8/8; managed scratch lifecycle 11/11; all remaining executed qualification steps succeeded. The two environment-conditional WRITE_RESTRICTED probes were skipped.
- Earlier Desktop E2E run 39227777510 on the pre-reconciliation head: passed.
- Desktop E2E on `0bef221a` (run 39234481322): the initial attempt had two renderer session-route tests fail to find their asynchronously loaded list; rerunning the failed job passed. The code changes between the successful `9dc27b4f` run and this attempt were confined to CI scheduling policy and a core storage test fixture.
- A prior Windows qualification run, 39224485280, also passed: process-launcher 8/8 and managed scratch lifecycle 11/11.
- Affected session, delegation, job admission, project picker, workspace identity, and worktree suites: 65 passed (managed scratch job admission 1, subagent 22, project picker 22, workspace isolation 6, worktree 14); tool-surface minimization 13 passed; schema migration regression 1 passed.
- Rust 1.89 ARM64 macOS focused selection test: 1 passed after correcting the fixture to use a canonical home.
- Rust 1.89 ARM64 macOS scratch-filtered suite: 21 passed, including terminal-run cleanup, nested deleted-session cleanup, active-job/restored-session deferral, workspace-temp classification, and permission regressions.
- `cargo fmt --all -- --check` and `git diff --check`: passed after documentation updates.
- The first focused run under Homebrew's x86_64 toolchain failed at linking because local native libraries did not match the target architecture; rerunning with the installed Rust 1.89 ARM64 toolchain succeeded.
- An initial local focused nextest invocation under the default x86_64 toolchain failed at link time on zlib/LZMA symbols; this matches the recorded native-library architecture issue. The Rust 1.89 ARM64 rerun with the `ci` profile and exact test filter passed 1/1 in 0.186s.
- Earlier integration-head CI failures were each corrected before this verification: rootless tool-palette fixtures (`39207889731`), a desktop projection fixture missing new optional fields (`39220086499`), a workspace symlink expectation inconsistent with current rejection behavior (`39220906792`), and a hard-coded schema version behind migration v83 (`39224485428`).
- The first local Rust 1.89 ARM64 macOS full-verifier run exposed symlinked temporary-home paths in two C006 tests; both now canonicalize their isolated homes and pass 7/7. A subsequent run found hard-coded `/tmp` workspaces in the two M004 controller-lease session-family tests; both now use canonical temporary workspaces and passed in the complete workspace rerun.
- Final local `scripts/verify.sh full`: quick guards and Clippy passed; the default workspace nextest phase passed 14,767 tests with 6 skipped. The additional `server,plugins,lsp-test-support` phase ran 2,775/11,600 before fail-fast: 2,772 passed, 2 failed, 1 received SIGKILL, and 6 were skipped. Failures were `security::sandbox::policy::tests::read_allowances_cover_the_interpreter_search_path` and `security::sandbox::policy::tests::sensitive_paths_are_denied`; `security::sandbox::policy::tests::policy_derives_from_a_launch_spec` was SIGKILLed. Those policy tests passed in the default-feature workspace phase. This optional macOS feature-tier result remains a recorded limitation; it does not change M004's scratch reclamation behavior or the hosted Linux, Windows scratch lifecycle, and Desktop qualification evidence.

## 5. Invariant review

- Cleanup inputs are typed host-owned identities, not caller paths.
- Owner markers and complete trees are validated under tombstone before removal; links and unsafe entries are rejected/quarantined by platform implementation.
- A child keeps its scratch lease until it returns; a terminal run cleanup cannot target a sibling.
- Session cleanup verifies soft deletion while holding the SQLite writer lock, then checks durable runs and nonterminal jobs before removing the tree.
- Restore or ambiguous durable state preserves the data; query failures defer cleanup.
- Authenticated workspace/private/explicit shared-scratch writes bypass only the implicit default Ask. Explicit tool/path Ask and Deny, hard denials, and path validation remain authoritative.

## 6. Failure and recovery review

Duplicate reclamation is idempotent. A tombstone left by interruption can be
revisited safely. Busy runs/jobs and store errors retain session scratch.
Restore wins before cleanup or waits for the cleanup transaction; once restored,
no watcher removes the tree. Startup recovery is bounded to 256 session IDs per
query page and retries through per-session watchers. Cleanup failures are
logged and deferred; they do not affect durable run results or worktrees.

## 7. Migration and compatibility review

No database migration, protocol, configuration, or serialized DTO change was
needed. The main-branch schema sequence is preserved at
`STORAGE_LAYOUT_VERSION = 83`. Existing scratch locators remain stable, and a
restored/used session can lazily recreate disposable scratch.

## 8. Security review

Cleanup never trusts a caller-supplied path or scratch path as authority. It
uses per-user managed roots, typed identity reconstruction, ownership records,
tombstones, and safe tree inspection. Cleanup is serialized against session
restore and checks run/job state. No home directory, global temp directory,
credential store, or sibling scratch authority was added. Shared scratch grants
remain direct-child and lease scoped. Secret values are not logged.

## 9. Documentation and operations

Updated `architecture/agent.md`, `architecture/jobs.md`,
`architecture/security.md`, `docs/tools.md`, `.opencode/skills/agent/SKILL.md`,
and `.opencode/skills/jobs/SKILL.md` with the cleanup and restart-recovery
contract. The workspace mutation default-Ask correction remains documented in
the permission and tool references.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Medium | Linux native scratch sandbox qualification and Windows Required-sandbox containment remain outside M004 and are still unqualified. | Do not claim OS-enforced scratch containment on those configurations. | Preserve M003's conditional status; qualify those backends before making platform support claims. |
| Low | No configurable quota or periodic collector exists for live, non-deleted session scratch. | Such data remains until its owner session is deleted or host cache maintenance removes it. | Any quota/retention collector requires a separate lifecycle design and race tests. |
| Low | The local optional `server,plugins,lsp-test-support` macOS nextest tier had two sandbox-policy assertions fail and one test process receive SIGKILL, although those policy tests passed in the default-feature suite. | The optional feature tier is not qualified on this local host; M004 scratch lifecycle remains covered by default workspace tests, hosted Linux CI, and Windows lifecycle qualification. | Diagnose the local feature-tier failures before citing a green macOS optional-feature run; do not expand M003 native sandbox claims. |

## 11. Roadmap disposition

M004 is closed and the session-private/agent-private accumulation gap is
resolved. M003 remains conditionally closed solely for its stated native
qualification limits; those limits do not block M004. No later milestone is
registered in the managed-agent-scratch line, so this closure unblocks no
successor plan. Agent-workstream M005 remains independent; its writer
containment research does not gate this private cleanup work.

## 12. Registry updates

- Mark M004 closed in its implementation plan, the managed-agent-scratch roadmap,
  and both managed-agent-scratch sections of `plans/registry.md`.
- Preserve M003's conditional status and correct its stale statement that no
  successor had been registered: M004 is that corrective successor.
- Record the exact-head verification results and the remaining M003 qualification
  limits below before committing this record. The registry audit found no registered
  downstream scratch milestone newly eligible to start. Agent-workstream M005 remains
  active and independent; M008 remains blocked on M006 and M007, none of which are
  dependencies of M004. No plan was unblocked by this closure.
