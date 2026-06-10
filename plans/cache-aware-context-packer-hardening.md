# Cache-Aware Context Packer Hardening Plan

## Purpose

Harden the first cache-aware context packer pass before any active context rewriting is allowed.

The repo now has useful scaffolding: `ContextBlock`, `ContextPacker`, `ContextBlockBuilder`, `ContextCacheStats`, tool-definition hashing, `[context_packer]` config, docs, and an initial `AgentLoop` observation path. The implementation is directionally right, but the current state is still diagnostic infrastructure, not safe active context mutation.

This pass should make observation diagnostics trustworthy, wire real cached-token telemetry, fix hashing stability, and disable/guard unsafe active mode. Do not implement effective-cost compaction yet.

## Current state summary

Relevant files:

- `src/context/block.rs`
  - Defines `ContextBlockId`, `ContextBlockKind`, `CacheClass`, `Lossiness`, and `ContextBlock`.
  - Uses `DefaultHasher` for block ids.
  - Has `source: String`, but no explicit `source_handle`.
- `src/context/packer.rs`
  - Deterministic sort by tier, priority, id.
  - Tracks stable/volatile token totals and omitted blocks.
  - Transcript-order test only checks presence, not ordering.
- `src/context/block_builder.rs`
  - Builds blocks for system prompt, profile, tool definitions, session frame, goal, memory, todo, control, artifact summaries.
  - Tool-definition block currently has empty text, so token accounting undercounts tool schemas.
- `src/context/cache_stats.rs`
  - Has in-memory cache stats and tests.
  - Does not appear to be wired to actual provider usage telemetry yet.
- `src/context/tool_hash.rs`
  - Hashes tool definitions with sorted names and canonical JSON shape.
  - Uses `DefaultHasher`, which is not appropriate for durable/cache identity.
- `src/agent/loop.rs`
  - Initializes `context_packer_config` and `ContextCacheStats`.
  - Runs observation early in `run()` before the main loop.
  - Optional active mode can replace system-message content with only packed frame text, which is unsafe.
- `crates/codegg-config/src/schema.rs`
  - Adds `[context_packer]` config.

## Non-goals

Do not implement effective-cost compaction in this pass.

Do not persist cache stats to SQLite yet.

Do not pack or reorder transcript messages.

Do not mutate provider request messages in active mode, except for a clearly safe no-op/disabled path.

Do not replace existing compaction logic.

Do not introduce vector/semantic retrieval.

## Phase 1: Hard-disable unsafe active mode

The current `observe_only=false` branch in `AgentLoop::run()` is unsafe because it can replace an entire system message with only the packed session-frame text when it finds `"Current session context:"`. That can delete the real system prompt, tool contract, security hints, steering, and other high-priority instructions.

For this pass, active mode should be disabled until a later plan introduces delimited section replacement.

Implementation options:

Preferred:

- Ignore `observe_only = false` for now.
- Emit a warning if active mode is requested:

```rust
if self.context_packer_config.enabled.unwrap_or(false)
    && !self.context_packer_config.observe_only.unwrap_or(true)
{
    tracing::warn!(
        "context-packer active mode is not yet safe; running in observe-only mode"
    );
}
```

- Remove or comment out the request-mutating active branch.

Alternative:

- Add a new config field `allow_unsafe_active_mode: Option<bool>` defaulting to false, and require it before mutation. Prefer not adding this unless necessary.

Tests:

- With `enabled=true, observe_only=false`, outgoing request messages are unchanged.
- A warning or diagnostic is produced when active mode is requested.
- Existing request/system prompt content is preserved exactly.

Acceptance:

- There is no code path where the packer can replace a full system prompt with only frame text.

## Phase 2: Replace unstable hashes with stable content hashes

`ContextBlockId` and `tool_definitions_hash()` currently use `DefaultHasher`. Rust's default hasher is not intended as a durable/content identity primitive. It should not be used for cache IDs or cross-run diagnostics.

Replace with a stable explicit hash.

Preferred dependency:

- Use `sha2` if already present.
- Use `blake3` if already present or acceptable to add.
- If avoiding new dependencies, implement a small wrapper around an already-used project hash utility if one exists.

Target helper:

```rust
pub fn stable_hash_hex(input: impl AsRef<[u8]>) -> String
```

Use it for:

- `ContextBlock.content_hash`, if not already using a stable hash.
- `ContextBlockId` derivation.
- `tool_definitions_hash()`.
- Any packer diagnostics that refer to hash identity.

Important:

- Preserve deterministic IDs across process restarts.
- Keep hashes long enough to avoid collisions in diagnostics. Prefer at least 16 bytes / 32 hex chars. Full SHA-256 hex is fine.

Tests:

- Same input hash is stable.
- Known test vector if using SHA-256.
- Tool-definition hash remains identical across reordered definitions.
- Block ID is deterministic and stable across test runs.
- Changing source/kind changes block ID.
- Changing block text changes content hash.

## Phase 3: Add `source_handle` to `ContextBlock`

The block model should connect to the artifact ledger rather than just having a free-form `source: String`.

Update `ContextBlock`:

```rust
pub struct ContextBlock {
    pub id: ContextBlockId,
    pub kind: ContextBlockKind,
    pub text: String,
    pub content_hash: String,
    pub estimated_tokens: usize,
    pub priority: u32,
    pub required: bool,
    pub lossiness: Lossiness,
    pub source: String,
    pub source_handle: Option<String>,
}
```

Keep `source` for stable identity labels. Add `source_handle` for recoverable context, usually `ctx://...`.

Builder changes:

- Existing non-artifact blocks use `None`.
- Artifact summary blocks should accept optional handle(s) or keep `None` until a richer artifact summary model exists.
- Do not fake handles.

Tests:

- Serialization roundtrip includes `source_handle`.
- Non-artifact blocks default to `None`.
- Artifact block can carry a `ctx://` source handle.

## Phase 4: Make tool-definition block account for real schema size

`ContextBlockBuilder::build_tool_definitions_block()` currently creates a block with empty text and a source based on the tool hash. That is useful for identity but bad for diagnostics because stable/slow-changing token counts undercount one of the biggest stable chunks: tool definitions.

Update it to render deterministic summary text for tool definitions.

Minimum acceptable text:

```text
Tool definitions hash: <hash>
Tools (<n>):
- bash: Run shell commands
- read: Read file contents
...
```

Better, if not too heavy:

- Include name, description, and compact canonical parameters hash per tool.
- Do not inline full schema JSON unless diagnostics need token realism. For token accounting, a `schema_tokens_estimate` could be included instead.

Recommended structure:

```text
Tool definitions hash: <hash>
Tools:
- <name> | defer=<...> | schema_hash=<...> | <description>
```

This preserves deterministic stable identity without flooding logs.

Tests:

- Tool block text is non-empty when definitions exist.
- Reordered definitions produce same block source/hash text order.
- Description/schema/defer changes alter the tool-definition hash.
- Estimated tokens are nonzero and roughly scale with tool count.

## Phase 5: Make observation diagnostics run at useful points

Currently observation appears to run once early in `AgentLoop::run()`, before the main tool loop and before most volatile context has accumulated. That limits usefulness.

Add a helper method on `AgentLoop`:

```rust
fn observe_context_pack(
    &self,
    request: &ChatRequest,
    model_profile: &ModelProfile,
    phase: ContextPackObservationPhase,
)
```

Suggested phase enum:

```rust
#[derive(Debug, Clone, Copy)]
enum ContextPackObservationPhase {
    InitialRequest,
    BeforeProviderCall,
    AfterToolResults,
    AfterCompaction,
    BeforeFinalization,
}
```

Call it at least:

1. After initial request construction.
2. Immediately before each provider call, after compaction/todo/control injections.
3. After tool results are appended/projected.
4. After compaction, if compaction ran.

Do not mutate requests in this helper.

Diagnostics should include:

- phase,
- model,
- candidate token estimate,
- packed token estimate,
- stable prefix tokens,
- slow-changing tokens if available,
- volatile tokens,
- omitted count,
- top omitted block ids/kinds/reasons,
- cache hit rate from `ContextCacheStats` if available,
- tool-definition hash.

Tests:

- Observation helper does not mutate request messages or tools.
- Diagnostics can be generated at multiple phases.
- Before/after tool result phases show different volatile estimates in a synthetic test.

## Phase 6: Wire provider cached-token telemetry into `ContextCacheStats`

`ContextCacheStats` exists but appears inert. Wire it into the existing provider usage path in `AgentLoop`.

Find where usage is recorded, including cached tokens if providers return them. On each provider response/usage update:

```rust
self.context_cache_stats.record_usage(
    &request.model,
    input_tokens as usize,
    cached_tokens.map(|v| v as usize),
    output_tokens as usize,
);
```

Requirements:

- Missing cached-token fields should be treated as `None`/zero.
- Avoid panics on negative or unavailable values.
- Track per model key.
- Include cache stats in context-packer diagnostics.

Tests:

- Synthetic usage with cached tokens updates stats.
- Missing cached tokens update input/output counts and cache hit rate remains 0.
- Multiple models remain independent.
- Observation diagnostics include cache hit rate when stats exist.

## Phase 7: Fix transcript-tail semantics before any transcript packing

The current `transcript_order_preserved()` test only checks containment, not ordering. Since the packer sorts by tier/priority/id, transcript order is not guaranteed.

For this pass, explicitly prevent transcript messages from being actively packed/reordered.

Options:

1. Do not build `UserMessage`, `AssistantMessage`, or `ToolResult` candidate blocks from live transcript yet.
2. Add a `TranscriptTail` wrapper that preserves chronological order as a single block.
3. Add `order_index: Option<usize>` to `ContextBlock` and teach the packer to preserve order within transcript-tail blocks.

Preferred for this pass: option 1 or 2. Keep it simple.

Update tests:

- Remove/rename misleading `transcript_order_preserved()` if transcript packing is not implemented.
- Add test proving the packer does not claim transcript order preservation unless using an explicit ordered wrapper.
- If implementing `TranscriptTail`, test exact order preservation.

Acceptance:

- No active path can reorder provider chat messages.
- Tests accurately reflect this.

## Phase 8: Improve config semantics and docs

Update `[context_packer]` docs and config semantics.

Recommended semantics for now:

```toml
[context_packer]
enabled = false           # default false
observe_only = true       # forced true internally for now
stable_prefix = true
max_stable_prefix_tokens = 32000
max_volatile_tokens = 24000
log_diagnostics = true
```

Document clearly:

- Active mutation is disabled for now.
- Observation mode does not change requests.
- Diagnostics are used to inform later effective-cost compaction.
- Tool-definition block token counts are approximate summaries, not full schema inlining.
- Cache stats are in-memory and per process/session for now.

Update:

- `architecture/cache-aware-context.md`
- `architecture/context-ledger.md` cross-reference if needed
- `.opencode/skills/context/SKILL.md` if it describes active behavior
- Any config example docs

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

- Active mode requested but request unchanged.
- Stable hash helper test vectors.
- Block ID/hash stability across deterministic inputs.
- Tool-definition summary block non-empty and deterministic.
- Tool-definition hash remains order-insensitive.
- Observation helper does not mutate request.
- Observation diagnostics include phase and cache stats.
- Cache stats update from synthetic provider usage.
- Transcript order test is corrected or transcript tail is explicitly modeled.
- Existing context ledger/artifact projection tests still pass.

## Acceptance criteria

This pass is complete when:

1. Packer active mode cannot mutate provider requests unsafely.
2. Observation mode is the only effective mode unless a future explicit safe active path is implemented.
3. Block/tool hashes use stable content hashing, not `DefaultHasher`.
4. `ContextBlock` can carry an optional recoverable `source_handle`.
5. Tool-definition block diagnostics account for real toolset size and identity.
6. Observation diagnostics run at useful phases, especially before provider calls and after tool results.
7. Provider cached-token telemetry updates `ContextCacheStats`.
8. Transcript messages are not reordered or falsely claimed to be order-preserved.
9. Docs accurately state that this is an observation/diagnostic layer, not active compaction.
10. Formatting, clippy, and targeted tests pass.

## Suggested stopping point

Stop once observation diagnostics are reliable and active mutation is disabled. The next pass can then implement effective-cost decisions using real diagnostics:

- preserve cached stable prefixes,
- compact volatile tail/middle first,
- avoid regenerating summaries that break prompt-cache reuse,
- introduce phase-scoped tool palettes,
- and eventually use provider pricing/cached-token discounts in packing decisions.
