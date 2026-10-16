# Agent Workstream Blackboard M008 — Comparative Multi-Agent Trajectory and Economics Qualification

Status: blocked — hard dependencies M004, M006, M007 closure (therefore M005 and ADR-0032 gates)

Planning baseline: de49884f3fdc6d528620f2a2645694b9b19fc087 (2026-10-15)
Source: plans/subsystems/agent-workstream-blackboard-roadmap.md M008
ADRs: accepted ADR-0001/0002/0004/0005/0031 and formally accepted ADR-0032 if shared mutation is included.
Canonical: plans/000-long-term-specification.md §§16–17, 26–29; plans/001-terminology-and-domain-model.md §§5, 7–8.
Primary class: invariant/capability closure and optional measured performance evaluation.

## Objective

Provide end-to-end correctness, authorization and restart evidence for blackboard/placement/compilation capabilities, and measure whether they help under controlled budgets. Do NOT claim improvement because internal primitives or a simulated test suite pass. Negative performance findings are valid outcomes; preserve one-strong-agent and isolated worktree paths.

## Experiment configurations

S0: one capable agent, ordinary checkout and compiler handling.
S1: 2 independently mutating children with separate worktrees, no peer board (existing baseline).
S2: identical isolated children with factual peer observations but no claim enforcement.
S3: isolated children with peer observations plus atomic task/dependency claims.
S4: 2 children within an explicitly serialized shared worktree with observations, claims and compilation-aware resource coordination.
Optional: read-only parallel specialists and N=3+ separately, only after N=2 stability; never imply previous PASC result generalizes to N=16.

Keep model/provider choice, output budget, task input, tool availability, sandbox/approval and total token/time budget comparable. Report attempted tasks, denominator, random seeds/order, retries, tool-call counts, queue times and timeouts. If any configuration lacks supported OS shared-write containment, mark unrun/unavailable rather than substituting an unsafe emulation.

## Task strata and metrics

Include real or deterministic captured tasks covering: (i) Python independent modules, (ii) Rust crates sharing heavy dependencies, (iii) Rust tightly coupled API/schema changes, (iv) disjoint edits vs intentionally overlapping paths, (v) sequential prerequisite chains, (vi) Git conflicts with branch drift, (vii) one read-only review+producer split, and (viii) interrupted background build.

Primary outcomes: executable tests/task success and valid integrated result under the correct base, total wall time and cost/tokens. Secondary: duplicate work/edits, collisions, clobbers, false ownership claims, stale peer observations, task-claim contention, interface mismatch, integration retries/conflicts, build CPU-seconds, peak memory, disk growth, cache hit/miss, model turns triggered by peer updates, queue backlog, restart reconciliation and operator interventions.

## Work packages

A. Freeze contract matrix. Compare implementation to M001–M007 closures, inspect source not labels, and enumerate supported CLI/TUI/daemon/agent paths. Require each new group join, blackboard claim, peer event, session-worktree launch, group lease and build optimization to have a production consumer.

B. Deterministic fault/replay suite. Scripted providers, fake agent run responses and local Git fixtures exercise simultaneous claim CAS, source fact ordering, direct-parent messaging, cross-tenant authorization, sandbox approval reduction, malicious peer text, incomplete stored snapshots, oversize model context, duplicate events, conflicting modified files, cancellation, process-tree kill and SQLite close/reopen. Group/child and worktree lifecycle may not leak permits or overwrite unrelated filesystem state.

C. Integration trajectory. Two children allocated to own branches with shared board; produce Git-verified commits, integrate sequentially after parent advances or report typed conflict requiring recovery. Repeat with explicitly serialized shared checkout (if formally accepted and supported). Verify no phantom auto-merge, no hidden index or dirty file discard, and no unowned mutation.

D. Performance study. Use small fixed task sets and publish full per-task results and raw measurement definitions; avoid extrapolation from tiny sample or synthetic fault suites. Include at least one Rust target-dir contention measurement on a reasonable developer machine. Document variability and statistical uncertainty, not only averages.

E. Protocol/TUI/CLI qualification. Project/tenant visibility, session worktree association, genuine /fork backward compatibility, stale frontend completions, group member order, optional no-blackboard single-agent path, notification persistence. Confirm no unapproved idle inference resume.

F. Closure and downstream audit. Update architecture docs, tools and skill material, roadmap and registry as findings warrant. File bounded corrective plans rather than reopening old M00N status as if it never closed.

## Required verification

Use existing tests as starting points: tests/worktree.rs; tests/subagent.rs; tests/scheduler_restart_recovery.rs; tests/work_orders_m002_materialization.rs; tests/work_orders_m007_trajectory.rs; tests/session_projection_consumer.rs; relevant git/integration, auth, background-job, tool, TUI/CLI suites. fmt, full applicable clippy, scripts/verify.sh quick/full where available, core boundary, scheduler bypass, execution ownership, Git safety and auth/projection guards. Hosted CI only through existing workflow with exact SHA, including supported platform-specific native evidence; never create a permanent benchmark job.

## Acceptance and stop criteria

- S0 remains operational and low-overhead; S1's previously accepted isolation behavior remains unchanged.
- In S2/S3, observations accurately attribute actual peer actions, do not become peer-issued instructions and stay bounded through restart/cancel.
- Claim races/replay never create double authoritative ownership or unauthorized scope expansion.
- S4 only if real shared-write execution guards cover every active mutation route; otherwise explicitly unavailable and the rest of the subsystem may close as partial capability.
- Rust cache strategy does not silently amplify CPU/disk or accept stale artifact reuse without diagnostics.
- Explicit integration works on supported targets or returns actionable recoverable conflict; no unqualified automatic success.
- At least one measured comparison against S0 and S1 with published denominators, resource costs and negative cases. No mandatory win threshold, since honest result determines future default/policy.
- No unresolved high or medium security/correctness findings. File a corrective and leave conditionally closed/blocked where evidence is missing.

## Closure record

Write plans/closure/agent-workstream-blackboard/008-status.md with requirement→evidence table, exact source SHAs, experimental matrix and raw per-task data pointers, tests/guard outcomes, unsupported surfaces, observed improvements AND regressions, security findings by severity, follow-on recommendations and registry unblock audit.
