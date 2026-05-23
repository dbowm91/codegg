# MCP Architecture Review

## Architecture Document
- Path: architecture/mcp.md

## Source Code Location
- src/mcp/

## Verification Summary
**Partial** - Most claims are accurate, but there are undocumented features and some inconsistencies.

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| McpClientType enum with Local/Remote variants | Pass | Exactly matches implementation |
| McpService struct with servers and oauth fields | Pass | Fields match, oauth is package-private |
| connect_stdio(), connect_http(), connect_from_config() methods | Pass | All present and match signature |
| disconnect() and shutdown_all() methods | Pass | Present and functional |
| LocalClient struct with all documented fields | Pass | Exact match including command, args, env, timeout, child, stdin, pending, shutdown_notify, request_id |
| LocalClient uses std::env::var_os("PATH") | Pass | Lines 88-92 correctly preserve user's PATH |
| LocalClient process spawn wrapped in 10s timeout | Pass | Line 98 uses `timeout.min(10000)` capped at 10s |
| LocalClient graceful shutdown via shutdown_notify | Pass | Uses Arc<Notify> mechanism |
| RemoteClient struct with all documented fields | Pass | All 11 fields present: url, headers, client, session_id, sse_url, oauth_token, sse_events, request_id, shutdown, sse_shutdown, validated_ips |
| McpConnectionManager struct fields | Partial | 10 fields documented, actual has 11 with heartbeat_cancellation added |
| ConnectionState enum with Connected/Disconnected/Reconnecting | Pass | Exact match |
| Auto-reconnect with exponential backoff 1s→2s→4s...→max 60s | Pass | Lines 128-173 implement correctly |
| Max 5 retry attempts | Pass | Line 72 sets max_retries = 5 |
| Heartbeat every 30s | Pass | Line 75 sets heartbeat_interval = Duration::from_secs(30) |
| ensure_connected() spawns reconnection in background task | Pass | Lines 176-226 implement this |
| OAuthManager struct fields | Pass | All 4 fields present: token_store, used_codes_store, servers, used_codes |
| TokenSet with access_token, refresh_token, token_type, expires_at, scope | Pass | Exact match |
| Tokens encrypted with AES-256-GCM using CODEGG_TOKEN_KEY | Pass | Lines 16-55 implement this |
| PKCE support for OAuth | Pass | generate_pkce_pair() at lines 124-135 |
| Replay protection via used codes store | Pass | exchange_code_for_tokens_with_replay_protection() at lines 284-322 |
| McpCli struct with config_path | Pass | Line 18 |
| McpCli commands: add, list, remove, enable, debug | Partial | add/list/remove/enable implemented, debug placeholder only |
| IdeServer struct with tools, pending, shutdown, shutdown_notify | Pass | Exact match |
| openDiff tool for IDE diff viewing | Pass | Lines 64-67, handler at lines 367-392 |
| IdeServer stdio and unix socket transport modes | Pass | run_stdio() and run_socket() both implemented |
| McpTool struct with server field (not server_id) | Pass | Line 58 has `server: String` |
| Tools exposed as mcp__<server>__<tool> | Pass | Line 215 formats name as "mcp__{}__{}" |
| McpServerStatus enum variants | Pass | Disconnected, Connecting, Connected, Error(String) |
| JSON-RPC protocol for requests/responses | Pass | All send_request/send_notification implementations use JSON-RPC 2.0 |
| DNS rebinding protection | Pass | validate_url_host() at new(), revalidate_dns() before each request |
| IPv4-mapped IPv6 addresses handled correctly | Pass | Security module handles this |
| Internal IP blocking | Pass | validate_host_ip() checks for internal IPs |

## Issues Found

### Bugs
1. **McpConnectionManager::Clone is manual but sound** - The Clone implementation at lines 43-59 is manually implemented because CancellationToken is !Clone. This is correct and the architecture doc doesn't mention Clone.

### Inconsistencies

1. **McpConnectionManager heartbeat_task vs heartbeat_cancellation**
   - Architecture doc shows: `heartbeat_task: Arc<AtomicBool>` (line 117)
   - Actual: `heartbeat_token: CancellationToken` and `heartbeat_cancellation: Arc<Mutex<Option<CancellationToken>>>` (lines 37-38)
   - The architecture doc's `heartbeat_task` field doesn't exist - it's been replaced with a CancellationToken-based implementation

2. **IdeServer debug command is placeholder**
   - Architecture doc lists "debug - Test connection to an MCP server" as a full command (line 187)
   - Actual implementation (cli.rs:309-318) only prints messages and doesn't actually test connections

3. **connect_from_config() not documented**
   - The architecture doc lists connect_stdio, connect_http, disconnect, shutdown_all
   - connect_from_config() exists at lines 281-309 but is not documented

4. **oauth_manager() and oauth_manager_mut() not documented**
   - Lines 318-324 have these public getter methods but they're not in the architecture doc

5. **IdeServer run_socket() not documented**
   - The architecture doc mentions "Unix socket mode" (line 207) but doesn't document run_socket() method

6. **IdeServer parse_file_reference() and parse_line_range() not documented**
   - These are internal helper functions that parse @file#L1-L99 syntax
   - They enable advanced file reference parsing that is not documented

### Missing Documentation

1. **LocalClient::new() takes command, args, env, timeout parameters** - Not documented in architecture

2. **LocalClient has additional methods**:
   - `list_prompts()` (line 220)
   - `get_prompt()` (line 263)
   - `list_resources()` (line 300)
   - `read_resource()` (line 330)

3. **RemoteClient has additional methods**:
   - `clear_oauth_token()` (line 434)
   - `connect_sse()` (line 698)
   - `shutdown()` (line 742)
   - `take_sse_events()` (line 802)

4. **McpService has additional public methods**:
   - `list_tools()` (line 210)
   - `server_tools()` (line 223)
   - `server_status()` (line 230)
   - `handle_tool_list_changed()` (line 237)
   - `oauth_manager()` / `oauth_manager_mut()` (lines 318-324)
   - `list_prompts()` / `get_prompt()` (lines 326-353)
   - `list_resources()` / `read_resource()` (lines 355-381)

5. **IdeServer parse_file_reference() supports @ syntax**:
   - Format: `path@file#L1-L99` or `path#L1-L99`
   - Enables line range specification in diff tool

6. **OAuthManager callback server**:
   - `start_callback_server()` (line 477) not documented
   - `handle_callback()` (line 775) not documented

7. **OAuthManager token encryption format**:
   - Uses `CODEGG_ENC_v1` magic bytes prefix (documented correctly)

8. **OAuthManager has sync and async versions**:
   - load_tokens_sync/async, save_tokens_sync/async
   - save_used_codes_sync/async, load_used_codes_sync/async

9. **Token expiration checking**:
   - `TokenSet::is_expired()` method (line 67) not documented

10. **McpConnectionManager accessor methods**:
    - `state()` (line 228)
    - `client_mut()` / `client()` (lines 232-238)
    - `set_oauth_token()` (line 249)
    - `max_retries()` / `set_max_retries()` (lines 295-301)
    - `base_delay()` / `set_base_delay()` (lines 303-309)
    - `max_delay()` / `set_max_delay()` (lines 311-317)
    - `heartbeat_interval()` / `set_heartbeat_interval()` (lines 319-325)

### Improvement Opportunities

1. **Add debug command implementation** - The debug command should actually test MCP server connections

2. **Document all public methods** - McpService has ~18 public methods but architecture doc only shows ~6

3. **Update McpConnectionManager documentation** - Replace `heartbeat_task: Arc<AtomicBool>` with actual `heartbeat_token: CancellationToken` and `heartbeat_cancellation: Arc<Mutex<Option<CancellationToken>>>`

4. **Document SSE support properly** - connect_sse(), connect_sse_stream(), take_sse_events() all exist but architecture doc says "SSE support not fully integrated" which is accurate but doesn't describe the actual implementation

5. **Add IdeServer run_socket() documentation** - The socket-based transport mode exists but is not documented

6. **Document prompt and resource APIs** - McpService has full support for prompts and resources with list_prompts, get_prompt, list_resources, read_resource but these are not documented

## Recommendations

1. **High Priority**:
   - Update McpConnectionManager field documentation to remove `heartbeat_task` and document `heartbeat_token` and `heartbeat_cancellation`
   - Document all public methods in McpService (currently shows ~6 but there are ~18)
   - Implement the debug command or remove it from documentation

2. **Medium Priority**:
   - Document SSE methods: connect_sse(), connect_sse_stream(), take_sse_events()
   - Document prompt/resource APIs in McpService
   - Document IdeServer::run_socket()

3. **Low Priority**:
   - Add documentation for OAuthManager::start_callback_server()
   - Document TokenSet::is_expired()
   - Document all the getter/setter methods in McpConnectionManager
