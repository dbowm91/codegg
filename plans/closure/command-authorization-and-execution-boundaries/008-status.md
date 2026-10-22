# Command Authorization M004-C1 — Closure Status

Status: **closed**

Source implementation plan:

- `plans/implementation/command-authorization-and-execution-boundaries/008-m004-constrained-specialist-execution.md`

Source subsystem roadmap:

- `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md#6-dependency-graph`

Repository baseline reviewed: `59f1e605`, after merging current `origin/main` at `f73afcecdac25841b742204a0f05da947113f0da`, correcting its archived ADR-0028 reference to canonical ADR-0032, and aligning all direct Git-worktree fixtures with the existing filter-backend gate. The production implementation and native platform qualification head is `a28523f5a9bf3e89a03d21929d8616417eb1022f`.

Implementation commits:

- `65e72626` — accepted constrained profile handoff for Test/Python and managed Test-runner ownership.
- `a9b88a8b` — keep Bubblewrap handshake code Linux-gated for macOS compilation.
- `f05c4b32` — include the required sandbox profile in Windows managed-request destructuring.
- `59aae2f6`, `4c00d5b5` — managed Test-runner cancellation regression and lint correction.
- `3399ab33`, `24593079` — align active routing ownership fixtures with accepted execution authority.
- `cf984c30` — update the remaining Git-origin and tool-registry fixtures to carry accepted authority.
- `b0778262` — preserve trusted Git commit identity as bounded data while global executable configuration remains disabled.
- `8347251c` — align direct Git network/reset/clean fixtures with the current fail-closed backend and authorization policy.
- `68527c5c` — align Git recovery fixtures with the contained-execution gate for merge and rebase.
- `865e26fe` — provision the trusted helper beside CI integration-test executables and keep the live audit Git operation on the allowed commit path.
- `bd0b842f` — add accepted FullHost receipts and matching configured profiles to the remaining identity/audit Git route fixtures.
- `395a4ebe` — align the Ctrl+N modifier regression test with the documented TUI contract: Ctrl+N belongs to panel-resize mode and is not `NewSession`.
- `699d7a17` — merge current mainline, including stronger typed Git clean/reset/remote policy fixtures and the shell test-safety guard.
- `a28523f5` — update the merged shell-policy corrective's historical ADR-0028 reference to canonical ADR-0032.
- `617409c5` — align the work-order managed-worktree integration fixture with the existing fail-closed `CleanSmudgeFilter` backend gate and assert no worktree materializes.
- `44fb0de0` — align the M007 work-order trajectory fixture with the same fail-closed disposition and verify three concurrent requests create no worktree.
- `20980103` — align the direct `tests/worktree.rs` integration fixture with the filter-backend refusal and verify the target is not registered or created.
- `59f1e605` — align `WorktreeService` unit fixtures with the same refusal while retaining reservation-store, lease-generation, and cleanup-safety coverage.

## 1. Executive finding

M004-C1 is complete. Typed Test and Python calls preserve accepted constrained authority through durable scheduler payloads and into `ManagedProcessService`. The process owner rejects a constrained request before target launch if its required filesystem sandbox is absent or broader than the accepted profile. WorkspaceWrite Test execution is demonstrated inside and outside the workspace; Python preserves its mode-specific ceiling, and ReadOnly write workflows are rejected before scheduler submission. The Test runner no longer owns a parallel child-spawn path: managed process streaming owns launch, timeout, cancellation, and descendant cleanup while the runner retains parsing and report behavior.

FullHost remains explicit and is not inferred from the tool name. Operator `/test`, direct human shell, plugins, Tool Programs, Git filters/hooks/remotes, and Linux Bubblewrap production promotion are separate scopes. No Linux production sandbox backend is promoted by this closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Validate accepted Allow authority and matching policy revision before submission | Python/Test executor and tool authorization tests; durable scheduler handoff fixtures | pass | Missing, denied, or mismatched authority is rejected. |
| Carry the accepted profile unchanged into the canonical process owner | Typed job payload fields and `ManagedProcessService` request ceiling tests | pass | The executor does not infer broader authority from route or tool name. |
| Refuse constrained execution when enforcement is unavailable or broader | Managed-process pre-launch ceiling and unavailable-helper regressions | pass | Refusal occurs before target launch; no FullHost fallback. |
| Keep ReadOnly from performing workspace writes | Python transform refusal and Test pre-submission cases | pass | WorkspaceWrite is required for these mutating workflows. |
| Preserve ordinary constrained Test/Python workflows | Python scheduler execution and live Test scheduler handoff tests | pass | Test/Python remain scheduler-owned and produce canonical records. |
| Preserve Test output/report, timeout, stall, cancellation, and persistence behavior | Test runner suite, managed cancellation regression, RunStore assertions | pass | Managed process owns the child lifecycle; domain parsing/reporting remains in the runner. |
| Demonstrate same-or-narrower WorkspaceWrite effects | Native sandbox containment fixture | pass | Child writes in workspace and cannot create sibling marker outside it. |
| Preserve active Bash route ownership under accepted authority | `command_routing_execution_ownership` integration suite | pass | Active Git, Test, Python, and search routes are exercised with an accepted FullHost receipt. |
| Keep Linux C006 scoped to a bounded candidate result | Exact-head Linux C006 hosted run | pass (bounded negative M003 disposition) | Bubblewrap test handoff passes; production M003 and Git C002/C003 remain gated. |

## 3. Production implementation evidence

- Python validates accepted authority, persists the accepted sandbox profile in the scheduler job, and passes it to the managed child request. Python's mode-specific filesystem policy remains the effective ceiling; ReadOnly transform requests refuse before scheduling.
- Test validates and persists the accepted profile. `TestJobExecutor` passes it into the domain runner, whose output stream is consumed from `ManagedProcessService`. Existing parsing, bounded/redacted logs, report persistence, run records, timeout, stall, and cancellation semantics remain in the runner.
- `ManagedProcessService` applies the accepted-profile pre-launch ceiling. A constrained profile with no required sandbox request or an unavailable helper is rejected without starting the command.
- Submission labels retain Python workspace/intent provenance under bounded key/value sizes while scheduler goal provenance retains precedence.
- Execution ownership, architecture, skill, and operator documentation describe the managed process owner and the independent operator routes.

## 4. Verification executed

### Commands run

```bash
cargo test --test python_scheduler_execution --locked
cargo test -p codegg --lib python_script
cargo test -p codegg --lib tool::test::tests --locked
cargo test -p codegg --lib test_runner::runner::tests --locked
cargo test -p codegg --lib managed_process --locked
cargo test --test sandbox_containment --locked -- --test-threads=1
cargo test --test command_routing_adversarial --locked
cargo test --test command_routing_execution_ownership --locked -- --test-threads=1
cargo test -p egggit --locked -- --test-threads=1
cargo test --test modifier_chords --locked
cargo test --test work_orders_m002_materialization --locked -- --test-threads=1
cargo test --test work_orders_m007_trajectory --locked -- --test-threads=1
cargo test --test worktree --locked -- --test-threads=1
cargo test -p codegg-core worktree_service::tests --locked -- --test-threads=1
cargo clippy -p codegg --lib --tests --locked -- -D warnings
scripts/verify.sh quick
python3 scripts/check_adr_integrity.py --self-test
python3 scripts/check_adr_integrity.py
python3 scripts/check_authorization_matrix.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
```

### Results

- Python scheduler execution: 11 passed; Python executor tests: 22 passed.
- Typed Test tool: 35 passed; Test runner: 17 passed, including managed cancellation.
- Managed process: 22 passed; sandbox containment: 15 passed.
- At the original implementation baseline, adversarial command routing passed 142, execution ownership integration passed 21, and `egggit` passed 91.
- Git network integration: 29 passed, including fail-closed assertions for operations requiring an unavailable contained backend or additional destructive authorization. Git mutation integration: 12 passed, including ordinary commit identity with global hooks disabled.
- Git recovery integration: 19 passed; merge/rebase continuation and abort attempts without the contained backend are refused while operation state remains intact.
- After merging current main, `egggit` passed 94/94 with serialized tests (the default parallel run exposed a global-config test interference); Git network/reset/clean passed 29/29, Git mutations 12/12, Git recovery 19/19, Python scheduler 11/11, typed Test 35/35, modifier-chord tests 15/15, work-order M002 materialization passed 10/10, M007 trajectory passed 14/14, direct worktree tests passed 14/14, `WorktreeService` unit tests passed 9/9, and `scripts/verify.sh quick` passed, including ADR integrity and the merged shell-test safety guard.
- Targeted Clippy and `scripts/verify.sh quick` passed. Quick verification included formatting, repository boundary and authority guards, the ADR integrity guard, and workspace all-target checking.
- ADR integrity self-test and normal integrity guard passed; active ADR identifiers are unique and local references resolve.
- Earlier hosted platform evidence on `b0778262f48b50e3629fc9a239af15d3ceb5f20a`:
  - Linux C006: [run 38638600698](https://github.com/dbowm91/codegg/actions/runs/38638600698) — bounded Bubblewrap protected-mount managed-child handoff, 1/1 passed.
  - Windows process launcher: [run 38638601461](https://github.com/dbowm91/codegg/actions/runs/38638601461) — launcher, setup-failure, restricted-process, and Windows policy-input checks passed.
  - macOS target compile: [run 38638601000](https://github.com/dbowm91/codegg/actions/runs/38638601000) — target compilation only; no native macOS runtime claim.
- Reconciled current-main source head `a28523f5a9bf3e89a03d21929d8616417eb1022f`:
  - Linux C006: [run 38650935114](https://github.com/dbowm91/codegg/actions/runs/38650935114) — Bubblewrap 0.9.0 protected-mount managed-child handoff passed 1/1.
  - macOS target compile: [run 38650935180](https://github.com/dbowm91/codegg/actions/runs/38650935180) — passed; compile-only, no native runtime claim.
  - Windows process launcher: [run 38650935131](https://github.com/dbowm91/codegg/actions/runs/38650935131) — passed its native launcher, suspended-target/setup-failure, restricted-process, ACL lease/recovery, and policy-input checks.
  - Full hosted CI on that production source head: [run 38650934892](https://github.com/dbowm91/codegg/actions/runs/38650934892) — guards, formatting, Clippy, and helper build passed; workspace tests then exposed stale worktree fixtures, corrected in the subsequent fixture-only commits listed above.
  - Final exact-head hosted CI: [run 38659312498](https://github.com/dbowm91/codegg/actions/runs/38659312498) on `59f1e605ac737fab55aa88a69676f2b068853225` — `verify` succeeded; 13,666 workspace tests passed and 5 were skipped.
- Hosted CI on `68527c5c` (run `38642582688`) exposed a missing trusted-helper fixture for integration executables and an audit trajectory that attempted a Git stage operation while filters remain gated. `865e26fe` builds and places the helper in the test executable directory and changes the audit trajectory to exercise the trusted commit path. Its run `38645315215` then identified two additional direct Git audit fixtures missing accepted authority; `bd0b842f` supplies accepted receipts with matching configured FullHost profiles. Run `38647345386` exposed the stale Ctrl+N assertion corrected in `395a4ebe`. After reconciling current mainline (`f73afcec`), run `38650934892` passed all guards, formatting, and Clippy but exposed a stale positive managed-worktree fixture; `617409c5` changes it to assert the typed unavailable disposition and absence of a created worktree. Rerun `38653537187` passed guards, formatting, Clippy, and helper build before exposing the same stale assumption in the M007 trajectory; `44fb0de0` corrected that fixture. Before later worktree targets were reached, review found a stale direct worktree positive and `20980103` changed it to assert refusal and no worktree creation. Run `38656334033` passed guards, formatting, Clippy, and helper build before exposing two stale positives in `WorktreeService`; `59f1e605` now pins the unavailable disposition while retaining reservation, lease-generation, and cleanup-safety coverage. Current exact-head CI is [run 38659312498](https://github.com/dbowm91/codegg/actions/runs/38659312498).

The hosted C006 lane is a bounded negative strict-M003 disposition. It does not register Bubblewrap as a production backend or close linked-worktree aliases, common-directory aliases, rename races, toolchain/LSP/PTY compatibility, startup timing, or descriptor-inheritance qualification.

## 5. Invariant review

- **Accepted authority:** profiles enter jobs only after accepted Allow and policy-revision validation; the accepted profile is carried unchanged.
- **No widening:** missing enforcement refuses before spawn; no raw shell, FullHost, or alternate-backend fallback is taken after denial.
- **Scheduler ownership:** Test/Python work remains `JobSubmissionService` → `JobScheduler` → executor → `ManagedProcessService`.
- **Effect ceiling:** WorkspaceWrite Test/Python behavior is bounded by the obtained filesystem request; ReadOnly write workflows are rejected.
- **Output and records:** streaming is bounded; raw logs remain redacted/non-model-safe; structured reports and canonical run records remain available.
- **Cancellation:** Test-runner cancellation reaches the managed process owner and terminates the target tree; report state is Cancelled.
- **FullHost:** explicit accepted FullHost behavior remains available and is not synthesized from a route label.

## 6. Failure and recovery review

- Missing, denied, stale, or mismatched authorization is rejected before scheduler submission or child start.
- Missing or unavailable filesystem enforcement is a terminal refusal; there is no weaker retry.
- Hosted CI initially exposed stale direct-call fixtures in active Bash routing tests. The authorization boundary was retained; fixtures were updated to supply accepted FullHost authority. The focused execution-ownership, Git-origin, and tool-registry suites pass with those receipts.
- Timeouts, stalls, scheduler cancellation, and Test cancellation use managed-process termination and descendant cleanup.
- Restarted accepted work retains its serialized profile; legacy payloads without the optional field keep legacy/operator semantics and are never interpreted as constrained authorization.
- Output and artifact sizes remain bounded; partial persistence does not trigger duplicate target execution or an uncontained retry.

## 7. Migration and compatibility review

The job payload profile is additive and optional for older/operator jobs. Absence does not imply constrained authority. No schema migration, protocol change, sandbox default change, or user preference migration was introduced. Operator `/test` remains an independent local job route. FullHost output contracts remain intact.

## 8. Security review

Accepted authorization and process-enforcement status remain distinct. Profile equality and the obtained sandbox request are checked at the process owner immediately before launch. Paths remain workspace-root validated; no secret-bearing command text is added to audit metadata or raw model-visible output. Missing enforcement fails closed. Network, IPC, host process, and seccomp isolation are not claimed by this filesystem-only evidence.

## 9. Documentation and operations

Architecture and tool-execution documentation describe the Test/Python managed-process handoff, execution-ownership metadata is synchronized, the implementation plan and roadmap carry this closure, and the authorization/ownership/ADR/Git guards pass. Linux C006 facts and the selected-worktree trusted-Git boundary remain documented in the Linux C006 closure and `research/linux-execution-isolation/m003-future-git-metadata-trust-boundary.md`.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Linux M003 compatibility and race qualification remains incomplete. | Bubblewrap is not production-admitted; protected Git workflows remain unavailable. | Continue the independent Linux M003 native matrix before backend promotion. |
| low | Operator `/test`, direct human shell, plugins, and Tool Programs use independent authority paths. | This closure does not claim constrained Test/Python parity for those surfaces. | Track those routes in their owning plans; no M004-C1 action remains. |

## 11. Roadmap disposition

M004-C1 is closed. The original M004 plan 007 remains historically closed for route inventory and fail-closed admission; this corrective completes constrained Test/Python execution. No downstream Git filter/hook/remote or Linux production-isolation plan is unblocked.

## 12. Registry updates

- Mark plan 008 implemented and M004-C1 closed in the command-authorization roadmap and registry.
- Mark the command-authorization subsystem roadmap closed for its registered M001–M004 and M004-C1 scope.
- Final exact-head hosted CI passed on `59f1e605`; the closure-only commit records this evidence without changing the implementation baseline.
- Leave M004 plan 007's closure record unchanged.
- Leave Linux M003 blocked and its C006 negative disposition intact; no Git C002/C003 plan is unblocked.
