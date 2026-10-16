# Agent Workstream Blackboard M012 — Session-Scoped Run Group Continuation

Status: implemented
Repository baseline: `2555b032ca91d49fbe7b8e1c38eac6a1b289ef87`
Source roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md`
Related evidence: M008 provider-backed S1 attempts in `plans/closure/agent-workstream-blackboard/008-isolated-results.md`
Applicable ADRs: ADR-0032; no new ADR is required because authority remains within the same already-authorized session and never crosses sessions or projects.
Primary class: authorization/lifecycle corrective

## 1. Objective

Allow a later top-level turn in the same durable session to inspect, wait for, and cancel a `Turn`-owned AgentRunGroup after the creating turn has ended, while preserving denial for other sessions and child runs.

## 2. Why this corrective is ready

M008 S1 showed that `wait_group` is capped at 30 seconds and the parent turn can end while children remain active. `AgentRunGroupOwner::Turn` stores both session and creating turn, and authorization currently requires both exact values. A later headless turn in the same authorized session therefore receives `group actor is not authorized`, leaving a live durable group without a continuation path. The creating turn remains valuable provenance, but should not be the only session-owner authority for group lifecycle operations.

## 3. Current implementation evidence

`AgentRunGroupService::authorize` requires an exact `session_id` and `turn_id` for turn-owned groups. `TaskTool` constructs the actor from the current top-level turn; child runs use a distinct run actor. M008's retained session events show the follow-up `wait_group` calls rejected with `group actor is not authorized`.

## 4. Invariants

- Only a top-level turn in the same durable session may access a turn-owned group.
- Other sessions, projects, and child run actors remain unauthorized.
- Run-owned groups retain exact `RunId` ownership.
- Creator `turn_id` remains persisted as origin/provenance and idempotency identity.
- No group operation changes tool, sandbox, approval, repository, or workspace authority.

## 5. Scope

In scope: `AgentRunGroupService` authorization, focused tests, architecture/skill docs, and registry/roadmap closure.

Out of scope: cross-session group discovery, group-ID-based authority, new protocols or migrations, run-control changes, shared-checkout execution, and M008 performance conclusions.

## 6. Required production changes

Separate turn-owned group access from run-owned access so a top-level actor with the exact owner session and a non-empty current turn ID can use group lifecycle APIs. Do not authorize a child run actor from matching session metadata. Preserve exact `RunId` ownership for run-owned groups.

## 7. Ordered work packages

1. Add a focused same-session later-turn test for status/wait and group cancellation.
2. Retain negative tests for a different session and a child `RunId` actor.
3. Update authoritative architecture and agent skill guidance.

## 8. Failure, cancellation, restart, contention semantics

Authorization occurs before any wait or cancellation mutation. Wait remains bounded by `MAX_GROUP_WAIT_MS`; repeated waits can resume from a later turn. A daemon restart preserves group and member records; the new turn is reauthorized using the durable session ID.

## 9. Compatibility and migration

No schema migration or protocol change. Existing owner-turn rows retain their origin turn ID and become accessible to later top-level turns only within the same session.

## 10. Required tests

Run the focused `codegg-core` AgentRunGroup service test covering original-turn success, same-session later-turn access, different-session denial, child-run denial, and run-owned exact-owner behavior.

## 11. Required verification commands

`cargo fmt --check`; the focused AgentRunGroup test; `git diff --check`; execution-ownership and scheduler-bypass guards.

## 12. Documentation updates

Update `architecture/agent.md` and `.opencode/skills/agent/SKILL.md` to state the durable session scope for turn-owned group lifecycle operations.

## 13. Acceptance criteria

A second authorized top-level turn in the same session can observe, wait for, and cancel an existing turn-owned group. A different session and a member child cannot. Run-owned groups remain exact-run scoped.

## 14. Stop conditions

Stop if the only feasible implementation expands access beyond the exact session or grants child runs owner control. Do not add a new authorization service, storage owner, or background poller.

## 15. Closure evidence required

Record exact implementation commit, focused test, relevant guards, same-session boundary proof, M008 downstream effect, and registry unblock audit.

## 16. Handoff notes

After closure, resume observation of M008's running S1 group from the same session if its children remain active; otherwise start a fresh bounded trial. The group must still complete successfully before any comparative effectiveness result is reported.
