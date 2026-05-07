# LSP Module

The `lsp` module provides Language Server Protocol support for IDE-like features.

## Overview

**Location**: `src/lsp/`

**Key Responsibilities**:
- LSP server management
- Diagnostics collection
- Code operations (rename, goto, etc.)
- Language detection
- Server binary download

## Key Types

### Lsp

```rust
pub struct Lsp {
    servers: HashMap<Language, LspServer>,
}
```

### LspOperations

```rust
impl Lsp {
    pub async fn goto_definition(&self, uri: &Uri, position: Position) -> Result<Location>;
    pub async fn rename(&self, uri: &Uri, position: Position, new_name: &str) -> Result<Vec<TextEdit>>;
    pub async fn find_references(&self, uri: &Uri, position: Position) -> Result<Vec<Location>>;
    pub async fn hover(&self, uri: &Uri, position: Position) -> Result<Hover>;
    pub async fn completion(&self, uri: &Uri, position: Position) -> Result<Vec<CompletionItem>>;
}
```

## Components

### service.rs - LspService

```rust
pub struct LspService {
    servers: RwLock<HashMap<Language, Arc<LspServer>>>,
    operations: LspOperations,
}
```

Manages LSP server instances per language.

### client.rs - LSP Client

```rust
pub struct LspClient {
    transport: Box<dyn Transport>,
    request_id: AtomicU64,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<LspResponse>>>>,
}
```

JSON-RPC client for LSP communication.

### diagnostics.rs - DiagnosticsCollector

```rust
pub struct DiagnosticsCollector {
    diagnostics: RwLock<HashMap<Uri, Vec<Diagnostic>>>,
}
```

Collects and stores diagnostics from LSP servers.

### download.rs - Server Download

```rust
pub fn download_server(language: Language, version: &str) -> Result<PathBuf>;
```

Downloads LSP server binaries.

### language.rs - Language Detection

```rust
pub fn detect_language(path: &Path) -> Option<Language>;

pub enum Language {
    Rust,
    TypeScript,
    Python,
    Go,
    Java,
    // ...
}
```

### launch.rs - Server Launching

```rust
pub fn launch_server(language: Language, cwd: &Path) -> Result<Child>;
```

Starts LSP server process.

## Supported Languages

| Language | Server |
|----------|--------|
| Rust | rust-analyzer |
| TypeScript | typescript-language-server |
| Python | pyright |
| Go | gopls |

## Tool Integration

LSP functionality is exposed via `tool::lsp` tool:

```rust
pub struct LspTool {
    service: Arc<LspService>,
}
```

## Events

- Publishes diagnostics to TUI via GlobalEventBus
- `GlobalEventBus::publish(Notification::PublishDiagnostics(...))`

## See Also

- [tool.md](tool.md) - LSP tool wrapper
