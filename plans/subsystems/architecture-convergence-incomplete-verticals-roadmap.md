# Architecture Convergence and Incomplete Verticals Roadmap

Status: active

Repository baseline reviewed: `3c4890035513cd4d74430b6f64523c8be676024e`

Long-term references:

- `plans/000-long-term-specification.md#1-product-definition`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#5-canonical-deployment-model`
- `plans/000-long-term-specification.md#9-project-repository-workspace-and-worktree-model`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Relevant closed dependencies and prior work:

- `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`
- `plans/closure/runtime-consolidation-deletion-footprint/010-status.md`
- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md`
- `plans/closure/agent-runtime-correctness-autonomy-simplification/013-status.md`
- `plans/subsystems/agent-run-worktree-concurrency-final-corrective-closure-addendum.md`
- `plans/closure/agent-run-worktree-concurrency/009-status.md`
- `plans/subsystems/session-projections-roadmap.md`
- `plans/subsystems/tool-programs-roadmap.md`
- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
- `architecture/agent.md`
- `architecture/git_phase_f_handoff.md`
- `architecture/git_polish_verification_handoff.md`
- `architecture/lsp.md`
- `architecture/protocol.md`
- `architecture/run_store.md`
- `architecture/server.md`

No new ADR is required to begin this roadmap. The work is intentionally constrained to converging existing ownership and completing already-established capability contracts. If implementation discovers that scheduler authority, durable identity, authorization, external protocol compatibility, or the canonical Git/worktree model must change, stop and register an ADR before widening scope.

## 1. Purpose and ownership boundary

CodeGG now contains most of the major product machinery described by the long-term specification, but several runtime domains remain split across adjacent abstractions and several vertical capabilities expose UI/protocol affordances without a complete production path. This roadmap closes those gaps without adding another scheduler, plugin runtime, tool runtime, agent hierarchy, memory system, or verification framework.

The roadmap owns four kinds of work:

1. make context/compaction ownership explicit and singular;
2. make process/tool execution and Git ownership explicit and singular;
3. reduce the root agent loop and command machinery to coordination over those owners;
4. complete existing vertical promises for rerun, LSP mutation application, and frontend-neutral projections.

The roadmap does not redefine the long-term architecture. It removes ambiguity inside it.

The governing rule is:

> Prefer one canonical production owner per mutable domain, adapters at the root/frontends, and completed vertical paths over additional parallel abstractions.

## 2. Current-state summary

At baseline `3c489003` the workspace contains extracted crates for `codegg-core`, `codegg-config`, `codegg-protocol`, `codegg-providers`, `eggcontext`, `egggit`, `codegg-git`, `egglsp`, and `eggsentry`, while the root crate still owns broad runtime behavior.

Material maintenance evidence includes:

- `src/agent/loop.rs` is roughly 200 KiB and remains the dominant runtime coordinator despite previous decomposition work;
- `src/agent/mod.rs` is also large and retains substantial runtime wiring/behavior;
- `src/agent/compaction.rs`, `src/agent/context_runtime.rs`, root `src/context/`, and `eggcontext` collectively represent overlapping context ownership;
- `src/tool/bash.rs`, `src/exec.rs`, tool backend/broker layers, shell-session behavior, and runtime-safety/process-control code collectively represent overlapping execution concerns;
- both `egggit` and `codegg-git` are direct workspace/root dependencies while root Git tools, mutation projection, run storage, and worktree orchestration remain substantial;
- command handling is distributed across `command/`, `command_intent/`, `command_planner.rs`, `command_routing.rs`, and `command_outcome.rs`;
- the protocol and TUI expose rerun semantics (`RunRerun`, `RunRerunLinked`, `can_rerun`) while the TUI production handler is still documented as a stub;
- LSP exposes safe preview-oriented mutation capabilities, but command-only/apply paths remain intentionally incomplete;
- frontend-neutral session projections are mature, but the TUI remains effectively the only complete product consumer and deprecated `/ws` compatibility remains present.

These are not eight unrelated features. They are manifestations of the same architectural phase: consolidation has established types and services, but root orchestration and complete consumer paths have not fully converged on them.

## 3. Invariants

All milestones MUST preserve these invariants:

- The daemon remains the production execution and durable-state authority.
- The scheduler remains the sole admission/resource authority for jobs and agent runs.
- Frontends render projections and issue typed requests; they do not become durable state owners.
- Workspace/project/session/run identity remains explicit and typed; process-global PWD inference must not spread.
- Git paths, branches, worktrees, and commits remain locators/state, not substitutes for durable identity.
- Child-agent authority can narrow but never widen parent authority.
- Tool authorization, sandboxing, cancellation, output bounds, and audit semantics must not be bypassed by consolidation.
- Provider session/run request context must continue propagating through all direct and indirect provider paths.
- Rerun must create new execution identity and preserve parent linkage; it must never mutate historical run records into a second execution.
- Secrets and sensitive Git/provider argv must not be persisted merely to make replay easier.
- LSP mutations must remain previewable and subject to the same edit authorization/history boundaries as ordinary edits.
- Deprecated compatibility transports must remain bounded and explicitly non-authoritative until removed.
- Verification stays deliberately light: focused tests plus existing `scripts/verify.sh quick` and existing hosted CI when strict closure requires it. No new CI lane, scanner, benchmark gate, coverage gate, dependency bot, or release automation is introduced by this roadmap.

## 4. Explicit non-goals

This roadmap MUST NOT:

- add another scheduler, worker pool, workflow/DAG engine, team runtime, plugin runtime, MCP runtime, or tool registry;
- redesign the durable agent-run hierarchy or convergence engine;
- add a new memory or skill-learning subsystem;
- rewrite the provider abstraction or add provider-specific runtime forks;
- merge all workspace crates back into the root crate merely to reduce crate count;
- split code into crates solely for line-count aesthetics;
- remove compatibility paths without proving no supported caller depends on them;
- implement a full desktop/web/mobile frontend;
- make Windows a newly guaranteed support tier;
- add packaging/release automation or a fixed release cadence;
- use file-size reduction as a substitute for ownership correctness.

## 5. Dependency graph

```text
M001 context/compaction ownership ---------+
                                            |
M002 process/tool execution ownership -----+--> M004 AgentLoop coordinator reduction
                                            |
M003 Git ownership convergence ------------+
       |                                    |
       +------------------> M005 durable rerun

M004 ---------------------> M006 command pipeline convergence
M002 ---------------------> M007 controlled LSP mutation apply
session-projection closure -> M008 headless projection consumer + legacy transport disposition
```

M001-M004 are conditionally closed and M005-M008 are dependency-ready; M002
and M003 may execute in
parallel if separate implementation agents avoid overlapping root wiring edits,
and M008 is independent. M004 was hard-dependent on M001-M003 because
it should consume the converged owners rather than invent temporary extraction
targets.

M005 has a hard dependency on M003 because replay/rerun must use the final Git/run ownership boundary. M006 depends on M004 so command simplification does not churn an unstable AgentLoop interface. M007 has an interface dependency on M002's canonical execution/edit boundary. M008 depends only on the already-closed session-projection subsystem and can execute independently.

## 6. Ordered milestones

### M001 — Context and compaction ownership convergence

Status: conditionally closed — `plans/closure/architecture-convergence-incomplete-verticals/001-status.md`

Primary class: infrastructure / polish.

Make `eggcontext` plus one clearly identified root adapter the canonical owner of token accounting, context selection, compaction inputs/outputs, bounded summaries, and context-policy state. Remove parallel context-policy logic from `src/agent` and root `src/context` where it duplicates the canonical owner. Preserve agent-specific orchestration as a consumer, not a second context engine.

Exit condition: a contributor can answer “where does context policy live?” with one production owner, and the agent runtime consumes typed context results rather than reaching into several independent policy implementations.

### M002 — Process and tool execution ownership convergence

Status: conditionally closed — `plans/closure/architecture-convergence-incomplete-verticals/002-status.md`

Primary class: invariant / infrastructure.

Establish one canonical process-execution path underneath Bash, shell sessions, Tool Programs, and other command-running tools. Separate tool schema/authorization/dispatch from process lifecycle, sandboxing, cancellation, process groups, output bounds, and result capture. Reuse existing runtime-safety machinery and delete bypass/duplicate helpers where safe.

Exit condition: every production subprocess execution path has an explicit disposition and either uses the canonical execution service or is a documented justified exception.

### M003 — Git ownership convergence

Status: conditionally closed — `plans/closure/architecture-convergence-incomplete-verticals/003-status.md`

Primary class: invariant / infrastructure.

Clarify the boundary among `egggit`, `codegg-git`, and root adapters. Generic safe Git/process/domain primitives belong in `egggit`; CodeGG-specific worktree/run/mutation orchestration belongs in `codegg-git`; root tools/TUI/projectors become adapters. Remove forwarding or duplicate domain logic where evidence supports deletion.

Exit condition: new Git behavior has one obvious home and production mutation/worktree/rerun code does not independently reconstruct Git safety/provenance rules in the root crate.

### M004 — AgentLoop coordinator reduction

Status: conditionally closed — `plans/closure/architecture-convergence-incomplete-verticals/004-status.md`

Primary class: infrastructure / polish.

After M001-M003, reduce `AgentLoop` to lifecycle orchestration over canonical context, provider, tool execution, progress/recovery/convergence, persistence, and projection services. Extract state machines rather than arbitrary helper functions. Remove dead compatibility branches made unnecessary by prior closed consolidation work.

Exit condition: the root loop no longer owns substantial context policy, subprocess policy, Git policy, or duplicated outcome/recovery mechanics; it sequences typed service results and remains behaviorally equivalent for ordinary turns.

### M005 — Durable run rerun/replay completion

Primary class: capability.

Complete the existing run-rerun vertical slice from TUI/request through daemon/service/run-store execution. A rerun creates a fresh run, reacquires any required credentials, reconstructs safe execution inputs from durable non-secret state, records parent/child linkage, and emits existing projection events. Remove the current placeholder behavior.

Exit condition: an eligible completed run can be rerun end-to-end, produces a new durable run identity, and survives restart/linkage inspection without secret persistence.

### M006 — Command pipeline convergence

Primary class: infrastructure / polish.

Audit `command`, `command_intent`, planner, routing, and outcome layers and collapse overlapping phases into one typed command pipeline with explicit parse/intent/authorization/dispatch/result boundaries. Preserve externally visible commands and deterministic routing behavior.

Exit condition: command routing has one canonical state/data flow and no duplicate planner/router interpretation path remains in production.

### M007 — Controlled LSP mutation application

Primary class: capability.

Extend the existing preview-oriented LSP surface so supported rename/code-action workspace edits can be applied through the normal checked-edit authorization/history path after explicit preview/approval semantics. Keep command-only or server-side arbitrary-command actions denied unless they can be safely mapped into existing tool authority.

Exit condition: at least rename plus one edit-only code-action path can run preview -> authorize -> apply -> edit-history/projection end-to-end without bypassing normal mutation controls.

### M008 — Headless projection reference consumer and legacy transport disposition

Primary class: capability / polish.

Add a small non-TUI reference consumer for the canonical frontend-neutral session projection protocol, covering snapshot, incremental events, reconnect/resume, bounded artifact access, and terminal state. Use it to identify and remove TUI-specific leakage. Re-audit deprecated `/ws` JSON-RPC and raw compatibility channels and either remove unsupported production paths or document/test the bounded compatibility period.

Exit condition: session projections have a second real consumer and every deprecated transport path has an explicit keep/remove disposition backed by caller evidence.

## 7. Cross-cutting security and failure semantics

Consolidation must fail closed where ownership is ambiguous. A path discovered during M001-M004 that appears to bypass canonical authorization, sandboxing, Git provenance, or session/run context must be treated as a correctness finding, not preserved merely for compatibility.

Cancellation must remain structured from caller -> daemon/tool service -> process/agent run. Consolidation must not replace bounded cancellation with detached background tasks.

Restart semantics must remain durable for runs, reruns, projections, and edit history. The in-memory AgentLoop is never the authoritative record of completed/active durable work.

Sensitive values remain handle/reference based. Rerun is specifically forbidden from solving replay by storing raw credentials, authenticated remotes, authorization headers, or provider secrets.

## 8. Migration and compatibility policy

M001-M004 SHOULD be behavior-preserving migrations with deletion where current production caller evidence permits it. Existing public/config/protocol fields are not removed solely to make internal architecture cleaner.

M005 uses existing run/projection concepts wherever possible; additive protocol fields are allowed only if the current `RunRerunLinked`/run request contracts are insufficient.

M007 should prefer existing checked edit and LSP preview types rather than a parallel mutation model.

M008 is the only milestone explicitly authorized to remove deprecated frontend transport code, and only after repository/current-client evidence shows it is unsupported or can be migrated without violating the documented compatibility contract.

## 9. Verification posture

Each milestone implementation plan defines focused commands. Broad verification remains the repository's existing minimal posture:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Use narrower package/test targets during implementation. Hosted `CI / verify` on the exact candidate is closure evidence when the repository's current closure conventions require it, not a reason to add new CI machinery.

## 10. User-visible exit conditions

This roadmap is complete when:

- ordinary agent turns retain behavior while core ownership is easier to reason about;
- context, subprocess, and Git policy each have one canonical production owner;
- `AgentLoop` is primarily an orchestrator over those owners;
- rerun works from the TUI through a durable fresh run rather than a stub;
- command routing has one canonical pipeline;
- supported LSP semantic mutations can be safely applied, not merely previewed;
- a second non-TUI projection consumer proves frontend neutrality;
- deprecated transport paths have an explicit evidence-backed disposition;
- no new scheduler/tool/plugin/memory runtime or verification framework was introduced.

## 11. Deferred work

The following remain intentionally deferred after this roadmap unless separately justified:

- packaging/distribution improvements such as crates.io/prebuilt binaries;
- expanding Windows from opportunistic compatibility to a guaranteed support tier;
- full web/desktop/mobile frontends;
- generic LSP arbitrary-command execution;
- broad provider capability taxonomy changes;
- additional agent-team abstractions;
- binary-size work not directly enabled by ownership consolidation.

These may be worthwhile later, but they are not prerequisites for closing the current maintenance and incomplete-vertical gaps.
