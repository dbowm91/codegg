# Agent Workstream Blackboard M008 — Comparative Multi-Agent Trajectory and Economics Qualification

Status: **closed 2026-10-22** — the contract matrix is frozen and guarded, the five configurations are qualified against it, and the measured comparison is published with its denominators and its negative cases. The shared-checkout arm (S4) is **qualified on Linux and macOS** and **unrun on Windows**, where the shared profile is disabled because no filesystem-enforcement backend is registered for Windows at all. Closure: `plans/closure/agent-workstream-blackboard/008-status.md`.

Closure note (2026-10-22): this milestone's first work package asked for the contract matrix to be frozen *before* any two configurations were compared against each other. That freeze is `architecture/blackboard_contract_matrix.md`, kept frozen by `scripts/check_blackboard_contract_matrix.py`, which fails when a capability loses its production consumer, when a consumer retreats into a test-only module, or when a row goes empty. The qualification suites are `tests/blackboard_trajectory_m008.rs` (the S0–S4 functional arms, the explicit-integration conflict case and the notification-durability case) and `tests/blackboard_economics_m008.rs` (the opt-in measured comparison and the Rust target-directory contention arm), with the peer-observation contract pinned in `tests/agent_loop_harness.rs`. **No live-model performance claim and no live two-agent daemon-race claim** are made; both are recorded as named non-claims in the closure record, alongside the M007 product-side umask defect that this milestone recorded and did not fix.

Gate status 2026-10-19: the gate this milestone waited on was M006 (isolated
session/worktree launch UX) and M007 (per-workspace build admission). Both
closed on 2026-10-19, so the configurations that need only isolated
checkouts — S0, S1, S2, S3 — may proceed. S4 (two children in an explicitly
serialized shared worktree) additionally requires M005 closure, and M005's
shared mutation is still unavailable behind its corrective sub-line
(C001–C002 closed, C003 ready, C004–C006 blocked). If a configuration lacks
supported OS shared-write containment, mark it unrun rather than substituting
an unsafe emulation.

Gate status 2026-10-21: the S4 gate was M005, and M005's corrective has now
closed it — C001–C006 are closed with C006 conditional, the shared checkout is
a real opt-in production path, and the platform table qualifies **Linux and
macOS** (`crates/codegg-core/src/run_placement.rs`). S4 therefore runs on those
two platforms and is marked **unrun on Windows**, where the profile is disabled
for a capability reason rather than an evidence reason. Two claims carry into
S4 unchanged and are not re-established by this gate: the three C005 process
surfaces that are **not** refused pre-effect (the local MCP stdio client, the
`egglsp` server launch and the hook shell adapter) keep their recorded
compensating controls, and a shared checkout still admits no unqualified
process — so S4's Rust build arm is unavailable by contract, exactly as C006
recorded.

Planning baseline: 171ef93b6ba67d50b0412ae23b7697eb02d072eb (main, inspected 2026-10-16)
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
