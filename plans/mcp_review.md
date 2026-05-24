# MCP Module Review

## Summary

This review compared the architecture document at `architecture/mcp.md` and the skill guide at `.opencode/skills/mcp/SKILL.md` against the actual implementation in `src/mcp/`. The MCP module is well-implemented and generally well-documented, with only minor discrepancies found.

**Overall Assessment**: The implementation is correct and matches the documentation in most respects. DNS rebinding protection, OAuth replay protection, auto-reconnect with exponential backoff, and async I/O are all properly implemented.

---

## Verified Correct Items

### 1. McpService (mod.rs)
- `McpService` struct with `servers: HashMap<String, McpServer>` and `oauth: OAuthManager` - matches docs
- `connect_stdio()`, `connect_http()`, `connect_from_config()`, `disconnect()`, `shutdown_all()` methods all exist as documented
- Tool naming convention `mcp__<server>__<tool>` confirmed at line 215

### 2. LocalClient (local.rs)
- Uses `std::env::var_os("PATH")` for user PATH preservation (lines 88-92)
- Process spawn wrapped in 10s timeout (lines 98-124)
- Graceful shutdown via `shutdown_notify` Notify mechanism
- Pending request map for correlating JSON-RPC responses

### 3. RemoteClient + McpConnectionManager (remote.rs)
- Manual `Clone` implementation for `McpConnectionManager` due to `CancellationToken` being `!Clone` (lines 43-59)
- Manual `Clone` implementation for `RemoteClient` (lines 342-358)
- DNS re-validation on each `initialize()` call (lines 438-450, 448 re-validates)
- Exponential backoff: 1s, 2s, 4s, ... up to 60s max (lines 131-137)
- Max 5 retry attempts (line 72)
- Heartbeat every 30s (line 75, 106-125)
- `ensure_connected()` spawns reconnection in background task (lines 176-226)

### 4. OAuthManager (auth.rs)
- Tokens encrypted with AES-256-GCM using `CODEGG_TOKEN_KEY` env var
- Uses `CODEGG_ENC_v1` magic bytes prefix (line 17)
- PKCE support via `generate_pkce_pair()` (lines 124-135)
- Replay protection: code marked as used BEFORE `exchange_code_for_tokens()` (lines 293-308)
- Used codes expire after 600 seconds (line 305)

### 5. IdeServer (ide_server.rs)
- Uses `tokio::io::stdin()` and `tokio::io::stdout()` for async I/O (lines 79-81)
- `BufReader` with `AsyncBufReadExt` for async line reading
- `AsyncWriteExt` for async write and flush operations
- Supports `openDiff` tool

### 6. McpConnectionManager Fields
All fields documented correctly:
- `client: RemoteClient` 
- `state: Arc<Mutex<ConnectionState>>`
- `retry_count: Arc<AtomicU64>`
- `max_retries: u64`
- `base_delay: Duration`
- `max_delay: Duration`
- `heartbeat_interval: Duration`
- `heartbeat_task: Arc<AtomicBool>` - note: removed in actual code, not present
- `heartbeat_token: CancellationToken`
- `heartbeat_cancellation: Arc<Mutex<Option<CancellationToken>>>`
- `shutdown: Arc<Notify>`
- `reconnect_needed: Arc<Notify>`

### 7. McpTool Structure
```rust
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub server: String,  // Note: not server_id
}
```

---

## Discrepancies Found

### 1. McpConnectionManager has `heartbeat_task: Arc<AtomicBool>` missing

**Architecture doc shows** (line 117):
```rust
heartbeat_task: Arc<AtomicBool>,
```

**Actual code** (remote.rs:29-41):
```rust
pub struct McpConnectionManager {
    client: RemoteClient,
    state: Arc<Mutex<ConnectionState>>,
    retry_count: Arc<AtomicU64>,
    max_retries: u64,
    base_delay: Duration,
    max_delay: Duration,
    heartbeat_interval: Duration,
    heartbeat_token: CancellationToken,
    heartbeat_cancellation: Arc<Mutex<Option<CancellationToken>>>,
    shutdown: Arc<Notify>,
    reconnect_needed: Arc<Notify>,
}
```

**Status**: The `heartbeat_task: Arc<AtomicBool>` field was removed from the implementation but remains in the architecture documentation. The actual implementation manages heartbeat cancellation via `CancellationToken` instead.

### 2. IdeServer `run_stdio()` Signature

**Skill doc shows** (line 220):
```rust
pub async fn run_stdio(&self) -> Result<(), McpError>;  // Uses tokio async I/O
```

**Actual code** (ide_server.rs:78):
```rust
pub async fn run_stdio(&self) -> Result<(), McpError>
```

**Status**: Correct, but note the return type in documentation should include `McpError` which is correct.

### 3. McpConnectionManager Clone Derivation Issue

**Architecture doc mentions** (line 112):
```rust
- `McpConnectionManager` has a manual `Clone` implementation (not derived)
- `CancellationToken` from `tokio_util::sync` is `!Clone`, so it requires special handling
```

This is **correctly implemented** in code (lines 43-59).

---

## Bugs or Issues in Code

### 1. SSE Support Not Fully Integrated (Known Issue)

**Location**: `src/mcp/remote.rs:698-800`

The `connect_sse()` and `connect_sse_stream()` methods exist but are not automatically called during remote connection setup. SSE events are collected but not yet processed by the agent.

This is documented as a known limitation in both the architecture doc (lines 305-309) and skill (lines 445-446).

### 2. Tool Definition Cache Staleness (Known Issue)

**Location**: Agent tool registration

Uses `mcp_tool_count` as proxy for MCP tool changes. If tool identities change without count changing, cache may be stale.

This is documented as a known limitation in architecture doc (lines 305-307).

---

## Recommendations

### 1. Update Architecture Document

Remove the `heartbeat_task: Arc<AtomicBool>` field from the `McpConnectionManager` struct shown in the architecture document (around line 117), as it no longer exists in the actual implementation.

### 2. Consider Integrating SSE Support

The SSE infrastructure is in place (`connect_sse()`, `connect_sse_stream()`, `take_sse_events()`) but not hooked into the main connection flow. Consider integrating SSE event processing into `McpConnectionManager::connect()` or `ensure_connected()`.

### 3. Document Missing OAuthManager Methods

The skill document shows `exchange_code_for_tokens_with_replay_protection()` but the actual code also has `exchange_code_for_tokens()` (without replay protection) which is used internally by the replay-protecting version. Consider documenting both methods separately.

---

## File Reference Summary

| File | Lines | Notes |
|------|-------|-------|
| `src/mcp/mod.rs` | 1-388 | Main entry point, correct |
| `src/mcp/local.rs` | 1-498 | Local stdio client, correct |
| `src/mcp/remote.rs` | 1-959 | Remote client + manager, mostly correct |
| `src/mcp/auth.rs` | 1-850 | OAuth manager, correct |
| `src/mcp/cli.rs` | 1-322 | CLI commands, correct |
| `src/mcp/ide_server.rs` | 1-446 | IDE MCP server, correct |
| `src/security/ssrf.rs` | 1-268 | DNS validation, correct |
| `architecture/mcp.md` | 1-315 | Contains incorrect `heartbeat_task` field reference |
| `.opencode/skills/mcp/SKILL.md` | 1-447 | Generally accurate |
