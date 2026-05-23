---
name: mcp
description: MCP client/server system, local vs remote, OAuth flow
version: 1.2.0
tags:
  - mcp
  - model context protocol
  - local
  - remote
  - oauth
---

# MCP System Guide

This skill covers the Model Context Protocol (MCP) implementation in opencode-rs, which enables connecting to external MCP servers for extended tool capabilities.

## Architecture Overview

```
McpService
├── LocalClient  (stdio-based)
├── RemoteClient (HTTP-based)
└── OAuthManager (OAuth 2.0 flow)
```

## Key Components

### McpService (`src/mcp/mod.rs`)

Main entry point for managing MCP servers:

```rust
pub struct McpService {
    servers: HashMap<String, McpServer>,
    oauth: OAuthManager,
}
```

**Connection methods:**
- `connect_stdio()` - Local servers via stdio
- `connect_http()` - Remote servers via HTTP
- `connect_from_config()` - Config-based connection

### LocalClient (`src/mcp/local.rs`)

For local MCP servers that communicate via stdio:

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

**Usage:**
```rust
let mut client = LocalClient::new(
    "npx".to_string(),
    vec!["-y".to_string(), "@modelcontextprotocol/server-filesystem".to_string(), "/path".to_string()],
    HashMap::new(),
    30,
);
client.initialize().await?;
let tools = client.discover_tools().await?;
```

**Implementation Notes:**
- Uses `std::env::var_os("PATH")` to preserve user's actual PATH (not hardcoded)
- Spawn timeout: process spawn is wrapped in `tokio::time::timeout` (capped at 10s) to prevent hangs
- Read loop runs in spawned task, handles JSON-RPC responses via pending request map
- Graceful shutdown via `shutdown_notify` Notify mechanism

### RemoteClient (`src/mcp/remote.rs`)

For remote MCP servers via HTTP with DNS rebinding protection:

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

**DNS Rebinding Protection:**
- `validate_url_host()` validates DNS at connection time, blocks internal IPs
- `revalidate_dns()` revalidates DNS before each HTTP request
- **IP re-validation on reconnect**: `initialize()` re-validates DNS on each call, preventing bypass via DNS changes between connections
- Detects IPv4-mapped IPv6 addresses
- Blocks loopback, private, link-local, and reserved IP ranges

### McpConnectionManager

Automatic reconnection with exponential backoff and heartbeat:

**Clone Implementation:**
- `McpConnectionManager` has a manual `Clone` implementation (not derived)
- `CancellationToken` from `tokio_util::sync` is `!Clone`, so it requires special handling
- `Clone` uses `Arc::clone()` for Arc fields and copies primitive types directly

```rust
// CancellationToken wrapped in Arc<Mutex<Option<CancellationToken>>> is cloned via:
Arc::clone(&self.heartbeat_cancellation)
```

### McpConnectionManager

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
    heartbeat_token: CancellationToken,                           // Cloneable
    heartbeat_cancellation: Arc<Mutex<Option<CancellationToken>>>, // Cloneable via Arc::clone
}

impl Clone for McpConnectionManager {
    fn clone(&self) -> Self {
        McpConnectionManager {
            client: self.client.clone(),
            state: Arc::clone(&self.state),
            // ... all fields cloned via Arc::clone or copy
            heartbeat_token: self.heartbeat_token.clone(),
            heartbeat_cancellation: Arc::clone(&self.heartbeat_cancellation),
        }
    }
}
```

**Features:**
- Exponential backoff: 1s → 2s → 4s → ... → max 60s
- Max 5 retry attempts before giving up
- Heartbeat every 30s to keep connection alive
- State notification via watch channel
- **ensure_connected()**: Spawns reconnection in background task, waits for notification, falls back to direct connect if needed

**Usage:**
```rust
let mut manager = McpConnectionManager::new(url, headers, timeout)?;
manager.connect().await?;

// Ensure connected before making requests
manager.ensure_connected().await?;
```

**DNS Rebinding Protection Details:**

```rust
fn validate_url_host(url: &str) -> Result<(String, Vec<IpAddr>), McpError> {
    let parsed = reqwest::Url::parse(url)?;
    let host = parsed.host_str().ok_or_else(|| ...)?;
    let socket_addrs: Vec<SocketAddr> = (host, port).to_socket_addrs()?.collect();
    let validated_ips: Vec<IpAddr> = socket_addrs.iter().map(|a| a.ip()).collect();
    for addr in &validated_ips {
        if is_internal_ip(addr) {
            return Err(McpError::Connection("access to internal addresses not allowed".into()));
        }
    }
    Ok((host.to_string(), validated_ips))
}

// Revalidation before each request
fn revalidate_dns(host: &str, port: u16, original_ips: &[IpAddr]) -> Result<(), McpError> {
    let current_addrs: Vec<SocketAddr> = (host, port).to_socket_addrs()?.collect();
    let current_ips: Vec<IpAddr> = current_addrs.iter().map(|a| a.ip()).collect();
    for ip in current_ips {
        if !original_ips.contains(&ip) {
            return Err(McpError::Connection("DNS rebinding attack detected".into()));
        }
    }
    Ok(())
}
```

### IdeServer (`src/mcp/ide_server.rs`)

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
- stdio mode (for IDE extensions) - uses async I/O via `tokio::io::stdin()/stdout()`
- Unix socket mode

```rust
impl IdeServer {
    pub fn new() -> Self;
    pub async fn run_stdio(&self) -> Result<(), McpError>;  // Uses tokio async I/O
    pub async fn run_socket(&self, socket_path: &str) -> Result<(), McpError>;
}
```

**Async I/O Implementation:**
- `run_stdio()` uses `tokio::io::stdin()` and `tokio::io::stdout()` (not blocking `std::io`)
- Uses `BufReader` with `AsyncBufReadExt` for async line reading
- Uses `AsyncWriteExt` for async write and flush operations

### OAuthManager (`src/mcp/auth.rs`)

Handles OAuth 2.0 authorization code flow with token encryption:

```rust
pub struct OAuthManager {
    token_store: PathBuf,
    used_codes_store: PathBuf,
    servers: std::collections::HashMap<String, ServerTokens>,
    used_codes: std::collections::HashMap<String, UsedCode>,
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
- Legacy unencrypted format supported for migration

**PKCE Support:**
- `generate_pkce_pair()` creates code_verifier and code_challenge
- S256 code challenge method used

**Replay Protection:**
- Used authorization codes tracked to prevent replay attacks
- Codes expire after 600 seconds
- **Important**: Code is marked as used BEFORE calling `exchange_code_for_tokens()` to eliminate race window
- If exchange fails after marking, code remains marked (acceptable - prevents replay of failed codes)

**OAuth Flow:**
1. `build_authorization_url()` creates OAuth URL with PKCE
2. `start_callback_server()` starts local server to receive callback
3. `exchange_code_for_tokens_with_replay_protection()` exchanges code for tokens
4. `refresh_tokens()` refreshes expired tokens
5. `revoke_token()` revokes tokens when logging out

```rust
pub fn generate_pkce_pair() -> (String, String);
pub fn build_authorization_url(&self, auth_url: &str, client_id: &str, code_challenge: &str, redirect_uri: &str, scope: &str) -> Result<String, McpError>;
pub async fn exchange_code_for_tokens_with_replay_protection(&mut self, token_url: &str, client_id: &str, client_secret: Option<&str>, code: &str, code_verifier: &str, redirect_uri: &str) -> Result<TokenSet, McpError>;
pub async fn refresh_tokens(&self, token_url: &str, client_id: &str, client_secret: Option<&str>, refresh_token: &str) -> Result<TokenSet, McpError>;
pub fn get_token_for_server(&self, server_url: &str) -> Option<String>;
```

## MCP Data Types

### McpTool

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub server: String,
}
```

### McpPrompt

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPrompt {
    pub name: String,
    pub description: Option<String>,
    pub arguments: Option<Vec<PromptArgument>>,
}
```

### McpResource

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}
```

## Tool Conversion

MCP tools are converted to AgentLoop ToolDefinitions:

```rust
pub fn list_tools(&self) -> Vec<ToolDefinition> {
    self.servers
        .values()
        .flat_map(|s| {
            s.tools.iter().map(|t| ToolDefinition {
                name: format!("mcp__{}__{}", s.name, t.name),  // Prefix with mcp__server__
                description: t.description.clone(),
                parameters: t.input_schema.clone(),
            })
        })
        .collect()
}
```

## Configuration

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

## Tool Execution

When an MCP tool is called (`mcp__server__tool_name`):

1. Parse tool name to extract server and tool
2. Look up server in `McpService.servers`
3. Call `server.client.call_tool(tool, arguments)`
4. Return result to agent

```rust
pub async fn call_tool(
    &self,
    server: &str,
    tool: &str,
    arguments: serde_json::Value,
) -> Result<String, McpError> {
    let srv = self.servers.get(server).ok_or_else(|| ...)?;
    match &srv.client {
        McpClientType::Local(c) => c.write().await.call_tool(tool, arguments).await,
        McpClientType::Remote(c) => c.write().await.call_tool(tool, arguments).await,
    }
}
```

## Server Status Tracking

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

## TUI Integration

MCP servers can be managed via TUI dialogs (`src/tui/components/dialogs/mcp.rs`):
- Add/remove servers
- View server status
- Refresh tool lists

## Error Handling

```rust
#[derive(Error, Debug)]
pub enum McpError {
    #[error("connection error: {0}")]
    Connection(String),
    #[error("server error: {0}")]
    Server(String),
    #[error("tool call failed: {0}")]
    ToolCall(String),
    #[error("oauth error: {0}")]
    OAuth(String),
    #[error("encryption error: {0}")]
    Encryption(String),
    #[error("timeout: {0}")]
    Timeout(String),
}
```

Note: `McpError` is defined in `src/error.rs` and re-exported in the `mcp` module.

## Security Considerations

1. **DNS Rebinding Protection**: RemoteClient validates DNS at connection time AND before each request
2. **Internal IP Blocking**: Only HTTP/HTTPS schemes allowed; internal IPs blocked
3. **OAuth Token Storage**: Tokens stored in memory, refreshed automatically
4. **Header Validation**: Custom headers validated at connection time

## Known Limitations

1. **Tool definition cache staleness**: Uses `mcp_tool_count` as proxy for MCP tool changes. If tool identities change without count changing, cache may be stale. MCP service would need to expose a version/hash for more precise invalidation.

2. **SSE support not fully integrated**: `connect_sse()` and `connect_sse_stream()` exist but are not automatically called during remote connection setup. SSE events are collected but not yet processed by the agent.

3. **OAuthManager structure**: The skill documentation showed an outdated `pending_auths`/`completed_flows` structure. Actual implementation uses `token_store` and `servers` HashMap for token persistence.