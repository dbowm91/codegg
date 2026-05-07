# MCP Module

The `mcp` module implements the Model Context Protocol client for connecting to external MCP servers.

## Overview

**Location**: `src/mcp/`

**Key Responsibilities**:
- MCP client implementation (local and remote)
- Server connection management
- OAuth authentication support
- Tool exposure to AgentLoop
- Auto-reconnection (exists but not fully wired)

## MCP Overview

MCP (Model Context Protocol) allows external tools and resources to be exposed to the agent as if they were native tools.

## Client Types

### McpClientType

```rust
pub enum McpClientType {
    Local {
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    Remote {
        url: Url,
        auth: Option<McpAuth>,
    },
}
```

### McpAuth

```rust
pub enum McpAuth {
    Bearer { token: String },
    OAuth { client_id: String, client_secret: String },
}
```

## Key Components

### local.rs - Local MCP Clients

Stdio-based communication with local MCP servers:

```rust
pub struct LocalMcpClient {
    command: String,
    args: Vec<String>,
    process: Child,
    writer: ChildStdin,
    reader: ChildStdout,
}
```

**Protocol**: JSON-RPC over stdio

### remote.rs - Remote MCP Clients

HTTP-based communication with remote MCP servers:

```rust
pub struct RemoteMcpClient {
    url: Url,
    auth: Option<McpAuth>,
    http_client: Client,
}
```

**Features**:
- Bearer token authentication
- OAuth flow support
- `reconnect()` method exists at line 470 but NOT wired to auto-retry

### auth.rs - OAuth Token Management

```rust
pub struct AuthManager {
    clients: HashMap<String, AuthState>,
}

pub enum AuthState {
    Pending { verifier: String },
    Authorized { access_token: String },
    Refreshed { access_token: String },
}
```

### McpService - Connection Manager

```rust
pub struct McpService {
    clients: HashMap<String, McpClient>,
    tools: HashMap<String, McpTool>,
}

impl McpService {
    pub async fn connect(&self, server_id: &str, config: &McpServerConfig) -> Result<()>;
    pub async fn disconnect(&self, server_id: &str) -> Result<()>;
    pub async fn reconnect(&self, server_id: &str) -> Result<()>;  // Not auto-wired
    pub fn list_tools(&self) -> Vec<McpTool>;
    pub async fn call_tool(&self, server: &str, tool: &str, params: Value) -> Result<Value>;
}
```

## McpTool

Tool definition from MCP server:

```rust
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub server_id: String,
}
```

**Naming**: Tools are exposed as `mcp__<server>__<tool>` to AgentLoop.

## McpServerStatus

```rust
pub enum McpServerStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}
```

## Protocol

### JSON-RPC Messages

**Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "tool_name",
    "arguments": {}
  }
}
```

**Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "content": [...]
  }
}
```

## Server Configuration

```toml
[mcp.servers.my-server]
type = "local"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]
cwd = "/workspace"

[mcp.servers.remote-server]
type = "remote"
url = "https://api.example.com/mcp"
auth = { type = "bearer", token = "..." }
```

## Events Published

- `McpServerStarted` - Server connected
- `McpServerStopped` - Server disconnected
- `McpServerError` - Server error
- `McpToolCall` - Tool called
- `McpToolResult` - Tool result

## Known Implementation Issues

1. **`reconnect()` not wired**: `remote.rs::reconnect()` exists at line 470 but needs to be wired up to auto-retry mechanism

## See Also

- [agent.md](agent.md) - Uses MCP tools via ToolRegistry
- [tool.md](tool.md) - Tool execution
- [provider.md](provider.md) - Provider that handles MCP tool calls
