# Agent Workstream Blackboard M008 — Comparative Multi-Agent Trajectory and Economics Qualification

Status: active — M006 and M007 are strictly closed and independent of M005; M011 corrected root-turn repository identity propagation after M008 exposed the gap. The frozen Rust/Python shared-interface S0 completed on 2026-10-16. Earlier S1 groups stalled and were cancelled; M012 corrected same-session continuation for live turn-owned groups. A fresh isolated-daemon S1 group now resumes across later turns but its children remain running without terminal results, so it is pending and not a successful trial. Evidence is in `plans/closure/agent-workstream-blackboard/008-isolated-results.md` and its JSON companion. Darwin terminal sandbox smoke passed for the tested path; complete sandbox-root evidence for every writer and S2/S3 remain open. S4 shared-checkout execution is unavailable until M005 independently qualifies it; do not emulate shared writes.

Planning baseline: 171ef93b6ba67d50b0412ae23b7697eb02d072eb (main, inspected 2026-10-16)
Source: plans/subsystems/agent-workstream-blackboard-roadmap.md M008
ADRs: accepted ADR-0001/0002/0004/0005/0031/0032/0033; S4 remains gated by M005 qualification under ADR-0032/0033.
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

Keep model/provider choice, output budget, task input, tool availability, sandbox/approval and total token/time budget comparable. Use real provider-backed CodeGG agents for the effectiveness comparison; scripted providers and deterministic fixtures qualify mechanics only. Freeze the task contract and evaluation tests before trials. At least one substantial Rust task and one substantial Python task MUST share a documented interface (for example, a versioned JSON schema or a Rust-produced artifact consumed by Python), with agent assignments that require coordination on that interface in S1/S2/S3. Include at least one substantial language-specific task in each language so the shared-interface case is not the whole sample. Report attempted tasks, denominator, random seeds/order, retries, tool-call counts, queue times and timeouts. If any configuration lacks supported OS shared-write containment, mark unrun/unavailable rather than substituting an unsafe emulation.

## Task strata and metrics

Include real or deterministic captured tasks covering: (i) Python independent modules, (ii) Rust crates sharing heavy dependencies, (iii) Rust tightly coupled API/schema changes, (iv) a substantial Rust/Python cross-language task with a frozen shared interface and coordinated producer/consumer changes, (v) disjoint edits vs intentionally overlapping paths, (vi) sequential prerequisite chains, (vii) Git conflicts with branch drift, (viii) one read-only review+producer split, and (ix) interrupted background build. The required provider-backed shared-interface workload must be run against S0 and at least one multi-agent isolated condition; report additional strata as unavailable when no qualified fixture or safe execution path exists.

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
- Provider-backed S0 and multi-agent isolated results include both a substantial Rust workload and a substantial Python workload, including a shared-interface cross-language task; deterministic M010 evidence cannot substitute for these runs.
- No unresolved high or medium security/correctness findings. File a corrective and leave conditionally closed/blocked where evidence is missing.

## Closure record

Write plans/closure/agent-workstream-blackboard/008-status.md with requirement→evidence table, exact source SHAs, experimental matrix and raw per-task data pointers, tests/guard outcomes, unsupported surfaces, observed improvements AND regressions, security findings by severity, follow-on recommendations and registry unblock audit.

### Partial S0/S1 pilot evidence

The user-authorized isolated S0-versus-S1 pilot was run before M006/M007
closure.
Its two repetitions and limitations are recorded in
`plans/closure/agent-workstream-blackboard/008-pilot-results.md`, with
sanitized per-trial measurements in the adjacent `008-pilot-data.json`.
This remains pilot evidence only: it does not qualify peer/task claims, the
broad M008 task strata, or shared-checkout execution. M006 and M007 have since
closed through their strict platform and daemon-backed acceptance gates, so
M008 is now active for its isolated S0–S3 scope. Any S4 shared-checkout arm
remains separately gated on M005.

### Additional isolated S0/S1 and Cargo measurements (2026-10-16)

Two more S0 trials completed and one of two S1 trials completed without
intervention. The other S1 Python client timed out after five minutes; its
staged change was recovered and passed integrated tests, but the trial is not
counted as autonomous success. A single cold Rust target-directory comparison
also completed. Full measurements and limitations are recorded in
`plans/closure/agent-workstream-blackboard/008-isolated-results.md` and
`008-isolated-results.json`.

The headless `codegg exec` context did not expose `task.spawn_many` or
`wait_group`, so no S2/S3 agent runs were started. It also reported that the
requested workspace-write profile had no workspace root, so those runs do
not establish sandbox containment. These results do not qualify M008 or
support an effectiveness-improvement claim. M008 remains active; no
downstream plan is unblocked by this partial evidence.

### Daemon-backed headless trajectory entry point (2026-10-16)

`codegg headless --session <id> --prompt <text>` (or stdin) submits the
prompt-only `SessionPromptSubmit` request through the singleton daemon and
waits for that session's matching completion event. The session must already
have a durable model selection and workspace binding; create/select it through
the normal session workflow, including `--worktree new` when an isolated
managed checkout is required. This path uses the daemon's production
TaskTool/scheduler and session-bound authoritative workspace root. It does not
change the local semantics of `codegg exec`, and a client timeout does not
cancel the durable turn. This removes the prior missing-TaskTool execution-context
blocker for isolated S2/S3, subject to a successful live smoke trajectory and
explicit sandbox-root evidence.

### Live isolated headless and sandbox qualification (2026-10-16)

The installed `codegg` predates `headless` and managed `--worktree` launch. A
source build succeeded with the explicit arm64 Rust 1.98.1 toolchain and an
isolated target directory; the default x86_64 Homebrew toolchain failed at
link because `/opt/local/lib/libz` is arm64. The source-built executable and
Seatbelt helper are used for this work. Keep trials isolated from the active
user daemon and its catalog by setting both `CODEGG_DAEMON_HOME` and
`CODEGG_DATA_HOME` to trial-owned roots; a daemon-home override alone shares
the catalog database.

The temporary project was given a resolved repository binding through the
normal workspace/project registration requests, then launched with
`--worktree new`. The launch produced a managed worktree and durable session;
provider metadata was staged from the existing personal connection into the
trial-only database without reading or copying credential material, and the
session selection was set through the normal daemon selection request. A
provider-backed headless turn completed against the session-resolved
worktree.

The first smoke exposed a real policy gap: the legacy `terminal` model tool
used `ManagedProcessService` without the accepted sandbox profile or
execution-authorization snapshot. A Python subprocess wrote to `/tmp` under
the constrained session. The exact temporary file was removed. The terminal
adapter was then wired to the session sandbox profile, workspace root, and
authorization snapshot, with fail-closed behavior when a constrained root or
backend is unavailable. Focused terminal tests passed (5/5), as did the
sandbox-policy and execution-ownership guards. A fresh daemon-backed retry
created, read, and removed a file inside the worktree, while Seatbelt denied
the terminal subprocess write to `/tmp` with `Operation not permitted`; the
outside probe was absent after the run. This qualifies the tested Darwin
terminal path for this isolated root and profile. It does not qualify every
writer surface, other platforms, or shared-checkout containment.

No comparative provider-backed task trial has started after this correction.
Next, freeze the substantial Rust and Python tasks around their shared
interface, then run provider-backed S0 and multi-agent isolated trials with
the same task, provider/model, tools, sandbox, approval, and budget. S2/S3
remain open; S4 remains unavailable pending M005.

### Frozen Rust/Python shared-interface workload (fixture v1)

The provider-backed comparison workload is staged in
`plans/closure/agent-workstream-blackboard/008-shared-interface-fixture/`.
Its Rust producer emits `codegg.project-summary.v1`; the Python consumer
strictly validates the same versioned JSON contract and renders a deterministic
Markdown report. The frozen golden test runs Rust output directly through the
Python consumer. S0 receives the complete task; S1/S2/S3 divide Rust producer
and Python consumer ownership across two isolated child worktrees, with the
same schema, prompt contract, acceptance tests, model, approval, sandbox, and
wall-clock cap. Record the fixture tree digest before the first provider-backed
trial. Fixture v1 tree digest (17 files, relative-path/content SHA-256 stream):
`b0f0ff1b3939c95a2113934e3669d26fee403c37ca302d28759ab3b78d864bf1`. The
starter evaluator is intentionally red: Rust baseline 0/7 and Python baseline
0/7. One provider-backed S0 run completed and is recorded in
`008-isolated-results.md`; S1 attempts after repository-identity correction
allocated isolated children but stalled before provider calls. These do not
yet establish a comparative outcome. Continue with a completed multi-agent
arm before making effectiveness claims.
