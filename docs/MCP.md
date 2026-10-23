# MCP (Model Context Protocol)

codegg is an MCP **client**: it connects to external MCP servers that provide
additional tools and resources. It also runs one small MCP **server** of its
own — `ide_server.rs` exposes IDE diff-viewing tools to IDE extensions over
stdio or a unix socket — but the model-facing integration is entirely
client-side.

## Overview

MCP enables extending the agent's capabilities through external servers that implement the MCP specification. These servers can provide:
- Custom tools with arbitrary functionality
- Resources (files, data, etc.)
- Prompts/templates

## Architecture

The MCP module (`src/mcp/`) consists of:

- **`mod.rs`** - `McpService` orchestrating all MCP connections
- **`local.rs`** - `LocalClient` for stdio-based local servers
- **`remote.rs`** - `RemoteClient` for HTTP-based remote servers
- **`auth.rs`** - `OAuthManager` for OAuth authentication flow
- **`cli.rs`** - CLI helper utilities
- **`protocol.rs`** - shared JSON-RPC/protocol types
- **`ide_server.rs`** - IDE MCP server for diff viewing (codegg as the *server*)

## Server Types

### Local Servers (stdio)

Local MCP servers communicate over stdin/stdout:

```rust
pub async fn connect_stdio(
    &mut self,
    name: &str,
    command: &str,
    args: &[String],
    env: HashMap<String, String>,
    timeout: u64,
) -> Result<(), McpError>
```

Example configuration:
```json
{
  "mcp": {
    "filesystem": {
      "type": "local",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/dir"]
    }
  }
}
```

### Remote Servers (HTTP+SSE)

Remote servers use HTTP for requests and SSE for notifications:

```rust
pub async fn connect_http(
    &mut self,
    name: &str,
    url: &str,
    headers: HashMap<String, String>,
    timeout: u64,
) -> Result<(), McpError>
```

## McpService

The `McpService` manages all MCP server connections:

```rust
pub struct McpService {
    servers: HashMap<String, McpServer>,
    oauth: OAuthManager,
}
```

Key methods:
- `connect_stdio()` / `connect_http()` - Connect to servers
- `disconnect()` - Disconnect a server
- `call_tool()` - Call a tool on a server
- `list_tools()` - List all available tools
- `handle_tool_list_changed()` - Refresh tools after server change

## Tool Naming

MCP tools are exposed with a prefixed name format: `mcp__{server}__{tool}`

For example, if a server named "filesystem" has a tool "read_file", it becomes `mcp__filesystem__read_file`.

Whether raw `mcp__*__*` tools actually reach the model is decided by
`McpExposurePolicy` (`McpService::list_filtered_tools`). The default policy is
`McpExposurePolicy::hide_all()`: raw definitions are hidden and callers see
only stable native wrappers, and `hidden_servers` can suppress a server even
when `show_raw` is `true`. `list_tools()` is the unrestricted variant
(`show_raw: true`, no hidden servers).

## CLI

MCP servers are managed from the command line:

```
codegg mcp list                        # list configured servers
codegg mcp add <NAME> [--server-type local|remote] [-c COMMAND] [-a ARGS] [-u URL]
codegg mcp remove <NAME>               # remove a server
codegg mcp enable <NAME> [--enabled]   # enable (default) or disable a server
codegg mcp debug [<NAME>] [-u URL]     # test a connection to a server
```

There is no `codegg plugins` subcommand: Agent Plugin packages are managed
from the TUI and the `plugin` config key (see [`PLUGINS.md`](PLUGINS.md)).

## Reconnection Behavior

### Resources and prompts

CodeGG exposes bounded MCP resources through deferred native wrappers. Search
returns descriptors and an integrity-bound handle; read accepts that handle
only after revalidation against the current connected `McpService` view.
Binary content is metadata-only, and external text is trust-framed and capped.

MCP prompts remain host/user actions. A caller that has an explicit selection
uses `McpService::invoke_user_selected_prompt`, which validates required
arguments and returns source role metadata as untrusted data. The model cannot
activate a prompt autonomously, and a remote role label cannot create a
privileged provider message.

The `RemoteClient::reconnect()` method handles reconnection:

```rust
pub async fn reconnect(&mut self) -> Result<(), McpError> {
    *self.session_id.lock().await = None;
    *self.sse_url.lock().await = None;
    self.initialize().await
}
```

The reconnection:
1. Clears the session ID and SSE URL
2. Re-runs the initialize handshake
3. Re-discovers tools

### Exponential Backoff

The `McpConnectionManager` (`remote.rs`) implements reconnection retry with exponential backoff:

```rust
// McpConnectionManager defaults:
max_retries: 5,
base_delay: Duration::from_secs(1),
max_delay: Duration::from_secs(60),
```

The agent loop's provider-level stream retry uses different values (3 retries, 30s max delay) and should not be confused with MCP reconnection.

## DNS Rebinding Protection

Remote MCP connections validate DNS to prevent rebinding attacks:

1. At connection time: `validate_host_ip()` checks resolved IPs
2. Before each request: `revalidate_dns()` re-validates IPs

```rust
// Connection time validation
let validated_ips = validate_host_ip(&host, port)?;

// Before each request
revalidate_dns(&host, port, ips)?;
```

## OAuth Support

Remote servers can use OAuth authentication:

```rust
pub async fn set_oauth_token(&self, token: String) {
    *self.oauth_token.lock().await = Some(token);
}
```

The `OAuthManager` (`auth.rs`) handles the OAuth flow for servers that require it.

## Configuration

Servers are keyed **directly** under `mcp` — there is no `servers` level. Note that
plugin MCP import files such as `assets/portable-plugins/playwright-mcp/mcp.json`
wrap their servers in an `mcpServers` object; that is a separate plugin file format,
not the `codegg.jsonc` schema, and the two are not interchangeable.

### Local Server Config
```json
{
  "mcp": {
    "server_name": {
      "type": "local",
      "command": "path/to/server",
      "args": ["arg1", "arg2"],
      "env": {
        "KEY": "value"
      },
      "timeout": 30000
    }
  }
}
```

### Remote Server Config
```json
{
  "mcp": {
    "remote_server": {
      "type": "remote",
      "url": "https://server.example.com/mcp",
      "headers": {
        "Authorization": "Bearer token"
      },
      "timeout": 30000
    }
  }
}
```

## Error Handling

| Error | Cause |
|-------|-------|
| `McpError::Connection` | Network/connection failures |
| `McpError::Server` | Server-side errors |
| `McpError::ToolCall` | Tool execution failures |
| `McpError::OAuth` | OAuth authentication errors |
| `McpError::Encryption` | Encryption/decryption errors |
| `McpError::Timeout` | Request timeout errors |

## SSE (Server-Sent Events)

Remote servers use SSE for server-to-client notifications. There is no public
`connect_sse`; the stream is consumed by a private helper on `RemoteClient`:

```rust
#[allow(dead_code)]
async fn connect_sse_stream(&self, mut resp: eggfetch_core::Response) -> Result<(), McpError>
```

Buffered events are drained with `RemoteClient::take_sse_events()`.

The SSE stream is parsed with:
- 1MB buffer limit to prevent unbounded memory
- Event data extracted from `data:` lines
- JSON parsed and stored in `sse_events` buffer

## Source verification

Checked 2026-10-23 against the source tree.

- Module layout (`mod.rs`, `local.rs`, `remote.rs`, `auth.rs`, `cli.rs`,
  `protocol.rs`, `ide_server.rs`): `src/mcp/`.
- `McpError` variants `Connection`, `Server`, `ToolCall`, `OAuth`,
  `Encryption`, `Timeout`: `crates/codegg-core/src/error.rs`.
- `McpExposurePolicy::hide_all()` as the default exposure and
  `McpService::list_filtered_tools` as the filtered variant:
  `src/mcp/mod.rs`.
- The `mcp__<server>__<tool>` naming: `src/mcp/mod.rs`.
- CLI verbs (`list`, `add`, `remove`, `enable`, `debug`) and their flags:
  `src/mcp/cli.rs`. There is no `codegg plugins` subcommand:
  `src/main.rs` `Commands`.
- `McpConnectionManager` defaults (5 retries, 1 s base delay, 60 s max
  delay): `src/mcp/remote.rs`.
- Provider-level stream retry (3 attempts, 30 s max backoff): distinct from
  MCP reconnection — `crates/codegg-providers/src/retry.rs` and
  `src/agent/provider_turn.rs`.
- The private `connect_sse_stream` helper, the 1 MB SSE buffer limit, and
  `RemoteClient::take_sse_events()`: `src/mcp/remote.rs`.
- `mcp` is keyed directly under the config root with no `servers` level, and
  the `type`/`command`/`args`/`env`/`environment`/`url`/`headers`/`transport`/
  `timeout`/`oauth`/`reconnect` fields: `crates/codegg-config/src/schema.rs`
  (`McpEntry`, `McpServerConfig`).
- The plugin-side `mcpServers` wrapper is a separate file format:
  `assets/portable-plugins/playwright-mcp/mcp.json`.
