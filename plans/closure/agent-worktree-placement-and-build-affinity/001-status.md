# A001 — Explicit Isolated Placement Contract — Closure Record

Plan: `plans/implementation/agent-worktree-placement-and-build-affinity/001-explicit-isolated-placement-contract.md`
Roadmap: `plans/subsystems/agent-worktree-placement-and-build-affinity-corrective-addendum.md`
Baseline: `80171f45295bebb9ab0b521262c0b295acb93074`
Implementation branch head: `5a479cea9924205c8828dcbeef72a40bf3391f63` (worktree changes remain uncommitted)
Implementation commit/PR: none; source and closure edits are uncommitted in the requested branch worktree.
Closed: 2026-10-27
Class: invariant
Status: **conditionally closed**
Recommendation: **conditionally closed**; keep A002 blocked until the named live-success condition passes.

## 1. Requirement → evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| Explicit `IsolatedNew` enters checkout preparation even for read-only tool profiles | `RunPlacement::requires_new_isolated_checkout()` participates in the executor preparation predicate and isolated creation branch | met in implementation |
| Missing repository, service, creator, create, or linkage failures are typed and terminal | failure classes distinguish explicit isolation; negative executor regression verifies terminal failure and `explicit_isolation_unavailable` | met |
| `Auto` and `SerializedShared` remain distinct | placement-specific predicate and existing M008 shared-placement trajectory; 15 focused `agent_loop` tests passed | met in focused coverage |
| Read-only explicit placement succeeds through live scheduler dispatch and produces a distinct contained managed checkout | The available regression uses `SubagentJobExecutor` and `WorktreeService::memory`; it proves refusal without a configured `ContainedWorktreeCreator`, but does not prove successful production creator dispatch | **open condition** |

## 2. Implementation and verification

The executor now treats explicit isolation as user intent independent of
whether the agent's denied-tool set would otherwise imply isolation. It refuses
repo-less requests and configured creator failures rather than executing in an
inherited root. The test first queues the durable run, matching scheduler
dispatch, then verifies a missing creator records a terminal failure.

- `cargo nextest run -p codegg --test agent_loop --locked --profile ci -E 'test(explicit_read_only_isolated_new_fails_closed_without_contained_creator) | test(blackboard_trajectory_m008) | test(workspace_root)'`: **15 passed**.
- Shared-checkout guard self-test and normal check passed.
- `scripts/verify.sh quick` passed, including all repository guards, 354 harness tests (1 skipped), and workspace all-target check.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- `git diff --check` passed.
- `cargo nextest run -p codegg --test contained_worktree_c008 --locked --profile ci -E 'test(the_creator_reaches_the_launch_instead_of_failing_on_its_own_paths)'`: **1 passed** on the current host. This regression deliberately accepts a typed containment refusal and only proves the creator reaches the launch stage without rejecting its own not-yet-created paths; it does not assert that this host obtained the required containment tiers or that scheduler dispatch succeeded.

## 3. Condition preventing strict closure

Strict closure requires a production-shaped live scheduler run in which a
read-only `IsolatedNew` request obtains a contained managed checkout, with two
children shown to have distinct roots and no parent-root access. The passing
creator regression is intentionally host-independent and does not require
successful containment; the scheduler regression uses an in-memory service
without a configured contained creator and proves refusal only. The
implementation does not register an uncontained creator to make the success
test pass. A002 remains blocked until this condition is discharged on a
qualified backend, with the host/backend qualification named.

Compatibility: no payload variant or migration was added. Legacy payloads
continue to decode to `Auto`; the explicit `IsolatedNew` behavior changes only
when that placement is actually requested.

## 4. Unblock audit

A001 is conditionally closed and does **not** satisfy A002's strict-closure
dependency. A003 is independent and is recorded separately.
