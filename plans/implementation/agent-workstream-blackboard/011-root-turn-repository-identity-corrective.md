# Agent Workstream Blackboard M011 — Root Turn Repository Identity Corrective

Status: implemented; closure evidence recorded in `plans/closure/agent-workstream-blackboard/011-status.md`.

Repository baseline: `809f1f5073d0fa4f1c61c796c1edab0b3e606fc0`
Source roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md`
Related closures: M006 `plans/closure/agent-workstream-blackboard/006-newmanaged-materializer-addendum.md`; M008 evidence `plans/closure/agent-workstream-blackboard/008-isolated-results.md`
Long-term requirements: `plans/000-long-term-specification.md` §§16, 26–29; `plans/001-terminology-and-domain-model.md` §§5, 7–8
Applicable ADRs: ADR-0002, ADR-0032, ADR-0035
Primary class: invariant/capability corrective

## 1. Objective

Preserve the resolved repository identity of a managed session workspace when constructing the root turn runtime, so delegated mutating tasks can use M006's isolated managed-worktree path.

## 2. Why this corrective is ready

M008's provider-backed isolated trajectory exposed that `TurnRunInput.repository_id` was always `None`. `TaskTool` consequently persisted repository-less child tasks, and the scheduler executor could not allocate a distinct managed checkout. M006 had closed its session launch/materialization boundary, but that closure did not exercise repository identity propagation from a root turn into delegated tasks. This bounded propagation correction does not depend on M005 or shared-write execution.

## 3. Current implementation evidence

`CoreDaemon` resolves the authoritative session workspace binding through `ProjectStorage` and passes its repository ID into `TurnRunInput`. A daemon-turn regression test constructs a repository-backed session and asserts the runtime receives the resolved ID.

## 4. Invariants

- Repository identity comes from the persisted workspace binding, never a caller path or prompt.
- Repository-less sessions retain repository-less behavior.
- A delegated repository-backed task receives the identity required to provision its own managed worktree.
- This correction grants no shared-checkout write authority.

## 5. Scope

In scope: root daemon turn construction, focused regression coverage, architecture/skill documentation, and plan/registry reconciliation.

Out of scope: M005 writer containment, changes to child placement policy, broad delegation redesign, or M008 performance conclusions.

## 6. Required changes

Resolve the binding for the runtime workspace and carry the resolved repository ID into the typed turn input. Preserve `None` for absent/unresolved bindings. Document the authoritative identity path.

## 7. Work package

Add the production propagation and a test that observes the value at the `TurnRuntime` boundary. Review `TaskTool`/executor behavior to confirm this is the field used for managed worktree allocation.

## 8. Failure and recovery semantics

This lookup is observational and does not mutate the workspace/session binding. Existing repo-less behavior remains the fallback for absent or unresolved bindings. No checkout is created by turn construction itself.

## 9. Compatibility

No protocol, storage schema, or CLI/TUI contract changes.

## 10. Required tests

Run the focused daemon-turn regression and formatting/diff validation. Record any broader trajectory evidence separately; do not count an incomplete provider child as success.

## 11. Required verification

`cargo fmt --check`; focused `core::daemon_turns::tests::repository_binding_reaches_turn_runtime_for_isolated_delegation`; `git diff --check`; relevant authority and execution ownership guards.

## 12. Documentation

Update `architecture/agent.md`, `architecture/core.md`, and the corresponding agent/core skills to describe workspace-binding-derived repository identity.

## 13. Acceptance

The regression proves a resolved workspace repository ID reaches `TurnRuntime`; source inspection confirms `TaskTool` forwards it to the task record consumed by the managed-worktree executor. M008 records the live provider trajectory status without overstating results.

## 14. Stop conditions

Do not infer a repository from filesystem names, remote text supplied by an agent, or prompt content. Do not enable shared-checkout mutation to work around missing identity.

## 15. Closure evidence

Record exact implementation commit, focused test result, static source path, documentation updates, and registry unblock audit.

## 16. Handoff

M008 remains active for provider-backed S0 and multi-agent S1/S2/S3 qualification. M005 remains separately gated on complete writer mediation and native lifecycle evidence.
