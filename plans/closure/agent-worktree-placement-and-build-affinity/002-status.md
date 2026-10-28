# Agent Worktree Placement and Build Affinity A002 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-worktree-placement-and-build-affinity/002-existing-managed-read-only-placement.md`
Source subsystem roadmap: `plans/subsystems/agent-worktree-placement-and-build-affinity-corrective-addendum.md#milestones-and-boundaries`
Repository baseline reviewed: `493e5ee111d133eb1cb99debddcc466122c7f7f9`
Implementation commits:
- `0ac17d38c` — typed placement, daemon authorization, lease lifecycle, tool restriction, projection, documentation, tests, and A004 harness.
- `271fd8d56` — structural guard for scope/owner checks and the read-only allowlist.
- `4ee3c07b5` — production-scheduler refusal matrix and typed missing/occupied-worktree classifications.

Closed conditionally: 2026-10-28

Primary class: capability

## 1. Executive finding

The A002 capability is implemented and its successful path is proven through the production scheduler on Apple Silicon macOS. A delegated child selected a pre-existing managed checkout by exact ID, read the checkout's content rather than the parent's, persisted the selected worktree/read-only flag, completed without allocating another checkout, released its exact lease generation, and left both checkout and parent contents unchanged. The same scheduler trajectory now covers the named refusal matrix locally.

A002 is **strictly closed**. The exact-head hosted Linux job passed the production-scheduler success and refusal matrix on `89076650f`. No Windows containment or platform-support claim is made. The fixture's worktree creator is test-only, so Linux evidence qualifies scheduler behavior, not native containment.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed exact WorktreeId placement per child | RunPlacement serde/parser, spawn_many shape/fingerprint test | pass | ID is identity, not path or authorization |
| Production scheduler success path | `existing_managed_read_only_runs_through_the_production_scheduler` | pass | JobSubmissionService → JobScheduler → SubagentJobExecutor → WorktreeService |
| Selected checkout is effective read root and retained | Test reads distinctive README; verifies selected content, parent unchanged, one worktree only | pass | No cleanup or extra checkout |
| Exclusive run-owned lease and release | `read_only_lease_is_exclusive_and_releases_without_cleanup` | pass | Second lease and ordinary writer lease both conflict; release retains Ready checkout |
| Production-scheduler refusal matrix | `existing_managed_read_only_runs_through_the_production_scheduler` | pass locally | Missing ID, dirty, unmanaged, and foreign project/repository/workspace refuse as unavailable; held lease, active session, and active job refuse as in-use |
| Effective read-only tool surface and no delegation | Child registry retains only `read`, `list`, `glob`; task tool is disabled; guard self-test rejects widened tools and re-enabled delegation | pass for enforced surface | Local production read trajectory verifies the selected checkout read |
| Hosted Linux production-scheduler qualification | Run `40697779631`, exact head `89076650f` | pass | Test-only creator; no native containment claim |

## 3. Production implementation evidence

The typed `ExistingManagedReadOnly { worktree_id }` mode is carried through per-child spawn/spawn_many, durable payload/fingerprint, AgentRun persistence and projection. The daemon locks the repository while it resolves checkout binding and ownership; validates same project/repository/workspace, managed root, clean health, Ready state and no current owner; checks non-archived sessions and every non-terminal scheduler job in the linked workspace; then takes an exclusive run-owned lease before provider execution. The child's workspace root is rebound to that checkout. Unknown effects are excluded by retaining only read/list/glob and disabling nested delegation. Every pre-dispatch failure and terminal path releases only the acquired generation; the checkout remains.

## 4. Verification executed

- `cargo test --target aarch64-apple-darwin --test agent_loop workspace_root_execution_authority::existing_managed_read_only_runs_through_the_production_scheduler --locked -- --exact`: **1 passed**.
- `PATH=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin:$PATH CARGO_BUILD_TARGET=aarch64-apple-darwin CARGO_TARGET_DIR=/tmp/codegg-a002-aarch64-target python3 scripts/check_test_filter_execution.py --run -- cargo test --locked --test agent_loop existing_managed_read_only_runs_through_the_production_scheduler -- --nocapture --test-threads=1`: **1 passed, 1 registered test witnessed, no self-skip**; covers missing, dirty, busy, foreign project/repository/workspace, unmanaged, active-session, and active-job cases through the production scheduler.
- `python3 scripts/check_test_assertion_quality.py`, `python3 scripts/check_test_duplication.py`, and `python3 scripts/check_existing_worktree_read_only.py` (including `--self-test`): passed after adding the refusal matrix.
- `cargo test -p codegg-core --lib read_only_lease_is_exclusive_and_releases_without_cleanup --locked`: **1 passed**.
- `cargo test --target aarch64-apple-darwin --test core_daemon checkout_reservation_c004::an_alias_path_resolves_to_the_same_reservation --locked -- --exact`: **1 passed**.
- `cargo test --target aarch64-apple-darwin -p codegg --lib placement_rationale_preserves_conservative_authority_boundaries --locked`: **1 passed**.
- `python3 scripts/check_existing_worktree_read_only.py --self-test` and normal guard: passed. The self-test rejects widened tool allowlists, re-enabled delegation, and omission of the active-job scan.
- `scripts/verify.sh quick`: passed locally on source commit `89076650f`; 357 Harness Lab tests (356 passed, 1 skipped), all quick guards and workspace all-target check passed.
- `CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings`: passed locally on source commit `89076650f`.
- `python3 scripts/check_test_assertion_quality.py`, `python3 scripts/check_test_duplication.py`, `python3 scripts/rust_test_scan.py --validate`, `python3 scripts/check_job_state_terminality_agreement.py`: passed.
- Formatting, comment-budget, and whitespace checks passed.

## 5. Invariant review

No arbitrary checkout path is accepted. The model-supplied WorktreeId does not authorize itself. Existing managed placement does not create, reset, merge, clean up, or delete the checkout. It does not expand writer permissions, process tooling, group-sharing authority, or concurrent-reader support. Default Auto, isolated-new, and serialized-shared semantics remain. Missing durable scheduler authority refuses explicit placement instead of silently ignoring it.

## 6. Failure and recovery review

Worktree service storage or catalog reads refuse on uncertainty. An occupied target loses the atomic lease claim; generation checks prevent a late run from releasing a newer owner's lease. Pre-dispatch and terminal paths release only this run's lease. Same-user external processes are outside the contract. The production scheduler test exercises successful release; the core lease test exercises loser refusal and retained checkout.

## 7. Migration and compatibility review

The placement enum defaults old payloads to Auto. Additive schema migrations v92/v93 persist the read-only worktree marker and replay projection. Existing session binding and prior placements remain unchanged.

## 8. Security review

A002 narrows to read/list/glob at tool construction and sets the ReadOnly sandbox profile. Mutating, process, dynamic tool and nested task surfaces are absent. Checkout mediation, process admission and writer-permit authorities refuse while the exclusive inspection lease is held. This record does not claim isolation from arbitrary same-user OS processes or native filesystem containment for this inspection-only profile.

## 9. Documentation and operations

Updated `architecture/agent.md`, `architecture/worktree.md`, `architecture/writer_permit.md`, `docs/tools.md`, task schema and A002 plan. The routine guard is wired into quick and CI and mutation-tests the core authorization/allowlist shape.

## 10. Unresolved findings

- **Low:** Windows behavior and arbitrary same-user OS writers are not qualified and are outside the current claim.

## 11. Roadmap disposition

A002 is closed for the implemented, locally and hosted Linux-verified behavior. No broader host claim is made.

## 12. Registry updates

- Move A002 to closed work; exact-head hosted Linux run `40697779631` passed.
- Keep A005 blocked until a supported CodeGG live-model route has an enforceable spend cap under the already authorized 240-request / 60-minute / $10 limits.
