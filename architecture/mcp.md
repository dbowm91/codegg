# MCP Module

The `mcp` module implements the Model Context Protocol client for connecting to external MCP servers.

## Overview

**Location**: `src/mcp/`

**Key Responsibilities**:
- MCP client implementation (local and remote)
- Server connection management
- OAuth authentication support
- Tool exposure to AgentLoop
- Auto-reconnection with exponential backoff

## MCP Overview

MCP (Model Context Protocol) allows external tools and resources to be exposed to the agent as if they were native tools.

## Client Types

### McpClientType (in `mod.rs`)

```rust
pub enum McpClientType {
    Local(Arc<RwLock<LocalClient>>),
    Remote(Arc<RwLock<McpConnectionManager>>),
}
```

Note: `Local` and `Remote` variants wrap Arc<RwLock> to allow shared access across the application.

## Key Components

### mod.rs - McpService

Main entry point for managing MCP servers:

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
```

**Connection methods:**
- `connect_stdio()` - Local servers via stdio
- `connect_http()` - Remote servers via HTTP
- `connect_from_config()` - Config-based connection (used by server startup)
- `disconnect()` - Gracefully disconnect a server
- `shutdown_all()` - Disconnect all servers

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
- Pending request map for correlating JSON-RPC responses

### remote.rs - Remote MCP Clients + McpConnectionManager

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
    validated_ips: Arc<Mutex<Option<Vec<IpAddr>>>>,  // Arc<Mutex<...>> for Clone semantics
}
```

**Auto-reconnect wrapper:**

```rust
pub struct McpConnectionManager {
    client: RemoteClient,
    state: Arc<Mutex<ConnectionState>>,
    retry_count: Arc<AtomicU64>,
    max_retries: u64,
    base_delay: Duration,
    max_delay: Duration,
    heartbeat_interval: Duration,
    heartbeat_token: CancellationToken,
    heartbeat_cancellation: Arc<Mutex<Option<CancellationToken>>>,
    shutdown: Arc<Notify>,
    reconnect_needed: Arc<Notify>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum ConnectionState {
    Connected,
    #[default]
    Disconnected,
    Reconnecting { attempt: u32 },
}
```

**Features:**
- Bearer token authentication
- OAuth flow support
- DNS rebinding protection (IP re-validation on each request)
- SSE (Server-Sent Events) support for server-initiated messages
- Exponential backoff: 1s → 2s → 4s → ... → max 60s
- Max 5 retry attempts before giving up
- Heartbeat every 30s to keep connection alive
- `ensure_connected()` spawns reconnection in background task when disconnected

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_at: Option<u64>,
    pub scope: Option<String>,
}
```

**Token Encryption:**
- Tokens encrypted with AES-256-GCM using `CODEGG_TOKEN_KEY` env var
- Uses `CODEGG_ENC_v1` magic bytes prefix for version detection
- PKCE support for OAuth authorization code flow
- Replay protection via used codes store

### cli.rs - MCP CLI Commands

CLI commands for managing MCP servers:

```rust
pub struct McpCli {
    config_path: PathBuf,
}
```

**Commands:**
- `add` - Add a new MCP server to configuration
- `list` - List configured MCP servers
- `remove` - Remove an MCP server from configuration
- `enable` - Enable or disable an MCP server
- `debug` - Test connection to an MCP server

### ide_server.rs - IDE MCP Server

IDE integration server providing diff viewing via MCP protocol:

```rust
pub struct IdeServer {
    tools: HashMap<String, ToolHandler>,
    pending: PendingRequests,
    shutdown: Arc<Mutex<bool>>,
    shutdown_notify: Arc<Notify>,
}
```

**Supported Tools:**
- `openDiff` - Opens file diff in IDE (VS Code extension, JetBrains plugin)

**Transport Modes:**
- stdio mode (for IDE extensions)
- Unix socket mode

## McpTool

Tool definition from MCP server:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub server: String,  // Note: not server_id as shown in old doc
}
```

**Naming**: Tools are exposed as `mcp__<server>__<tool>` to AgentLoop.

## McpServerStatus

```rust
#[derive(Debug, Clone, Default)]
pub enum McpServerStatus {
    #[default]
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

MCP servers configured in `config.json`:

```json
{
  "mcp": {
    "servers": {
      "filesystem": {
        "type": "local",
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/projects"]
      },
      "github": {
        "type": "remote",
        "url": "https://api.github.com/mcp",
        "headers": {
          "Authorization": "Bearer ${GITHUB_TOKEN}"
        }
      }
    }
  }
}
```

## DNS Rebinding Protection

RemoteClient validates DNS at connection time AND before each request:

```rust
fn validate_url_host(url: &str) -> Result<String, String>  // Called on RemoteClient::new()
fn validate_host_ip(host: &str, port: u16) -> Result<Vec<IpAddr>, String>  // Validates IPs
fn revalidate_dns(host: &str, port: u16, validated_ips: &[IpAddr]) -> Result<(), String>  // Called before each request
```

- `initialize()` re-validates DNS on each call (prevents bypass via DNS changes between connections)
- IPv4-mapped IPv6 addresses are handled correctly
- Internal IPs (loopback, private, link-local) are blocked

## Known Implementation Issues

1. **Tool definition cache staleness**: Uses `mcp_tool_count` as proxy for MCP tool changes. If tool identities change without count changing, cache may be stale. MCP service would need to expose a version/hash for more precise invalidation.

2. **SSE support not fully integrated**: `connect_sse()` and `connect_sse_stream()` exist but are not automatically called during remote connection setup. SSE events are collected but not yet processed by the agent.

## See Also

- [agent.md](agent.md) - Uses MCP tools via ToolRegistry
- [tool.md](tool.md) - Tool execution
- [provider.md](provider.md) - Provider that handles MCP tool calls
