---
name: lsp
description: LSP client-side integration for Language Server Protocol support
version: 1.1.0
tags:
  - lsp
  - language-server
  - diagnostics
  - code-intelligence
  - client-side
---

# LSP (Language Server Protocol) Guide

This skill covers the LSP module for language server integration in opencode-rs.

## Overview

The LSP module (`src/lsp/`) implements a **client-side LSP integration** - it spawns and manages external LSP server processes, communicating via JSON-RPC over stdin/stdout. It does NOT implement an LSP server itself.

## Directory Structure

```
src/lsp/
├── mod.rs           # Main Lsp struct
├── client.rs        # LspClient - JSON-RPC communication, process I/O
├── service.rs       # LspService - manages clients per project root
├── server.rs        # LspServerDef - 30+ server definitions
├── operations.rs    # LspOperations - goto definition, hover, completion, etc.
├── diagnostics.rs   # DiagnosticsCollector with debouncing
├── language.rs      # Language detection from file extensions
├── root.rs          # Project root detection
├── launch.rs        # Process spawning with Content-Length framing
└── download.rs      # Binary download and caching
```

## Key Types

### Lsp (`mod.rs`)

Main entry point combining service, operations, and diagnostics:

```rust
pub struct Lsp {
    pub service: Arc<LspService>,
    pub operations: Arc<LspOperations>,
    pub diagnostics: Arc<DiagnosticsCollector>,
}
```

### LspClient (`client.rs`)

JSON-RPC client managing LSP server process:

```rust
pub struct LspClient {
    pub server_id: String,
    pub root: PathBuf,
    pub process: tokio::sync::Mutex<LspProcess>,
    pub request_id: AtomicI64,
    pub capabilities: Mutex<Option<ServerCapabilities>>,
    pub opened_files: Mutex<HashMap<String, i32>>,
    pub diagnostics: Arc<Mutex<HashMap<String, Vec<lsp_types::Diagnostic>>>>,
    pub notif_tx: mpsc::UnboundedSender<String>,
    pub notif_rx: Mutex<Option<mpsc::UnboundedReceiver<String>>>,
}
```

### LspServerDef (`server.rs`)

Server definition with 42 server implementations:

```rust
pub struct LspServerDef {
    pub id: &'static str,           # e.g., "rust-analyzer"
    pub languages: &'static [&'static str],
    pub extensions: &'static [&'static str],
    pub repo: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub download: Option<DownloadSpec>,
}
```

## Supported LSP Servers

| Language | Server ID | Command |
|----------|-----------|---------|
| Rust | `rust-analyzer` | `rust-analyzer` |
| Python | `pyright` | `pyright-langserver --stdio` |
| JS/TS | `typescript-language-server` | `typescript-language-server --stdio` |
| Go | `gopls` | `gopls` |
| C/C++ | `clangd` | `clangd` |
| Java | `jdtls` | `jdtls` |
| C# | `omnisharp` | `OmniSharp` |
| Ruby | `ruby-lsp` | `ruby-lsp` |
| Kotlin | `kotlin-language-server` | `kotlin-language-server` |
| Scala | `metals` | `metals` |
| Dart | `dart-analysis-server` | `dart language-server --client-id codegg` |
| Swift | `swift-sourcekit` | `sourcekit-lsp` |
| Haskell | `haskell-language-server` | `haskell-language-server-wrapper --lsp` |
| Lua | `lua-language-server` | `lua-language-server` |
| PHP | `php-language-server` | `php-language-server` |
| Perl/Raku | `perl-language-server` | `perl-language-server` |
| Zig | `zls` | `zls` |
| ... and more | | |

## Key Operations

### File Lifecycle

```rust
// Open file
lsp.service.open_file(path, content).await

// Update file content
lsp.service.update_file(path, content).await

// Save file
lsp.service.save_file(path, None).await

// Close file
lsp.service.close_file(path).await
```

### Code Intelligence

```rust
// Goto definition
let locations = lsp.operations.go_to_definition(file_path, line, column).await

// Find references
let refs = lsp.operations.find_references(file_path, line, column).await

// Hover
let hover = lsp.operations.hover(file_path, line, column).await

// Document symbols
let symbols = lsp.operations.document_symbols(file_path).await

// Code actions
let actions = lsp.operations.code_actions(file_path, start_line, start_col, end_line, end_col, Vec::new(), None).await

// Completion
let completions = lsp.operations.completion(file_path, line, column, None, None).await

// Signature help
let sig = lsp.operations.signature_help(file_path, line, column).await
```

## Tool Integration

LSP is exposed via `LspTool` in `src/tool/lsp.rs`:

```rust
pub struct LspTool {
    service: Arc<crate::lsp::service::LspService>,
    allowed_root: PathBuf,
}
```

Operations available via tool:
- `goToDefinition`
- `findReferences`
- `hover`
- `documentSymbol`
- `workspaceSymbol`
- `goToImplementation`
- `prepareCallHierarchy`
- `incomingCalls`
- `outgoingCalls`
- `codeAction`
- `codeLens`

## Project Root Detection

The module detects project roots by looking for marker files:
- `.git`, `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`
- `build.gradle`, `CMakeLists.txt`, `Makefile`, `Gemfile`, `mix.exs`
- `tsconfig.json`, `vite.config.ts`, `next.config.js`, etc.

## Binary Download/Caching

1. Checks PATH first for server binaries
2. Falls back to cached download in `$HOME/.cache/codegg/lsp/`
3. Only rust-analyzer has download specification currently
4. Supports zip, tar.gz, tar.xz extraction
5. Sets executable permissions on Unix (0o755)

## Bug Fixes Applied (2026-05-22)

### PATH Parsing Fixed (`download.rs`)

```rust
// ❌ Before - broken on Unix (split by wrong separator)
for dir in paths.split(std::path::MAIN_SEPARATOR) { ... }

// ✅ After - uses std::env::split_paths correctly
let path_var = std::env::var("PATH").ok()?;
let paths = std::env::split_paths(&path_var);
for dir in paths { ... }
```

### PHP Server Mapping Fixed (`language.rs`)

```rust
// ❌ Before - intelephense doesn't exist in server definitions
"php" => Some("intelephense"),

// ✅ After - correct server ID
"php" => Some("php-language-server"),
```

### Request Timeout Added (`client.rs`)

```rust
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub async fn send_request(...) -> Result<...> {
    // ... request setup ...
    let result = tokio::time::timeout(Self::REQUEST_TIMEOUT, async {
        // ... read loop ...
    }).await;

    match result {
        Ok(inner) => inner,
        Err(_) => Err(LspError::RequestTimeout(...)),
    }
}
```

### Hardcoded PATH Fixed (`launch.rs`)

```rust
// ❌ Before - hardcoded PATH ignored user's environment
.env_clear()
.env("PATH", "/usr/local/bin:/usr/bin:/bin")

// ✅ After - preserves user's PATH if available
.env_clear()
if let Some(user_path) = std::env::var_os("PATH") {
    cmd.env("PATH", user_path);
} else {
    cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
}
```

### Stderr Logging (`client.rs`)

Server stderr is now logged during initialization:

```rust
let mut process = launch::spawn_server(...).await?;
let stderr_output = launch::drain_stderr(&mut process).await;
if !stderr_output.is_empty() {
    info!(server = server.id, stderr = %stderr_output, "LSP server stderr");
}
```

## Additional Bug Fixes (2026-05-22 - Session Review)

### Notification Loop Redundancy Fixed (`client.rs`)

The `send_request` method had redundant notification handling:

```rust
// ❌ Before - duplicate branches, silent ignore on send failure
if let Some(resp_id) = resp.get("id") {
    if resp_id.as_i64() == Some(id) { ... }
    let _ = self.notif_tx.send(resp_str);  // Always runs after match
} else {
    let _ = self.notif_tx.send(resp_str);  // Duplicate branch
}

// ✅ After - cleaner logic, logged send failures
if let Some(resp_id) = resp.get("id") {
    if resp_id.as_i64() == Some(id) { ... }
}
if let Err(e) = self.notif_tx.send(resp_str) {
    warn!(error = %e, "failed to send notification to channel");
}
```

### close_file Race Condition Fixed (`service.rs`)

The `close_file` method had lock handling issues that could cause race conditions:

```rust
// ❌ Before - dropped read lock before acquiring write lock (race!)
let clients = self.clients.read().await;
let key = { /* find key */ };
drop(clients);  // Lock dropped here
if let Some(key) = key {
    let mut clients = self.clients.write().await;  // Another task could modify between
    // ...
}

// ✅ After - uses single write lock, removes from opened_files
let client_idx = {
    let clients = self.clients.read().await;
    // find client index
};
// ...
let mut clients = self.clients.write().await;
if let Some(entry) = clients.values_mut().nth(client_idx) {
    let was_open = entry.client.opened_files.lock().await.contains_key(&uri_str);
    if was_open {
        let _ = entry.client.close_file(&uri).await;
        entry.client.opened_files.lock().await.remove(&uri_str);
    }
}
```

### save_file Race Condition Fixed (`service.rs`)

Similar fix for `save_file`:

```rust
// ❌ Before - dropped read lock before acquiring write lock
let clients = self.clients.read().await;
let key = { /* find key */ };
drop(clients);
if let Some(key) = key {
    let mut clients = self.clients.write().await;
    // ...
}

// ✅ After - uses single write lock
let client_idx = {
    let clients = self.clients.read().await;
    // find client index
};
// ...
let mut clients = self.clients.write().await;
if let Some(entry) = clients.values_mut().nth(client_idx) {
    return entry.client.save_file(&uri, text).await;
}
```

## Error Handling

```rust
pub enum LspError {
    ServerNotFound(String),
    DownloadFailed(String),
    LaunchFailed(String),
    NotInitialized(String),
    RequestFailed(String),
    RequestTimeout(String),  // Added 2026-05-22
    UnsupportedLanguage(String),
    Io(std::io::Error),
    Json(serde_json::Error),
}
```

## Architecture Notes

### Client-Per-Root Pattern

`LspService` maintains a `HashMap<String, ClientEntry>` where the key is `"{project_root}:{server_id}"`. This means one LSP client per project root per language.

### Content-Length Framing

LSP messages use Content-Length headers for framing:
```
Content-Length: <bytes>\r\n\r\n<json payload>
```

### Notification Handling

Server→client notifications (like `textDocument/publishDiagnostics`) are:
1. Parsed from stdout
2. Sent through `notif_tx` channel
3. Currently only logged in a spawned task (not processed)

## See Also

- [tool.md](tool.md) - LSP tool wrapper
- [architecture/lsp.md](../../architecture/lsp.md) - Architecture documentation