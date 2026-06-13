# LSP (Language Server Protocol)

> **Note:** This document is partially outdated. For comprehensive LSP documentation, see `architecture/lsp.md` and `.opencode/skills/lsp/SKILL.md`.

codegg integrates with Language Server Protocol (LSP) to provide IDE-like features including diagnostics, code navigation, and intelligent completions.

## Architecture

The authoritative LSP implementation is in **`crates/egglsp/`**. The `src/lsp/` directory is a thin shim that re-exports from egglsp.

The egglsp crate consists of:

- **`src/server.rs`** - Server definitions (39 servers: clangd, rust-analyzer, gopls, pyright, typescript-language-server, etc.)
- **`src/service.rs`** - `LspService` managing LSP client lifecycle, explicit leader/waiter init election, lifecycle-validated publication, and unpublished-client disposal
- **`src/client.rs`** - Low-level LSP client implementation
- **`src/operations.rs`** - `LspOperations` for code actions (goto definition, find references, etc.), `WorkspaceEditPreview`/`FileEditPreview`/`TextEditPreview`
- **`src/diagnostics.rs`** - `DiagnosticsCollector` for collecting and debouncing diagnostics

## Key Components

### LspService

Manages the lifecycle of LSP clients per project/language. Uses explicit leader/waiter init election for single-flight initialization (the first caller becomes leader, concurrent callers wait on the same completion fan-out), validates lifecycle generation before publication, and uses `Arc`-based handles to avoid serialization of unrelated clients behind process I/O:

```rust
pub struct LspService {
    // Manages multiple language server clients
    clients: Arc<RwLock<HashMap<String, Arc<LspClient>>>>,
    config: LspConfig,
}
```

If publication loses to an existing client or is invalidated by shutdown, the unpublished client is shut down with a bounded timeout before waiters are notified.

### LspOperations

Provides code navigation and analysis:

- `go_to_definition()` - Jump to symbol definitions
- `find_references()` - Find all references to a symbol
- `hover()` - Get type/info hover for cursor position
- `document_symbols()` - List all symbols in a document
- `code_actions()` - Get available code actions/quick fixes
- `completion()` - Trigger completion at cursor
- `signature_help()` - Show function signature hints
- `code_lens()` - Get CodeLens data

### DiagnosticsCollector

Collects and manages diagnostics with 150ms debouncing:

```rust
pub struct DiagnosticsCollector {
    service: Arc<LspService>,
    last_update: Arc<Mutex<HashMap<String, Instant>>>,
}
```

## Supported Languages

Servers are automatically downloaded for:

| Language | Server |
|----------|--------|
| Rust | rust-analyzer |
| Python | pyright |
| TypeScript/JavaScript | typescript-language-server |
| Go | gopls |
| C/C++ | clangd |

## Configuration

LSP is configured via `config.json`:

```json
{
  "experimental": {
    "lsp_tool": true
  },
  "lsp": {
    "servers": {
      "rust": {
        "command": "rust-analyzer",
        "args": []
      }
    }
  }
}
```

## Integration with Tools

The `lsp` tool in the tool registry allows the agent to:

1. **Goto definition** - Jump to symbol definitions
2. **Find references** - Find all symbol references
3. **Hover** - Get type information
4. **Document symbols** - List file symbols
5. **Code actions** - Get quick fixes
6. **Semantic checks** - Run `semanticCheckPreview` with either full proposed content or a single-file unified diff patch; the patch is applied in memory only, the overlay is restored after the check, and diagnostics/restore errors stay surfaced in the result

Example usage in agent prompts:
```
Use the lsp tool to find the definition of the `processRequest` function
```

## Diagnostics Flow

1. File changes are sent to LSP server via `textDocument/didChange`
2. Server publishes diagnostics via `textDocument/publishDiagnostics`
3. `DiagnosticsCollector` receives and debounces updates (150ms)
4. TUI displays diagnostics with severity indicators

## Error Handling

- Server launch failures return `LspError::LaunchFailed`
- Invalid file paths return `LspError::LaunchFailed`
- Request timeouts send a best-effort `$/cancelRequest`; if that cancel write fails, the transport is marked failed and later calls fail fast with `LspError::WriterClosed`
- Immediate request/notification I/O failures surface as `LspError::RequestFailed`; once the transport is failed, later calls fail fast with `LspError::WriterClosed`
