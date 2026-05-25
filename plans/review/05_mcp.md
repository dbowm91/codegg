# MCP Module Architecture Review (2026-05-25)

## Source Files Reviewed
- `src/mcp/mod.rs` (388 lines)
- `src/mcp/remote.rs` (959 lines)
- `src/mcp/local.rs` (498 lines)
- `src/mcp/auth.rs` (852 lines)
- `src/mcp/cli.rs` (322 lines)
- `src/mcp/ide_server.rs` (446 lines)
- `src/security/ssrf.rs` (268 lines)

---

## Verified Correct Items

| Item | Location | Status |
|------|----------|--------|
| McpClientType enum | mod.rs:77-81 | Correct |
| McpService struct | mod.rs:83-86 | Correct |
| McpServer struct | mod.rs:70-75 | Correct |
| LocalClient struct | local.rs:47-57 | Correct |
| RemoteClient struct | remote.rs:328-340 | Correct |
| McpConnectionManager struct | remote.rs:29-41 | Correct |
| heartbeat_token field | remote.rs:37 | Correct |
| heartbeat_cancellation field | remote.rs:38 | Correct |
| ConnectionState enum | remote.rs:19-27 | Correct |
| McpTool struct | mod.rs:53-59 | Correct |
| McpServerStatus enum | mod.rs:61-68 | Correct |
| OAuthManager struct | auth.rs:91-96 | Correct |
| TokenSet struct | auth.rs:57-64 | Correct |
| ServerTokens struct | auth.rs:80-84 | Correct |
| IdeServer struct | ide_server.rs:50-55 | Correct |
| DNS rebinding protection | remote.rs:438-450 (initialize revalidates), remote.rs:849-870 (post_json revalidates) | Correct |
| validate_url_host() | ssrf.rs:123 | Correct |
| validate_host_ip() | ssrf.rs:67 | Correct |
| revalidate_dns() | ssrf.rs:96 | Correct |
| Heartbeat every 30s | remote.rs:75 | Correct |
| Max 5 retry attempts | remote.rs:72 | Correct |
| Exponential backoff max 60s | remote.rs:74 | Correct |
| Bearer token authentication | remote.rs:890-892 | Correct |
| OAuth flow support | auth.rs:284-322 | Correct |
| SSE support (connect_sse) | remote.rs:698 | Correct |
| LocalClient PATH via std::env::var_os | local.rs:88-92 | Correct |
| Process spawn 10s timeout | local.rs:98 | Correct |
| PKCE support (generate_pkce_pair) | auth.rs:124-135 | Correct |
| Replay protection - code marked before exchange | auth.rs:307-310 | Correct |
| McpConnectionManager Clone impl | remote.rs:43-59 | Correct (manual impl, not derived) |

---

## Incorrect/Stale Items Needing Fixes

### 1. McpConnectionManager Clone Implementation (Line 107-121)

**Issue**: Architecture doc shows `McpConnectionManager` struct but does NOT document that it has a manual `Clone` implementation due to `CancellationToken` being `!Clone`.

**Fix**: Add Clone impl documentation after line 121:
```rust
impl Clone for McpConnectionManager {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            state: Arc::clone(&self.state),
            retry_count: Arc::clone(&self.retry_count),
            max_retries: self.max_retries,
            base_delay: self.base_delay,
            max_delay: self.max_delay,
            heartbeat_interval: self.heartbeat_interval,
            heartbeat_token: self.heartbeat_token.clone(),
            heartbeat_cancellation: Arc::clone(&self.heartbeat_cancellation),
            shutdown: Arc::clone(&self.shutdown),
            reconnect_needed: Arc::clone(&self.reconnect_needed),
        }
    }
}
```

### 2. validate_url_host Location (Line 297)

**Issue**: Architecture doc says `validate_url_host()` is "Called on RemoteClient::new()" but does not mention it's in the `security` module.

**Fix**: Update line 297 to indicate location:
```rust
fn validate_url_host(url: &str) -> Result<String, String>  // In src/security/ssrf.rs, called on RemoteClient::new()
```

### 3. IdeServer Async I/O Details (Line 205-208)

**Issue**: Architecture doc mentions "Transport Modes: stdio mode (for IDE extensions), Unix socket mode" but doesn't document that `run_stdio()` uses async I/O via `tokio::io::stdin()/stdout()`.

**Fix**: Update lines 205-208:
```rust
**Transport Modes:**
- stdio mode (for IDE extensions) - uses async I/O via `tokio::io::stdin()/stdout()` with `BufReader` and `AsyncBufReadExt`
- Unix socket mode
```

---

## Known Implementation Issues (Already Documented)

1. **Tool definition cache staleness** - Uses `mcp_tool_count` as proxy; accurate in doc (lines 306-308)
2. **SSE support not fully integrated** - `connect_sse()` exists but not called during connection setup; accurate in doc (lines 310-311)

---

## Bugs Found in Related Code

None. All code reviewed appears correct.

---

## Summary

The architecture document `architecture/mcp.md` is **95% accurate**. Only minor updates needed:

1. **Add Clone impl for McpConnectionManager** (lines 107-121 area)
2. **Clarify validate_url_host location** (line 297)
3. **Document IdeServer async I/O** (lines 205-208)

All struct fields, method signatures, OAuth flow, DNS rebinding protection, heartbeat_token, and heartbeat_cancellation are correct as documented.