# Server Module

The `server` module provides HTTP server functionality for remote TUI connections.

## Overview

**Location**: `src/server/`

**Key Responsibilities**:
- Axum-based HTTP server (feature-gated)
- WebSocket support for remote TUI
- RPC protocol handling
- Route definitions

**Requires**: `server` feature flag

## Components

### http.rs - Server Entry

```rust
pub async fn run_server(config: &ServerConfig) -> Result<()> {
    let app = Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_handler))
        .route("/rpc", post(rpc_handler));

    let listener = TcpListener::bind(&config.addr).await?;
    axum::serve(listener, app).await?;
}
```

### ws.rs - WebSocket Handler

```rust
pub async fn ws_handler(ws: WebSocket) -> Result<()> {
    // Upgrade to WebSocket
    // Handle TUI protocol messages
}
```

### rpc.rs - RPC Protocol

```rust
pub enum RpcRequest {
    Execute { command: String },
    Query { path: String },
    Subscribe { events: Vec<String> },
}

pub enum RpcResponse {
    Result(Value),
    Error(String),
    Event(AppEvent),
}
```

### routes.rs - HTTP Routes

```rust
pub async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

pub async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}
```

### state.rs - ServerState

```rust
pub struct ServerState {
    pub session_store: SessionStore,
    pub config: Config,
    pub bus: GlobalEventBus,
}
```

### middleware.rs - Request Middleware

```rust
pub async fn auth_middleware(request: Request, next: Next) -> Response {
    // Validate auth token
    // Add user context to request
}
```

## Protocol

### WebSocket Messages

```json
// Client → Server
{ "type": "execute", "command": "/help" }
{ "type": "input", "content": "fix the bug" }
{ "type": "subscribe", "events": ["MessageAdded"] }

// Server → Client
{ "type": "output", "content": "I'll help you..." }
{ "type": "event", "event": { "type": "MessageAdded", ... } }
{ "type": "error", "message": "..." }
```

## Configuration

```toml
[server]
enabled = true
host = "0.0.0.0"
port = 8080
auth_token = "..."

[server.tls]
enabled = false
cert = "/path/to/cert.pem"
key = "/path/to/key.pem"
```

## See Also

- [client.md](client.md) - Remote TUI client
