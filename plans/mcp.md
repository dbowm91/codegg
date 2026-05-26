# mcp Architecture Review Findings

## Verified Claims

- **McpClientType enum**: `Local(Arc<RwLock<LocalClient>>)`, `Remote(Arc<RwLock<McpConnectionManager>>)` - CORRECT (mod.rs:77-81)
- **McpService struct**: `servers: HashMap<String, McpServer>`, `oauth: OAuthManager` - CORRECT (mod.rs:83-86)
- **McpServer struct**: `name`, `status`, `tools`, `client` - CORRECT (mod.rs:70-75)
- **Connection methods**: `connect_stdio()`, `connect_http()`, `connect_from_config()`, `disconnect()`, `shutdown_all()` - ALL PRESENT (mod.rs:96-316)
- **LocalClient struct fields**: `command`, `args`, `env`, `timeout`, `child`, `stdin`, `pending`, `shutdown_notify`, `request_id` - CORRECT (local.rs)
- **RemoteClient struct fields**: `url`, `headers`, `client`, `session_id`, `sse_url`, `oauth_token`, `sse_events`, `request_id`, `shutdown`, `sse_shutdown`, `validated_ips` - CORRECT (remote.rs:88-103)
- **McpConnectionManager struct**: All 10 fields present as documented - CORRECT (remote.rs:29-41)
- **ConnectionState enum**: `Connected`, `Disconnected` (default), `Reconnecting { attempt: u32 }` - CORRECT (remote.rs:19-27)
- **OAuthManager struct**: `token_store`, `used_codes_store`, `servers`, `used_codes` - CORRECT (auth.rs)
- **TokenSet fields**: `access_token`, `refresh_token`, `token_type`, `expires_at`, `scope` - CORRECT (auth.rs:179-185)
- **Token encryption**: `CODEGG_TOKEN_KEY` env var, `CODEGG_ENC_v1` magic bytes - CORRECT (auth.rs)
- **PKCE support**: Documented - need to verify implementation
- **McpTool struct**: `name`, `description`, `input_schema`, `server` - CORRECT (mod.rs:53-59)
- **McpServerStatus enum**: `Disconnected`, `Connecting`, `Connected`, `Error(String)` - CORRECT (mod.rs:61-68)
- **Tool naming**: `mcp__<server>__<tool>` format - CORRECT (mod.rs:215)
- **IdeServer struct**: `tools`, `pending`, `shutdown`, `shutdown_notify` - CORRECT (ide_server.rs)
- **openDiff tool support**: Documented - CORRECT
- **SSE methods location**: Lines 698-747 in remote.rs - CORRECT
- **SSE methods signatures**: `connect_sse()`, `connect_sse_stream()`, `take_sse_events()` - CORRECT
- **DNS validation functions**: `validate_url_host`, `validate_host_ip`, `revalidate_dns` - CORRECT (remote.rs imports from security/ssrf)
- **revalidate_dns called before each request**: Line 869 in remote.rs - CORRECT
- **initialize() re-validates DNS**: Line 400 in remote.rs (RemoteClient::initialize) - CORRECT
- **Heartbeat interval**: 30s - CORRECT (remote.rs:75)
- **Max retries**: 5 - CORRECT (remote.rs:72)
- **Base delay**: 1s, Max delay: 60s - CORRECT (remote.rs:73-74)
- **Exponential backoff**: 1s → 2s → 4s → ... → max 60s - CORRECT (documented, implementation verified)
- **McpEntry config fields**: `server_type`, `command`, `args`, `env`, `environment`, `url`, `headers`, `transport`, `timeout`, `oauth`, `reconnect` - CORRECT (verified via http.rs:176-191)
- **reconnect config**: `enabled`, `max_retries`, `base_delay_secs`, `max_delay_secs`, `heartbeat_interval_secs` - CORRECT

## Stale Information

- **Line reference for SSE methods outdated**: Document says "src/mcp/remote.rs:698-747" for SSE connection methods. Actual lines are 698-746 (connect_sse) and 747+ for connect_sse_stream. Close but slight offset due to code changes.

## Bugs Found

- **SSE not integrated during remote connection setup**: Document correctly identifies this as "Known Implementation Issue" at lines 375-376. Verified: `connect_sse()` is defined but never called in the normal connection flow. SSE events are collected but not processed by agent.
- **Tool definition cache staleness**: Document correctly identifies this as "Known Implementation Issue" at lines 373-374. Using `mcp_tool_count` as proxy is acknowledged limitation.

## Improvements Suggested

- **Document cross-reference update**: The doc at line 206 mentions "Client SSE methods are documented in architecture/mcp.md" - this is self-referential. Should say "Client SSE connection methods are implemented in src/mcp/remote.rs:698-747"
- **PKCE verification**: Document states PKCE support exists but couldn't verify implementation details - recommend verification if OAuth flows are critical.

## Cross-Module Issues

- **McpService used in server**: Server's `ServerState` holds `Arc<RwLock<McpService>>` (state.rs:16). Server startup connects to MCP servers from config (http.rs:169-201).
- **MCP tool integration**: `list_tools()` in McpService produces `ToolDefinition` for agent tool registry. Tools formatted as `mcp__<server>__<tool>`.
- **OAuthManager dependency**: MCP auth.rs handles token encryption which may interact with crypto module.
- **SSRF protection**: RemoteClient uses `validate_url_host`, `validate_host_ip`, `revalidate_dns` from `crate::security::ssrf` - correctly isolates security concern.