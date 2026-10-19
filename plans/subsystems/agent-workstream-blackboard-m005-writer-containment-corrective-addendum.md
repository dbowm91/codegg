# M005 Writer-Containment Corrective Addendum — Restricted Shared Checkout

Status: active planning; C001 closed; C002 active (ADR-0042 accepted, option B); C003–C006 blocked. SerializedShared remains unavailable.

Planning baseline: branch plan/agent-workstream-m005-writer-audit inspected 2026-10-19, main ancestor 61cbe9222c442c443cc7dfb3bd3a5b7d99ecd649. Revalidate exact branch head and mainline status before any implementation.

Authoritative sources:
- plans/003-planning-process.md §§4–10 (corrective and closure governance)
- plans/000-long-term-specification.md §§9, 16–17, 26–29
- plans/001-terminology-and-domain-model.md §§5, 7–8
- plans/subsystems/agent-workstream-blackboard-roadmap.md (parent M005 stays active)
- plans/implementation/agent-workstream-blackboard/005-group-worktree-placement.md (Q0, Q1–Q5, audited sinks, 2026-10-19)
- plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md (accepted)
- plans/adrs/ADR-0032-agent-workstream-coordination-and-shared-checkout-ownership.md (accepted architecture)
- plans/adrs/ADR-0042-restricted-agent-shared-checkout-writer-contract.md (**accepted 2026-10-19**, option B — restricted CodeGG-managed coordination; does not enable shared mutation)

## Purpose and root cause

The original M005 scope did not bound the meaning of checkout writer exclusion or enumerate every effectful owner. The 2026-10-19 audit demonstrated that model ToolBatchExecutor coordination is intra-batch only, WorktreeService leases are not all-writer mutexes, scheduler resource keys are global and omit inline or human/HTTP writers, and some CodeGG-managed writers bypass ToolBroker. PTYs and LSP/MCP/plugin services persist beyond a tool request; Unix children may detach with setsid and survive group teardown. External same-user programs never participate in CodeGG locks. Previously closed test suites focused on isolation, process cleanup and peer coordination; they did not test a cross-run cross-surface shared-write permit. Q0 verifies the safe isolated default; Q1–Q5 remain unrun.

This corrective line addresses the core **writer-containment contract** before any attempt to enable shared checkout. A restricted CodeGG-mediated coordination profile is the recommended feasible direction. ADR-0042 was proposed for the scope change and required explicit approval; that planning was not itself approval. **ADR-0042 option B was accepted on 2026-10-19**, which unblocks C002 only.

## Ownership and invariants

AgentRun/Group and WorktreeService remain canonical for run, reservation and lease ownership; Git mutation/RunStore/document/file services remain owners of their effects; JobScheduler alone admits heavy work. One daemon-owned typed writer authority may coordinate final effects without becoming a scheduler or a replacement Git service. Authorization comes from project/principal/run/group/attempt records and current sandbox/tool rights, never agent text, filenames, WorkstreamClaim or model/client-supplied generations.

Shared profile is opt-in, conservatively disabled, with serial CodeGG writers and concurrent proven reads. It does **not** prevent edits by unrelated same-user programs; if external-writer exclusion is required, stop and start independent OS isolation feasibility work. All CodeGG-controlled project-root writers must take the common permit or refuse before effect; there is no silent human/HTTP/editor exemption. Persistent and arbitrary subprocess capability is denied until native lifetime proof exists. Permit expiry cannot release an uncertain writer. No default shell regex blocker, second scheduler, generic workflow framework, new CI lane, unbounded polling, automatic merge, or compulsory Cargo target/cache sharing.

Independent M006 isolated session/worktree launch, M007 per-workspace build admission and M008 isolated-only experiments remain eligible on their own original gates; this corrective does not block them.

## Dependency graph and handoff

    M005 audit / safe Q0 closed
       |
       +-- C001 writer-policy decision + complete allow/deny inventory [CLOSED]
               |
               +-- explicit acceptance of proposed ADR-0042   [HARD GATE — SATISFIED
                   |                                            2026-10-19, option B]
                   +-- C002 durable fenced checkout permit Q1 [ACTIVE]
                           |
                           +-- C003 native file/Git writers Q2 [BLOCKED]
                                   |
                                   +-- C004 non-broker sink mediation Q3 [BLOCKED]
                                           |
                                           +-- C005 process denial /
                                           |        lifecycle gating Q3/Q4 [BLOCKED]
                                           |
                                           +-- C006 restricted end-to-end
                                                    Q1–Q5 / opt-in [BLOCKED: C004+C005]

C001 may document and test negative current behavior without accepted ADR-0042. C002 implementation may now proceed under accepted ADR-0042, but no C002 implementation or product enablement may proceed on an unaccepted decision, and acceptance alone still enables nothing: C002–C006 must land and qualify before any shared mutation exists. C006 must not claim M005 strict closure if it cannot demonstrate a production path and native evidence.

## Milestone contracts

| Milestone | Primary class | Status | Objective | Hard gate |
|---|---|---|---|---|
| C001 | invariant / decision contract | closed | Explicit boundary proposal, canonical effect-sink inventory, pre-effect deny matrix and handoff decision record | M005 audit |
| C002 | infrastructure / invariant | active | One durable CAS/generation-fenced permit with quarantine, contention and restart tests (Q1) | C001 closed + ADR-0042 accepted — **both met** |
| C003 | invariant / capability | blocked | Final-effect file and typed Git write interposition without double acquisition/deadlock (Q2) | C002 closed |
| C004 | invariant / capability | blocked | Every other CodeGG project-root writer either holds permit or refuses; includes human CodeGG file APIs and workspace-local state (Q3) | C003 closed |
| C005 | invariant / capability | blocked | Prevent unqualified direct/persistent process launch pre-effect; per-OS negative/proven lifecycle matrix (Q3–Q4) | C004 closed |
| C006 | capability / closure | blocked | Real two-AgentRun restricted checkout, full Q1–Q5, restart, Git provenance, honest supported-OS opt-in or negative disposition | C004+C005 closed |

Implementation handoffs: plans/implementation/agent-workstream-m005-writer-containment-corrective/001-*.md through 006-*.md. Closure records: plans/closure/agent-workstream-m005-writer-containment-corrective/001-status.md through 006-status.md. C001 closure includes maintainer decision status but cannot presume it; C002 is ready only after acceptance is independently recorded. **Acceptance was independently recorded on 2026-10-19** (ADR-0042 "Acceptance record" section, option B), which satisfies C002's second gate condition.

## Cross-cutting storage, migration, API and security

Do not duplicate AgentRun or WorktreeService state; use additive SQLite migrations only if needed for reservation/permit state, with schema and index bounds, restart/reconcile, version/CAS/idempotency and authorization. Any native protocol/CLI/TUI extension must be backward-compatible and show supported/denied operations; no rollout before C006. All final-effect sinks must consult daemon-resolved worktree scope and revalidate lease generation. A paused/expired permit does not imply a dead external process. Cancellation/restart ambiguity quarantines checkout; release requires positive evidence or explicit operator recovery, never dirty-file deletion.

Existing permission/tool/sandbox/approval policies still apply and can only narrow. Shared mode refuses unqualified mutations, including via aliases, plugins, LSP/MCP launch, tool programs, human shell and HTTP APIs. C004 must account for CodeGG writes under .codegg/runs and legacy project DB migration or affirmably exclude/rehome them with proof. Do not use Git path claims as write enforcement.

## Verification and closure

Each milestone has a bounded, reproducible test plan, code/doc/guard touchpoints and stop conditions. Use scripted/local Git and deterministic two-run race/restart fixtures before any optional live models. Baseline: cargo fmt --all -- --check; focused cargo test -p codegg-core and cargo test -p codegg; scripts/verify.sh quick; cargo clippy --workspace --all-targets --locked -- -D warnings (or approved actual workspace command); bash scripts/check-core-boundary.sh; python3 scripts/check_execution_ownership.py and scripts/check_scheduler_bypass.py; project/security/approval/TUI guards as touched. No new CI or benchmark gate; hosted native workflows only where closure requires. Closure records MUST report exact SHA, requirement-to-evidence, authorization/tenant and cancellation/race, negative unsupported writer paths, supported-platform matrix, known gaps, regression commands and status, and downstream unblocking. Green compilation alone is not capability qualification.

## Stop / fallback

ADR-0042 option B was accepted on 2026-10-19, so the rejection branch below is counterfactual; had ADR-0042 been rejected, C002–C006 would have stopped and M005 shared mutation would have stayed unavailable, and a rejection still wanting independent same-user writer exclusion would have required a separate OS-isolation ADR. Discovery of one CodeGG-owned final write bypass blocks all shared enablement, not isolated execution. If finite-process descendants cannot be positively contained, keep the entire process surface denied rather than marking it read-only. If any platform lacks full native evidence, opt-in remains unavailable on that platform. Product demands for universal same-user writer exclusion belong to a distinct platform isolation plan, not a silently enlarged M005.
