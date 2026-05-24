# Server Module Review

## Summary

Reviewed `architecture/server.md`, `.opencode/skills/server/SKILL.md`, and `src/server/` implementation. **The server module does not compile** - there are critical missing imports and undefined types.

## Verification Status

| Component | Status |
|-----------|--------|
| mod.rs | OK |
| http.rs | **BROKEN** - references non-existent `routes::GlobalEventBus` and `routes::health::health_check` |
| state.rs | **BROKEN** - imports non-existent `routes::GlobalEventBus` |
| ws.rs | **BROKEN** - references undefined `RpcRequest`, `RpcResponse`, `RpcError` |
| rpc.rs | Has `JsonRpcMessage` but missing `RpcRequest`/`RpcResponse`/`RpcError` types |
| routes/mod.rs | Missing `health` module export |
| routes/event.rs | Minor: unused `mut` on `rx` |

## Critical Bugs (Compilation Failures)

### 1. Missing RPC Types (`src/server/ws.rs`)

The `ws.rs` file references `RpcRequest`, `RpcResponse`, and `RpcError` (lines 116, 120, 183, etc.), but these types are **not defined anywhere** in the codebase.

```rust
// ws.rs:116 - RpcResponse is undefined
let resp = RpcResponse { ... }

// ws.rs:130 - RpcRequest is undefined
if let Ok(req) = serde_json::from_str::<RpcRequest>(&text) { ... }
```

The `rpc.rs` file exists but only contains `JsonRpcMessage` and `JsonRpcError` - not the types used in `ws.rs`.

**Fix**: Either:
- Add missing `RpcRequest`, `RpcResponse`, `RpcError` types to `rpc.rs`
- Or use the existing `JsonRpcMessage`/`JsonRpcError` types

### 2. Missing GlobalEventBus in routes (`src/server/state.rs:11`, `src/server/http.rs:207`)

Both files import and use `routes::GlobalEventBus`, but:
- `routes/mod.rs` does not re-export any `GlobalEventBus`
- The actual `GlobalEventBus` is in `crate::bus::global::GlobalEventBus`

```rust
// state.rs:11 - broken import
use crate::server::routes::GlobalEventBus;

// http.rs:207 - broken usage
event_bus: routes::GlobalEventBus::new(),
```

**Fix**: Change to `crate::bus::global::GlobalEventBus` or fix routes/mod.rs to re-export it.

### 3. Missing health module export (`src/server/http.rs:23`)

```rust
// http.rs:23 - health module not in routes
use super::routes::health::health_check;
```

But `routes/mod.rs` doesn't include `health` in its pub mods, and the file `routes/health.rs` exists but returns a `&'static str` not `Json`.

**Fix**: Either:
- Add `pub mod health;` to routes/mod.rs and fix health_check return type
- Or inline the health_check handler in http.rs

## Documentation vs Code Discrepancies

### Verified Correct

1. **Entry point** (`run_server`) - accurate
2. **ServerState fields** - mostly accurate (but event_bus exists in code contrary to doc)
3. **WsRateLimiter** - accurate, correctly shared
4. **Session routes** - all 9 endpoints match
5. **Config routes** - accurate
6. **MCP routes** - accurate
7. **Provider/Tool routes** - accurate
8. **File routes** - accurate
9. **Project/Workspace routes** - accurate
10. **Middleware auth** - accurate
11. **Path sanitization** - accurate
12. **TuiMessage protocol** - accurate (from src/protocol/tui.rs)
13. **WebSocket auth validation** - accurate

### Documentation Issues

1. **Architecture doc line 70**: "Note: event_bus field was removed" - FALSE. The field EXISTS in code at `state.rs:18`
2. **Architecture doc line 71**: "SSE handler and TUI WebSocket directly use GlobalEventBus::subscribe()" - PARTIALLY TRUE. SSE uses it correctly (`routes/event.rs:13`), but ws.rs line 401 uses `GlobalEventBus::subscribe()` directly. However, the `event_bus: GlobalEventBus` field is still created in http.rs:207 and stored in ServerState.
3. **Architecture doc line 194-198**: SSE handler description - inaccurate about "15-second heartbeat comments" (uses `keep_alive interval`)
4. **Skill doc line 121**: Health route standalone - accurate that it's separate

### Skills File Issues

1. **Skill line 210**: "Dead EventBus Struct" - WRONG. There was never a local EventBus in routes/event.rs. The code correctly uses `crate::bus::global::GlobalEventBus::subscribe()`.

## Recommendations

### Priority 1: Fix Compilation

1. **Add missing RPC types to `rpc.rs`**:
```rust
#[derive(Serialize, Deserialize, Debug)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}
```

2. **Fix GlobalEventBus imports**:
   - In `state.rs:11`: Change to `use crate::bus::global::GlobalEventBus;`
   - In `http.rs:207`: Change to `event_bus: crate::bus::global::GlobalEventBus::new(),`

3. **Fix health module**:
   - Add `pub mod health;` to `routes/mod.rs`
   - Or create inline handler in http.rs

### Priority 2: Update Documentation

1. **architecture/server.md line 70-71**: Remove claim that event_bus field was removed - it still exists
2. **Skill line 210**: Remove claim about dead EventBus struct

### Priority 3: Code Cleanup

1. **routes/event.rs:13**: Remove `mut` from `let mut rx`
2. Consider whether the `event_bus` field in ServerState is actually used or is dead code

## File References

| File | Line(s) | Issue |
|------|---------|-------|
| `src/server/state.rs` | 11 | Wrong import: `routes::GlobalEventBus` doesn't exist |
| `src/server/http.rs` | 23 | Wrong import: `routes::health::health_check` not exported |
| `src/server/http.rs` | 207 | Wrong usage: `routes::GlobalEventBus::new()` |
| `src/server/ws.rs` | 116, 120, etc. | `RpcResponse`, `RpcRequest`, `RpcError` undefined |
| `src/server/rpc.rs` | 1-87 | Has wrong types (`JsonRpcMessage`) vs what's needed |
| `src/server/routes/mod.rs` | 1-24 | Missing `health` module export |
| `src/server/routes/event.rs` | 13 | Unused `mut` on `rx` |
