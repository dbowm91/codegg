# MCP Module Override

This file contains MCP-specific guidance and overrides root AGENTS.md.

## MCP Connection Manager

The `McpConnectionManager` in `src/mcp/remote.rs` handles:

- Automatic reconnection with exponential backoff (1s-60s, max 5 retries)
- Heartbeat every 30s to keep connection alive
- State transitions: Connected, Disconnected, Reconnecting

## Known Issues

### McpConnectionManager Clone Unsound (HIGH)
**File:** `src/mcp/remote.rs:179-193`

The `Clone` impl clones internal `Arc` pointers but shares mutable state (`client`, `state`, `retry_count`). Multiple clones can modify the same underlying connection simultaneously, violating Rust's aliasing rules.

**Recommendation:** Either remove `Clone` implementation or use `Arc<Mutex<...>>` internally.

### IdeServer Blocking I/O (HIGH)
**File:** `src/mcp/ide_server.rs:79-113`

`stdin.read_line()` and `stdout.write_all()` are synchronous operations in async context, blocking the Tokio thread.

**Recommendation:** Use `tokio::io::stdin()`/`stdout()` with async read/write methods.

### OAuth Replay Protection Race (HIGH)
**File:** `src/mcp/auth.rs:318-332`

Code is marked as used BEFORE verifying the token exchange succeeds. If exchange fails after marking, the code is permanently unusable.

**Recommendation:** Mark code as used only AFTER successful token exchange.