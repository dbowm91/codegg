# MCP (Model Context Protocol)

codegg implements the Model Context Protocol for connecting to external MCP servers that provide additional tools and resources.

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
- **`transport.rs`** - Transport implementations
- **`cli.rs`** - CLI helper utilities
- **`ide_server.rs`** - IDE MCP server for diff viewing

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
    "servers": {
      "filesystem": {
        "type": "local",
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/dir"]
      }
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

## Reconnection Behavior

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

For connection failures, the agent loop implements retry with exponential backoff:

```rust
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
let max_retries = 3;
let mut delay = Duration::from_secs(1);

// Retry logic
tokio::time::sleep(delay).await;
delay = delay.saturating_mul(2).min(MAX_RETRY_DELAY);
```

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

### Local Server Config
```json
{
  "mcp": {
    "servers": {
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
}
```

### Remote Server Config
```json
{
  "mcp": {
    "servers": {
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
}
```

## Error Handling

| Error | Cause |
|-------|-------|
| `McpError::Connection` | Network/connection failures |
| `McpError::Server` | Server-side errors |
| `McpError::ToolCall` | Tool execution failures |
| `McpError::Transport` | Transport-level errors |

## SSE (Server-Sent Events)

Remote servers use SSE for server-to-client notifications:

```rust
pub async fn connect_sse(&self) -> Result<(), McpError>
```

The SSE stream is parsed with:
- 1MB buffer limit to prevent unbounded memory
- Event data extracted from `data:` lines
- JSON parsed and stored in `sse_events` buffer
