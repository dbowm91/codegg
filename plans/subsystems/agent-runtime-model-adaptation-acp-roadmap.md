# Agent Runtime, Delegation, Model Adaptation, and ACP Roadmap

Status: closed — Milestones 001–011 closed

Long-term references:

- `plans/000-long-term-specification.md#4.4-frontends-render-projections`
- `plans/000-long-term-specification.md#8.7-project-authorization`
- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#23-acp-boundary`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-1--runtime-asset-registry-interoperability-and-refresh-correctness`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`

Related accepted architecture:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `architecture/agent.md`
- `architecture/cache-aware-context.md`
- `architecture/projection.md`
- `architecture/protocol.md`
- `architecture/tool_programs.md`

Primary specifications and research anchors:

- Agent Client Protocol: `https://agentclientprotocol.com/`
- ACP Rust library: `https://agentclientprotocol.com/libraries/rust`
- Poolside Laguna model cards and serving guidance: `https://huggingface.co/poolside`

## 1. Purpose and ownership boundary

This subsystem converges CodeGG's prompt construction, agent registry, tool surface, delegation, specialized security/research behavior, progress recovery, model-specific harness adaptation, context/cache planning, and ACP frontend integration into one coherent daemon-owned execution path.

It owns:

- one canonical prompt compiler used by root agents and descendants;
- deterministic agent inheritance and overlay resolution;
- one resolved capability and tool surface per turn;
- bounded nested delegation over the existing scheduler/subagent foundation;
- host-side specialized runtime behavior for security and research agents;
- observable progress detection and graduated loop/tool recovery;
- declarative model-adapter assets compiled into the binary;
- provider-neutral reasoning preservation required by interleaved-reasoning models;
- one context plan that coordinates prompt ordering, compaction, artifact projection, and cache policy;
- ACP v1 as a thin adapter over the native daemon protocol and session projections.

It consumes:

- immutable `ProjectAssetSnapshot` and runtime pins;
- explicit `ExecutionContext` and workspace identity;
- the existing `AgentLoop`, tool registry, permission checker, provider registry, scheduler, subagent pool, event log, and session projections;
- existing projection replay and transport ownership;
- provider-specific request/response adapters.

It does not own:

- replacing the singleton daemon or scheduler;
- a second ACP-specific agent runtime;
- team identity or project authorization implementation beyond preserving authority seams;
- worktree-native mutation isolation beyond retaining its required interfaces;
- a generalized workflow language;
- dynamic code plugins for arbitrary model adapters;
- exposing hidden chain-of-thought to users or frontends;
- broad CI expansion or automated release changes.

## 2. Current-state summary

The implementation is now converged on the target path:

- built-in and file-based agents resolve through generated assets and the
  canonical registry with bounded overlays and source-aware diagnostics;
- root and descendant turns use the same prompt compiler, model-adapter
  resolver, tool-surface resolver, and provider-facing context plan;
- shared bounded delegation, specialized security/research preparation,
  observable recovery, Laguna reasoning round trips, and compound cache
  identity are integrated with ordinary daemon ownership;
- ACP v1 is a thin stdio adapter over singleton-daemon sessions and
  canonical projections, with no independent runtime or durable state;
- the remaining long-term durable AgentRun/worktree/team authorization work
  remains owned by the broader Phase 9 roadmap and is not claimed here.

The roadmap corrects these seams without redesigning already-closed runtime-assets, session-projection, provider-connection, or tool-program subsystems.

## 3. Invariants

### Execution and ownership

- The daemon remains the sole session, provider, permission, scheduler, and durable event authority.
- Root and child turns use the same prompt compiler, tool-surface resolver, model-adapter resolver, and context-plan contract.
- Child authority is the intersection of principal/session policy, parent delegation, child definition, tool policy, workspace policy, and hard runtime limits.
- A child may narrow authority and must never widen it.
- Delegation, cancellation, retry, and frontend retransmission are idempotent where identities are available.
- Parent cancellation propagates to descendants and descendant-owned work by default.

### Prompt and tool correctness

- The prompt describes only tools and agents actually available for the turn.
- Canonical internal tool names remain stable; model-specific names exist only at provider/model adapter boundaries.
- Prompt, provider schema, permission checking, tool execution, and telemetry consume one resolved tool surface.
- Invalid agent or adapter definitions fail with source-aware diagnostics rather than silently degrading.

### Model adaptation

- Model adapters are declarative data with a versioned schema and deterministic precedence.
- Built-in adapter TOMLs are compiled into Rust during Cargo build and distributed in the binary.
- Build-time generation uses Rust/Cargo infrastructure and does not require Python at install time.
- Adapter behavior cannot bypass permissions, scheduler authority, tool provenance, or audit/event publication.
- Unknown models receive a conservative generic adapter.

### Context and disclosure

- Stable prompt content remains ordered before slow-changing and volatile context where provider semantics permit.
- Hidden/provider reasoning may be preserved opaquely for provider round-tripping but is not projected to ordinary clients, ACP, logs, or user-visible artifacts.
- Large context and tool outputs remain bounded or handle-backed.
- Active turns remain pinned to immutable asset snapshots and adapter fingerprints.

### ACP

- ACP is a frontend adapter over the native daemon protocol and canonical session projections.
- ACP code does not instantiate a second independent agent runtime or bypass native authorization, scheduler, workspace, agent-run, or audit boundaries.
- ACP stdout contains protocol frames only; diagnostics use stderr.
- Advertised ACP capabilities are truthful and unsupported optional operations fail explicitly.

## 4. Non-goals and complexity limits

- Do not migrate the complete transient subagent system to the final durable `AgentRun` database in one pass. This roadmap establishes correct bounded nesting and durable lineage seams; the long-term agent-run service remains the broader Phase 9 owner.
- Do not introduce a generic policy DSL. Use typed Rust structures populated from bounded TOML schemas.
- Do not permit arbitrary adapter scripting or runtime-loaded executable transformations.
- Do not create separate security/research schedulers. Specialized agents use the ordinary task, scheduler, tool, and event infrastructure.
- Do not add broad model/provider matrices to routine CI. Each adapter receives focused fixtures; external serving validation remains optional/manual.
- Do not add ACP web/network transports before stdio v1 is correct.
- Do not activate aggressive context omission by default. Active policies remain conservative, observable, reversible, and fallback-safe.

## 5. Target architecture

```text
TurnSubmit / descendant request
        |
        v
Resolved execution identity
  project + workspace + session + parent lineage + asset pin
        |
        v
AgentRegistryResolver
  built-in -> extends chain -> global -> project -> session -> hard ceiling
        |
        v
ModelAdapterResolver
  generic API -> provider -> family -> exact model -> user override
        |
        v
ResolvedToolSurface
  canonical tools + wire aliases + capabilities + omissions + fingerprint
        |
        v
PromptCompiler + ContextPlanner
  stable blocks + slow blocks + volatile blocks + transcript + controls
        |
        v
AgentLoop
  provider adapter <-> canonical tool calls <-> permission/tool registry
        |
        +--> ProgressRecoveryController
        |
        +--> shared bounded descendant spawner
        |
        `--> canonical events / projection / ACP adapter
```

Specialized runtime hooks operate before and after the ordinary loop rather than replacing it:

```text
SecurityReview: deterministic evidence preflight -> normal loop -> typed report
Research: bounded decomposition/evidence children -> normal synthesis loop -> typed report
```

## 6. Dependency graph

```text
M001 prompt compiler and agent registry correctness                 [closed]
        |
        v
M002 resolved capability and tool surface                          [closed]
        |
        v
M003 bounded nested delegation                                     [closed]
        |\
        | +--> M004 specialized security runtime                   [ready]
        | +--> M005 specialized research runtime                   [ready]
        |             \                                             /
        |              +----------------+---------------------------+
        v                               v
M006 progress, loop, and tool recovery controller                  [closed]
        |
        v
M007 declarative model-adapter registry and build generation        [closed]
        |
        v
M008 reasoning preservation and Poolside Laguna vertical slice      [closed]
        |
        v
M009 context-plan and cache convergence                            [closed]
        |
        v
M010 ACP v1 daemon/projection adapter                              [closed]
        |
        v
M011 integration evidence and closure                              [ready]
```

M004 and M005 may proceed in parallel after M003 if they use the same accepted specialized-runtime hook and report envelope contracts. M006 may begin after M002 but cannot close until descendant recovery/cancellation paths from M003 are represented.

## 7. Milestones

### Milestone 001 — Prompt compilation and agent registry correctness

Primary class: invariant/correctness.

Establish one production prompt compiler for root and child turns; correct agent overlay/inheritance, strict diagnostics, fallback-model semantics, remote-instruction handling, and asset-snapshot use.

Plan: `plans/implementation/agent-runtime-model-adaptation-acp/001-prompt-compilation-and-agent-registry-correctness.md`

Exit conditions:

- profile-aware prompt policy is on the production root and child paths;
- no separate thin descendant prompt path remains;
- documented merge/replace semantics match runtime behavior;
- custom agents can safely extend built-ins;
- invalid definitions fail explicitly;
- prompts are built only from the pinned turn asset snapshot and explicit context.

### Milestone 002 — Resolved capability and tool surface

Primary class: infrastructure/invariant.

Create one resolved capability/tool surface used by prompts, provider schemas, permissions, execution, model adapters, and telemetry.

Plan: `plans/implementation/agent-runtime-model-adaptation-acp/002-resolved-capability-and-tool-surface.md`

Exit conditions:

- tool availability is not inferred from names/roles;
- canonical and wire names are separated;
- prompt/schema/runtime disagreements are impossible by construction or diagnosed;
- palette reduction and model filtering derive from an unreduced base surface and restore safely.

### Milestone 003 — Bounded nested agent delegation

Primary class: capability/invariant.

Make descendant delegation functional through the shared pool/scheduler seam with explicit depth, fan-out, budget, lineage, permission/path ceilings, join, and cancellation behavior.

Plan: `plans/implementation/agent-runtime-model-adaptation-acp/003-bounded-nested-agent-delegation.md`

Exit conditions:

- at least three levels can execute in a focused fixture;
- read-only parents may delegate to narrower approved children;
- child authority cannot exceed parent authority;
- cancellation and duplicate spawn behavior are deterministic;
- the work does not claim completion of the final durable agent-run service.

### Milestone 004 — Specialized security-review runtime

Primary class: capability.

Turn `runtime_kind = security_review` into a host-side deterministic preflight and typed evidence/report workflow while retaining ordinary tools, permissions, and scheduler ownership.

Plan: `plans/implementation/agent-runtime-model-adaptation-acp/004-specialized-security-review-runtime.md`

Exit conditions:

- changed-file/hunk scope, deterministic scans, and LSP security evidence are assembled before synthesis;
- marker-only prompts remain distinct from findings;
- output is schema-validated;
- approved read-only security specialists may be delegated with depth one.

### Milestone 005 — Specialized research runtime

Primary class: capability.

Turn `runtime_kind = research` into a bounded coordinator/evidence workflow using explicit workspace context and structured child reports.

Plan: `plans/implementation/agent-runtime-model-adaptation-acp/005-specialized-research-runtime.md`

Closure: `plans/closure/agent-runtime-model-adaptation-acp/005-status.md`

Exit conditions:

- no process-global cwd is used for research service identity;
- research is read-only by default;
- bounded child scouts return evidence records rather than unstructured essays;
- synthesis distinguishes supported, conflicting, and unresolved claims.

### Milestone 006 — Progress, loop, and tool recovery controller

Primary class: reliability.

Replace single-pattern doom-loop termination with observable progress tracking and graduated recovery for exact repeats, equivalent errors, short cycles, malformed calls, missing tools, and narration without structured action.

Plan: `plans/implementation/agent-runtime-model-adaptation-acp/006-progress-loop-and-tool-recovery-controller.md`

Exit conditions:

- first incidents produce a bounded corrective nudge;
- repeated incidents restore tools or change execution constraints before termination;
- final failure is a typed stalled outcome with evidence;
- hidden reasoning is neither inspected nor exposed.

### Milestone 007 — Declarative model-adapter registry and build generation

Primary class: infrastructure.

Define versioned model-adapter TOML assets, deterministic matching/layering, canonical tool aliases, prompt/control behavior, provider-request transforms, recovery hints, and Rust build-time generation.

Plan: `plans/implementation/agent-runtime-model-adaptation-acp/007-declarative-model-adapter-registry.md`

Exit conditions:

- `assets/model-adapters/*.toml` compile into `$OUT_DIR` Rust through `build.rs` or an equivalent Cargo-owned path;
- unknown keys, conflicts, invalid regexes, and non-reversible aliases fail clearly;
- adapter ID/version/fingerprint enter runtime diagnostics and cache identity;
- unknown models use a conservative fallback.

### Milestone 008 — Reasoning preservation and Poolside Laguna vertical slice

Primary class: capability/compatibility.

Extend provider-neutral messages to preserve non-user-visible reasoning where required and implement the first complete Laguna adapter, including tool/argument aliases, thinking controls, interleaved reasoning history, and serving-requirement diagnostics.

Plan: `plans/implementation/agent-runtime-model-adaptation-acp/008-reasoning-preservation-and-poolside-laguna-adapter.md`

Exit conditions:

- reasoning can round-trip to a compatible provider without entering ordinary projections/logs;
- Laguna tool and reasoning behavior is represented declaratively except for bounded typed transforms;
- serving-stack requirements are diagnostic, not silently assumed;
- existing providers remain compatible.

### Milestone 009 — Context-plan and cache convergence

Primary class: infrastructure/performance correctness.

Create one context plan coordinating prompt blocks, messages, tool definitions, compaction, artifact handles, active palette decisions, and cache identities.

Plan: `plans/implementation/agent-runtime-model-adaptation-acp/009-context-plan-and-cache-convergence.md`

Exit conditions:

- actual requests consume ordered stable/slow/volatile context rather than observation-only duplicates;
- chronological transcript semantics remain correct;
- active omission is conservative and fallback-safe;
- cache metrics distinguish provider/model/adapter/prompt/tool fingerprints.

### Milestone 010 — ACP v1 daemon/projection adapter

Primary class: capability.

Implement `codegg acp` as a stdio ACP v1 adapter over native daemon operations and canonical session projections.

Plan: `plans/implementation/agent-runtime-model-adaptation-acp/010-acp-v1-daemon-projection-adapter.md`

Exit conditions:

- initialize, new, prompt, cancel, load, resume, close, and truthful capability negotiation work;
- session/tool/permission/plan/usage updates derive from canonical projections;
- stdout remains protocol-pure;
- cancellation reaches descendants;
- no second agent runtime or ACP-owned durable session state exists.

### Milestone 011 — Integration evidence and closure

Primary class: closure.

Reconcile architecture documentation, remove superseded paths, run focused cross-milestone fixtures, and produce an independent closure record.

Plan: `plans/implementation/agent-runtime-model-adaptation-acp/011-integration-evidence-and-closure.md`

Exit conditions:

- requirement-to-evidence matrix covers every milestone;
- dead prompt/runtime paths are removed or explicitly retained as compatibility seams;
- custom nested security/research agents and Laguna/ACP scenarios are demonstrated;
- no unresolved high/medium correctness finding remains;
- closure does not depend on broad CI expansion or external provider availability.

## 8. Cross-cutting implementation rules

### Storage and protocol

- M001-M002 should avoid new durable storage.
- M003 may add optional lineage/attempt metadata or consume existing task/event storage, but must not preempt the full long-term `AgentRun` schema without an explicit ADR.
- M004-M006 add typed internal events/results only where existing projection variants cannot represent status safely.
- M007-M009 persist only configuration or fingerprints where necessary; built-ins remain compiled assets.
- M010 adds ACP adapter state that is transient and reconstructible from native session/projection state.

### Compatibility

- Existing native protocol and TUI behavior remain operational throughout.
- Agent file formats remain readable; strict diagnostics may reject previously ignored invalid fields.
- Canonical internal tool names remain stable for permissions, plugins, logs, and tests.
- Provider message changes must retain serde compatibility or include an explicit migration/version boundary.
- ACP optional capabilities are additive and negotiated.

### Security

- Custom agents and adapters are untrusted configuration data.
- Prompt fragments cannot grant authority.
- Tool aliases cannot bypass permission checks or provenance.
- Child paths and tools are bounded by parent authority.
- Preserved reasoning is private provider-round-trip state.
- ACP inherits existing projection redaction and artifact-handle policy.

### Observability

Record bounded diagnostics for:

- resolved agent source/extends chain and digest;
- prompt compiler version/fingerprint;
- resolved tool surface and omissions;
- parent/child lineage and delegation rejection reason;
- specialized-runtime preflight/report status;
- progress-recovery state transitions;
- model adapter ID/version/match source;
- context-plan composition/cache identity;
- ACP session/request/native-turn correlation.

## 9. Verification strategy

Verification remains local-first and focused:

- each milestone adds unit tests for its pure contracts;
- each capability milestone adds one or two production-shaped integration fixtures;
- broad workspace checks run at milestone closure, not after every small edit;
- no new release automation is introduced;
- no external model or editor service is required for routine tests;
- Laguna and ACP interoperability use captured/golden protocol fixtures, with optional manual live validation documented separately;
- static guards are limited to high-value ownership invariants that normal Rust typing/tests cannot express.

## 10. Risks and mitigations

- **Risk: one roadmap spans several ownership boundaries.** Mitigation: each milestone is independently executable, ACP remains a thin adapter, and security/research share ordinary runtime contracts.
- **Risk: model adapters become a policy language.** Mitigation: fixed schema, no scripting, strict unknown-key rejection, typed transform enum.
- **Risk: nested agents recreate a scheduler.** Mitigation: shared pool/submission authority only; no specialized queues.
- **Risk: prompt convergence changes behavior broadly.** Mitigation: M001 captures golden root/child prompt fixtures and preserves immutable asset inputs.
- **Risk: active context packing drops required transcript state.** Mitigation: M009 starts conservative, preserves chronological message order, and restores full context on uncertainty.
- **Risk: reasoning preservation leaks hidden content.** Mitigation: private visibility type, negative projection/log tests, no user-facing serialization by default.
- **Risk: ACP duplicates projection logic.** Mitigation: event mapping consumes the canonical projection reducer/service and contains no independent state reducer.

## 11. Deferred work

- Full durable agent-run persistence/restart recovery beyond the bounded delegation seam remains owned by long-term Phase 9.
- Team/principal authorization completion remains owned by its subsystem; this roadmap preserves the authority intersection seam.
- Worktree allocation for mutation-capable descendants remains an interface dependency until worktree-native concurrency lands.
- Dynamic third-party adapter packages, remote adapter registries, and executable adapter plugins are deferred.
- ACP v2/draft features, network ACP transport, and editor-specific extensions are deferred until ACP v1 closure.
- Broad automatic model benchmarking or adaptation learning is deferred; adapters remain explicit maintained assets.
