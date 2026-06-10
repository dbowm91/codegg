# Cache-Aware Context Packer Plan

## Purpose

Implement the first architectural pass after the context ledger/artifact projection stabilization work.

The context ledger now gives codegg a recoverable artifact layer and compact model-facing projections for tool output. The next step is to make context construction cache-aware rather than only threshold-driven. The goal is to preserve stable provider prompt-cache prefixes, reduce volatile context churn, and make compaction decisions based on effective cost instead of raw token count alone.

This pass should introduce a `ContextBlock` model and a deterministic packer that assembles model requests in stable tiers:

```text
stable prefix blocks
  -> slow-changing session/project blocks
  -> active working-set blocks
  -> volatile recent transcript tail
```

This is not a full memory/retrieval rewrite. It is the first bounded context-packer layer that uses existing codegg state: system prompt, model profile, tool definitions, `ContextFrame`, recent messages, context ledger artifacts, and provider usage telemetry.

## Current repo state

Relevant existing systems:

- `src/agent/loop.rs`
  - Main `AgentLoop` request loop.
  - Builds provider requests and tool definitions.
  - Calls `compact_if_needed()` before provider calls and after tool results.
  - Records usage including provider cached tokens where available.
  - Stores projected tool results as artifact-backed `Message::Tool` content.
- `src/agent/compaction.rs`
  - Existing `ContextTracker`, compaction modes/policies, hybrid compaction, `ProgrammaticCompactionState`, evidence refs, and context-frame injection.
- `src/agent/context_frame.rs`
  - Live `ContextLedgerState` and `ContextFrame` with touched files, commands, test results, unresolved errors, security findings, next steps.
- `src/context/`
  - Artifact store, typed `ContextHandle`, projection, and `context_read` recovery tool.
- `src/tool/mod.rs`, `src/tool/factory.rs`, `src/tool/catalog.rs`
  - Tool registry, deferred tool discovery, tool-search catalog, and session-aware `context_read` registration.
- `crates/codegg-config/src/schema.rs`
  - Existing `[context]`, `[compaction]`, `[tool_deferral]`, and model-profile configuration surfaces.
- `architecture/context-ledger.md`
  - Current context ledger documentation.

## Non-goals

Do not implement vector search or semantic embeddings in this pass.

Do not replace the entire compaction engine at once.

Do not make SQLite artifact persistence mandatory for this pass.

Do not rewrite provider adapters.

Do not change the visible behavior of tool execution except for context packing and diagnostics.

Do not aggressively minify code/text in ways that harm model performance. This pass is about stable ordering, block metadata, recoverability, and cache-aware omission/reduction.

## Design principles

1. Stable bytes are valuable.

Provider prompt caching rewards stable, repeated prefixes. Avoid rewriting, reordering, or regenerating stable instruction/context blocks unless their source material changes.

2. Volatile context belongs late.

Recent user/assistant/tool messages, ephemeral nudges, post-tool continuation instructions, and changing todo reminders should appear after stable blocks.

3. Handles beat blobs.

Large recoverable artifacts should be represented by compact summaries and `ctx://` handles, not repeatedly inlined.

4. Compaction should consider effective cost.

A large cached stable prefix may be cheaper than a small uncached volatile tail. Raw token count is not enough.

5. Pack deterministically.

For a given input state, block order and block text should be deterministic. Stable block hashes should change only when source content changes.

## Phase 1: Add `ContextBlock` types

Add a new module. Suggested location:

```text
src/context/block.rs
```

or, if agent-owned feels cleaner:

```text
src/agent/context_packer.rs
```

Prefer `src/context/block.rs` plus `src/context/packer.rs`, because this should eventually outgrow the agent loop.

Suggested core types:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContextBlockId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextBlockKind {
    SystemPrompt,
    ModelProfile,
    ToolDefinitions,
    ProjectInstructions,
    SessionFrame,
    GoalContext,
    MemoryContext,
    ActiveWorkingSet,
    UserMessage,
    AssistantMessage,
    ToolResult,
    ControlInstruction,
    TodoReminder,
    ArtifactSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheClass {
    StablePrefix,
    SlowChanging,
    Volatile,
    NeverCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lossiness {
    Lossless,
    ProjectedRecoverable,
    SummaryOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBlock {
    pub id: ContextBlockId,
    pub kind: ContextBlockKind,
    pub cache_class: CacheClass,
    pub lossiness: Lossiness,
    pub priority: i32,
    pub estimated_tokens: usize,
    pub content_hash: String,
    pub source_handle: Option<String>,
    pub text: String,
}
```

Notes:

- `id` should be stable across turns for stable blocks, e.g. `system:build:gpt-5.5`, `tools:hash:<toolset_hash>`, `frame:session:<session_id>`.
- `content_hash` should hash the final text, not only metadata.
- `estimated_tokens` can use the existing estimator for now.
- `source_handle` should point to `ctx://` artifacts or other recoverable sources when applicable.

Tests:

- Stable block id/hash does not change for identical content.
- Hash changes when text changes.
- Serialization roundtrip works.

## Phase 2: Add `ContextPacker`

Create a deterministic packer that accepts candidate blocks and emits ordered blocks under a budget.

Suggested API:

```rust
#[derive(Debug, Clone)]
pub struct ContextPackBudget {
    pub max_tokens: usize,
    pub reserved_output_tokens: usize,
    pub emergency_margin_tokens: usize,
}

#[derive(Debug, Clone)]
pub struct ContextPackResult {
    pub blocks: Vec<ContextBlock>,
    pub estimated_tokens: usize,
    pub omitted_blocks: Vec<OmittedContextBlock>,
    pub stable_prefix_tokens: usize,
    pub volatile_tokens: usize,
}

#[derive(Debug, Clone)]
pub struct OmittedContextBlock {
    pub id: ContextBlockId,
    pub kind: ContextBlockKind,
    pub estimated_tokens: usize,
    pub reason: OmissionReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmissionReason {
    OverBudget,
    LowerPriority,
    VolatileOverflow,
    ReplacedByHandle,
}

pub struct ContextPacker;

impl ContextPacker {
    pub fn pack(blocks: Vec<ContextBlock>, budget: ContextPackBudget) -> ContextPackResult;
}
```

Initial packing rules:

1. Sort by cache tier first:
   - `StablePrefix`
   - `SlowChanging`
   - `Volatile`
   - `NeverCache`
2. Within each tier, sort by priority descending, then stable id ascending.
3. Always keep required stable blocks unless doing so would exceed hard context limit; in that emergency case, return an explicit diagnostic rather than silently dropping system/tool-contract content.
4. Prefer dropping low-priority volatile/control blocks before stable blocks.
5. Keep recent transcript tail order intact. Do not reorder user/assistant/tool messages within the transcript tail even if their block ids sort differently.

Important: provider message order still matters. The packer can order block-level system/control context, but it must not violate provider chat semantics. For this first pass, use the packer mainly for system/control/context-frame material and diagnostics; transcript message packing should be conservative.

Tests:

- Stable blocks come before volatile blocks.
- Low-priority volatile blocks are omitted before high-priority slow-changing blocks.
- Omitted blocks include reasons.
- Transcript-tail order remains intact for message blocks.
- Budget accounting includes reserved output tokens and emergency margin.

## Phase 3: Build block candidates from existing state

Add a builder that constructs candidate blocks from existing runtime state without changing behavior yet.

Suggested module:

```text
src/agent/context_block_builder.rs
```

or methods near `AgentLoop` if simpler.

Initial candidate blocks:

- System prompt block.
- Model profile/tool policy block, if there is stable model-profile text currently injected.
- Tool definition block metadata or summary block, not necessarily full schema text yet.
- `ContextFrame` block.
- Goal context block, if present.
- Memory context block, if present.
- Todo reminder/control instruction blocks.
- Artifact summary blocks for recent important artifacts, if already model-visible.

Do not attempt to convert every chat message to blocks in this phase unless straightforward. The first safe integration can pack additional control/context material before it becomes provider messages.

Block construction requirements:

- Stable blocks must use deterministic rendering.
- Avoid timestamps or unordered map iteration in stable block text.
- Sort lists before rendering when semantic order does not matter.
- Preserve chronological order for commands/test results/recent events where order matters.
- Content hash must be based on final rendered text.

Tests:

- Two builds from identical state produce identical block ids/hashes/order.
- Changing `ContextFrame.touched_files` changes only the frame block hash.
- Empty frame does not create a noisy block.

## Phase 4: Integrate packer in observation mode

Before changing request construction, wire the packer in observation/diagnostic mode.

In `AgentLoop::run()` before provider request dispatch:

1. Build candidate blocks from current state.
2. Pack them using configured budget.
3. Emit diagnostics/logging about:
   - total candidate tokens,
   - packed tokens,
   - stable prefix tokens,
   - volatile tokens,
   - omitted blocks,
   - hashes for stable blocks.
4. Do not change provider request messages yet, except optionally behind an experimental flag.

Suggested config:

```toml
[context_packer]
enabled = false
observe_only = true
stable_prefix = true
max_stable_prefix_tokens = 32000
max_volatile_tokens = 24000
log_diagnostics = true
```

If adding a new config section is too much for this pass, place this under existing `[context]`:

```toml
[context]
cache_aware_packer = false
cache_aware_observe_only = true
```

Prefer a new `[context_packer]` section if the config layout can tolerate it; this is distinct from artifact projection.

Tests:

- Observation mode does not modify outgoing `ChatRequest` messages.
- Diagnostics can be produced from a synthetic agent-loop state.
- Packer can be disabled cleanly.

## Phase 5: Add provider usage feedback model

Create a small telemetry model that consumes existing provider usage records, especially `cached_tokens` where available.

Suggested type:

```rust
#[derive(Debug, Clone, Default)]
pub struct ContextCacheStats {
    pub last_input_tokens: usize,
    pub last_cached_tokens: usize,
    pub last_output_tokens: usize,
    pub rolling_cache_hit_rate: f64,
    pub rolling_uncached_input_tokens: usize,
}
```

Integrate with existing usage recording in `AgentLoop` where cached token data is already processed.

First-pass behavior:

- Track rolling cache hit rate per session/model in memory.
- Expose it to context-packer diagnostics.
- Do not yet make aggressive omission decisions based on it.

Later passes can use it to decide when preserving a large stable prefix is cheaper than rewriting summaries.

Tests:

- Cache hit rate updates from synthetic usage samples.
- Missing cached-token fields are treated as zero/unknown without panics.
- Rolling stats are bounded and deterministic.

## Phase 6: Strengthen tool-definition cache identity

The current tool-definition cache has historically risked stale invalidation if it keys on counts rather than tool identity/schema content. This pass should add a deterministic toolset hash, even if not yet fully used for provider caching decisions.

Add helper:

```rust
pub fn tool_definitions_hash(definitions: &[ToolDefinition]) -> String
```

Hash should include:

- tool name,
- description,
- parameters schema JSON in canonical order if possible,
- exposure/deferred status if available.

If canonical JSON sorting is not currently available, serialize with stable map ordering or add a small canonicalization helper.

Use this hash for:

- `ContextBlockId` for tool definitions,
- diagnostics,
- cache invalidation where currently safe.

Tests:

- Same tool definitions in same order produce same hash.
- Description/schema changes alter hash.
- Reordering definitions either does not alter hash if sorted before hashing, or the behavior is explicitly documented. Prefer order-insensitive hashing by sorting by tool name.

## Phase 7: Optional minimal active integration

After observation mode is working and tests are stable, add one low-risk active behavior behind a feature/config flag.

Recommended active behavior:

- Replace repeated context-frame/control-instruction injection with a packed `ContextFrame` block when `[context_packer].enabled = true`.
- Keep transcript messages and tool-call pairing unchanged.
- Do not alter system prompt/tool definition behavior yet.

This gives an end-to-end path without risking provider message validity.

Acceptance for active mode:

- Tool-call/result pairing remains valid.
- Existing compaction tests continue passing.
- Packed context-frame content is equivalent to current frame control text, just emitted through the packer.
- Disabling the packer restores old behavior.

If active integration becomes messy, leave this phase unimplemented and stop at observation mode. Observation mode still has value because it validates block construction and cache diagnostics.

## Phase 8: Documentation

Add or update:

```text
architecture/cache-aware-context.md
```

and link from `architecture/context-ledger.md` if appropriate.

Document:

- Difference between artifact projection and cache-aware packing.
- Context block tiers.
- Why stable prefix preservation matters.
- What observation mode reports.
- What is not implemented yet.
- How this prepares for future effective-cost compaction.

Also add a short config example if new config fields are introduced.

## Phase 9: Tests and validation

Run:

```bash
cargo fmt
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features context
cargo test --workspace --all-features
```

If full workspace tests are too expensive or have unrelated failures, document exact commands and failures.

Minimum targeted tests:

- `ContextBlock` hashing and serialization.
- Deterministic pack ordering.
- Budget omission reasons.
- Transcript tail ordering preservation.
- Stable/volatile token accounting.
- Candidate block builder determinism.
- Observation mode does not mutate request messages.
- Cache stats rolling update.
- Tool-definition hash stability and sensitivity.
- Config defaults and disabling behavior.

## Acceptance criteria

This pass is complete when:

1. `ContextBlock` types exist and are tested.
2. `ContextPacker` deterministically orders and budgets candidate blocks.
3. Candidate blocks can be built from current agent/session state.
4. Observation mode reports packed/stable/volatile/omitted diagnostics without changing request behavior.
5. Provider cached-token telemetry is captured into a small rolling stats model.
6. Tool definitions have a deterministic content hash suitable for cache identity.
7. Optional active mode, if implemented, only affects low-risk context-frame/control injection and is fully gated by config.
8. Documentation explains the new cache-aware context layer and its current limits.
9. Formatting, clippy, and targeted tests pass.
10. Existing context ledger/artifact projection behavior does not regress.

## Suggested stopping point

Stop after observation mode plus diagnostics, unless the minimal active context-frame integration is clearly low-risk.

The follow-up pass should use these diagnostics to implement actual effective-cost compaction decisions:

- preserve cached stable prefixes when beneficial,
- compact volatile middle/tail first,
- avoid regenerating summaries that break cache reuse,
- dynamically choose tool palettes by phase,
- feed provider-specific pricing/cached-token discounts into packing decisions.
