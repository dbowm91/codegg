# Agent Tool-Use Operability Corrective C007 — Follow-up Closure Status

Status: closed
Source plan: `plans/implementation/agent-tool-use-operability-post-audit-corrective/007-real-agent-tool-trajectory-qualification.md`
Source subsystem roadmap: `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md#7-milestones`
Historical blocker record (unchanged): `plans/closure/agent-tool-use-operability-post-audit-corrective/007-status.md`
Implementation revisions: CodeGG `c23951db96e93d9ddf9cc022f98d03b336df93ac` (host-scoped retrieval and broker dispatch), `928609dd0cc44bb44400d92df98d05b9e449af7e` (subagent workspace qualification), `a8068df9e62bb5dda10c6d9bac99080601ae3e04` (AgentLoop verifier/todo integration). C002 strict follow-up closure: `a97085d9c4f51bea045dd1f685bdd5657672570a`.
Eggsearch: v0.4.4, exact source commit `32f48bb57a43274d763c7f9ba919eab84d125edc`.
Recommendation: strict closure of the C007 production-path qualification. No real-model endurance claim is made.

## 1. Finding and disposition

C007's normal production paths now have scripted AgentLoop evidence across local retrieval, first-use editing, scoped verification, explicit multi-active todos, restricted Tool Programs, durable task lookup, and concurrent parent/subagent work. The pinned Eggsearch process-pair and AgentLoop tests exercise accepted workspace scope through the ordinary ToolBroker path; no model-supplied root or live provider is used.

The resumed C002 closure supplies the workspace-to-Eggsearch authority contract that blocked this milestone. C001–C006 and C008 are also strictly closed. The final AgentLoop addition covers a previously missing broker-path combination for scoped `verify(lint)` and two simultaneous in-progress todo items. The verifier fixture uses a private temporary Cargo home so the test proves workspace verification without reading the operator's Cargo configuration.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| Local discovery is bound to the accepted session root; no web fallback is presented as local | `pinned_eggsearch_agent_loop_uses_each_accepted_workspace_root` and `pinned_eggsearch_process_pair_is_compatible_offline`; two concurrent roots, A→B→A reuse, spoofed model scope ignored, local-only telemetry | Pass |
| First-use file edit and grep run through AgentLoop and broker | `first_use_apply_patch_create_then_grep_runs_through_agent_broker`; unique fixture file is created and then found without a read prerequisite | Pass |
| Scoped project verification reaches the model with truthful outcome | `verify_and_two_explicit_in_progress_todos_reach_the_model_through_agent_loop`; real `verify(lint)` executes against a temporary Cargo package through the production registry and broker | Pass |
| Multiple explicit tasks can be in progress and are visible | Same verifier/todo AgentLoop test writes two `in_progress` items in one call and verifies state plus model-visible result | Pass |
| Restricted Tool Program foreground/background paths and denial use the broker/scheduler | `tool_program_foreground_and_background_run_through_agent_broker_and_scheduler`; asserts terminal jobs, model-visible receipts, explicit Deny creates no job, and missing/mismatched/revoked/expired authority is refused | Pass |
| Durable child status/get/group lookup is model-visible and owner-scoped | `task_tool_status_get_group_and_cross_session_contracts_are_model_visible`; seeded durable completed run, status/wait/get/group results, cross-session denial without result disclosure | Pass |
| Concurrent use and queue behavior stay bounded | AgentLoop parent/subagent permit overlap and deadline-race tests; C006 closure's queue cancellation/accounting, native timeout/partial-output, and permit-width evidence | Pass |
| Negative authority, containment, malformed edit, and no-index cases remain truthful | AgentLoop deny/invalid authority tests; tool execution traversal/symlink tests; pair no-index/no-match/disabled-local tests; predecessor C001–C006 closure matrices | Pass |
| No unapproved permission widening, replay, or live public network call | Explicit Deny remains denied, C006 cancellation does not execute later, pair uses Eggsearch `mode=off` and records zero remote attempts | Pass |

The production-path evidence is a suite of focused scripted trajectories using the existing AgentLoop, registry, broker, scheduler, durable stores, and pinned child fixtures. It does not claim that one model session autonomously completes an endurance benchmark; each stateful and scheduled contract has its owning deterministic trajectory.

## 3. Verification

Passed:

- `cargo test --quiet -p codegg --test agent_loop_harness --locked` — 67 passed, 1 intentionally ignored (the ignored test is run by the pinned pair script).
- `cargo test -p codegg --test tool_execution --locked` — 61 passed.
- `cargo test -p codegg --test eggsearch_pair_compat --locked` — compiled; the one real-child test is intentionally ignored unless driven by the pair script.
- `scripts/test-eggsearch-pair.sh` with Eggsearch v0.4.4 built from exact commit `32f48bb57a43274d763c7f9ba919eab84d125edc`, SHA-256 `21f01a76d32277fab4346dc1d55452fc55bd024f44456d2d657b63d819adafa2` — real child, real AgentLoop concurrent A/B/A and subagent trajectories, and `scripts/verify.sh quick` passed.
- `cargo fmt --all -- --check` and `git diff --check` — passed.

No hosted CI, live-provider request, real model, release publication, Windows-only behavior, or SBC benchmark was needed or claimed. Existing predecessor closure records remain the authority for their individual tests; they were not rewritten.

## 4. Security, compatibility, and failure behavior

The accepted workspace root remains the only local retrieval authority. Model output and repository snippets remain untrusted. The pinned real child returns no other workspace's marker and does not attempt remote providers in local-only mode. Tool Program execution remains scheduler-owned and permission-bound. Explicit Deny, invalid receipts, workspace containment, stale task ownership, and queued cancellation remain covered by production-path tests and closed predecessor evidence.

No storage migration, new scheduler, permission policy, public MCP API, upstream live provider, or release workflow was added for C007. C008's host scope and Eggsearch 0.4.4 compatibility floor remain unchanged.

## 5. Limitations and severity

No in-scope C007 failure remains. These are scripted provider and local-process qualifications, not evidence of live-model reliability or long-duration SBC performance. The temporary Cargo package contains no third-party dependencies; the test validates the semantic verifier route and scoped command execution, not a full repository clippy-duration claim.

## 6. Unblock audit

C001–C006, C008, and now C007 are strictly closed. No registered plan in this corrective line remains blocked or became newly eligible from C007 closure. Separate Eggsearch evidence-workflow plans and any broader live-provider/endurance qualification remain independently owned and are not silently marked complete.

Historical C002 blocker evidence remains at `002-status.md`, and historical C007 blocker evidence remains at `007-status.md`. This follow-up record contains the resumed C007 evidence.

## 7. Registry and roadmap disposition

`plans/implementation/agent-tool-use-operability-post-audit-corrective/007-real-agent-tool-trajectory-qualification.md` is implemented and strictly closed. `plans/registry.md` and the source addendum mark C007 closed and the corrective line complete.
