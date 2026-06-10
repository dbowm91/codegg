# Context Ledger Architecture

## Overview

The context ledger system (`src/context/`) manages tool output artifacts with in-memory storage and token-budget-aware projection. It reduces context window usage by compressing verbose tool outputs while preserving diagnostic detail, and provides a `context_read` tool for on-demand artifact recovery.

**Session-local and in-memory.** Artifacts are not persisted across sessions. SQLite persistence is a future follow-up.

## Module Structure

```
src/context/
├── mod.rs           # Module root, re-exports, integration tests
├── artifact.rs      # ContextArtifact, ArtifactKind, ContextArtifactStore trait, InMemoryArtifactStore
├── handle.rs        # ContextHandle parser, ContextHandleError, ContextHandleKind, clamp_to_char_boundary
├── projection.rs    # ToolOutputProjection, ProjectionConfig, project_tool_output()
└── read_tool.rs     # ContextReadTool (Tool trait impl)
```

## Key Types

### ContextHandle (`handle.rs`)

A typed parser for `ctx://` artifact handles.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextHandle {
    pub kind: ContextHandleKind,
    pub session_id: String,
    pub turn_index: usize,
    pub tool_call_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextHandleKind {
    Tool,
}
```

Handle format: `ctx://tool/{session_id}/{turn_index}/{tool_call_id}`

Rules:
- Requires exactly 4 path segments after `ctx://`
- Rejects empty session_id or tool_call_id segments
- Rejects `/`, control characters, and whitespace in segments
- Parses turn_index as `usize`; rejects invalid numbers

Key methods:
- `parse(input: &str) -> Result<Self, ContextHandleError>` — parse a handle string
- `build_tool(session_id, turn_index, tool_call_id) -> Result<String, ContextHandleError>` — build a handle, rejecting unsafe characters
- `same_session(session_id: &str) -> bool` — exact session match (not substring)

**Handle building is always checked.** The agent loop uses `ContextHandle::build_tool()` (not raw string formatting) to construct handles. If any segment contains `/`, control characters, or whitespace, handle creation fails and no artifact is stored under an invalid key. A legacy `build_handle()` free function exists in `artifact.rs` but is not used by the agent loop.

### ContextArtifact (`artifact.rs`)

```rust
pub struct ContextArtifact {
    pub handle: String,           // "ctx://tool/{session_id}/{turn_index}/{tool_call_id}"
    pub session_id: String,
    pub turn_index: usize,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub kind: ArtifactKind,
    pub created_at_ms: i64,
    pub content_hash: String,
    pub redacted_content: String, // Full content stored here
    pub raw_bytes_len: usize,
    pub estimated_tokens: usize,
}
```

### ArtifactKind

```rust
pub enum ArtifactKind {
    ToolResult,
    CommandOutput,
    ReadResult,
    Diff,
    TestOutput,
    WebFetch,
    Image,
}
```

### ContextArtifactStore trait

```rust
#[async_trait]
pub trait ContextArtifactStore: Send + Sync {
    async fn put(&self, artifact: ContextArtifact) -> anyhow::Result<()>;
    async fn get(&self, handle: &str) -> anyhow::Result<Option<ContextArtifact>>;
    async fn list_recent(&self, session_id: &str, limit: usize) -> anyhow::Result<Vec<ContextArtifact>>;
}
```

The only current implementation is `InMemoryArtifactStore` — a `RwLock<HashMap<String, ContextArtifact>>` keyed by handle.

## Projection System (`projection.rs`)

### How Projection Works

When a tool returns output, `project_tool_output()` is called with the tool name, arguments, raw output, success flag, effective handle, and `ProjectionConfig`. The function:

1. **Detects artifact kind** from the tool name (`bash`/`exec` → ToolResult, `read` → ReadResult, `diff` → Diff, `webfetch` → WebFetch, `image` → Image).
2. **Extracts metadata**: touched files (by extension pattern matching), commands run (from bash/exec args JSON), test results (tightened patterns), errors (pattern matching).
3. **Projects output** based on success/failure and token budget:
   - **Success**: If output ≤ `max_success_tokens`, pass through. Otherwise truncate to 20 lines with a token count summary.
   - **Failure**: Collect high-priority lines (`error[`, `error:`, `failed`, `panicked`, `traceback`, etc. — deduplicated, capped at 30) and medium-priority lines (`warning:`, `test result:` — deduplicated, capped at 20). If the combined high+medium lines fit within `max_failure_tokens`, use them.
4. **Returns** `ToolOutputProjection` with `model_text` (what the model sees), `summary`, `status`, and extracted metadata.

### ProjectionConfig

```rust
pub struct ProjectionConfig {
    pub max_success_tokens: usize,      // default: 800
    pub max_failure_tokens: usize,      // default: 2000
    pub enabled: bool,                  // default: true
    pub artifact_store_enabled: bool,   // default: true — if false, no handles emitted
    pub lossless_debug: bool,           // default: false — if true, bypass projection
}
```

When `enabled: false` or `lossless_debug: true`, all output passes through unmodified. When `artifact_store_enabled: false`, no `ctx://` handles are emitted even if projection is active.

### Model-Facing Header Format

When a handle is available (store succeeded + `artifact_store_enabled: true`), projected output uses the format:

```
[tool output captured]
Tool: {tool_name}
Handle: ctx://tool/{session_id}/{turn_index}/{tool_call_id}
Full output: use context_read with this handle.
```

When no handle is available (store failed or disabled), the header is:

```
[tool output captured]
Tool: {tool_name}
```

## Turn Indexing

Handles use `state.turn_count` (the agent loop's turn counter) as the turn index. This is incremented at the start of each provider turn. Multiple tool results in the same turn share the same turn index but differ by tool_call_id.

## Store Failure Handling

If handle creation via `ContextHandle::build_tool()` fails (e.g., unsafe characters in session id or tool-call id):
- A warning is logged with tool name, tool-call id, session id, and error
- No `ctx://` handle is emitted
- Artifact storage is skipped entirely

If `artifact_store.put()` fails after a valid handle is created:
- A warning is logged with tool name, tool-call id, session id, and error
- No `ctx://` handle is emitted in the projected output
- The model sees projected text without the recovery affordance
- Message/tool-call pairing remains valid regardless of store failure

When `artifact_store_enabled: false`:
- `ContextHandle::build_tool()` is not called
- `artifact_store.put()` is not called
- No `ctx://` handles are emitted
- Projection may still compress output if `project_tool_outputs: true`

## context_read Tool (`read_tool.rs`)

The `ContextReadTool` implements the `Tool` trait and allows the model to recover full artifact content by handle. It accepts:

- `handle` (required): The `ctx://` handle of the artifact.
- `offset` (optional, default 0): Byte offset for pagination.
- `max_bytes` (optional, default 20000): Maximum bytes to return.

**Security:**
- Uses `ContextHandle::parse()` for exact parsed session matching (not substring `contains`)
- Rejects `ctx://` handles that do not match the current session
- Rejects malformed handles with a format error before store lookup
- Safe UTF-8 slicing: `clamp_to_char_boundary()` prevents panics on non-ASCII boundaries

**Registration:** `context_read` is registered when `artifact_store = true` (the default), regardless of `project_tool_outputs`. This is because `context_read` is read-only and session-scoped, so exposing it when storage is active is reasonable even if projection is disabled. The tool is wired from `build_session_tool_registry` → `DefaultTurnRuntime` → `AgentLoopBuildInput` → `AgentLoop`.

## Integration with AgentLoop

The agent loop stores tool output artifacts and projects them at all three tool result insertion sites:
1. Bootstrap tool loop (list tool)
2. Main tool execution loop
3. Streaming/retry tool processing

All three paths use the same semantics:
- **Checked handle building**: `ContextHandle::build_tool()` validates segments before creating handles. Invalid inputs produce no handle and no artifact storage.
- **Config gating**: When `artifact_store_enabled: false`, handle building and storage are skipped entirely.
- **Store failure logging**: Both handle creation failures and store failures emit `tracing::warn!` with tool name, tool-call id, session id, and error.
- **No unrecoverable handles**: Malformed `ctx://` strings are never exposed to the model.

The resulting `model_text` is what the model sees in the conversation. The full content is stored in the artifact store.

## ContextFrame Population

### ContextLedgerState (`src/agent/context_frame.rs`)

```rust
pub struct ContextLedgerState {
    pub touched_files: Vec<String>,       // max 20, deduplicated
    pub commands_run: VecDeque<String>,    // max 10, FIFO
    pub test_results: Vec<String>,        // max 10, deduplicated
    pub unresolved_errors: Vec<String>,   // max 10, deduplicated
    pub artifact_handles: Vec<String>,    // all handles seen
}
```

`record_projection()` is called after each projection to accumulate metadata. `to_context_frame()` converts to a `ContextFrame` which is merged into the system prompt context, giving the model awareness of files touched, commands run, test results, and open errors across the session.

### ContextFrame

```rust
pub struct ContextFrame {
    pub user_goal: Option<String>,
    pub current_task: Option<String>,
    pub constraints: Vec<String>,
    pub decisions: Vec<String>,
    pub touched_files: Vec<String>,
    pub commands_run: Vec<String>,
    pub test_results: Vec<String>,
    pub unresolved_errors: Vec<String>,
    pub security_findings: Vec<String>,
    pub next_steps: Vec<String>,
}
```

`to_control_text()` renders the frame as a human-readable block injected into the system prompt.

## Config Options

In `opencode.json`:

```json
{
  "context": {
    "artifact_store": true,
    "project_tool_outputs": true,
    "max_success_tokens": 800,
    "max_failure_tokens": 2000,
    "lossless_debug": false
  }
}
```

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `artifact_store` | `Option<bool>` | `true` | Enable artifact storage; if false, no artifacts stored, no handles emitted, `context_read` not registered |
| `project_tool_outputs` | `Option<bool>` | `true` | Enable projection before model sees output; artifacts may still be stored if `artifact_store` is true |
| `max_success_tokens` | `Option<usize>` | `800` | Token budget for successful outputs |
| `max_failure_tokens` | `Option<usize>` | `2000` | Token budget for failed outputs |
| `lossless_debug` | `Option<bool>` | `false` | Bypass projection, preserve full output; still stores artifact if `artifact_store` is true |

**`context_read` registration**: The `context_read` tool is registered when `artifact_store = true`, regardless of `project_tool_outputs`. This allows explicit artifact recovery even when projection is disabled.

### Semantic Notes

- `artifact_store: false` — Do not store artifacts, do not emit handles, do not register `context_read`. Projection text does not imply recoverability. No `ctx://` handles are generated.
- `project_tool_outputs: false` — Do not compress/project model-facing tool results. Preserve full redacted tool output. Artifacts may still be stored if `artifact_store` is true.
- `lossless_debug: true` — Bypass projection, append full redacted output to model transcript. If `artifact_store` is also true, still store the artifact for diagnostics/replay.

## Persistence Note

Artifacts are currently **session-local and in-memory only**. SQLite persistence is a future follow-up. The API is designed to allow persistent backing without interface changes.

## Cache-Aware Context Packing

The context ledger provides artifact storage and projection. The cache-aware context packer builds on this by ordering context blocks for stable provider prompt caching. See [cache-aware-context.md](cache-aware-context.md) for the full architecture.