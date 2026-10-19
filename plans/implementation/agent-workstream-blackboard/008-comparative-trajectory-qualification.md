# Agent Workstream Blackboard M008 — Comparative Multi-Agent Trajectory and Economics Qualification

Status: active — M006/M007 and M011/M012 are closed. Historical frozen shared-interface S0–S3 and overlapping/dependent API runs qualify coordination mechanics, not effectiveness. The repeated M008 matrix is now frozen with equal total agent-token ceilings per condition: S0 spends the ceiling in its coding turn; S1–S3 reserve a bounded coordinator allocation and divide the remainder across children. A headless per-turn token ceiling was added to support this. One calibration attempt produced a passing candidate only after evaluator recovery, exceeded its child cap, and left the coordinator uncapped; it is excluded from scored comparisons. Repeated, budget-matched trials across independent-module, shared-interface, and overlapping-API strata remain outstanding. Provider cost remains unavailable. M005 remains independent; S4 shared-checkout execution is unavailable until M005 qualifies it. Do not emulate shared writes.

The per-run model-token ceiling and provider-usage journal are now implemented
for the repeated comparison protocol. The existing S0–S3 results above used
equal tool-call caps, not equal aggregate model-token ceilings, and do not
record attributable provider usage/cost. They remain mechanics evidence and
are not pooled into the repeated budget-controlled trial matrix. New trials
under `008-task-matrix-v1.md` remain outstanding.

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

Keep model/provider choice, output budget, task input, tool availability, sandbox/approval and total token/time budget comparable. Use real provider-backed CodeGG agents for the effectiveness comparison; scripted providers and deterministic fixtures qualify mechanics only. Freeze the task contract and evaluation tests before trials. At least one substantial Rust task and one substantial Python task MUST share a documented interface (for example, a versioned JSON schema or a Rust-produced artifact consumed by Python), with agent assignments that require coordination on that interface in S1/S2/S3. Include at least one substantial language-specific task in each language so the shared-interface case is not the whole sample. Report attempted tasks, denominator, random seeds/order, retries, tool-call counts, queue times and timeouts. Attribute provider-reported tokens and estimated USD to each durable child `AgentRunId`; report parent and child totals separately and aggregate only for the trial total. Session-level totals alone are not comparable because they mix sibling runs. If any configuration lacks supported OS shared-write containment, mark unrun/unavailable rather than substituting an unsafe emulation.

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
0/7; these are harness checks, not agent-trial results. No provider-backed
comparison has started.

### Comparative trial execution update (2026-10-17)

The frozen fixture was used first. Provider-backed S0 completed on the frozen
Rust/Python producer-consumer contract and all three acceptance checks passed.
The trial used OpenCode Go `gpt-6-luna`; provider usage reports 292,298 input
and 7,473 output tokens across 13 parent-session usage rows. The provider did
not report a nonzero cost and the local model pricing catalog had no matching
entry, so USD cost is unavailable rather than treated as zero. Child-run token
and cost attribution is now persisted by `AgentRunId` (storage migration v80)
and covered by `crates/codegg-core/tests/agent_run_usage.rs`. The first S2
attempt after that change exposed that a child's synthetic transcript session
ID violates the usage table's durable-session foreign key. Child usage now
records against its originating durable session while retaining the child
`AgentRunId`; foreign-key-backed aggregation coverage passes. A fresh run is
still needed to produce comparable child-level measurements.

Two S2 starts were attempted. The first exposed that daemon turn admission did
not carry the session's repository identity into the durable child task; this
was corrected in `src/core/daemon_turns.rs`. The second confirmed both child
tasks received repository identity, then exposed a separate executor defect:
relative child scopes (`rust/`, `python/`) were interpreted as repository
roots, so managed worktree creation failed before child execution. The executor
now roots runs in the scheduler-owned workspace and resolves/remaps child path
scopes within the managed worktree. After those fixes, one S2 and one S3 group
were started against the same fixture and isolated trial daemon. Neither
produced a source change or acceptance result, so neither is an effectiveness
trial:

| Condition | Group / parent session | Child `AgentRunId`s | Child usage rows (input/output; reported USD) | Durable outcome at inspection |
|---|---|---|---|---|
| S2 factual peer observations, no claim enforcement | group `b37c8d61-4e0f-46fb-ba99-d3b4b3f9961e`; parent session `8f3676dd-1275-4ecc-b8ae-3673114e6943` | Rust `c08fd232-54ad-4c5c-b148-7d3d0cd87901`; Python `c71b2f7e-e1b9-4f8b-aff9-e687f8e2fac7` | Rust 1 row, 8,901/52 tokens; Python 1 row, 8,900/34 tokens; cost unavailable (stored 0.0, no provider cost or catalog price) | Both children `interrupted` with `scheduler_recovery`; group still `running`; no edits or acceptance checks |
| S3 observations plus atomic claims | group `10f975ea-50f0-4012-bc9e-09800ee5c8b7`; parent session `af77adf3-9739-4d9f-b176-36887292a5ef` | Rust `fb34669a-1b58-4102-b0a5-3857b40dfe01`; Python `49f5a1ab-0e63-40e9-8a51-0735421a3332` | Rust 3 rows, 27,980/423 tokens; Python 2 rows, 18,295/300 tokens; cost unavailable (stored 0.0, no provider cost or catalog price) | Both children and group remained `running` after daemon shutdown; no claims, edits, or acceptance checks; recovery did not reconcile this group |

Both parent turns ended with empty results. The S3 parent attempted the board,
status, and message actions; the board action returned “workstream action
requires a bound group actor”, and no durable claims were created. The isolated
workspace remained unchanged. The child-level usage rows now correctly carry
their own `AgentRunId`, making the failure attributable, but these incomplete
turns cannot be compared to S0. Do not aggregate their token counts as trial
totals or claim USD cost. The stale S3 group/run state is an unresolved restart
reconciliation defect to investigate before another comparative attempt. The
durable run journals contain only run lifecycle, safe-boundary, and queued /
delivered-control events for these children; no completed child tool action is
recorded.

The user-approved credential selection used the most recently updated existing
OpenCode Go credential in the trial-only database; no secret was displayed or
copied into the report. No S2/S3 effectiveness result is claimed, and no
dependent plan is unblocked.

### Post-accounting S2/S3 retries and coordination corrections (2026-10-18)

The provider-backed retries below used the frozen v1 fixture and isolated
trial-owned daemon/data roots. Per-child usage is now queryable by each durable
`AgentRunId`; `cost_usd=0` is reported as unavailable because the provider sent
no cost and the local model catalog has no matching price. These usage totals
are not successful-trial comparisons because required work, claims, commits,
or acceptance checks did not complete.

| Condition / group | Child runs and final usage (input/output tokens; USD) | Outcome |
|---|---|---|
| S2 factual observations, `df49011f-c5b6-4e5e-bc19-b4a47b5628e9` | Rust `0f6c3ae6-d76f-4200-8391-b76ace68968f`: 666,945/7,370; Python `4114f366-f58c-40af-ae22-144abf2c0088`: 564,630/7,523; USD unavailable for both | The parent reached the 10-minute cap while both runs were active. After stopping the trial daemon, Python was completed and Rust recovered as interrupted. Their managed worktrees held uncommitted edits, but neither child commit nor frozen acceptance result was available. No integration was performed. |
| S3, `712bbd65-3dd4-493a-a8ac-7928e4db7574` | Rust `a9942108-bb4e-4307-8cbf-7ff594dafc11`: 63,448/2,076; Python `ecd57c24-9a33-43df-b9c3-dd492365617e`: 91,183/1,672; USD unavailable | Both children completed without edits. Their reports said board/claim prerequisites failed; observed errors included missing `group_id` and unauthorized group-member board reads. No commits or tests. |
| S3, `4e5be18e-bc4b-4bcf-97f3-755130dd0b57` | Python `2b1e04ff-5697-4879-92c9-c875e265f67f`: 46,930/666; Rust `c3ee9894-c64d-4c57-b0c0-4fbcafb1235f`: 879,888/8,273; USD unavailable | Python stopped because path intent `python/` has an invalid trailing slash. Rust remained active at the 10-minute cap and recovered as interrupted. No claims, commits or acceptance checks. |
| S3, `53f4e9e8-417e-43ed-9026-5e48282af9c4` | Rust `a83bbd02-407b-49df-a1df-ccc0884f5e5b`: 9,055/493; USD unavailable | `spawn_many` accepted the Rust child but rejected the Python request before creating its durable task. Rust remained active at the cap and recovered as interrupted. No claims or code changes. The rejection detail was not retained in the final parent report. A daemon restart marked the child `interrupted` with `scheduler_recovery`, but the `join_policy=all` group remained `running`, reproducing the group reconciliation defect. |

The source follow-up carries the daemon-authenticated principal through
`TurnRunInput` into `ToolExecutionContext` for parent workstream claim actors.
`AgentRunGroupService::status` now permits direct member runs to read the
group board; wait and cancel remain owner-only. The task tool now includes
bounded rejection details in `spawn_many` summaries so a future retry can
record why one child was refused. Focused tests cover the member read boundary,
rejection detail bounds, and the claim actor's requirement for a valid bound
principal.
The `python/` failure was prompt input: claim targets are normalized relative
intent strings such as `python`, without a trailing slash. The second-child
`spawn_many` rejection remains unexplained. Restarting the isolated daemon
after capped attempts recovered active child runs as interrupted, but left
their `join_policy=all` groups in `running`; startup does not reconcile these
group records. These changes do not qualify the provider-backed S2/S3 arms. The frozen task acceptance tests, typed integration,
and successful two-child claim/board trajectory remain outstanding. Do not
expand to overlapping Rust APIs or dependency-heavy tasks until S2/S3 complete.

### Registered-agent S2 retry and exact-head qualification (2026-10-18)

The frozen shared-interface fixture digest was independently verified by the
supervisor. One dispatch attempt used the unregistered agent name `default`;
both durable runs failed immediately with `Agent 'default' not found` and did
not start provider work. A corrected retry used the registered `build` agent
for both child assignments in separate managed worktrees, `join_policy=all`,
and no claims. Python completed its assigned 7 tests, but its branch/stage/
commit operations were denied by the child worktree policy. Rust produced
uncommitted changes but continued consuming provider turns without a terminal
result. The isolated daemon was stopped at 273.5 seconds to bound the run;
Rust had accumulated 1,650,511 input and 14,466 output tokens. The provider
reported no cost. Startup recovery marked Rust interrupted but left the group
`running`. No child commit or integrated acceptance run exists, so this is
non-qualifying diagnostic evidence, not an S2 effectiveness result.

The retry revealed two prerequisites for another provider-backed arm: a
finite child tool-call budget and explicit use of the typed Git mutation
surface for branch/stage/commit operations. TaskTool now accepts an optional
per-child `max_tool_calls` from 1 to 128 and carries it through the durable
budget and scheduled payload to the existing loop enforcement. Focused tests
cover validation, schema exposure, and idempotency fingerprints. Use one
explicit limit for every child in each comparative arm. Do not expand to
overlapping Rust APIs or dependency-heavy tasks until bounded S2 and S3
trajectories complete; do not create an unbounded replacement trial.
The interrupted S2 also reproduces the stale `join_policy=all` group after
recovery. Full run identifiers, per-child usage, tests, dirty worktrees and
exact-head hosted workflows are recorded in `008-isolated-results.md` and
`008-isolated-results.json`.


### Bounded S2 retry: scheduler wiring and workspace write denial (2026-10-18)

The next S2 retry used `max_tool_calls=32` for both registered `build`
children. Durable payload inspection confirms the cap. It also confirms child
usage attribution is comparable by `AgentRunId`, with 9 Python usage rows
(143,025 input / 8,157 output) and 20 Rust rows (376,316 input / 13,512
output). Provider-reported cost remains unavailable.

Python observed the production board successfully, but its first workspace
write was denied, so it stopped per the prompt; no tests or commit were
produced. Rust completed without usable text, and its managed worktree was
clean. No acceptance evaluator ran. The parent consumed 803,723 input tokens
after group completion and was stopped with the isolated daemon. The full
record is in `plans/closure/agent-workstream-blackboard/008-isolated-results.md`
and `.json`.

This attempt exposed a missing `JobSubmissionService` in child tool registries
and loss of authenticated origin attribution at the broker boundary. Both
paths are fixed and focused tests pass, but the write denial still requires a
deterministic root-cause check. Do not start S3 or expand task strata until a
bounded S2 can write, run tests, commit, and pass the frozen evaluator. M005
remains separate; no downstream plan is unblocked.

### Exact-head qualification after M003 receipt reconstruction (2026-10-18)

The historical M003 receipt now reconstructs the old opaque
`spawn_many` item schema while separately asserting the live expanded schema;
the frozen receipt was not modified. Hosted CI passed on exact implementation
SHA `7bdfaa18dfb5bbec6bb887adb140d51573e15c76` (run
[39112674196](https://github.com/dbowm91/codegg/actions/runs/39112674196)),
including guards, formatting, workspace Clippy, and workspace tests. Windows
deep-path linked-worktree qualification passed on the same SHA (run
[39112675703](https://github.com/dbowm91/codegg/actions/runs/39112675703)).
The branch includes `origin/main` as an ancestor.

This closes exact-head integrated qualification for the current implementation
source. It does not close S2/S3: the persisted Python child summary says its
workspace write was denied but contains no permission decision, tool name, or
error trace, and the daemon log does not retain one. Do not rerun provider
trials or expand into overlapping Rust APIs/dependency-heavy tasks until that
denial can be reproduced deterministically and a bounded S2 passes tests,
commit, and the frozen evaluator. M005 remains independent and its native
escaped-descendant finding still blocks S4; no future plan is newly unblocked.


### S3 message-delivery retry and reconciled branch (2026-10-18)

After merging `plan/managed-scratch-spaces` into M008, the branch head is
`19e63685146a651dbed9ebe23f95e26812bf52b1`. The receipt reconstruction
retains the live expanded child schema assertions, and the Bash fixture uses an
absolute registered workspace root. Focused local verification passes:
`tests/bash_background_m002_routes.rs` 12/12 and the frozen M003 schema
receipt test 1/1.

A final single-group S3 retry used the frozen fixture digest, the selected
`opencode_go/gpt-6-luna` connection, `max_tool_calls=32` on each request item,
and one two-member `join_policy=all` group. Both durable budgets confirmed the
cap. The children were asked to wait for a full parent message containing the
group ID, their claim target, and implementation instructions. Both completed
their initial wait before the parent delivered either message; neither read
the board, claimed a path, edited, tested, or committed. Rust used 9,217 input /
134 output tokens (1 usage row, 1.615 s queue delay, 2.963 s run wall); Python
used 9,234 input / 269 output tokens (1 usage row, 0.453 s queue delay,
5.289 s run wall). Provider-reported costs were unavailable. The group is
`157547d5-c5ef-4e64-82fc-457a98cfdc32`; run IDs, exact summaries, and cap
records are in `008-isolated-results.json`.

This confirms a scheduler/message ordering gap: new child jobs may complete
before their parent can deliver group-scoped instructions. It is not an S3
result and requires a bounded implementation correction so a child cannot
complete its pre-work wait before receiving the initial group context (or an
equivalent host-owned initial instruction). Do not spend more provider budget
on comparative runs until that race is fixed and a focused deterministic test
proves both children can observe the same group and claim board. The integrated
branch is pushed; exact-head hosted CI and Windows runs are pending. No
overlapping Rust API/dependency-heavy strata were started. M005's native
`setsid()` escape still independently blocks S4.


### S3 preassigned-group retry (2026-10-18)

A further bounded S3 arm used one preassigned group ID embedded in both full
child prompts, eliminating post-spawn message delivery. The single two-member
group `4db9ae26-4092-45f5-bbac-1e1c7c9e88f4` completed with both durable
`max_tool_calls=32` budgets. Both children acquired the required disjoint claims
on the same board. Rust run `1ec4d16f-f3d8-49cd-a341-15d5305fe42d` used
441,430 input / 6,540 output tokens (21 usage rows; 1.615 s queue; 106.366 s
run); its changed Rust producer passed 7/7 frozen Rust tests. Python run
`509bc580-514b-4e10-84e1-0a34f570bab9` used 345,259 input / 6,791 output
(19 usage rows; 0.453 s queue; 99.795 s run), but its worktree changed only
`__pycache__`; Python's 7 frozen tests produced 6 failures and 1 error at the
intentional TODO implementation. Neither child committed. Provider cost was
unavailable. See the raw run/claim/usage records in
`plans/closure/agent-workstream-blackboard/008-isolated-results.json`.

This S3 arm fixes the group/message ordering path but still fails functional
acceptance because one package is unimplemented. Together with the earlier S2
source-copy evaluation (all acceptance tests pass, but no child commits), it
provides no Git-verified S2/S3 comparative result. Do not expand into
intentional-overlap Rust APIs or dependency-heavy strata yet. A source change
must give children observable task results and produce actual assigned source
changes; rerun only after a deterministic regression test covers that child
result path. M008 remains active. The separate M005 writer inventory is
complete, while its Darwin `setsid()` descendant test still fails the native
lifecycle gate; S4 remains unavailable.

### Exact-head qualification after audit fixture correction (2026-10-18)

The integrated M008 branch is reconciled with `origin/main` and the scratch
branch fixture correction. Hosted CI passed on exact implementation head
`a5977dd7342945990d7749ebaeb163374125ec8f` in run
[39132947871](https://github.com/dbowm91/codegg/actions/runs/39132947871):
formatting, guards, workspace Clippy, and 14,446 workspace tests passed with
6 configured skips. The Windows process launcher workflow also passed on that
exact head in run
[39132947859](https://github.com/dbowm91/codegg/actions/runs/39132947859),
covering process lifecycle, managed scratch, ACL, AppContainer, and native
shell/policy checks. The final test-fixture correction supplies the canonical
workspace root to `bash_shell_success_emits_one_structural_event`; it changes
no runtime behavior.

These results qualify the integrated source for hosted CI and Windows checks.
M008 remains active because the latest S2 pair still has no child commits and
the S3 Python child failed the frozen acceptance tests. Do not expand into
overlapping Rust APIs or dependency-heavy tasks until both conditions have
Git-verified integrated acceptance. M005 remains separate: writer inventory
is complete, but the Darwin escaped-descendant finding still blocks S4. No
downstream plan is unblocked.

### Reconciled-branch S2 startup, Git-result, and retry follow-up (2026-10-18)

The shared-interface fixture was recreated from its frozen source in a clean
disposable Git repository with digest
`b0f0ff1b3939c95a2113934e3669d26fee403c37ca302d28759ab3b78d864bf1` and baseline
`382c2978b21fb2ae27dad5094aea8285c89bb13c`. A new project/workspace/repository
identity and managed parent worktree were used; retained dirty checkouts from
earlier trials were left intact. Group membership was visible to child startup
before the model execution began.

Three bounded S2 attempts did not qualify:

| Group | Child evidence | Outcome |
|---|---|---|
| `agent-group-f7ada846109fd169821a67179f33b65c46989cb0ea125c54116c21f6042be42e` | Rust `8cf6f7d1-17f4-42a6-9ea2-dfe42374f2ee`: 382,779/5,902 tokens; Python `d4ab8b1a-3c49-4585-8513-62fd8c34f170`: 372,820/5,021; provider/catalog cost unavailable | Both runs completed, but each structured result had `result_commit: null`; neither exposed test evidence. `task.get` exposed the exact child worktree, base commit, changed paths, and dirty repository state. No integration. |
| `agent-group-b72218ce8277f8258e5668552b8675bf3d1806895c6e1409c8a6a3b1768fca1a` | Accepted Python child `c4282b9b-a08a-4dda-8de7-3c9b6f75c024`: 83,646/3,858 tokens; cost unavailable | Only one member was admitted; the sibling request failed with SQLite database-lock contention. The accepted child produced no change, test evidence, or commit. |
| `agent-group-d4719b1744f94b106aca2ec8a060cfe73e28983b2718c7915049e0facd48a7b6` | Rust `1c5b7b27-a505-4e4d-8e4b-c9da3860031d`: 2,591,151/21,869; Python `a84f2b98-2db4-4acf-8717-993c3f71a44b`: 490,614/7,899; cost unavailable | Both completed. Rust tests passed 7/7 and `rust/src/lib.rs` was staged, but typed commit failed with `Git backend unavailable for [CleanSmudgeFilter]`. Python produced its source change but no test/commit evidence. No result commit and no integration. |

The SQLite admission failure led to a bounded retry around idempotent
`AgentRunStore::create_or_get`; a focused classifier test passes. The Git
failure showed that child `commit` with no explicit paths was being interpreted
as `StageAll`, re-entering ordinary content-filtered staging after the child
had staged its explicit path. Isolated-child commits now use the already-staged
index only; a regression test passes. These fixes are in the current uncommitted
source and have not yet been provider-qualified.

A fourth S2 group,
`agent-group-20c90a40a3cd81431237d49dff8ae3f239355ec3550d0caf0451863b91196f53`,
admitted two children with 128-call caps, but the isolated trial daemon was
killed with exit 137 before results: Rust had 32,959/306 tokens (3 rows),
Python 57,164/492 (5 rows), both at cost unavailable. The durable group and
parent turn were still nonterminal when inspected. No source/test/commit result
or integration is claimed from this run. This host was concurrently running
the active user daemon; it was not stopped or modified.

The reconciled branch reran M005's independent Darwin suites: attach/resume
19/19, interactive sessions 11/11, managed descendants 6/6. The final suite
passes by observing that a `setsid()` descendant outlives the managed job and
writes afterward. This corroborates the M005 no-go for S4; it does not block
isolated S2/S3 or qualify shared writes. M005 evidence is recorded in its plan.

S2 and S3 still have no pair with two passing child suites, two typed commits,
an authorized combined Git result, and a passing frozen evaluator. Therefore
the controlled S0-S3 comparison and additional overlapping API, sequential,
and dependency-heavy Rust strata remain unrun. M008 stays active; M005 stays
active independently; S4 stays unavailable. No downstream plan is unblocked.

### Local verification of child startup and result follow-up (2026-10-18)

After the group-readiness, typed child commit, bounded SQLite retry, and
child-specific usage-accounting changes, `scripts/verify.sh full` passed its
quick checks and all-target Clippy with `-D warnings`. The focused Darwin
`managed_process_descendants` suite passed 6/6 after canonicalizing temporary
paths in the test fixtures. A repeated full run then stopped during root test
harness compilation: the worktree's `target/` directory disappeared while
rustc was writing dependency metadata. Nextest did not begin on that run, so
this is not recorded as a completed exact-head suite. The prior run exposed a
probabilistic name-pool assertion; that test now checks deterministic lease
release/reassignment, and its focused rerun passed. Hosted CI on the pushed
revision remains required.

The local changes establish the intended write guidance: implicit `Ask` is
relaxed only for validated workspace and authorized scratch roots, while
explicit Ask/Deny decisions and path rules remain authoritative. The existing
permission regression suite covers these cases. These runtime and test changes
do not qualify an S2/S3 provider pair. No downstream plan is unblocked.

The draft PR's Desktop E2E run found that the artifact route test could click
before the connection generation settled, causing its project-detail result
to be discarded as stale. The resulting early failure also left one-time
projection mocks queued for later tests. The route fixture now waits for the
connected state before project selection, and the suite resets mock
implementations before each test. Local `npm test` passes 18/18; the updated
hosted E2E run is pending.

Hosted verify on `9ed8df7b` reached workspace tests and reported 12,456 passed,
1 failed, 6 skipped before stopping. The failure was the old
`user_prompt_queue` test pinning storage layout 82 after the new v83/v84
migrations; the assertion now expects the current v84. The next exact-head CI
run must confirm this correction.

### Completed frozen S0–S3, API follow-up, and exact-head qualification (2026-10-18)

The readiness and typed Git-result fixes landed in `9e109e7b` and
`475ce4a5`. On the integrated source head `803b586531ac9cd09cf268bed05c818e3cafc8a6`,
one successful provider-backed S0–S3 round used the same frozen fixture digest,
provider/model, and 128-call per-child cap. Both S2/S3 child pairs completed,
published commits, passed their own language tests, and passed the frozen
integrated evaluator. The per-child measurements and all IDs are recorded in
`plans/closure/agent-workstream-blackboard/008-isolated-results.md` and `.json`.
One sample per arm, unequal aggregate call caps, and unavailable provider USD
prevent an effectiveness/cost claim. Two 64-call sensitivity attempts were
incomplete and remain excluded.

The overlapping Rust API pair produced an expected `rust/src/lib.rs`
cherry-pick conflict. The two child changes were combined on the capable S0
result; 11/11 Rust tests and the frozen evaluator passed. The children could
not pass the seven starter producer tests before integration because those tests
exercise an intentional TODO. A follow-up child, started after both prerequisite
APIs were integrated, passed all 13 Rust workspace tests and the frozen
evaluator. The cold full-workspace `cargo check --locked --offline --workspace`
and the second serialized shared-target check both passed; detailed wall, CPU,
RSS, target-disk and lock-wait measurements are in the same results ledger.

Exact-head hosted qualification passed on `803b5865`: verify run `39171202574`,
Desktop E2E `39171202524`, managed-worktree `39171202506`, native-probe-contract
`39171202512`, and process-launcher `39171202606`; GitGuardian passed and the
C014 negative reproduction was skipped by workflow policy. M005 remains active
for independent writer-path/native lifecycle qualification; shared-write S4 is
unavailable. Registry audit: no downstream plan became newly eligible. M008
remains active for the other task strata, repeated controlled comparisons, and
broader resource measurements.

### All-agent budget calibration and headless turn ceiling (2026-10-19)

The initial repeated-budget setup exposed that an aggregate child ceiling alone
does not bound S1/S2/S3 coordinators. A calibrated IM-1 attempt used a
120,000-token child cap but no parent cap: the child consumed 132,104 tokens,
the coordinator consumed 249,041, and the combined 381,145-token trajectory
is excluded. The child changed both modules but did not commit or report test
results. Evaluator recovery ran the frozen suites (Rust 5/5, Python 5/5), made
a candidate commit, merged it without conflict, and reran both suites. Cost
was unavailable, integration time was not measured, and this is not an
autonomous-success or effectiveness observation. Full details are in
`plans/closure/agent-workstream-blackboard/008-isolated-results.md` and `.json`.

To cap root coordinator turns, `codegg headless` now accepts
`--max-model-tokens` (1–2,000,000). The daemon carries the optional limit
through `SessionPromptSubmit` and `TurnSubmit` into `ExecutionLimits`; absent
limits remain omitted in serialized requests so existing submission digests
remain stable. The active provider response completes before its usage can be
checked, so actual usage can exceed the requested ceiling by an in-flight
response. The CLI documents this. The protocol backward-compatibility test
and local root `cargo check` pass; full current-head CI is pending.

The frozen repeated protocol now compares a common all-agent ceiling `B` per
trial. S0 gives `B` to its direct coding turn. S1/S2/S3 reserve `C = 300,000`
for one coordinator turn and allocate `B - C` across delegated workers; unused
allocation is not transferred, and a coordinator must not continue in a
second turn. IM-1 uses `B = 700,000`; SI-1 uses `B = 1,400,000`. IM-2, SI-2,
OA-1, and OA-2 remain unavailable until their frozen evaluators and budgets
are added to `008-task-matrix-v1.md`. IM-1 S0 repetition 1 is the first scored
cell and is recorded in the closure evidence. All other repeated cells remain
outstanding. M005 remains separately gated by escaped-descendant writer
evidence, and no downstream plan is newly unblocked.

The rebuilt headless binary passed a protocol smoke with a 10,000-token turn
ceiling. The usage journal reported 10,850 total tokens (10,845 input, 5
output), confirming that one completed provider response can exceed the cap.
This is calibration-only evidence from the reused setup session; no scored
trial or additional stratum was completed. Exact usage and the unknown-cost
status are in `008-isolated-results.json`.

The first budgeted IM-1/S0 repetition now has a valid integrated candidate:
Rust and Python each pass 5/5 frozen tests at 373,768 actual input-plus-output
tokens under the 700,000 ceiling. The provider-usage span was 92.307 seconds;
the Python tool test was unavailable inside the agent run, so the evaluator
reran both suites and committed the candidate. This counts as one evaluator
intervention, not autonomous success. Cost is unknown and full integration
wall time was not instrumented. Repetitions and all other conditions remain
outstanding; M008 remains active.
