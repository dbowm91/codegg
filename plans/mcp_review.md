# MCP Architecture Review

## Summary
The MCP architecture document is generally accurate with well-documented features. Two known issues are correctly identified in the "Known Implementation Issues" section.

## Verified Correct

### McpClientType (mod.rs:77-81)
```rust
pub enum McpClientType {
    Local(Arc<RwLock<LocalClient>>),
    Remote(Arc<RwLock<McpConnectionManager>>),
}
```
Matches documented structure.

### McpService (mod.rs:83-86)
```rust
pub struct McpService {
    servers: HashMap<String, McpServer>,
    oauth: OAuthManager,
}
```
Matches, though `McpServer` struct uses `client: McpClientType` directly (not wrapped).

### LocalClient (local.rs:47-57)
All fields match documented structure:
- Uses `std::env::var_os("PATH")` at local.rs:88 for proper PATH discovery
- Process spawn wrapped in 10s timeout at local.rs:98
- Graceful shutdown via `shutdown_notify` at local.rs:74
- Pending request map for correlating responses at local.rs:54

### RemoteClient (remote.rs:328-340)
All fields match:
- `validated_ips: Arc<Mutex<Option<Vec<IpAddr>>>>` for Clone semantics correctly documented
- SSE events storage at `sse_events: Arc<Mutex<Vec<serde_json::Value>>>`

### McpConnectionManager (remote.rs:29-41)
All fields match documented structure including:
- Exponential backoff: `base_delay: Duration`, `max_delay: Duration`
- `reconnect_needed: Arc<Notify>` for reconnection signaling

### ConnectionState (remote.rs:19-27)
```rust
pub enum ConnectionState {
    Connected,
    #[default]
    Disconnected,
    Reconnecting { attempt: u32 },
}
```
Matches exactly.

### OAuthManager Token Encryption (auth.rs)
- Uses `CODEGG_TOKEN_KEY` env var at auth.rs:16
- Uses `CODEGG_ENC_v1` magic bytes prefix at auth.rs:17
- PKCE support via `generate_pkce_pair()` at auth.rs:124-135
- Replay protection via used codes store at auth.rs:87-89 and `is_code_used()` at auth.rs:256-268

### DNS Rebinding Protection
- `validate_url_host()` called on `RemoteClient::new()` at remote.rs:400
- `validate_host_ip()` called on `RemoteClient::new()` at remote.rs:407
- `initialize()` re-validates DNS at remote.rs:438-450
- `revalidate_dns()` called before each request at remote.rs:868-870
- IPv4-mapped IPv6 addresses handled correctly in `validate_host_ip()`
- Internal IPs blocked via `validate_host_ip()`

### IdeServer (ide_server.rs:50-55)
```rust
pub struct IdeServer {
    tools: HashMap<String, ToolHandler>,
    pending: PendingRequests,
    shutdown: Arc<Mutex<bool>>,
    shutdown_notify: Arc<Notify>,
}
```
Matches. Supports both stdio and Unix socket modes at ide_server.rs:78 and ide_server.rs:121.

### McpTool (mod.rs:53-59)
```rust
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub server: String,  // Note: not server_id as shown in old doc
}
```
The doc correctly notes "server" not "server_id". This is verified correct.

## Discrepancies Found

### IdeServer Tool Name in Doc vs Code
**Doc says** (mcp.md:204): `openDiff` tool
**Actual** (ide_server.rs:64-67): `"openDiff".to_string()` - correct

The doc is accurate on this point.

### Server Definition Configuration Example
**Doc shows** (mcp.md:271-289):
```json
"mcp": {
  "servers": {
    "filesystem": {
      "type": "local",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/projects"]
    }
  }
}
```
**Actual**: The actual config schema (checked via cli.rs:63-77) uses `McpEntry` with `McpServerConfig` which has fields like `server_type`, `command`, `args`, `env`, `url`, `headers`, `transport`, `timeout`, `oauth`, `reconnect`. The example in doc is simplified but conceptually correct.

## Bugs Identified

### None - Known Issues Correctly Documented

The "Known Implementation Issues" section (mcp.md:306-310) correctly identifies:

1. **Tool definition cache staleness** - This is a known limitation. The code at `src/mcp/mod.rs:210-221` uses `mcp__<server>__<tool>` naming but does not have explicit cache invalidation beyond tool list re-discovery.

2. **SSE support not fully integrated** - `connect_sse()` exists at remote.rs:698 but is NOT called automatically during remote connection setup. The SSE events are collected via `take_sse_events()` at remote.rs:802 but nothing in the current flow automatically calls `connect_sse()`. This is correctly documented.

## Improvement Suggestions

### Update Config Example to Match Schema
The JSON example at mcp.md:271-289 could be updated to show all current config fields, or reference the actual schema.

### Missing: connect_sse Not Called Automatically
While documented as a known issue, consider whether `connect_sse()` should be called automatically in `connect()` or `ensure_connected()`. Currently at remote.rs:83-88 `connect()` does not call SSE setup.

### Document IdeServer run_stdio/run_socket
The IdeServer supports stdio mode (`run_stdio()`) and Unix socket mode (`run_socket()`), but the architecture doc only mentions these briefly. Could be documented more thoroughly.

## Stale Items in Architecture Doc

### Server Count Reference
The doc does not state a specific server count for MCP servers, so no discrepancy here.

### Line Numbers
Similar to LSP doc, specific line numbers like "remote.rs:698" are fragile. Consider using function names instead.

### "2024-11-05" Protocol Version
The protocol version at local.rs:145 and remote.rs:453 shows `"protocolVersion": "2024-11-05"`. This should be verified as current and potentially documented as a configurable or versioned item.