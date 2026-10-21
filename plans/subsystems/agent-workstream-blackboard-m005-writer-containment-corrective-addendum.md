# M005 Writer-Containment Corrective Addendum — Restricted Shared Checkout

Status: active planning; C001–C005 closed; C006 conditionally closed. The shared checkout is now an opt-in, platform-gated path (Linux qualified; macOS and Windows disabled with a named reason).

Planning baseline: branch plan/agent-workstream-m005-writer-audit inspected 2026-10-19, main ancestor 61cbe9222c442c443cc7dfb3bd3a5b7d99ecd649. Revalidate exact branch head and mainline status before any implementation.

Authoritative sources:
- plans/003-planning-process.md §§4–10 (corrective and closure governance)
- plans/000-long-term-specification.md §§9, 16–17, 26–29
- plans/001-terminology-and-domain-model.md §§5, 7–8
- plans/subsystems/agent-workstream-blackboard-roadmap.md (parent M005 stays active)
- plans/implementation/agent-workstream-blackboard/005-group-worktree-placement.md (Q0, Q1–Q5, audited sinks, 2026-10-19)
- plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md (accepted)
- plans/adrs/ADR-0032-agent-workstream-coordination-and-shared-checkout-ownership.md (accepted architecture)
- plans/adrs/ADR-0044-restricted-agent-shared-checkout-writer-contract.md (**accepted 2026-10-19**, option B — restricted CodeGG-managed coordination; does not enable shared mutation)

## Purpose and root cause

The original M005 scope did not bound the meaning of checkout writer exclusion or enumerate every effectful owner. The 2026-10-19 audit demonstrated that model ToolBatchExecutor coordination is intra-batch only, WorktreeService leases are not all-writer mutexes, scheduler resource keys are global and omit inline or human/HTTP writers, and some CodeGG-managed writers bypass ToolBroker. PTYs and LSP/MCP/plugin services persist beyond a tool request; Unix children may detach with setsid and survive group teardown. External same-user programs never participate in CodeGG locks. Previously closed test suites focused on isolation, process cleanup and peer coordination; they did not test a cross-run cross-surface shared-write permit. Q0 verifies the safe isolated default; Q1–Q5 remain unrun.

This corrective line addresses the core **writer-containment contract** before any attempt to enable shared checkout. A restricted CodeGG-mediated coordination profile is the recommended feasible direction. ADR-0044 was proposed for the scope change and required explicit approval; that planning was not itself approval. **ADR-0044 option B was accepted on 2026-10-19**, which unblocks C002 only.

## Ownership and invariants

AgentRun/Group and WorktreeService remain canonical for run, reservation and lease ownership; Git mutation/RunStore/document/file services remain owners of their effects; JobScheduler alone admits heavy work. One daemon-owned typed writer authority may coordinate final effects without becoming a scheduler or a replacement Git service. Authorization comes from project/principal/run/group/attempt records and current sandbox/tool rights, never agent text, filenames, WorkstreamClaim or model/client-supplied generations.

Shared profile is opt-in, conservatively disabled, with serial CodeGG writers and concurrent proven reads. It does **not** prevent edits by unrelated same-user programs; if external-writer exclusion is required, stop and start independent OS isolation feasibility work. All CodeGG-controlled project-root writers must take the common permit or refuse before effect; there is no silent human/HTTP/editor exemption. Persistent and arbitrary subprocess capability is denied until native lifetime proof exists. Permit expiry cannot release an uncertain writer. No default shell regex blocker, second scheduler, generic workflow framework, new CI lane, unbounded polling, automatic merge, or compulsory Cargo target/cache sharing.

Independent M006 isolated session/worktree launch, M007 per-workspace build admission and M008 isolated-only experiments remain eligible on their own original gates; this corrective does not block them.

## Dependency graph and handoff

    M005 audit / safe Q0 closed
       |
       +-- C001 writer-policy decision + complete allow/deny inventory [CLOSED]
               |
               +-- explicit acceptance of proposed ADR-0044   [HARD GATE — SATISFIED
                   |                                            2026-10-19, option B]
                   +-- C002 durable fenced checkout permit Q1 [CLOSED]
                           |
                           +-- C003 native file/Git writers Q2 [CLOSED]
                                   |
                                   +-- C004 non-broker sink mediation Q3 [CLOSED]
                                           |
                                           +-- C005 process denial /
                                           |        lifecycle gating Q3/Q4 [CLOSED]
                                           |
                                           +-- C006 restricted end-to-end
                                                    Q1–Q5 / opt-in [CONDITIONALLY CLOSED]
                                                    (opt-in, platform-gated:
                                                     Linux qualified; macOS/Windows
                                                     disabled with a reason)

C001 may document and test negative current behavior without accepted ADR-0044. C002 implementation proceeded under accepted ADR-0044 and closed on 2026-10-19; no product enablement proceeded on it, and acceptance alone still enables nothing: C003–C006 must land and qualify before any shared mutation exists. C002 built the permit's authority; C003 routed the model's native file and typed Git writers through it, C004 routed the non-broker writers through the checkout reservation gate, and C005 made the process surface refuse pre-effect through the authoritative process admission gate. The permit still fences the *model* path only; the non-broker and process paths refuse pre-effect instead of taking a permit, deliberately, so they cannot invert the document/repository/permit lock order. C006 wired the production shared-reservation path — the durable `RunPlacement`, the group-authorized shared-checkout coordinator, the daemon's writer-fence factory and the task tool's opt-in placement — and it consults C005's reservation preflight; it did **not** claim M005 strict closure, because macOS/Windows native evidence and the composed two-agent daemon race are both still open, the latter blocked on the pre-existing absent contained Git backend that managed worktree creation needs.

## Milestone contracts

| Milestone | Primary class | Status | Objective | Hard gate |
|---|---|---|---|---|
| C001 | invariant / decision contract | closed | Explicit boundary proposal, canonical effect-sink inventory, pre-effect deny matrix and handoff decision record | M005 audit |
| C002 | infrastructure / invariant | closed | One durable CAS/generation-fenced permit with quarantine, contention and restart tests (Q1) | C001 closed + ADR-0044 accepted — **both met**; closure `plans/closure/agent-workstream-m005-writer-containment-corrective/002-status.md` |
| C003 | invariant / capability | closed | Final-effect file and typed Git write interposition without double acquisition/deadlock (Q2) | C002 closed — **met**; closure `plans/closure/agent-workstream-m005-writer-containment-corrective/003-status.md` |
| C004 | invariant / capability | closed | Every other CodeGG project-root writer either holds permit or refuses; includes human CodeGG file APIs and workspace-local state (Q3) | C003 closed — **met**; closure `plans/closure/agent-workstream-m005-writer-containment-corrective/004-status.md` |
| C005 | invariant / capability | closed | Prevent unqualified direct/persistent process launch pre-effect; per-OS negative/proven lifecycle matrix (Q3–Q4) | C004 closed — **met**; closure `plans/closure/agent-workstream-m005-writer-containment-corrective/005-status.md` |
| C006 | capability / closure | conditionally closed | Real two-AgentRun restricted checkout, full Q1–Q5, restart, Git provenance, honest supported-OS opt-in or negative disposition | C004+C005 closed — **met** (`plans/closure/agent-workstream-m005-writer-containment-corrective/004-status.md`, `plans/closure/agent-workstream-m005-writer-containment-corrective/005-status.md`); the production shared-reservation path is wired and calls C005's reservation preflight; closure `plans/closure/agent-workstream-m005-writer-containment-corrective/006-status.md`. **Two named follow-ups keep it from strict closure**: macOS/Windows native containment evidence, and the composed two-agent daemon race, which this tree cannot compose because managed worktree creation needs a contained Git backend that does not exist here (pre-existing; it blocks the isolated path identically) |

Implementation handoffs: plans/implementation/agent-workstream-m005-writer-containment-corrective/001-*.md through 006-*.md. Closure records: plans/closure/agent-workstream-m005-writer-containment-corrective/001-status.md through 006-status.md. C001 closure includes maintainer decision status but cannot presume it; C002 is ready only after acceptance is independently recorded. **Acceptance was independently recorded on 2026-10-19** (ADR-0044 "Acceptance record" section, option B), which satisfies C002's second gate condition.

## Cross-cutting storage, migration, API and security

Do not duplicate AgentRun or WorktreeService state; use additive SQLite migrations only if needed for reservation/permit state, with schema and index bounds, restart/reconcile, version/CAS/idempotency and authorization. Any native protocol/CLI/TUI extension must be backward-compatible and show supported/denied operations; no rollout before C006. All final-effect sinks must consult daemon-resolved worktree scope and revalidate lease generation. A paused/expired permit does not imply a dead external process. Cancellation/restart ambiguity quarantines checkout; release requires positive evidence or explicit operator recovery, never dirty-file deletion.

Existing permission/tool/sandbox/approval policies still apply and can only narrow. Shared mode refuses unqualified mutations, including via aliases, plugins, LSP/MCP launch, tool programs, human shell and HTTP APIs. C005 added the process half of that refusal: the authoritative process admission gate refuses every unqualified process surface of a live-group-reserved checkout before a process exists (`crates/codegg-core/src/process_admission.rs`, `src/tool/process_admission.rs`, `src/tool/process_capability.rs`), with a durable `process_denial` audit action, a live persistent-process registry with a reservation preflight, and the `scripts/check_process_denial.py` guard. Three surfaces are recorded as explicit open items rather than implied guarantees: the local MCP stdio client launch, the egglsp language-server launch and the shell-command hook adapter (closure `plans/closure/agent-workstream-m005-writer-containment-corrective/005-status.md` §6). C004 accounted for CodeGG writes under .codegg/runs and the legacy project DB by an explicit, evidenced disposition rather than a false "outside checkout" claim (`docs/writer-disposition.toml`, stable store policy; closure `plans/closure/agent-workstream-m005-writer-containment-corrective/004-status.md` §5). Do not use Git path claims as write enforcement.

## Verification and closure

Each milestone has a bounded, reproducible test plan, code/doc/guard touchpoints and stop conditions. Use scripted/local Git and deterministic two-run race/restart fixtures before any optional live models. Baseline: cargo fmt --all -- --check; focused cargo test -p codegg-core and cargo test -p codegg; scripts/verify.sh quick; cargo clippy --workspace --all-targets --locked -- -D warnings (or approved actual workspace command); bash scripts/check-core-boundary.sh; python3 scripts/check_execution_ownership.py and scripts/check_scheduler_bypass.py; project/security/approval/TUI guards as touched. No new CI or benchmark gate; hosted native workflows only where closure requires. Closure records MUST report exact SHA, requirement-to-evidence, authorization/tenant and cancellation/race, negative unsupported writer paths, supported-platform matrix, known gaps, regression commands and status, and downstream unblocking. Green compilation alone is not capability qualification.

## Stop / fallback

ADR-0044 option B was accepted on 2026-10-19, so the rejection branch below is counterfactual; had ADR-0044 been rejected, C002–C006 would have stopped and M005 shared mutation would have stayed unavailable, and a rejection still wanting independent same-user writer exclusion would have required a separate OS-isolation ADR. Discovery of one CodeGG-owned final write bypass blocks all shared enablement, not isolated execution. If finite-process descendants cannot be positively contained, keep the entire process surface denied rather than marking it read-only. If any platform lacks full native evidence, opt-in remains unavailable on that platform. Product demands for universal same-user writer exclusion belong to a distinct platform isolation plan, not a silently enlarged M005.
