# Context Ledger Architecture

Tool output artifact storage with in-memory storage and
token-budget-aware projection.

## Purpose

Reduces context window usage by compressing verbose tool outputs while
preserving diagnostic detail. Provides `context_read` for on-demand
artifact recovery. Separately, the `eggcontext` crate provides
deterministic token counting (see [Related Docs](#related-docs)).

**Session-local and in-memory.** Artifacts are not persisted across
sessions. `FileArtifactStore` exists as an alternative backing store.

## Where It Lives

### Application context module (`src/context/`)

| File | Purpose |
|------|---------|
| `mod.rs` | Module root, re-exports, integration tests |
| `artifact.rs` | `ContextArtifact`, `ArtifactKind`, store trait & impls |
| `handle.rs` | `ContextHandle` parser, error types, `clamp_to_char_boundary` |
| `projection.rs` | `ToolOutputProjection`, `ProjectionConfig`, `project_tool_output()` |
| `read_tool.rs` | `ContextReadTool` (Tool trait impl) |
| `block.rs` | `ContextBlock`, `CacheClass`, `ContextBlockId`, `Lossiness` |
| `block_builder.rs` | `ContextBlockBuilder` |
| `packer.rs` | `ContextPackBudget`, `ContextPackResult`, `OmissionReason` |
| `plan.rs` | `ContextPlan`, `ContextPlanMode`, `PlannedMessage` |
| `policy.rs` | `decide_policy`, `ContextPolicyDecision`, tool palette reduction |
| `cache_stats.rs` | `ContextCacheStats`, `CacheStatsEntry` |
| `effective_cost.rs` | `EffectiveCostAnalysis`, `EffectiveCostAction` |
| `tool_hash.rs` | `tool_definitions_hash` for stable tool-definition fingerprints |
| `usage_normalize.rs` | `normalize_from_finish`, `NormalizedProviderUsage` |
| `volatile_tail.rs` | Volatile-tail compaction (disabled by default, observe-only) |

### Token counting crate (`crates/eggcontext/`)

Deterministic token estimation. See [Related Docs](#related-docs).

## How It Works

### Artifact Lifecycle

1. **Tool executes** and produces raw output.
2. **`project_tool_output()`** detects kind from tool name, extracts
   metadata (touched files, commands, errors), and compresses output
   to fit a token budget.
3. **Artifact stored** via `ContextArtifactStore::put()` keyed by
   `ctx://` handle.
4. **Model sees** projected text with optional `ctx://` handle.
5. **Model recovers** full content via `context_read` tool.

### Handle Format

`ctx://tool/{session_id}/{turn_index}/{tool_call_id}`

Parsed by `ContextHandle::parse()` (`handle.rs`). Built by
`ContextHandle::build_tool()` which rejects `/`, control chars,
and whitespace in segments.

### Projection Logic (`projection.rs`)

When `project_tool_output()` is called:

1. **Detect kind** from tool name (`bash`/`exec` → ToolResult,
   `read` → ReadResult, `diff` → Diff, `webfetch` → WebFetch,
   `image` → Image).
2. **Extract metadata**: touched files, commands run, test results,
   errors.
3. **Project output**:
   - **Success**: pass through if ≤ `max_success_tokens`, else
     truncate to 20 lines with token count summary.
   - **Failure**: collect high-priority lines (errors, panics,
     tracebacks — deduplicated, capped 30) and medium-priority
     lines (warnings, test results — deduplicated, capped 20).
4. **Returns** `ToolOutputProjection` with `model_text`, `summary`,
   `status`, and metadata.

### Artifact Store Implementations

- `InMemoryArtifactStore` — `tokio::sync::RwLock<HashMap<String, ContextArtifact>>`
- `FileArtifactStore` — filesystem-backed (`artifact.rs:98`)

## Key Types & APIs

### ContextHandle (`handle.rs`)

```rust
pub struct ContextHandle {
    pub kind: ContextHandleKind,
    pub session_id: String,
    pub turn_index: usize,
    pub tool_call_id: String,
}
```

| Method | Signature | Notes |
|--------|-----------|-------|
| `parse()` | `fn(&str) -> Result<Self, ...>` | Validates format, rejects unsafe chars |
| `build_tool()` | `fn(&str, usize, &str) -> Result<String, ...>` | Checked construction |
| `same_session()` | `fn(&self, &str) -> bool` | Exact session match |

### ContextArtifact (`artifact.rs:22`)

```rust
pub struct ContextArtifact {
    pub handle: String,
    pub session_id: String,
    pub turn_index: usize,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub kind: ArtifactKind,
    pub created_at_ms: i64,
    pub content_hash: String,
    pub redacted_content: String,
    pub raw_bytes_len: usize,
    pub estimated_tokens: usize,
}
```

### ArtifactKind (`artifact.rs:9`)

```rust
pub enum ArtifactKind {
    ToolResult, CommandOutput, ReadResult, Diff,
    TestOutput, WebFetch, Image,
}
```

### ContextArtifactStore (`artifact.rs:36`)

```rust
#[async_trait]
pub trait ContextArtifactStore: Send + Sync {
    async fn put(&self, artifact: ContextArtifact) -> anyhow::Result<()>;
    async fn get(&self, handle: &str) -> anyhow::Result<Option<ContextArtifact>>;
    async fn list_recent(&self, session_id: &str, limit: usize)
        -> anyhow::Result<Vec<ContextArtifact>>;
}
```

### ProjectionConfig (`projection.rs:22`)

```rust
pub struct ProjectionConfig {
    pub max_success_tokens: usize,      // default: 800
    pub max_failure_tokens: usize,      // default: 2000
    pub enabled: bool,                  // default: true
    pub artifact_store_enabled: bool,   // default: true
    pub lossless_debug: bool,           // default: false
}
```

### ToolOutputProjection (`projection.rs:11`)

```rust
pub struct ToolOutputProjection {
    pub model_text: String,
    pub summary: String,
    pub status: ProjectionStatus,
    pub detected_kind: ArtifactKind,
    pub touched_files: Vec<String>,
    pub commands_run: Vec<String>,
    pub test_results: Vec<String>,
    pub unresolved_errors: Vec<String>,
}
```

### ContextReadTool (`read_tool.rs`)

Tool trait impl. Accepts `handle` (required), `offset` (default 0),
`max_bytes` (default 20000). Uses `ContextHandle::parse()` for exact
session matching and `clamp_to_char_boundary()` for safe UTF-8 slicing.

Registered when `artifact_store = true` regardless of `project_tool_outputs`.

### ContextLedgerState (`src/agent/context_frame.rs`)

Accumulates metadata across projections:

| Field | Cap | Dedup |
|-------|-----|-------|
| `touched_files` | 20 | Yes |
| `commands_run` | 10 | No (FIFO via VecDeque) |
| `test_results` | 10 | Yes |
| `unresolved_errors` | 10 | Yes |
| `artifact_handles` | unlimited | Yes |

`to_context_frame()` merges into the system prompt for model awareness.

## Configuration Surface

In `opencode.json` under `context`:

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `artifact_store` | `Option<bool>` | `true` | Enable artifact storage |
| `project_tool_outputs` | `Option<bool>` | `true` | Enable projection |
| `max_success_tokens` | `Option<usize>` | `800` | Token budget for successful outputs |
| `max_failure_tokens` | `Option<usize>` | `2000` | Token budget for failed outputs |
| `lossless_debug` | `Option<bool>` | `false` | Bypass projection, full output stored |

### Semantic Notes

- `artifact_store: false` — no artifacts, no handles, no `context_read`.
- `project_tool_outputs: false` — no compression; artifacts may still
  be stored if `artifact_store` is true.
- `lossless_debug: true` — bypass projection but still store artifact
  if `artifact_store` is true.

## Invariants & Gotchas

- **Handle building is always checked**: the agent loop uses
  `ContextHandle::build_tool()`, not raw formatting.
- **Store failure is non-fatal**: if `put()` fails, no handle is
  emitted but the model still sees projected text.
- **`context_read` registration depends only on `artifact_store`**:
  registered even when projection is disabled.
- **Turn indexing uses `state.turn_count`**: incremented at the start
  of each provider turn; multiple tool results in the same turn share
  a turn index.

## Integration with AgentLoop

All three tool result insertion sites (bootstrap, main loop,
streaming/retry) use the same semantics: checked handle building,
config gating, store failure logging, no unrecoverable handles.

## Testing

Integration tests live in `src/context/mod.rs` (projection, artifact,
ledger, config tests). LLM-specific tests: `cargo test -p codegg-core`
for core context types.

## Related Docs

- [cache-aware-context.md](cache-aware-context.md) — cache-aware packing
  (post-hardening: observe-only diagnostics, stable SHA-256 hashes,
  `source_handle` on `ContextBlock`, cache stats from telemetry)
- [compaction.md](compaction.md) — volatile-tail compaction policy
- [context-ledger.md](context-ledger.md) — this document
