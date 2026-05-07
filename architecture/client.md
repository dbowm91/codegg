# Client Module

The `client` module provides the WebSocket client for remote TUI connections.

## Overview

**Location**: `src/client/`

**Key Responsibilities**:
- WebSocket connection to server
- Remote TUI protocol
- Session attach/detach

## Components

### attach.rs

```rust
pub async fn run_attach(server_url: &str, session_id: &str) -> Result<()>;
```

Connects to server and attaches to existing session.

### sdk.rs

```rust
pub struct Client {
    ws: WebSocket,
    session_id: String,
}

impl Client {
    pub async fn connect(url: &str) -> Result<Self>;
    pub async fn send(&self, msg: ClientMessage) -> Result<()>;
    pub async fn recv(&self) -> Result<ServerMessage>;
}
```

## Protocol

### ClientMessage

```rust
pub enum ClientMessage {
    Input { content: String },
    Execute { command: String },
    Subscribe { events: Vec<String> },
    Detach,
}
```

### ServerMessage

```rust
pub enum ServerMessage {
    Output { content: String },
    Event { event: AppEvent },
    Error { message: String },
    Ack { id: u64 },
}
```

## See Also

- [server.md](server.md) - Server that accepts connections
