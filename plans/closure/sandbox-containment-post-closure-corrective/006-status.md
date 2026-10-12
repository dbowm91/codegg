# Sandbox Containment Post-Closure Corrective C006 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/sandbox-containment-post-closure-corrective/006-hosted-integration-qualification-and-closure.md`

Source subsystem roadmap:

- `plans/subsystems/sandbox-containment-post-closure-corrective-roadmap.md#7-milestones-and-useroperator-exits`

Repository baseline: `main` and PR #112 merge base `7317a67ad604fe39fd5282ddca07496255faa4d1`.

Final sandbox candidate: PR [#112](https://github.com/dbowm91/codegg/pull/112), branch `sandbox/c001-c004-review`, head `cbed1e5aeff6507cb542a6ab44209da7908c6bff`. The candidate is 0 commits behind and 22 commits ahead of `main`. PR #112 remains open; no merge or approval is claimed. PR #110 remains preserved mixed provenance. PR #111 remains the separate human-shell/editor/desktop/Windows owner branch.

Predecessor closure: C005 remains conditionally closed at `plans/closure/sandbox-containment-post-closure-corrective/005-status.md`. C004 remains conditionally closed at `plans/closure/sandbox-containment-post-closure-corrective/004-status.md`.

## 1. Executive finding

C006 repaired a real fail-closed defect found by the required hosted test: normalization discarded nonexistent required read/write grants, allowing the target to execute with a narrower-than-requested policy. The final candidate validates every required read/write path before backend selection and before direct Landlock application. The existing Linux helper regression passed on the final SHA.

The full hosted CI suite did not pass. Both exact-head attempts passed formatting, every static guard, and Workspace Clippy, but each stopped on a different test in code unchanged by PR #112. The one allowed retry was consumed. The result is **conditionally closed**, with the full-suite failure as an explicit medium evidence blocker; no further same-SHA retry is authorized by C006. No other plan in this corrective line is dependency-ready.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Ownership-correct final candidate and current base | PR #112 head `cbed1e5`; base/merge base `7317a67`; `git rev-list --left-right --count origin/main...HEAD` | Pass | Candidate is 0 behind / 22 ahead. Diff remains sandbox C001–C004 scope plus narrow CI cleanup and required-grant validation. |
| Requested required grants cannot be silently discarded | `src/security/sandbox.rs::validate_required_grant_paths`; CI run `38281750403`, retry job `114927563136`, test `sandbox_landlock::required_rule_failure_stops_before_target_execution` | Pass | The test passed at test 7,547. Missing read/write paths fail before target execution; absent deny paths retain their future-file behavior. |
| Existing Linux/macOS evidence remains applicable | C002/C003 Linux kernel 6.8 / Landlock ABI 4; C001/C003 native macOS 15.7.9 run `38166386985` | Pass with limitation | The final source change is backend-neutral validation for required grant roots. Existing qualified policy behavior is unchanged. The new validation was exercised on Linux; no new macOS run was performed. |
| Static guards, formatting, and workspace Clippy | CI `38281750403`, jobs `114920421499` and `114927563136`; local `scripts/verify.sh quick` on `cbed1e5` | Pass | Both hosted attempts passed the sandbox/ownership guards, formatting, and Clippy. Local quick verification passed, including `cargo check --workspace --all-targets --locked`. |
| Full hosted workspace tests on final SHA | CI `38281750403`, first job `114920421499`, retry job `114927563136` | Fail; unrelated tests | Attempt 1: 9,896 passed, one `codegg-core::goal::checkpoint::tests::test_append_checkpoint_update` assertion failed, 5 skipped. Retry: the checkpoint test passed; 11,554 passed before unchanged `egggit::process::tests::run_command_times_out_instead_of_waiting_forever` was aborted by SIGKILL, 5 skipped. One retry exhausted. |
| Desktop E2E coverage disposition | Workflow path filter; PR #112 diff; sibling run [38278829963](https://github.com/dbowm91/codegg/actions/runs/38278829963) | Pass for unchanged surface | #112 changes no Desktop E2E trigger paths. Sibling PR #111 Desktop E2E passed at `3f8453bab75966a4086bcfc009edccbfd6901bfe`. |
| Sibling Windows owner qualification | PR #111 Windows run [38278824470](https://github.com/dbowm91/codegg/actions/runs/38278824470) | Fail; sibling finding | C013 existing-object ACL/alias fixture failed with `Access is denied`. This code and test are outside #112 and remain with the #111 owner. |
| PR metadata and review handoff | PR #112 summary and open status | Partial | PR remains open and unmerged. The full hosted suite is red, so this record does not recommend merge. |

## 3. Failure, repair, and retry record

The prior exact-head run on `e9ea207` had an unrelated timing failure in `causal_observe_replay::replay_p95_within_budget`; its single retry surfaced a real sandbox defect. `BackendPolicy::normalized` canonicalized requested grant roots and silently omitted nonexistent entries. Landlock then applied the remaining policy and launched the target. The regression `required_rule_failure_stops_before_target_execution` expected exit 125, no target marker, and a setup error identifying the missing path.

Commit `cbed1e5aeff6507cb542a6ab44209da7908c6bff` adds a shared validator called by both `apply_backend` and direct `apply_landlock`. It canonicalizes each requested read/write grant, returns a setup error when a required path is missing or cannot be resolved, and runs before backend selection/application. It does not reject absent `deny_paths`, which remain necessary for protecting future-created sensitive files.

The final exact-head hosted CI attempts were:

- Attempt 1, run [38281750403](https://github.com/dbowm91/codegg/actions/runs/38281750403), job `114920421499`: formatting, all static guards, and Clippy passed. Workspace tests stopped after 9,896 passes because unchanged `codegg-core::goal::checkpoint::tests::test_append_checkpoint_update` failed to observe `Phase 1` in its appended checkpoint. No sandbox test failed in this attempt.
- The one allowed same-SHA retry, same run `38281750403`, job `114927563136`: formatting, all static guards, and Clippy passed. The checkpoint test passed. The sandbox required-rule regression passed. Workspace tests stopped after 11,554 passes when unchanged `egggit::process::tests::run_command_times_out_instead_of_waiting_forever` was aborted with SIGKILL. Five tests were skipped. No second retry was started.

These failures are recorded as real hosted test failures. Their source paths are unchanged by #112, and neither failure is attributed to sandbox behavior. They still prevent a clean full-workspace qualification under C006's acceptance criteria.

## 4. Security and compatibility review

The final source change preserves fail-closed behavior at the common application boundary. Each requested read/write grant must resolve before the backend receives a normalized policy; missing grants can no longer silently narrow authority. Direct Landlock callers receive the same check. Deny-path behavior is unchanged. No storage, wire, configuration, migration, scheduler, approval, or process-supervision contract changed.

The existing C004 residuals remain: Seatbelt has broad read permissions, unrestricted network access, inherited descriptors, and unqualified Apple Events/Launch Services, XPC/Mach, and setuid behavior. No full Linux-equivalent or complete IPC-isolation claim is made.

## 5. Ownership and sibling findings

PR #112 contains the sandbox candidate only. PR #111 retains human-shell, LSP/editor, desktop and Windows-native ownership. Its hosted CI `38278829788` and Desktop E2E `38278829963` passed at head `3f8453b`; its Windows process-launcher run `38278824470` failed the C013 forensic ACL/alias fixture. That sibling finding is not copied into #112 or fixed under this plan.

PR #110 remains open as preserved mixed provenance and is not the sandbox review candidate. No PR was merged or closed by this plan.

## 6. Unresolved findings

| Severity | Finding | Required disposition |
|---|---|---|
| Medium | Full hosted workspace tests remain red after the one permitted retry: first an unchanged checkpoint assertion, then an unchanged egggit timeout test process was SIGKILLed. | Keep C006 conditionally closed; do not retry the same SHA again. Revisit this qualification when a normal later candidate run passes the full suite or the owning test issues are resolved. |
| Medium | PR #111 Windows process-launcher C013 ACL/alias fixture failed with `Access is denied`. | Leave with the separate Windows/sibling owner; do not broaden PR #112. |
| Medium | C004 macOS IPC/privilege/read-surface limitations remain unqualified. | Preserve C004's conditional status and its existing limitations. |
| Low | Local focused Rust tests cannot link on the configured x86_64 macOS toolchain due target/system-library mismatch. | Record the limitation; do not claim local test execution. |

No critical or high sandbox enforcement finding remains after the required-grant fix. The full hosted-suite evidence blocker remains unresolved.

## 7. Roadmap and registry disposition

C006 is conditionally closed: the ownership-correct candidate, code fix, focused regression, guards, formatting, and Clippy are complete, but a required full hosted workspace pass is missing after the bounded retry. C005 remains conditionally closed because its final hosted qualification did not complete cleanly and the local linker limitation remains documented. C004 remains conditionally closed for its existing macOS limitations.

The registry and roadmap now mark C005 and C006 as conditionally closed. C006 unblocks no successor: C001–C006 are the complete plan line, and no other dependency-ready plan exists within it. Do not start another retry-only qualification loop or create C007 solely for these unrelated test failures.

Recommendation: retain PR #112 open and unmerged with the final hosted test failures visible. Do not describe the full CI as green or recommend merge from this conditional closure.
