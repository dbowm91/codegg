# Context Ledger Architecture

Tool output artifact storage with in-memory storage and
token-budget-aware projection.

## Purpose

Reduces context window usage by compressing verbose tool outputs while
preserving diagnostic detail. Provides `context_read` for on-demand
artifact recovery. Separately, the `eggcontext` crate provides
deterministic token counting (see [Related Docs](#related-docs)).

**Production persistence.** Production session tools construct
`FileArtifactStore::new(&execution.workspace_root)`, writing bounded
10 MiB records under `.codegg/context_artifacts` via temp-file + rename
so handles survive daemon restart. `InMemoryArtifactStore` remains for
tests. The stale claim that production artifacts are session-local /
in-memory is corrected: `FileArtifactStore` is the current production
store.

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
`ctx://evidence/{session_id}/{checkpoint_id}/{evidence_id}`

Parsed by `ContextHandle::parse()` (`handle.rs`). Tool handles are built
by `ContextHandle::build_tool()`; checkpoint-scoped evidence handles are
built by `ContextHandle::build_evidence()`. Both reject empty segments
and `/`, control chars, and whitespace in segments. Tool-handle wire
strings are unchanged from all historical runtimes.

Evidence handles resolve through the same durable `FileArtifactStore`
via `context_read` with exact same-session matching. `evidence_id` is a
deterministic host-chosen identity (source ordinal/kind plus digest),
never a model-invented path. `ArtifactKind::ContinuationEvidence`
(`continuation_evidence`) backs new evidence artifacts; old artifact JSON
remains readable.

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
    pub session_id: String,
    pub kind: ContextHandleKind,
}

pub enum ContextHandleKind {
    Tool { turn_index: usize, tool_call_id: String },
    Evidence { checkpoint_id: String, evidence_id: String },
}
```

| Method | Signature | Notes |
|--------|-----------|-------|
| `parse()` | `fn(&str) -> Result<Self, ...>` | Validates both `tool` and `evidence` forms, rejects unsafe chars |
| `build_tool()` | `fn(&str, usize, &str) -> Result<String, ...>` | Checked construction, wire-compatible |
| `build_evidence()` | `fn(&str, &str, &str) -> Result<String, ...>` | Checkpoint-scoped evidence handles (M003) |
| `render()` | `fn(&self) -> String` | Canonical wire string |
| `turn_index()` / `tool_call_id()` | `fn(&self) -> Option<...>` | Tool accessors preserving historical call sites |
| `checkpoint_id()` / `evidence_id()` | `fn(&self) -> Option<&str>` | Evidence accessors |
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
    ContinuationEvidence, // `continuation_evidence` (M003)
}
```

M003 evidence bounds (`src/context/evidence.rs`): max 64 refs per
checkpoint, max 256 KiB total new evidence bytes per checkpoint, max
64 KiB per single evidence artifact, max 280 chars per ref summary.
Selection prioritizes user steering → failures/errors → tests →
decisions/constraints → recent assistant evidence; `ToolCall` argument
JSON is never persisted. Evidence text persists visible `User`/`Assistant`
text only (provider-private reasoning excluded) through secret/URL
redaction; the content digest always covers the redacted stored body.
Deterministic `ctx://evidence/...` handles converge on identical content;
conflicting writes to the same handle fail closed. Missing optional
evidence degrades to the checkpoint summary via `context_read` `NotFound`.

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

### ProjectionConfig (`projection.rs:23`)

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

Tool trait impl. Accepts `handle` (required; `ctx://tool/...` or
`ctx://evidence/...`), `offset` (default 0), `max_bytes` (default
20000). Uses `ContextHandle::parse()` for exact session matching and
`clamp_to_char_boundary()` for safe UTF-8 slicing. Returns kind/offset
metadata; never enumerates other sessions. Category remains read-only.

Registered when `artifact_store = true` regardless of `project_tool_outputs`.

### ContextLedgerState (`src/agent/context_frame.rs`)

Accumulates metadata across projections:

| Field | Cap | Dedup |
|-------|-----|-------|
| `touched_files` | 20 | Yes |
| `commands_run` | 10 | No (FIFO via VecDeque) |
| `test_results` | 10 | Yes |
| `unresolved_errors` | 10 | Yes |
| `artifact_handles` | unlimited in the ledger; projected bounded (most recent 32, dedup) into continuation state via `bounded_artifact_handles` | Yes |

`to_context_frame()` merges files/commands/tests/errors plus the bounded
artifact-handle projection into the frame for model awareness (M002
§6.6). The full unbounded ledger never goes to the prompt.

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
