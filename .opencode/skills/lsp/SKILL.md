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

The LSP implementation lives in the `egglsp` workspace crate (`crates/egglsp/`). `src/lsp/mod.rs` is a thin compatibility shim that re-exports `egglsp::*` and bridges config/error types. The model-facing tool is at `src/tool/lsp.rs`.

LSP is exposed as a native tool via `LspTool`, returning compact agent-facing summaries (not raw LSP JSON). Model-facing line and column are 1-indexed; the wrapper converts to LSP 0-indexed.

## Directory Structure

```
crates/egglsp/src/          # Authoritative LSP implementation
├── client.rs               # LspClient - JSON-RPC, diagnostics cache, notification parser
├── config.rs               # LspConfig, LspRule types
├── diagnostics.rs          # DiagnosticsCollector
├── edit.rs               # Workspace edit preview, text edit application, unified diff generation
├── download.rs             # Binary download/cache
├── error.rs                # LspError
├── language.rs             # Language detection from file extensions
├── launch.rs               # Process spawning, Content-Length framing, background stderr drain
├── operations.rs           # LspOperations - goto definition, hover, etc.
├── root.rs                 # Project root detection
├── server.rs               # 39 server definitions
├── service.rs              # LspService - client management, file-based routing

src/lsp/mod.rs              # Thin re-export shim (compatibility only)
src/tool/lsp.rs             # Model-facing LSP tool with compact DTOs
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

JSON-RPC client managing LSP server process. Uses a background reader
task for message dispatch (no more request-owned reads):

```rust
pub struct LspClient {
    pub server_id: String,
    pub root: PathBuf,
    pub process: tokio::sync::Mutex<LspProcess>,
    pub request_id: AtomicU64,
    pub capabilities: Mutex<Option<ServerCapabilities>>,
    pub opened_files: Mutex<HashMap<String, i32>>,
    pub last_opened_at: Mutex<HashMap<String, Instant>>,
    pub diagnostics: Arc<Mutex<HashMap<String, Vec<lsp_types::Diagnostic>>>>,
    pub pending: PendingMap,
    _reader_task: tokio::task::JoinHandle<()>,
}
```

The `pending` map routes response IDs to oneshot senders. The
`_reader_task` continuously reads framed JSON-RPC messages from stdout
and classifies them via `classify_json_rpc_message`. Responses are
routed to pending senders; notifications are dispatched via
`dispatch_notification`.

**Request ID Generation:**
- Uses `AtomicU64` for wrap-around safety (was `AtomicI64`)
- `fetch_add(1, Ordering::SeqCst)` for sequential IDs
- No special wrap-around check needed with unsigned integer

### Edit Preview Types (`edit.rs`)

```rust
pub struct WorkspaceEditPreview {
    pub title: String,
    pub files: Vec<FileEditPreview>,
    pub total_files: usize,
    pub total_edits: usize,
    pub truncated: bool,
}

pub struct FileEditPreview {
    pub file: PathBuf,
    pub original_hash: String,
    pub edits: Vec<TextEditPreview>,
    pub patch: String,
    pub patch_omitted: bool,
}

pub struct TextEditPreview {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub replacement_preview: String,
}
```

These types are re-exported from `egglsp` at the crate root (e.g. `egglsp::WorkspaceEditPreview`).

### LspServerDef (`server.rs`)

Server definition with 39 server implementations:

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

// Preview-only rename (returns WorkspaceEditPreview with unified diff patches; does not write)
let preview = lsp.operations.rename_preview(file_path, line, column, "new_name", Some(allowed_root)).await

// Preview-only format
let preview = lsp.operations.format_preview(file_path, Some(allowed_root)).await

// Preview-only source action (organize imports)
use egglsp::operations::SourceActionPreviewKind;
let kind = SourceActionPreviewKind::parse("source.organizeImports")?;
let preview = lsp.operations.source_action_preview(file_path, kind, Some(allowed_root)).await
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
- `workspaceSymbol` (returns `WorkspaceSymbolSummary` with name, kind, file, start_line, start_column, container_name)
- `diagnostics` (returns `diagnostics_may_still_be_warming: bool` to indicate if the server may not have responded yet after a recent `didOpen`/`didChange`)
- `renamePreview` (preview-only; returns `WorkspaceEditPreview` {title, files:[{file, original_hash, edits, patch}], total_*, truncated}; never mutates)
- `formatPreview` (preview-only; same `WorkspaceEditPreview` shape)
- `sourceActionPreview` (preview-only; same `WorkspaceEditPreview` shape; accepts `action` parameter — initially only `source.organizeImports` with aliases `organizeImports`/`organize_imports`)

**Preview-only contract**: `renamePreview` / `formatPreview` / `sourceActionPreview` (and future edit previews) produce bounded unified-diff patches for review via `WorkspaceEditPreview`. `sourceActionPreview` only accepts `source.organizeImports`; arbitrary code actions, command-only actions, and command execution are intentionally rejected. `format_preview` enforces `allowed_root` at the crate layer. Large patches are structurally flagged via `FileEditPreview.patch_omitted` (not string matching). They are `ToolCategory::ReadOnly`. Actual file changes require the separate mutating `apply_patch` tool (or equivalent). `codeLens` is not exposed in the model-facing schema.

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
    RequestTimeout(String),
    UnsupportedLanguage(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    UnsupportedEdit(String),
    PathOutsideRoot(String),
    Utf16Position(String),
    OverlappingEdits,
    UnsupportedSourceAction(String),
    CommandOnlySourceAction(String),
    NoEditForSourceAction(String),
    AmbiguousSourceAction(String, String),
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

1. Read by the background `_reader_task` from stdout
2. Classified via `classify_json_rpc_message` into `JsonRpcMessage::Notification`
3. Dispatched via `dispatch_notification` which updates the shared `diagnostics` map
4. Diagnostics are now updated independently of pending requests (no more "diagnostics only consumed while request is pending")

### Background Dispatcher Architecture

The background reader task is spawned during `LspClient::new()`. It:

- Continuously reads Content-Length framed JSON-RPC messages from stdout
- Classifies each message via `classify_json_rpc_message` (Response, ErrorResponse, Notification, Unknown)
- Routes responses to pending oneshot senders via the `pending` map
- Dispatches notifications via `dispatch_notification` (currently handles `textDocument/publishDiagnostics`)
- Diagnostics freshness is tracked via `last_opened_at` timestamps; the `diagnostics` operation reports `diagnostics_may_still_be_warming` when a file was recently opened or changed

Key helper functions (exported from `client.rs`):
- `classify_json_rpc_message(value) -> JsonRpcMessage`
- `dispatch_notification(diagnostics, method, params)`
- `url_to_uri(url) -> Uri`

## See Also

- [tool.md](tool.md) - LSP tool wrapper
- [architecture/lsp.md](../../architecture/lsp.md) - Architecture documentation