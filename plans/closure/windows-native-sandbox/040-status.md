# Windows Native Sandbox C040 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/windows-native-sandbox/040-pr111-mainline-integration-and-merge-readiness.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#post-adr-0026-pr-111-mainline-reconciliation`
Feature branch: `work/windows-native-sandbox-foundation`
Integrated main: `b9ba494c1b21638c2e2f660301ba207ce8859682`
Feature branch before merge: `a00a766af7ebdd7db770182ac47e4ddc824dd9d8`
Common base: `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`
Integration commit: `b89ba9e364bbf492d19048bed54803c4a8fd6d8e`
PR: [#111](https://github.com/dbowm91/codegg/pull/111)

## 1. Executive finding

C040 is closed as a mainline integration and merge-readiness handoff. Current
`origin/main` was integrated into the existing feature branch with an ordinary
merge commit; published branch history was not rewritten. The sole textual
conflict, `plans/registry.md`, was resolved semantically with both the mainline
TUI rows and Windows/shell corrective rows preserved. The result is pushed to
the feature branch, globally unique active ADR identities pass the integrity
guard, and GitHub reports PR #111 mergeable with a clean merge state.

PR #111 remains draft and was not merged into `main`. C040 does not qualify or
admit Windows production containment. `SandboxRequest::Required` remains
unavailable/fail-closed, filesystem/network `Enforced` remains unclaimed, and
no original-user ACL mutation, privileged broker or guest, or security
architecture decision occurred.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Integrate the refreshed mainline without rewriting published history | Merge commit `b89ba9e` has first parent `a00a766` and second parent `b9ba494`; pushed with a non-forced fast-forward update | Pass |
| Reconcile registry without losing either workstream | `plans/registry.md` retains mainline TUI entries, Windows milestones and the shell-policy corrective; C040/C001 closures are registered | Pass |
| Eliminate cross-branch ADR collision | `python3 scripts/check_adr_integrity.py` and its synthetic merged-tree fixture pass; TUI keeps ADR-0021/0022 and Windows uses ADR-0023–0026 | Pass |
| Import and validate shell policy correction | Mainline `9d32c35` rewrite is present; direct Ask classification plus the actual agent-loop approval/no-dispatch test pass | Pass; see C001 closure |
| Run integrated Linux verification at exact merge head | [CI run 38435606749](https://github.com/dbowm91/codegg/actions/runs/38435606749), SHA `b89ba9e` | Pass |
| Run required native Windows workflows at exact merge head | [Process launcher 38435598282](https://github.com/dbowm91/codegg/actions/runs/38435598282), [native probe 38435598288](https://github.com/dbowm91/codegg/actions/runs/38435598288), [native probe PR run 38435606473](https://github.com/dbowm91/codegg/actions/runs/38435606473), [dedicated principal 38435606574](https://github.com/dbowm91/codegg/actions/runs/38435606574), [host eligibility 38435606951](https://github.com/dbowm91/codegg/actions/runs/38435606951) | Pass; test/research evidence only |
| Verify PR readiness without merging | GitHub reports `mergeable=MERGEABLE`, `mergeStateStatus=CLEAN`, `isDraft=true` at head `b89ba9e` | Pass; draft PR intentionally unmerged |

## 3. Merge and scope review

The refreshed merge preview found only `plans/registry.md` modified on both
sides. The ordinary merge preserved mainline TUI editor and directory-listing
planning and implementation, Windows security history and negative/blocked
roadmap state, and the shell-policy corrective. The merge commit has parents
`a00a766` (feature) and `b9ba494` (main).

The resulting feature diff against current `main` contains the existing Windows
feature line and its registered corrective records. No production sandbox
admission or PR merge was performed. The active ADR namespace is collision
free; the historical Windows ADR aliases remain traceable under
`plans/archive/windows-adr-id-aliases/`.

## 4. Verification executed

On the integration tree before merge commit creation:

- `cargo fmt --all` — passed.
- `cargo test --locked --test agent_loop_harness shell_escalation_ask_waits_for_explicit_approval_before_dispatch` — passed (1 test).
- `cargo test --locked --test tool_execution` — passed (59 tests).
- `cargo test --locked --test permission` — passed (44 tests).
- `cargo test --locked --test approval_router` — passed (17 tests).
- `cargo test --locked --test command_routing_adversarial` — passed (139 tests).
- `cargo test --locked --test windows_native_probe_contract` on Linux — zero tests due Windows cfg; not counted as native evidence.
- `bash scripts/verify.sh quick` — passed, including workspace check and repository guards.
- `python3 scripts/check_adr_integrity.py` and `git diff --check` — passed.
- Final `bash scripts/verify.sh quick` on the closure/registry documentation tree — passed, including formatting, repository guards and `cargo check --workspace --all-targets --locked`.

Hosted checks on exact integrated SHA `b89ba9e`:

- [Linux CI 38435606749](https://github.com/dbowm91/codegg/actions/runs/38435606749) — success; repository guards, formatting, Clippy and workspace tests passed.
- [Windows process launcher 38435598282](https://github.com/dbowm91/codegg/actions/runs/38435598282) — success; required managed-process, failure/recovery, root-adversarial, AppContainer-ordering and Windows policy-input stages passed. The separate C014 negative-reproduction job was skipped by its workflow condition and is not claimed as a pass.
- [Windows native probe contract 38435598288](https://github.com/dbowm91/codegg/actions/runs/38435598288) and PR-triggered duplicate [38435606473](https://github.com/dbowm91/codegg/actions/runs/38435606473) — success.
- [Windows dedicated-principal feasibility 38435606574](https://github.com/dbowm91/codegg/actions/runs/38435606574) and [MXC host eligibility 38435606951](https://github.com/dbowm91/codegg/actions/runs/38435606951) — success as test-only feasibility/host evidence, not security qualification.

## 5. Security and compatibility review

C040 changes integration and planning records only, with the C001 test addition
described in its own closure. No production permission or Windows sandbox
behavior changed. The Windows runs validate existing test-only paths and do
not prove that a restricted child reaches user entry or denies reads to
arbitrary host data. No production Windows containment capability is enabled.

## 6. Unresolved findings and handoff

No C040 integration blocker remains. C011/M004–M006 and C015–C018 retain their
existing blocked state; C030/C031 remain terminally blocked; C029/C034/C038
no-admission dispositions are unchanged. Any future Windows production
candidate still requires a new accepted architecture decision and the
independent positive security/lifecycle evidence specified by the roadmap.

## 7. Roadmap disposition

C040 is closed. Shell corrective C001 is also closed at this integrated head;
its behavior and evidence are recorded in
`plans/closure/shell-command-policy-ci-corrective/001-status.md`. No Windows
capability or downstream security milestone was unblocked.

## 8. Registry and PR disposition

The registry and both subsystem roadmaps record C040/C001 closed. The branch
was pushed without force. PR #111 is mergeable and clean, but remains draft and
unmerged as required by the plan. The user-facing release/security posture is
unchanged.
