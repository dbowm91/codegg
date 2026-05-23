# MCP Module Architecture Review

## Verification Results

### Claims (table format: Claim | Status | Evidence)

| Claim | Status | Evidence |
|-------|--------|----------|
| **McpClientType enum** - `Local(Arc<RwLock<LocalClient>>)`, `Remote(Arc<RwLock<McpConnectionManager>>)` | VERIFIED | `src/mcp/mod.rs:77-81` matches exactly |
| **McpService struct** - has `servers: HashMap<String, McpServer>`, `oauth: OAuthManager` | VERIFIED | `src/mcp/mod.rs:83-86` matches exactly |
| **McpServer struct** - has `name`, `status`, `tools`, `client` | VERIFIED | `src/mcp/mod.rs:70-75` matches exactly |
| **Connection methods**: `connect_stdio()`, `connect_http()`, `connect_from_config()`, `disconnect()`, `shutdown_all()` | VERIFIED | `src/mcp/mod.rs:96-316` all methods present |
| **LocalClient struct** - has `command`, `args`, `env`, `timeout`, `child`, `stdin`, `pending`, `shutdown_notify`, `request_id` | VERIFIED | `src/mcp/local.rs:47-57` matches exactly |
| **Protocol**: JSON-RPC over stdio | VERIFIED | `src/mcp/local.rs:15-46` shows JsonRpcRequest/Response over stdio |
| **Uses `std::env::var_os("PATH")`** to preserve user's actual PATH | VERIFIED | `src/mcp/local.rs:88-91` uses `std::env::var_os("PATH")` |
| **Process spawn wrapped in 10s timeout** to prevent hangs | VERIFIED | `src/mcp/local.rs:98-124` uses `spawn_blocking` with `timeout.min(10000)` |
| **Graceful shutdown via `shutdown_notify`** Notify mechanism | VERIFIED | `src/mcp/local.rs:365-377` uses `shutdown_notify.notify_waiters()` |
| **Pending request map** for correlating JSON-RPC responses | VERIFIED | `src/mcp/local.rs:44-45`, `397` stores pending senders by request ID |
| **RemoteClient struct** - has `url`, `headers`, `client`, `session_id`, `sse_url`, `oauth_token`, `sse_events`, `request_id`, `shutdown`, `sse_shutdown`, `validated_ips` | VERIFIED | `src/mcp/remote.rs:297-309` matches exactly |
| **McpConnectionManager struct** - has all listed fields | VERIFIED | `src/mcp/remote.rs:28-40` matches exactly |
| **ConnectionState enum**: `Connected`, `Disconnected`, `Reconnecting { attempt: u32 }` | VERIFIED | `src/mcp/remote.rs:18-26` matches exactly |
| **Bearer token authentication** | VERIFIED | `src/mcp/remote.rs:859-860` adds `Authorization: Bearer` header |
| **OAuth flow support** | VERIFIED | `src/mcp/auth.rs:124-170` PKCE support, `284-321` replay protection |
| **DNS rebinding protection (IP re-validation on each request)** | VERIFIED | `src/mcp/remote.rs:831-839` calls `revalidate_dns` on each request |
| **SSE (Server-Sent Events) support** for server-initiated messages | VERIFIED | `src/mcp/remote.rs:667-769` `connect_sse()` and `connect_sse_stream()` exist |
| **Exponential backoff**: 1s → 2s → 4s → ... → max 60s | VERIFIED | `src/mcp/remote.rs:108-114` uses `2^i * base_delay` capped at max_delay |
| **Max 5 retry attempts** before giving up | VERIFIED | `src/mcp/remote.rs:53` `max_retries: 5`, checked at lines 126, 143 |
| **Heartbeat every 30s** to keep connection alive | VERIFIED | `src/mcp/remote.rs:56` `heartbeat_interval: Duration::from_secs(30)` |
| **`ensure_connected()`** spawns reconnection in background task when disconnected | VERIFIED | `src/mcp/remote.rs:154-197` spawns reconnection task |
| **OAuthManager** - TokenSet with `access_token`, `refresh_token`, `token_type`, `expires_at`, `scope` | VERIFIED | `src/mcp/auth.rs:57-64` matches exactly |
| **Tokens encrypted with AES-256-GCM** using `CODEGG_TOKEN_KEY` env var | VERIFIED | `src/mcp/auth.rs:16-17` env var, `34-46` encryption |
| **Uses `CODEGG_ENC_v1` magic bytes prefix** for version detection | VERIFIED | `src/mcp/auth.rs:17` `MAGIC_BYTES`, line 522 checks prefix |
| **PKCE support for OAuth authorization code flow** | VERIFIED | `src/mcp/auth.rs:124-135` `generate_pkce_pair()` |
| **Replay protection via used codes store** | VERIFIED | `src/mcp/auth.rs:284-321` `exchange_code_for_tokens_with_replay_protection()` |
| **McpCli struct** - `config_path: PathBuf` | VERIFIED | `src/mcp/cli.rs:16-19` matches exactly |
| **CLI Commands**: `add`, `list`, `remove`, `enable`, `debug` | VERIFIED | `src/mcp/cli.rs:183-236` all commands defined |
| **IdeServer struct** - has `tools`, `pending`, `shutdown`, `shutdown_notify` | VERIFIED | `src/mcp/ide_server.rs:51-56` matches exactly |
| **Supported Tools**: `openDiff` - Opens file diff in IDE | VERIFIED | `src/mcp/ide_server.rs:65-68` registers `openDiff` handler |
| **Transport Modes**: stdio mode, Unix socket mode | VERIFIED | `src/mcp/ide_server.rs:79-113` stdio, `115-138` socket |
| **McpTool struct** - has `name`, `description`, `input_schema`, `server` | VERIFIED | `src/mcp/mod.rs:53-59` matches exactly (note: `server` not `server_id`) |
| **Tools exposed as `mcp__<server>__<tool>`** to AgentLoop | VERIFIED | `src/mcp/mod.rs:215` format: `mcp__{}_{}` |
| **McpServerStatus enum**: `Disconnected`, `Connecting`, `Connected`, `Error(String)` | VERIFIED | `src/mcp/mod.rs:61-68` matches exactly |
| **JSON-RPC protocol** request/response format | VERIFIED | Architecture doc matches code implementation |
| **Server Configuration** in `config.json` with `mcp.servers` structure | VERIFIED | Config schema confirms structure |
| **`initialize()` re-validates DNS** on each call | VERIFIED | `src/mcp/remote.rs:407-419` re-validates in `initialize()` |
| **IPv4-mapped IPv6 addresses** are handled correctly | VERIFIED | `src/security/ssrf.rs` has `ipv6_segments_to_ipv4()` |
| **Internal IPs** (loopback, private, link-local) are blocked | VERIFIED | `src/security/ssrf.rs:67-95` `validate_host_ip()` checks internal |
| **Tool definition cache staleness**: Uses `mcp_tool_count` as proxy | VERIFIED | Known issue documented - actual MCP service doesn't expose version/hash |
| **SSE support not fully integrated**: `connect_sse()` exists but not called during remote connection setup | VERIFIED | `src/mcp/mod.rs:132-172` `connect_http()` doesn't call `connect_sse()` |

## Bugs Found

### Critical

1. **SSE events never processed by agent** (`src/mcp/remote.rs:667-769`)
   - `connect_sse()` is defined but never called during normal connection flow
   - SSE events are collected in `sse_events` buffer but never dispatched to the agent
   - `take_sse_events()` exists but is never called anywhere in the codebase
   - **Impact**: Server-initiated notifications via SSE are silently ignored

2. **SSE response parsing may lose data** (`src/mcp/remote.rs:902-904`)
   - When response text starts with `event:`, it delegates to `parse_sse_response()` but the HTTP response was already consumed by `resp.text()` 
   - The `text()` call at line 885-887 consumes the body; subsequent calls may not work correctly
   - If `parse_sse_response()` fails, the error message is opaque

### High

3. **Race condition in `ensure_connected()`** (`src/mcp/remote.rs:154-197`)
   - After spawning reconnection task, `reconnect_needed.notified().await` waits
   - The spawned task may fail and never call `reconnect_needed.notify_one()`, causing the waiter to hang indefinitely
   - No timeout on the notification wait

4. **`shutdown_all()` may not complete properly** (`src/mcp/mod.rs:311-316`)
   - Iterates servers and calls `disconnect()` but doesn't handle failures
   - If a server is already disconnected, `disconnect()` returns `Ok(())` but doesn't clean up properly

5. **Missing error handling in `start_callback_server()`** (`src/mcp/auth.rs:476-501`)
   - Spawns `handle_callback` but doesn't handle the case where the spawned task panics
   - The TCP listener is bound but there's no graceful shutdown mechanism

### Medium

6. **OAuth token not loaded automatically for local servers** (`src/mcp/mod.rs:96-130`)
   - `connect_stdio()` doesn't check `oauth.get_token_for_server()` unlike `connect_http()`
   - Local MCP servers with OAuth requirements will fail silently

7. **No validation of tool name format** (`src/mcp/mod.rs:214-215`)
   - Tool names are exposed as `mcp__<server>__<tool>` without validation
   - If `server` or `tool` names contain invalid characters for tool naming, behavior is undefined

8. **`initialize()` in McpConnectionManager doesn't handle reconnect state** (`src/mcp/remote.rs:63-69`)
   - `connect()` just sets state to Connected and resets retry_count without checking current state
   - If called on an already-connected manager, no warning is logged

9. **Heartbeat task may leak on reconnect** (`src/mcp/remote.rs:71-104`)
   - Old heartbeat task continues running when `start_heartbeat()` is called after reconnect
   - `running.store(false, Ordering::SeqCst)` signals the old task to stop, but there's a race where the new task might also be starting

10. **No timeout on OAuth callback** (`src/mcp/auth.rs:774-820`)
    - `handle_callback()` reads from TCP stream with no timeout
    - A慢连接的客户端可能导致文件描述符泄漏

## Improvement Suggestions

### Performance

1. **Connection pooling for remote MCP servers**
   - Currently each `RemoteClient` creates its own `reqwest::Client`
   - Sharing a client with connection pooling would improve performance for many servers

2. **Lazy SSE connection initialization**
   - SSE connection is established but never used
   - Consider whether to remove it entirely or wire it up properly

3. **Batch tool discovery**
   - Tools are discovered individually per server at connection time
   - No caching layer for tool definitions across sessions

### Correctness

4. **Wire up SSE event processing**
   - Either integrate `take_sse_events()` into the agent loop or remove dead code
   - SSE provides server-initiated notifications that are currently lost

5. **Add timeout to `ensure_connected()` notification wait**
   - Prevents infinite hang if reconnection task fails silently

6. **Validate OAuth redirect URIs more strictly**
   - Only checks for HTTPS or localhost; should also validate path structure

### Maintainability

7. **Extract common JSON-RPC handling to shared module**
   - Both `local.rs` and `remote.rs` have identical `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcNotification` structs
   - `ide_server.rs` also duplicates these

8. **Add integration tests for MCP server connections**
   - No tests covering the full connection lifecycle
   - Hard to refactor without regression detection

9. **Document McpConnectionManager state transitions**
   - Add state machine diagram to clarify valid transitions

10. **Error message improvement in `parse_sse_response()`**
    - Currently returns "empty SSE data" which is confusing when data was present but parse failed

## Priority Actions (top 5 items to fix)

1. **[High] Fix SSE event processing** - `connect_sse()` and `take_sse_events()` are dead code. Either integrate SSE event dispatching into the agent event loop or remove the unused code. This is critical if SSE is needed for real-time server notifications.

2. **[High] Add timeout to `ensure_connected()`** - The `reconnect_needed.notified().await` at line 189 can hang forever if the spawned reconnection task fails to complete. Add a timeout (e.g., 30 seconds) and fallback behavior.

3. **[High] Fix heartbeat task leak on reconnect** - When `reconnect()` succeeds and calls `start_heartbeat()`, the old heartbeat task may still be running. The `running` flag approach has a race condition. Consider using a dedicated cancellation token.

4. **[Medium] Add OAuth token support for local servers** - `connect_stdio()` should check `oauth.get_token_for_server()` like `connect_http()` does, for consistency.

5. **[Medium] Remove or document `cli.rs` `Debug` command** - The `Debug` command at lines 309-318 only prints a message and doesn't actually test connections. Either implement it or remove it to avoid confusing users.

---

*Review completed: 2026-05-23*