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
pub struct LocalClient {
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    timeout: u64,
    child: Option<Child>,
    stdin: Option<tokio::process::ChildStdin>,
    pending: PendingSenders,
    shutdown_notify: Arc<Notify>,
    request_id: AtomicU64,
}
```

**Protocol**: JSON-RPC over stdio

**Features:**
- Uses `std::env::var_os("PATH")` to preserve user's actual PATH
- Process spawn wrapped in 10s timeout to prevent hangs
- Graceful shutdown via `shutdown_notify` Notify mechanism

### remote.rs - Remote MCP Clients

HTTP-based communication with remote MCP servers:

```rust
pub struct RemoteClient {
    url: String,
    headers: HashMap<String, String>,
    client: reqwest::Client,
    session_id: Mutex<Option<String>>,
    sse_url: Mutex<Option<String>>,
    oauth_token: Mutex<Option<String>>,
    sse_events: Arc<Mutex<Vec<serde_json::Value>>>,
    request_id: AtomicU64,
    shutdown: Arc<Mutex<bool>>,
    sse_shutdown: Arc<Notify>,
    validated_ips: Arc<Mutex<Option<Vec<IpAddr>>>>,
}
```

**Features:**
- Bearer token authentication
- OAuth flow support
- DNS rebinding protection (IP re-validation on each request)
- SSE (Server-Sent Events) support for server-initiated messages

### McpConnectionManager - Auto-reconnect wrapper

```rust
pub struct McpConnectionManager {
    client: RemoteClient,
    state: Arc<Mutex<ConnectionState>>,
    retry_count: Arc<AtomicU64>,
    max_retries: u64,
    base_delay: Duration,
    max_delay: Duration,
    heartbeat_interval: Duration,
    heartbeat_task: Arc<AtomicBool>,
    shutdown: Arc<Notify>,
    reconnect_needed: Arc<Notify>,
}

pub enum ConnectionState {
    Connected,
    Disconnected,
    Reconnecting { attempt: u32 },
}
```

**Features:**
- Exponential backoff: 1s → 2s → 4s → ... → max 60s
- Max 5 retry attempts before giving up
- Heartbeat every 30s to keep connection alive
- `ensure_connected()` spawns reconnection in background task

### auth.rs - OAuth Token Management

```rust
pub struct OAuthManager {
    token_store: PathBuf,
    used_codes_store: PathBuf,
    servers: HashMap<String, ServerTokens>,
    used_codes: HashMap<String, UsedCode>,
}

pub struct ServerTokens {
    pub server_url: String,
    pub tokens: TokenSet,
}
```

**Token Encryption:**
- Tokens encrypted with AES-256-GCM using `CODEGG_TOKEN_KEY` env var
- Uses `CODEGG_ENC_v1` magic bytes prefix for version detection

### McpService - Connection Manager

```rust
pub struct McpService {
    servers: HashMap<String, McpServer>,
    oauth: OAuthManager,
}

pub struct McpServer {
    pub name: String,
    pub status: McpServerStatus,
    pub tools: Vec<McpTool>,
    pub client: McpClientType,
}

pub enum McpClientType {
    Local(Arc<RwLock<LocalClient>>),
    Remote(Arc<RwLock<McpConnectionManager>>),
}
```

**Connection methods:**
- `connect_stdio()` - Local servers via stdio
- `connect_http()` - Remote servers via HTTP
- `connect_from_config()` - Config-based connection (used by server startup)
- `disconnect()` - Gracefully disconnect a server
- `shutdown_all()` - Disconnect all servers

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

## Events Published

- `McpServerStarted` - Server connected
- `McpServerStopped` - Server disconnected
- `McpServerError` - Server error
- `McpToolCall` - Tool called
- `McpToolResult` - Tool result

## Known Implementation Issues

1. **Tool definition cache staleness**: Uses `mcp_tool_count` as proxy for MCP tool changes. If tool identities change without count changing, cache may be stale. MCP service would need to expose a version/hash for more precise invalidation.

2. **SSE support not fully integrated**: `connect_sse()` and `connect_sse_stream()` exist but are not automatically called during remote connection setup. SSE events are collected but not yet processed by the agent.

3. **Remote reconnection via spawn**: `ensure_connected()` spawns reconnection in a background task and waits for notification. If the spawned task fails silently, the wait may timeout.

## See Also

- [agent.md](agent.md) - Uses MCP tools via ToolRegistry
- [tool.md](tool.md) - Tool execution
- [provider.md](provider.md) - Provider that handles MCP tool calls
