# Agent Runtime, Model Adaptation, and ACP Milestone 009 — Context Plan and Cache Convergence

Status: blocked — requires Milestones 001, 002, and 007 closure

Repository baseline: `672479726f1c79bbc931d70f084cd1649e8b2ed4`

Source roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-009--context-plan-and-cache-convergence`

Long-term requirements:

- `plans/000-long-term-specification.md#4.6-progressive-disclosure`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`

Related architecture:

- `architecture/cache-aware-context.md`
- `architecture/projection.md`
- `architecture/tool_programs.md`

Primary class: infrastructure/performance correctness

## 1. Objective

Converge prompt compilation, context blocks, conversation messages, tool definitions, artifact projection, compaction, tool-palette policy, cache telemetry, and control instructions into one typed `ContextPlan` that is actually consumed by provider requests.

The milestone must move beyond observation-only cache packing without introducing aggressive default omission. It must preserve chronological transcript and tool-call semantics, keep stable provider-cache prefixes deterministic, make active reductions reversible, and key cache observations by provider/model/adapter/prompt/tool identity rather than model name alone.

## 2. Dependencies

Hard dependencies:

- M001 canonical prompt compiler with deterministic blocks/fingerprint;
- M002 resolved base/reduced tool surfaces;
- M007 pinned declarative model adapter and control/message placement policy.

Soft dependency:

- M008 private reasoning preservation must be represented before M009 closes so context planning does not leak or reorder reasoning/tool history incorrectly.

Existing interfaces:

- `ContextBlock`, cache classes, packer, cache stats, usage normalization, effective-cost analysis, context policy, artifact handles/projection, compaction, context ledger/frame, tool-definition hash, and provider usage telemetry.

## 3. Current implementation evidence

Re-audit:

- context blocks distinguish stable prefix, slow-changing, volatile, and never-cache material;
- the packer sorts/budgets candidates and records omitted blocks;
- `AgentLoop::observe_context_pack` runs at several phases but is explicitly observation-only;
- candidate construction does not yet model the full live transcript as ordered blocks;
- production root prompt assembly concatenates memory, goals, LSP, Git, and mode guidance into one system string;
- tool-palette reduction is a separate gated active policy based on an unreduced base list;
- compaction and artifact projection operate through separate paths;
- cache stats are primarily per model and session-local;
- private reasoning from M008 requires special placement/visibility rules.

## 4. Invariants

- Chronological conversation/tool ordering remains provider-correct.
- Stable and slow-changing content is deterministic and precedes volatile content where provider semantics permit.
- Required system/tool/history content is never omitted by an experimental policy.
- Active reduction starts conservative, is disabled by default unless explicitly configured, and restores full authorized context/surface on uncertainty.
- Context planning cannot grant tools or authority.
- Large tool/artifact/source bodies remain bounded or handle-backed.
- Private reasoning remains provider-round-trip-only and follows adapter policy.
- Active turns pin prompt/compiler/adapter/asset/tool fingerprints.
- Cache metrics distinguish provider endpoint/API family, model, adapter version, compiler version, tool surface, and relevant reasoning mode.
- A context-planner failure falls back to a correct full request, not an empty or corrupted request.
- No separate transcript reducer becomes a second session authority.

## 5. Scope

### In scope

- Define `ContextPlan`, `ContextPlanBudget`, ordered segments/blocks, omission records, and stable identity.
- Build the plan from:
  - compiled prompt stable blocks;
  - model adapter/profile contract;
  - project instructions and skill metadata;
  - resolved tool definitions;
  - goal/memory summaries;
  - chronological provider message history;
  - tool results/artifact handles;
  - current Git/LSP/evidence context;
  - todos/task state;
  - recovery/control instructions;
  - private reasoning where adapter-required.
- Preserve separate stable, slow-changing, volatile, and never-cache/control classes while maintaining message chronology.
- Make provider request construction consume the plan.
- Integrate compaction and artifact projection as transformations over typed plan inputs/results rather than independent opaque string mutation.
- Integrate M002 tool-surface reduction and restore behavior.
- Define cache identity and metrics keyed by provider/model/adapter/prompt/tool/context mode.
- Add conservative active modes:
  - full/default;
  - observation;
  - bounded optional omission of explicitly non-required recoverable/summary blocks;
  - tool-surface reduction already governed by M002.
- Add fallback/backoff when required tools/context are missing or the recovery controller detects starvation.
- Add plan diagnostics and fingerprints without logging private bodies.

### Out of scope

- Semantic vector retrieval redesign.
- Automatic long-term memory policy changes.
- Aggressive transcript reordering or lossy deletion by default.
- Provider-side cache control APIs beyond existing supported request fields.
- A generalized prompt optimizer.
- Model benchmarking/online learning.
- Replacing artifact stores or session projections.

## 6. Required production changes

### Context plan types

Suggested shape:

```rust
pub struct ContextPlan {
    pub stable_blocks: Vec<ContextBlock>,
    pub slow_blocks: Vec<ContextBlock>,
    pub messages: Vec<PlannedMessage>,
    pub control_blocks: Vec<ContextBlock>,
    pub tool_definitions: Vec<ToolDefinition>,
    pub omissions: Vec<OmittedContextBlock>,
    pub stable_prefix_hash: String,
    pub tool_surface_hash: String,
    pub adapter_fingerprint: String,
    pub plan_fingerprint: String,
}
```

Do not force transcript messages into a global sort that destroys chronology. Use tiering around message envelopes or provider-specific request segments rather than sorting all volatile messages by hash/priority.

### Builder and transformations

Create one builder that receives immutable inputs from the turn runtime/agent loop. Transformations should be ordered and typed:

1. construct full plan;
2. project large recoverable outputs to handles;
3. apply required compaction to historical content when actual limits demand it;
4. apply optional conservative policy omissions;
5. apply model-adapter placement/serialization;
6. validate required content/tool-call pairing;
7. build provider request.

Each transformation records reason and before/after fingerprints/counts.

### Stable prefix design

Keep truly stable content first:

- harness/compiler version;
- role/model adapter contract;
- pinned project instructions;
- stable tool schemas when provider format permits.

Keep slow-changing summaries next when provider message semantics allow:

- tool surface metadata;
- goal/memory summaries;
- skill catalog.

Keep volatile current evidence, transcript tail, tool outputs, and control nudges later. Do not move a required tool result before its assistant tool call merely for cache stability.

### Cache identity and telemetry

Use a compound key including at least:

- provider connection/API family;
- model ID;
- adapter ID/version/fingerprint;
- prompt compiler version/fingerprint;
- tool-surface hash;
- reasoning/thinking mode;
- relevant provider cache mode.

Record input/output/cached/reasoning tokens and plan tier counts. Keep metrics bounded and avoid unbounded per-unique-prompt cardinality; aggregate by stable adapter/tool/compiler identity rather than full user content hash.

### Active policy/fallback

- default remains full/correct request;
- observation mode compares planned/full cost without mutation;
- active omission only affects blocks explicitly marked optional and recoverable/summary-only;
- required blocks and current user/tool protocol state are never omitted;
- empty/invalid result restores full plan;
- missing-tool/recovery signal disables reduction for bounded turns;
- all policy decisions are reversible per provider call.

### Compaction

Unify compaction triggers with actual adapter/model context limits. Compaction output must retain active task state, decisions, file paths, tool results needed for continuation, lineage, and private reasoning only when provider-required and policy-safe. Record compaction fingerprint/source generation.

## 7. Ordered work packages

### A — Inventory and context-plan contract

- inventory production request assembly, compaction, projection, cache observation, and palette-reduction paths;
- define plan segments, chronology rules, required/optional semantics, and compound cache identity;
- add failing fixture showing observation output differs from actual request structure.

### B — Full-plan builder and provider consumption

- build full correct plan from canonical prompt/tool/history/runtime inputs;
- make provider request assembly consume it;
- validate tool-call/result pairing and adapter reasoning placement;
- retain full-mode behavioral compatibility.

### C — Projection/compaction convergence

- route large outputs through artifact handles within the plan;
- move compaction into a typed transformation with actual model limit inputs;
- preserve required state and chronology;
- remove duplicate string-level mutations where safe.

### D — Conservative active policy and recovery

- integrate optional block omission and M002 tool reduction;
- add fallback/backoff/full restore;
- consume M006 starvation/missing-tool signals;
- keep active mode opt-in until closure evidence supports defaults.

### E — Telemetry, docs, and cleanup

- implement compound cache metrics with bounded cardinality;
- compare observed versus actual cached-token outcomes;
- update architecture docs;
- remove obsolete observation-only duplicate builders or retain one diagnostic wrapper over actual plan construction.

## 8. Failure, cancellation, restart, and contention semantics

- Planning/building is deterministic and cancellation-aware before provider call.
- Validation failure falls back to the full canonical request when safe; irreparable tool-history corruption returns a typed error.
- Concurrent turns use immutable snapshots/adapter/surface plans and isolated volatile history.
- Cache stats updates are race-safe and bounded.
- Restart reconstructs plans from durable session/provider messages plus current pinned assets according to existing session policy.
- A compaction failure retains prior valid history and either retries once through the ordinary bounded path or returns a context-limit error.
- Active reduction state/backoff is per session/lineage/model identity and bounded.

## 9. Compatibility

- Full/default mode initially reproduces current provider request semantics.
- Existing context-packer config maps to observation/full/active modes with deprecation documentation if fields change.
- Existing artifact handles and compaction agents remain reusable through adapters.
- Existing provider usage telemetry remains accepted and is normalized into richer keys.
- Existing sessions without adapter/private reasoning fields remain valid.
- No public projection DTO needs complete context-plan bodies.

## 10. Required tests

Focused:

- deterministic plan/fingerprints;
- stable/slow/volatile/control classification;
- chronological message and tool-call/result ordering;
- required block retention;
- artifact projection/handle substitution;
- compaction preserves active task/lineage/tool protocol;
- private reasoning placement/non-disclosure;
- compound cache key stability/cardinality bounds;
- optional omission and reasons;
- empty/invalid reduction full restore;
- M006 starvation backoff;
- context-limit calculation by adapter/model.

Production-shaped:

- multi-turn root with tools, child result, goal/memory, LSP/Git context, and compaction;
- same stable prefix across two turns with changed volatile tail;
- provider cached-token telemetry associated with the correct compound key;
- palette omission triggers recovery and next call restores full surface;
- Laguna interleaved history remains ordered.

Negative/security:

- optional policy cannot omit current user request, unmatched tool result, required system contract, or parent authority guidance;
- plan diagnostics contain hashes/counts, not secret bodies;
- private reasoning absent from projection/ACP/log output;
- oversized volatile/artifact content is bounded;
- malicious tool output cannot alter plan metadata/classification.

## 11. Verification commands

```bash
cargo fmt --all -- --check
cargo test context::
cargo test agent::compaction
cargo test agent::loop
cargo test --test agent_loop_harness
cargo test --test asset_snapshot
cargo check --workspace
```

Add one context-plan integration target. Run one broad local library suite. Do not add repeated cache-performance CI; optional live provider cache evidence is manual and supplementary.

## 12. Acceptance criteria

- One `ContextPlan` is the actual provider-request source.
- Prompt, tools, history, artifacts, compaction, control instructions, and adapter placement converge through it.
- Full mode preserves correctness and compatibility.
- Stable/slow/volatile organization improves prefix stability without breaking chronology.
- Active omission is optional, bounded, recoverable, and full-restore safe.
- Cache telemetry uses bounded compound identity.
- Private reasoning and large content remain correctly contained.
- Observation diagnostics derive from the actual plan rather than a parallel approximation.

## 13. Stop conditions

Stop if:

- provider APIs require incompatible message ordering that cannot be represented by adapter placement rules;
- active planning would require reordering tool protocol messages;
- private reasoning retention/security remains unresolved from M008;
- compaction cannot preserve required active state without redesigning session storage;
- cache-key cardinality cannot be bounded;
- scope expands into vector memory/retrieval or automatic prompt optimization.

## 14. Closure evidence

Include:

- before/after request-assembly inventory;
- actual plan examples for full and reduced modes;
- chronology/tool protocol fixtures;
- stable prefix/cache identity evidence;
- compaction/artifact/recovery integration results;
- private-content negative evidence;
- focused and broad local verification results;
- optional live cache telemetry if available, clearly labeled;
- closure recommendation.
