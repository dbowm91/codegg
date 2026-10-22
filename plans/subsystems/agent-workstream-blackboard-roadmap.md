# Agent Workstream Blackboard, Peer Awareness, and Workspace Placement Roadmap

Status: active; M001–M004, M006, M007 and M010 closed (M004 strict closure through corrective M009); ADR-0032 accepted 2026-10-15; M005 active for checkout-scoped writer-containment feasibility and qualification. Its writer-containment corrective closed **C001–C006** on 2026-10-21, with **C006 conditional**: the restricted shared checkout is now a real, opt-in, platform-gated path (Linux and macOS qualified on native containment evidence; Windows disabled with a named reason), while the WorkOrder `shared`/`serialized` mutation policy still fails closed. M007 per-workspace build admission and M006 isolated session/worktree UX are both closed (2026-10-19). **M008 closed on 2026-10-22** with its contract matrix frozen and guarded, its five configurations qualified against that matrix, and its measured comparison published with its denominators; the shared-checkout arm (S4) is qualified on Linux and macOS and **unrun on Windows**, where the shared profile has no filesystem-enforcement backend at all. M008 claims no live-model performance and no live two-agent daemon race — both are recorded as named non-claims. The macOS native containment evidence is **closed** (hosted lane run `39616896281` passed on 2026-10-21, `macos` joined the qualified table on that evidence, and the lane was re-run **on the flip itself** — `39627440691` — passing again with the gate asserting *available* on macOS).

Repository planning baseline: 171ef93b6ba67d50b0412ae23b7697eb02d072eb (main inspected 2026-10-16). Re-inspect exact working branch before implementation.

Normative sources:
- plans/000-long-term-specification.md §§4, 6, 9, 16–17, 26–29
- plans/001-terminology-and-domain-model.md §§5, 7–8, 12–13
- plans/002-long-term-roadmap.md phases 9–11
- plans/003-planning-process.md
- plans/adrs/ADR-0001-programmatic-tool-execution-authority.md
- plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md (accepted, retains precedence)
- plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md
- plans/adrs/ADR-0032-agent-workstream-coordination-and-shared-checkout-ownership.md (accepted; shared-write execution awaits M005 qualification)

Existing closed substrate (do not reopen closures): agent-run-worktree-concurrency M001–M009; background-jobs M001–M004; WorkOrders M001–M007 and its UX corrective. The separate async-background-jobs post-C005 PR #119 remains draft/conditionally closed and is NOT a prerequisite or implicit merge for this roadmap.

## Purpose, domain and user-facing outcome

Enable a strong primary agent to delegate bounded parallel work to peers that know each other's responsibility, live execution facts, blockers and relevant changes without polling or sibling chat. Preserve an efficient single-agent path, isolated-worktree default, and an optional explicitly serialized shared-worktree group for costly compiled repositories, without making shared writes unsafe or creating a second scheduler.

The daemon owns the coordination record and authorization; AgentRun/AgentRunGroup own execution lineage and joins; JobScheduler/JobStore own admission, attempts and build/process resources; WorktreeService owns physical checkout records and leases; typed Git execution owns repository mutations and integration; session projections and frontend surfaces are derived, not authoritative. A workstream is neither a new WorkOrder nor a file-based task tracker; don't replace eggplan plans/Todos with this state.

## Research basis and hypotheses (not performance promises)

- PASC, ESEM 2026, https://doi.org/10.4230/LIPIcs.ESEM.2026.18: two homogeneous agents on Python SWE-Bench Pro benefited from structured peer action observations; silent parallelism did not. Generalization to Rust, 3+ writers and different models is unproven.
- CooperBench team trajectories, https://huggingface.co/datasets/CooperBench/team-trajectories: a structured lead/member task list and scratchpad outperformed plain two-agent cooperative Git in one harness; different evaluation denominators and no causal isolation of each component.
- Google Research agent scaling, https://research.google/blog/towards-a-science-of-scaling-agent-systems-when-and-why-agent-systems-work/: performance depends on decomposition and coordination; sequential/tool-heavy work can lose to a single agent.
- Grite preprint, https://arxiv.org/abs/2606.19616: append-only coordination and task claims motivated by synthetic agents, not evidence that a new Git-ref coordination store should replace CodeGG's daemon.

Hypothesis: bounded peer observations and atomic task claims improve correctness/duplicate-work metrics more than naive fan-out, while sharing compiled workspaces only helps if build and mutation resources are actually coordinated. M008 must retain negative results.

## Current production evidence and known defects

- src/tool/task.rs supports spawn, spawn_many and bounded group joins, without a workspace placement parameter. Mutation-capable durable children always require a managed worktree in src/scheduler/executors.rs via SubAgentPool::requires_isolation; read-only children share an inherited root.
- crates/codegg-core/src/agent_run_group.rs owns membership and statuses; crates/codegg-core/src/agent_run_control.rs and src/agent/run_control.rs own bounded parent/child mailboxes, safe-boundary journal/notifications. There is no authoritative task-claim blackboard or model-facing peer action projection.
- crates/codegg-core/src/worktree_service.rs has managed checkout identity, generation-fenced leases and safe cleanup. A lease is currently effectively single-owner. Parent integration in src/agent/run_integration.rs requires an unchanged clean parent base; multiple child results and nested parent checkout must be qualified.
- SessionStore::fork retains the same directory/workspace; the CLI '--fork' and TUI '/fork' fork conversation, while '/worktree' only lists. There is no native session-fork-and-worktree unit.
- WorkOrder WorkspacePolicy AutoIsolated/Shared/Serialized is not the same as task-spawn placement. src/core/work_order_coordinator.rs computes is_mutation as NOT Shared, allowing shared write-capable work to take a ShareReadOnly branch. Its serialized contention key is the literal global exclusive:workspace-mutation, rather than scoped to a canonical workspace.
- Generic process tools and user-supplied Cargo commands have no workstream-aware compiler-target/cache placement. CARGO_BUILD_JOBS=2 in scripts/verify.sh is only repository verification convention.
- ADR-0002 requires distinct managed worktrees for concurrent mutation-capable children; any group-shared checkout exception is blocked on accepting ADR-0032 and proving an effective serial write boundary across all tool classes.

## Invariants and non-goals

- No second scheduler, group admission queue, generic workflow engine, agent chat platform or independent blackboard daemon.
- The workstream grants no new Git, filesystem, sandbox, approval, provider, network, principal or sibling-control authority. Agent self-report is never authoritative evidence.
- Read-only peer access is group/project/principal scoped and bounded. No raw transcripts, reasoning, prompts, secrets, arbitrary tool output or full local paths in snapshots.
- Idempotent creation, monotonic revisions, CAS claims, generation-fenced release, stable actor identity, recoverable notifications and crash-safe cleanup.
- A shared checkout is NOT equivalent to isolated worktrees; shared mutation must be explicit and serialized at the effective tool/process boundary, or denied. Do not implement simultaneous disjoint writers during this roadmap.
- Read-only work is never forced into an extra worktree; single-agent/solo operations are no heavier by default.
- No automatic cherry-pick/merge/commit, broad target sharing, mandatory sccache, hardcoded Rust detection, unsafe shared Cargo target, external cache service, network-dependent CI, permanent benchmark gate or arbitrary sibling mailbox.
- No automatic regression toward more subagents or an SBC-specific release/compile benchmark. Development-host measurements are enough initially.

## Dependency graph and statuses

~~~
existing durable AgentRun/Group/JobScheduler/WorktreeService (closed)
  |
  +-- M001 work-order shared/serialized authority corrective   [closed]
  |
  +-- M002 read-only durable workstream snapshot and evidence   [closed]
       |
       +-- M003 atomic task claims, dependencies, handoff       [closed]
            |
            +-- M004 peer observation and model-facing tools    [strictly closed via M009]
                 |
                 +-- M005 explicit group checkout placement     [active: corrective C001-C006 closed, C006 conditional; shared profile opt-in, Linux- and macOS-qualified]
            +-- M006 isolated session/worktree launch UX          [closed; M005 not required]
            +-- M007 per-workspace build admission                [closed; M005 not required]
            +-- M010 isolated Rust/Python trajectory               [closed; deterministic functional evidence]
            +-- M008 broad trajectory and economics                [closed; S4 qualified on Linux and macOS, unrun on Windows]
~~~

M001 was an independently eligible safety corrective on already-closed WorkOrders, and M002 did not depend on that correction. M005 became active after strict M004 closure and ADR-0032 acceptance; its shared-write capability remains gated on effective writer containment, which the corrective now provides on Linux and leaves conditional elsewhere. M006's isolated session ownership and M007's per-workspace scheduler quota do not depend on shared writes and may proceed alongside M005. M010 qualifies the closed M003/M004 substrate on isolated checkouts. M008 retained broad comparative and economic requirements; M006 and M007 both closed their half of that gate on 2026-10-19, and C006 discharged the shared-checkout gate for Linux and macOS, so M008 closed on 2026-10-22 with its shared-checkout arm qualified on those platforms and unrun on Windows.

## Milestones / deliverables

### M001 — WorkOrder workspace-policy correctness corrective (closed)

Repair the Shared classification so mutation capability is derived from effective agent/tool authority, not selected placement. Ensure Shared mutation is deliberately serialized or explicitly denied; scope scheduler exclusivity keys by canonical workspace/repository identity, and ensure all admission variants use identical keys. Test cross-workspace parallelism, same-workspace seriality, permission narrowing, non-Git behavior and plan/agent-run compatibility. Reference prior WorkOrders M002/M007 closures; preserve them as historical evidence. No blackboard schema.

Plan: plans/implementation/agent-workstream-blackboard/001-work-order-workspace-policy-corrective.md

Closure: plans/closure/agent-workstream-blackboard/001-status.md. Shared and Serialized mutations now fail closed because an ordinary AgentTurn can bypass the former global scheduler key. M005 owns the common reservation follow-up; the writer-path containment it required was delivered by corrective C002–C005, and the shared profile itself closed conditionally at C006.

### M002 — Durable read-only workstream projections (closed)

Derive a group-scoped peer board from existing run, task, group, worktree and job stores. Reuse the authorized session projection snapshot and its monotonic replay cursor; do not duplicate status truth or add a second journal. Support reconnection/restart/visibility semantics using one daemon authority. No mutation permissions, model integration, or shared worktree.

Plan: plans/implementation/agent-workstream-blackboard/002-durable-workstream-state.md
Closure: plans/closure/agent-workstream-blackboard/002-status.md. No persistence migration was necessary; facts remain derived from canonical stores and absent job/worktree records are explicit.

### M003 — Atomic responsibility claims, handoffs and dependency state (closed)

Implement typed task/path claims with revision-CAS, overlap and dependency checks, ownership transfer, lease expiry and cancellation, preserving provenance, audit and no-respawn semantics. Advisory task intent may be visible under isolated worktrees; claims do NOT themselves authorize writes or imply sandbox enforcement. No new workflow scheduler.

Plan: plans/implementation/agent-workstream-blackboard/003-task-claims-and-dependencies.md
Closure: plans/closure/agent-workstream-blackboard/003-status.md. Migration v79 adds bounded durable claims, dependencies and provenance; M002 now has an authorized storage-backed consumer. Claims remain advisory and do not grant shared-write access.

### M004 — Peer action observations and bounded agent integration (strictly closed via M009)

Host-computed, redactable observations from commits/status/test jobs/changed paths at safe loop boundaries; explicit read/claim/request-handoff tool actions and push-on-meaningful-change without per-agent polling. Bounded incremental state, relevance filtering, project/group privacy, hostile tool output tests. Preserve existing direct-parent control authorization.

Plan: plans/implementation/agent-workstream-blackboard/004-peer-observation-and-tools.md
Historical closure: `plans/closure/agent-workstream-blackboard/004-status.md` (conditional, preserved unchanged). Current strict status: `plans/closure/agent-workstream-blackboard/009-status.md`. Bounded result/claim peer hints, model actions, and read-only TUI display are implemented; a post-provider fence refuses the complete returned tool batch when relevant peer evidence or a continuity gap arrived in flight.

### Corrective M009 — Peer evidence before mutation (closed)

Close M004's named timing and qualification findings without changing the
in-flight provider transcript or adding mutation/control authority. Recheck
bounded same-group peer observations after a provider response and before
dispatching the returned tool batch; fence the batch if new evidence or a
continuity gap must be shown to the model. Add scripted two-worktree/provider
acceptance coverage for event timing, relevance, recovery, isolation, bounds,
and the single-agent path. The deterministic harness represents two group
members and host-owned fake Git evidence; physical worktree allocation remains
outside M004. The original 004 closure remains immutable historical evidence;
strict status is controlled by corrective closure 009.

Plan: `plans/implementation/agent-workstream-blackboard/009-peer-observation-before-mutation-corrective.md`.
Closure: `plans/closure/agent-workstream-blackboard/009-status.md`.

### M005 — Explicit group-owned worktree placement (active: corrective C001–C006 closed, C006 conditional; shared profile opt-in and Linux/macOS-qualified)

Add separate checkout ownership and participating execution access; task spawn/group placement choices (inherit read-only; auto isolated; new isolated; explicitly serialized shared worktree). With shared mutation, retain a single file/index/Git writer boundary covering shell, terminal, LSP, plugin, tool programs and subprocesses; fail closed where absent. Nested children inherit policy ceilings, not implicit leases. No simultaneous disjoint writers.

Plan: plans/implementation/agent-workstream-blackboard/005-group-worktree-placement.md

**M005 writer-containment corrective (2026-10-19 → 2026-10-21):** The source audit and native lifecycle record exposed an unresolved product-scope decision (independent same-user writers) and uncovered CodeGG-controlled non-broker and persistent-process write paths. A bounded corrective addendum owns **C001–C006**, starting with a ready writer capability/denial matrix and ADR-0047. C001 closed with the complete 48-root writer inventory; the maintainers then **accepted ADR-0047 option B on 2026-10-19**, and C002 closed on the same day with the durable generation-fenced checkout writer permit (authority, daemon adapter, bounded waiter, quarantine/recovery, additive migration v86, 33 qualification tests). C003 then routed every native file writer and the typed Git mutations through that permit, C004 mediated the fourteen non-broker checkout writers, and C005 refused unqualified and persistent processes on a reserved checkout — so **writers are fenced and the mediation is complete**. C006 closed the shared profile itself **conditionally** (2026-10-21): the restricted shared checkout is a real, **opt-in** production path (the durable `RunPlacement`, the group-authorized `SharedCheckoutCoordinator`, the daemon's `DaemonWriterFenceFactory`, the executor's shared branch and the task tool's `placement` parameter), **platform-gated** — Linux and macOS qualified, Windows disabled with a named reason because it has no filesystem-enforcement backend at all. The WorkOrder `shared`/`serialized` **mutation policy** still fails closed, so no WorkOrder agent is placed on a shared checkout; the shared profile is reached only through the explicit opt-in. The accepted restricted profile coordinates only CodeGG-managed final writers; unrelated OS programs are outside its guarantee. Strict parent M005 closure requires a positive C006 result and Q1–Q5, and C006's result was conditional rather than positive. Re-measured on 2026-10-21: the **composed two-agent race is now composed at the carrier level** — the real scheduler and the real executor, carrying the daemon's own coordinator and fence factory, place two group members on one checkout and their daemon-resolved fences queue behind each other on its permit — so what remains is a *live* two-agent race inside one daemon process, which needs a live model and the contained Git backend that does not exist here (pre-existing; it blocks the isolated delegated-run path identically). The platform evidence was also re-measured: **macOS** is now **qualified** — a hosted macOS lane and a macOS whole-file native fixture were added, the lane run `39616896281` passed on a native macOS runner (the native fixture 2/2, the C006 suite 16/16, the containment matrix 16/16), and `macos` joined the qualified table on that evidence — the lane was then re-run **on the flip itself** (`39627440691`) and on the reconciled head (`39635541949`), both passing with the gate asserting *available* on macOS — while **Windows** lacks a *capability*, because no filesystem-enforcement backend is registered for Windows at all, so no CI run could produce the evidence and the profile stays disabled there. A negative/unsupported outcome preserves isolated worktrees rather than redefining closure.

Corrective: plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md.
Decision: plans/adrs/ADR-0047-restricted-agent-shared-checkout-writer-contract.md (**accepted 2026-10-19**, option B — restricted CodeGG-managed coordination; enables no shared mutation).

### M006 — Isolated session/worktree fork-and-launch UX (closed)

Native daemon operation for 'new session in new/existing isolated worktree' and 'fork session in new/existing isolated worktree', with idempotent allocation/bind/rollback/recovery, TUI/CLI affordances and authorization. Session fork alone retains old semantics. Shared checkout execution is excluded and M005 is not a dependency.

Plan: plans/implementation/agent-workstream-blackboard/006-session-worktree-launch.md
Closure: `plans/closure/agent-workstream-blackboard/006-status.md`.

Closed 2026-10-19. The durable launch saga, the `worktree.create` authorization for the checkout half of a launch, the TUI surface and the CLI affordance are delivered. Three of work package E's cases need a fault injector the codebase does not have; they are recorded as a named gap in the closure record rather than claimed.

### M007 — Per-workspace build admission (closed; independent of M005)

Use the canonical scheduler to enforce a bounded per-workspace concurrent-build limit with correct waiting/cancellation/restart behavior and authorized coarse job status. No shared target, compiler cache, build coalescing, or shared checkout is introduced; those optimizations remain separate future work.

Plan: plans/implementation/agent-workstream-blackboard/007-build-resource-coordination.md

Closure: `plans/closure/agent-workstream-blackboard/007-status.md`.
The plan's deferred cache note is research only and does not authorize target sharing or coalescing.

### Corrective M010 — Isolated Rust/Python coordination trajectory (closed)

M010 closed the deterministic claim/worktree/Rust/Python/integration trajectory. The exact CI and known limits are recorded in `plans/closure/agent-workstream-blackboard/010-status.md`. It does not establish real-agent performance improvement or shared-write safety.

Plan: `plans/implementation/agent-workstream-blackboard/010-isolated-rust-python-trajectory.md`.
Closure: `plans/closure/agent-workstream-blackboard/010-status.md`.

### M008 — Comparative validation and closure (closed; S4 qualified on Linux and macOS, unrun on Windows)

Measure one strong agent vs silent isolated agents vs peer-aware isolated vs claims+peer-aware using identical task/evaluation budgets; include Rust and Python fixtures, overlapping API changes, failure/recovery, CPU/RSS/disk/token cost, useful throughput and integration outcomes. Any shared checkout arm runs only on a platform where M005 qualified it — Linux and macOS since C006 — and is marked unrun elsewhere; never emulate shared writes unsafely. Deterministic traces support CI; provider-backed experiments are separately reported. Negative/no-improvement results must be reported without hiding them.

Plan: plans/implementation/agent-workstream-blackboard/008-comparative-trajectory-qualification.md
Closure: plans/closure/agent-workstream-blackboard/008-status.md

**Closed 2026-10-22.** M008's first work package was to freeze the contract
every comparison arm runs against, and that freeze is now a document and a
guard: `architecture/blackboard_contract_matrix.md` names the six capabilities
the blackboard added, the authority that owns each one and the **production**
consumer that exercises it, and `scripts/check_blackboard_contract_matrix.py`
fails when a capability loses its production consumer, when a consumer
retreated into a test-only module, or when a row goes empty. The five
configurations are qualified against that matrix: S0 performs no blackboard
work at all, S1's worktree isolation is unchanged, S2/S3's peer observations
attribute the actual peer action and stay untrusted data, S3's claim
arbitration yields exactly one owner without widening a scope, and S4 places
two children on one shared checkout with the real writer fence serializing
them and both commits Git-verified. The measured comparison is published with
its denominators and its negative cases, and the Rust target-directory
contention measurement shows what M007's counted quota buys: the serialized
arm cannot wait for a lock it never contends for, while the concurrent arm's
second build does. The shared-checkout arm is **qualified on Linux and macOS
and unrun on Windows**, where the shared profile has no filesystem-enforcement
backend at all. M008 makes **no** live-model performance claim and **no** live
two-agent daemon-race claim; both are recorded as named non-claims.

## Security, persistence, protocol and operations

Use additive schema migrations only when canonical stores lack the information; one source of truth per fact. Additive native protocol shapes have typed IDs, revision, bounded batch/snapshot, explicit auth and redaction. TUI/ACP observe derived projections, never mutate database directly. Enforce group membership AND project/principal scope; cross-team projections are not implicitly public. Cancellation or daemon restart must fence stale actors; journal redelivery may duplicate transport but never logical mutation. Refuse gaps instead of inventing peer state. No background scan loop; derive updates from canonical state transitions, with bounded reconciliation.

Existing authoritative docs to update with implementations: architecture/agent.md, architecture/worktree.md, architecture/scheduler.md, architecture/session.md, architecture/projection.md, architecture/authorization.md, docs/tools.md, docs/cli.md and the agent/planning skills if contracts change.

## Qualification, stop rules and closure

Each milestone requires its own closure record at plans/closure/agent-workstream-blackboard/00N-status.md, exact commit/PR, requirement-evidence matrix, migrations, schema/transport compatibility, restart/CAS/cancellation/security/contention results, focused tests, static guards, documentation, limitations and downstream unblock audit.

Baseline verification: cargo fmt --all -- --check; narrow cargo test -p codegg-core / -p codegg; scripts/verify.sh quick; cargo clippy --workspace --all-targets --locked -- -D warnings where applicable; bash scripts/check-core-boundary.sh, scripts/check_scheduler_bypass.py, scripts/check_execution_ownership.py, plus changed-surface authorization/projection guards. Use existing hosted CI only when acceptance requires it; do not introduce a benchmark CI gate.

Stop and create a new corrective plan if acceptance needs a second admission authority; shared mutation can bypass the mandatory execution fence; any principal obtains unauthorized sibling visibility/control; source Git evidence is replaced by agent prose; or the benchmark silently changes task difficulty/budget. An opt-in feature may remain disabled if reliable cross-platform shared writes are not demonstrated. The rest of the blackboard may close independently.

## Registry/unblock responsibility

After M001 closure M005 remained blocked on M004 and ADR-0032. After M002 closure M003 became ready; after M003, M004 became ready. M004 is now strictly closed through M009 and ADR-0032 was accepted on 2026-10-15, making M005 eligible and active for containment feasibility and qualification. The shared-write capability remains unavailable until writer-path containment is verified. M010 is closed with deterministic functional evidence only; it unblocks no new milestone. M008 broad comparison waited on M006, with any shared-checkout arm separately gated by M005 — a gate C006 discharged on 2026-10-21 for Linux and macOS, where the shared checkout is now a real opt-in path; the arm stayed unrun on Windows. **M008 closed on 2026-10-22** with the contract matrix frozen and guarded, the five configurations qualified against it, and the measured comparison published with its denominators. Registry updates must occur in the same closure commit as milestone transitions, not based on plans merely being written.
