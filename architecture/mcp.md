# MCP Module

Model Context Protocol (MCP) client implementation for connecting to
external tool servers — local (stdio) and remote (HTTP).

## Purpose

The MCP module provides a JSON-RPC 2.0 client that discovers, connects
to, and calls tools on external MCP servers. It is the transport layer
that powers both general-purpose MCP integrations (file system, git,
database servers) and Codegg-managed backends like `eggsearch`.

## Where It Lives

```
src/mcp/
├── mod.rs          # McpService, McpTool, McpClientType, McpExposurePolicy
├── local.rs        # JSON-RPC over stdio (child process)
├── remote.rs       # JSON-RPC over HTTP + optional SSE, McpConnectionManager
├── auth.rs         # OAuthManager, token encryption, PKCE, callback server
├── cli.rs          # CLI subcommands: add, list, remove, enable, debug
└── ide_server.rs   # IDE integration MCP server (openDiff tool)
```

## How It Works

### Connection Lifecycle

1. **Startup**: `bootstrap::bootstrap_eggsearch` creates an `McpService`,
   calls `connect_stdio` (local) or `connect_http` (remote) for each
   configured server, and stores the service in a process-global slot
   (`search_backend::state`).

2. **Tool discovery**: After connection, `discover_tools()` sends
   `tools/list` over JSON-RPC and collects `McpTool` definitions.

3. **Tool calls**: `McpService::call_tool_structured()` looks up the
   target server, acquires a write lock on the client, and dispatches
   `tools/call`. The response is returned as `McpToolCallResult` with
   both text and optional structured JSON.

4. **Shutdown**: `McpService::shutdown_all()` disconnects every server
   in order. Local clients kill the child process; remote clients send
   `notifications/cancelled` and cancel heartbeat tasks.

### Client Types

```rust
pub enum McpClientType {
    Local(Arc<RwLock<LocalClient>>),
    Remote(Arc<RwLock<McpConnectionManager>>),
    Mock(Arc<Mutex<Box<dyn Fn(&str, Value) -> Result<String, McpError> + Send + Sync>>>),
}
```

`Mock` is a test-only variant. Production never constructs it; the
`register_mock_server` helper is the single entry point for integration
tests.

### Local Client (`local.rs`)

Spawns a child process, communicates via JSON-RPC over stdin/stdout.

- **PATH preservation**: `env_clear()` then re-injects the user's
  `PATH` so the child inherits the real environment.
- **Spawn timeout**: Process spawn wrapped in `min(timeout, 10s)`
  timeout to prevent hangs on slow `npx`/`uvx` cold starts.
- **Read loop**: A background task reads stdout lines, correlates
  JSON-RPC responses by `id` via a `PendingSenders` map, and delivers
  results through `oneshot::Sender`. On EOF, all pending senders
  receive `McpError::Connection("MCP server connection closed")`.
- **Stderr drain**: A separate task continuously drains stderr to
  prevent pipe deadlock when the child writes diagnostic output.
- **Graceful shutdown**: Sends `notifications/cancelled`, then
  `shutdown_notify.notify_waiters()` to break the read loop, then
  `child.kill()` + `child.wait()`. `Drop` calls `start_kill()` as a
  safety net.
- **Server version**: Extracted from `initialize` response at
  `/serverInfo/version` (`local.rs:163`).

### Remote Client (`remote.rs`)

Sends JSON-RPC over HTTP POST. Supports server-initiated SSE for
responses that arrive as `text/event-stream`.

- **DNS rebinding protection**: Validates host IP on `new()` and
  before every `post_json()` call via `revalidate_dns()`. Internal IPs
  (loopback, private, link-local, CGNAT) are blocked.
- **Session management**: Stores `Mcp-Session-Id` from `initialize`
  response; includes it as a header on subsequent requests.
- **OAuth**: Bearer token injected from `OAuthManager` when available.
- **Redirect policy**: `reqwest::redirect::Policy::none()` — redirects
  are not followed to prevent SSRF via redirect chains.
- **SSE response parsing**: When `post_json` receives a body starting
  with `event:`, it parses the SSE data lines as a JSON-RPC response.
  The `connect_sse_stream` method for persistent SSE subscriptions
  exists but is `#[allow(dead_code)]` and not called in production.

### McpConnectionManager (`remote.rs`)

Wraps `RemoteClient` with auto-reconnect and heartbeat:

- **Exponential backoff**: 1s → 2s → 4s → ... → max 60s
- **Max retries**: 5 attempts before giving up
- **Heartbeat**: Every 30s sends a `ping` notification. On failure,
  triggers reconnect.
- **`ensure_connected()`**: If disconnected, spawns reconnect in a
  background task, waits up to 30s for success, then falls back to a
  synchronous `connect()`.

### McpService (`mod.rs`)

Central registry holding `HashMap<String, McpServer>` and an
`OAuthManager`. Key methods:

| Method | Description |
|--------|-------------|
| `connect_stdio` | Spawn local server via stdio |
| `connect_http` | Connect to remote server via HTTP |
| `connect_from_config` | Dispatch to `stdio`/`http` based on `server_type` |
| `disconnect` | Gracefully disconnect one server |
| `shutdown_all` | Disconnect all servers |
| `call_tool` | String-only tool call |
| `call_tool_structured` | Tool call retaining `structuredContent` |
| `list_tools` | All MCP tools as `ToolDefinition` |
| `list_filtered_tools` | Tools filtered through `McpExposurePolicy` |
| `handle_tool_list_changed` | Re-discover tools for a server |
| `list_prompts` / `get_prompt` | Prompt management |
| `list_resources` / `read_resource` | Resource management |

### McpExposurePolicy (`mod.rs`)

Controls which raw `mcp__<server>__<tool>` definitions reach the model:

```rust
pub struct McpExposurePolicy {
    pub show_raw: bool,           // default false
    pub hidden_servers: Vec<String>, // servers to hide even when show_raw=true
}
```

When `show_raw` is `false` (the default), `list_filtered_tools` returns
an empty vec. This is how eggsearch's raw MCP tools are kept hidden
from the model — the native wrappers (`websearch`, `webfetch`) own that
surface.

### Structured Call Path

`McpToolCallResult` (`mod.rs:68`) carries both the text projection and
an optional `structured` JSON value. The structured value is extracted
from either:
1. `result.structuredContent` (protocol-level), or
2. `content[type=json].json` (content-level)

The eggsearch adapter uses `call_tool_structured` to retain machine-
readable evidence before display caps are applied.

### IDE Server (`ide_server.rs`)

An MCP *server* (not client) exposing the `openDiff` tool for IDE
integration. Supports stdio transport. The `handle_connection` method
for Unix socket mode exists but is `#[allow(dead_code)]`.

### OAuth (`auth.rs`)

`OAuthManager` manages per-server token storage with encryption:

- **Encryption**: AES-256-GCM with 12-byte random nonce. Key from
  `CODEGG_TOKEN_KEY` env var (SHA-256 hashed if <32 bytes). Magic
  bytes `CODEGG_ENC_v1` prefix for version detection.
- **Token storage**: `~/.config/codegg/mcp_tokens.json` (encrypted),
  `~/.config/codegg/mcp_used_codes.json` (plaintext, 0600 perms).
- **PKCE**: S256 challenge, callback server on `127.0.0.1:0`.
- **Replay protection**: Used authorization codes tracked with expiry.
- **Redirect validation**: Must be HTTPS or `localhost`/`127.0.0.1`.

### CLI (`cli.rs`)

```
codegg mcp add <name> --type local|remote [--command ...] [--url ...]
codegg mcp list
codegg mcp remove <name>
codegg mcp enable <name> --enabled true|false
codegg mcp debug [--name <server>] [--url <url>]
```

The `debug` command tests connection to a named server from config or
an arbitrary URL. For remote servers, it connects, discovers tools,
and reports the count.

## Key Types & APIs

| Type | File:Line | Purpose |
|------|-----------|---------|
| `McpService` | `mod.rs:109` | Server registry + OAuth manager |
| `McpServer` | `mod.rs:82` | Per-server state (name, status, tools, version, client) |
| `McpClientType` | `mod.rs:91` | Local / Remote / Mock dispatch |
| `McpExposurePolicy` | `mod.rs:122` | Controls raw MCP tool visibility |
| `McpTool` | `mod.rs:56` | Tool definition from server |
| `McpToolCallResult` | `mod.rs:68` | Text + optional structured JSON |
| `McpServerStatus` | `mod.rs:73` | Disconnected / Connecting / Connected / Error |
| `McpPrompt` | `mod.rs:26` | Prompt definition |
| `McpResource` | `mod.rs:39` | Resource definition |
| `McpResourceContent` | `mod.rs:47` | Resource content (text or blob) |
| `LocalClient` | `local.rs:47` | JSON-RPC over stdio child process |
| `RemoteClient` | `remote.rs:345` | JSON-RPC over HTTP with SSE parsing |
| `McpConnectionManager` | `remote.rs:29` | Auto-reconnect + heartbeat wrapper |
| `ConnectionState` | `remote.rs:19` | Connected / Disconnected / Reconnecting |
| `OAuthManager` | `auth.rs:109` | Token lifecycle, encryption, PKCE |
| `TokenSet` | `auth.rs:76` | Access + refresh tokens with expiry |
| `McpCli` | `cli.rs:17` | CLI command handler |
| `McpCommand` | `cli.rs:184` | Clap subcommand enum |
| `IdeServer` | `ide_server.rs:50` | IDE MCP server (openDiff) |
| `McpError` | `error.rs:177` | Connection, Server, ToolCall, OAuth, Encryption, Timeout |
| `parse_mcp_tool_server` | `mod.rs:164` | Extract server from `mcp__<server>__<tool>` |

## Configuration Surface

MCP servers are configured under `[mcp]` in `config.json`:

```json
{
  "mcp": {
    "servers": {
      "filesystem": {
        "enabled": true,
        "type": "local",
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/projects"],
        "env": { "PATH": "${PATH}" },
        "timeout": 30000
      },
      "github": {
        "enabled": true,
        "type": "remote",
        "url": "https://api.github.com/mcp",
        "headers": { "Authorization": "Bearer ${GITHUB_TOKEN}" },
        "timeout": 60000,
        "reconnect": {
          "enabled": true,
          "max_retries": 5,
          "base_delay_secs": 1,
          "max_delay_secs": 60,
          "heartbeat_interval_secs": 30
        }
      },
      "slack": {
        "enabled": true,
        "type": "remote",
        "url": "https://slack-mcp.example.com",
        "oauth": {
          "client_id": "${SLACK_CLIENT_ID}",
          "client_secret": "${SLACK_CLIENT_SECRET}",
          "scope": "chat:write,channels:read"
        }
      }
    }
  }
}
```

Config types (`codegg-config/src/schema.rs`):

| Struct | Key | JSON field |
|--------|-----|------------|
| `McpEntry` | `:881` | `enabled`, flattened `McpServerConfig` |
| `McpServerConfig` | `:889` | type, command, args, env, url, headers, etc. |
| `McpReconnectConfig` | `:906` | `enabled`, `max_retries`, delay, heartbeat intervals |
| `McpOAuthConfig` | `:914` | `client_id`, `client_secret`, `scope` |

Note: The JSON key is `"type"` (via `#[serde(rename = "type")]` on
`McpServerConfig.server_type`). The `environment` field is merged with
`env` when present.

## Invariants & Gotchas

- **`McpService` is not `Send + Sync`** — it is always wrapped in
  `Arc<RwLock<McpService>>` for shared access.
- **Write lock during tool calls** — `call_tool_structured` acquires a
  write lock on the inner client, so concurrent calls to the same
  server serialize. This is acceptable because tool calls are
  infrequent relative to other operations.
- **Mock variant is always compiled** — `McpClientType::Mock` is
  behind `#[cfg(test)]` neither in the enum definition nor the match
  arms. The variant exists unconditionally so integration test binaries
  (which don't share `cfg(test)`) can reach `register_mock_server`.
- **SSE is partially integrated** — `parse_sse_response` handles SSE
  formatted responses in `post_json`, but `connect_sse_stream` for
  persistent SSE subscriptions is `#[allow(dead_code)]`. The agent
  does not initiate long-lived SSE connections.
- **Heartbeat uses `send_notification`** — the heartbeat `ping` is a
  notification (no response expected). If it fails, reconnect is
  triggered.
- **Protocol version**: `2024-11-05` is sent in `initialize` params.
- **Spawn env is cleared** — `env_clear()` then re-injects PATH and
  configured env. This prevents the child from inheriting Codegg's
  own environment variables.
- **Tool definition cache**: Uses `mcp_tool_count` as a proxy for
  changes. If tool identities change without count changing, cache
  may be stale.

## Testing

```bash
# MCP unit + integration tests
cargo test -p codegg --test mcp
cargo test -p codegg --test mcp_reconnect
cargo test -p codegg --test fake_eggsearch_mcp

# Unit tests within the module
cargo test -p codegg mcp::tests
cargo test -p codegg mcp::local::tests
```

`fake_eggsearch_mcp` exercises the end-to-end eggsearch dispatch
path using an in-process mock `McpService` (no real binary needed).

## Related Docs

- [search_backend.md](search_backend.md) — eggsearch adapter consumes
  `McpService` for search/fetch operations
- [tool.md](tool.md) — tool execution and `McpExposurePolicy`
- [agent.md](agent.md) — uses MCP tools via `ToolRegistry`
- [security.md](security.md) — DNS rebinding protection
