# MCP Module Architecture Review

## Verified Claims

### Core Types (all match)
- `McpClientType` (mod.rs:77-81) - matches exactly
- `McpService` (mod.rs:83-86) - matches
- `McpServer` (mod.rs:70-75) - matches
- `McpTool` (mod.rs:54-59) - matches
- `McpServerStatus` (mod.rs:61-68) - matches
- `ConnectionState` (remote.rs:19-27) - matches
- `OAuthManager` (auth.rs:91-96) - matches
- `TokenSet` (auth.rs:57-64) - matches

### LocalClient (local.rs)
- Fields match (command, args, env, timeout, child, stdin, pending, shutdown_notify, request_id)
- Uses `std::env::var_os("PATH")` for PATH preservation (line 88-91)
- Process spawn wrapped in 10s timeout (line 98)
- Graceful shutdown via Notify mechanism

### RemoteClient (remote.rs)
- Fields match (url, headers, client, session_id, sse_url, oauth_token, sse_events, request_id, shutdown, sse_shutdown, validated_ips)
- DNS rebinding protection correctly implemented with IP re-validation before each request
- IPv4-mapped IPv6 addresses handled correctly via `ipv6_segments_to_ipv4()`

### McpConnectionManager (remote.rs)
- Exponential backoff: `2^i` seconds capped at max_delay (line 131)
- Max 5 retry attempts (line 72)
- Heartbeat every 30s (line 75)
- `ensure_connected()` spawns reconnection in background task when disconnected

### OAuth Flow (auth.rs)
- PKCE support via `generate_pkce_pair()` (lines 124-135)
- Replay protection via used codes store (lines 270-308)
- Token encryption with AES-256-GCM using `CODEGG_TOKEN_KEY` env var
- Magic bytes prefix `CODEGG_ENC_v1` for version detection

### Protocol
- JSON-RPC over stdio for LocalClient (local.rs)
- JSON-RPC over HTTP for RemoteClient (remote.rs)
- Bearer token authentication implemented

## Bugs/Discrepancies Found

### 1. McpConnectionManager field mismatch (HIGH)
**Doc says**: `heartbeat_task: Arc<AtomicBool>` (line 117)
**Actual**: `heartbeat_token: CancellationToken` and `heartbeat_cancellation: Arc<Mutex<Option<CancellationToken>>>` (lines 37-38)

The documentation is outdated - the implementation uses CancellationToken for cleaner task management, not AtomicBool.

### 2. IdeServer struct missing `client` field in doc (MEDIUM)
**Doc says**: IdeServer has `tools`, `pending`, `shutdown`, `shutdown_notify`
**Actual**: Same, but doc mentions `client` field that doesn't exist

### 3. McpService::connect_http key generation inconsistency (LOW)
**Doc implies**: Server key is just `name`
**Actual**: Uses `format!("{}:{}", name, url)` (mod.rs:139)

This means HTTP servers use name+URL as key while stdio servers use just name. This inconsistency is not documented.

### 4. SSE not actually called during connect (BUG - MEDIUM)
`connect_sse()` and `connect_sse_stream()` exist but `McpConnectionManager::connect()` never calls them. SSE events are collected but never processed by the agent. This matches the "Known Implementation Issues" section, but the issue persists - `take_sse_events()` is never called anywhere.

### 5. Missing documented methods (MEDIUM)

**McpService undocumented methods:**
- `oauth_manager()` (mod.rs:318)
- `oauth_manager_mut()` (mod.rs:322)
- `list_prompts()` (mod.rs:326)
- `get_prompt()` (mod.rs:338)
- `list_resources()` (mod.rs:355)
- `read_resource()` (mod.rs:367)
- `handle_tool_list_changed()` (mod.rs:237)

**McpConnectionManager undocumented methods:**
- `state()` (remote.rs:228)
- `client_mut()` (remote.rs:232)
- `client()` (remote.rs:236)
- `set_oauth_token()` (remote.rs:249)
- `max_retries()` / `set_max_retries()` (remote.rs:295-301)
- `base_delay()` / `set_base_delay()` (remote.rs:303-309)
- `max_delay()` / `set_max_delay()` (remote.rs:311-317)
- `heartbeat_interval()` / `set_heartbeat_interval()` (remote.rs:319-325)

**RemoteClient undocumented methods:**
- `clear_oauth_token()` (remote.rs:434)
- `connect_sse()` (remote.rs:698)
- `take_sse_events()` (remote.rs:802)
- `reconnect()` (remote.rs:692)
- `parse_sse_response()` (remote.rs:941)

**OAuthManager undocumented methods:**
- `generate_pkce_pair()` (auth.rs:124)
- `build_authorization_url()` (auth.rs:137)
- `exchange_code_for_tokens()` (auth.rs:172)
- `refresh_tokens()` (auth.rs:324)
- `revoke_token()` (auth.rs:404)
- `store_tokens_async()` (auth.rs:436)
- `get_valid_token()` (auth.rs:453)
- `get_token_for_server()` (auth.rs:461)
- `remove_tokens_async()` (auth.rs:466)
- `generate_state()` (auth.rs:471)
- `start_callback_server()` (auth.rs:477)
- Various sync/async save/load methods

**IdeServer undocumented methods:**
- `run_socket()` (ide_server.rs:121)
- `clone_for_connection()` (ide_server.rs:146)
- `handle_connection()` (ide_server.rs:155)
- `handle_request()` (ide_server.rs:196)

### 6. Missing undocumented types (LOW)
- `McpPrompt`, `PromptArgument` (mod.rs:24-35) - not in doc
- `McpResource`, `McpResourceContent` (mod.rs:38-51) - not in doc
- `McpServerInfo` (cli.rs:174-181) - not in doc
- `ServerType` enum (cli.rs:238-252) - not in doc
- `McpCommand` enum (cli.rs:183-236) - not in doc

### 7. Token encryption key env var name mismatch (LOW)
**Doc says**: Tokens encrypted with `CODEGG_TOKEN_KEY` (line 167) - CORRECT
**Implementation**: Same at auth.rs:16 - VERIFIED CORRECT

## Improvement Suggestions

### HIGH Priority

1. **Fix McpConnectionManager heartbeat field documentation**
   - Update architecture/mcp.md line 117 to show `heartbeat_token: CancellationToken` and `heartbeat_cancellation: Arc<Mutex<Option<CancellationToken>>>`

2. **Document SSE integration status accurately**
   - Either document that SSE support is experimental/not-integrated, OR wire `connect_sse()` into the connection flow
   - Currently `connect_sse_stream()` spawns a background task that stores events, but `take_sse_events()` is never called

### MEDIUM Priority

3. **Document all public methods** - The following are missing from the doc:
   - McpService: `oauth_manager`, `oauth_manager_mut`, `list_prompts`, `get_prompt`, `list_resources`, `read_resource`, `handle_tool_list_changed`
   - McpConnectionManager: `state`, `client_mut`, `client`, setter/getter pairs for all config fields
   - RemoteClient: `clear_oauth_token`, `connect_sse`, `take_sse_events`, `reconnect`
   - OAuthManager: PKCE methods, token management methods, callback server
   - IdeServer: `run_socket`, `handle_connection`

4. **Clarify server key generation**
   - Document that `connect_http` uses `name:url` as key while `connect_stdio` uses just `name`

### LOW Priority

5. **Add missing types to doc**
   - `McpPrompt`, `PromptArgument`, `McpResource`, `McpResourceContent`
   - `McpServerInfo`, `ServerType`, `McpCommand`

6. **Consider adding McpCli section**
   - The CLI for managing MCP servers (`add`, `list`, `remove`, `enable`, `debug` commands) isn't documented