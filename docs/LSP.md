# LSP (Language Server Protocol)

codegg integrates with Language Server Protocol (LSP) to provide IDE-like features including diagnostics, code navigation, and intelligent completions.

## Architecture

The LSP module (`src/lsp/`) consists of:

- **`mod.rs`** - Main `Lsp` struct exposing service, operations, and diagnostics
- **`service.rs`** - `LspService` managing LSP client lifecycle
- **`client.rs`** - Low-level LSP client implementation
- **`server.rs`** - Server management (download, launch, process handling)
- **`launch.rs`** - Language server launch configuration
- **`download.rs`** - Automatic server binary downloads
- **`language.rs`** - Language detection and server mapping
- **`root.rs`** - Project root detection
- **`operations.rs`** - `LspOperations` for code actions (goto definition, find references, etc.)
- **`diagnostics.rs`** - `DiagnosticsCollector` for collecting and debouncing diagnostics

## Key Components

### LspService

Manages the lifecycle of LSP clients per project/language:

```rust
pub struct LspService {
    // Manages multiple language server clients
    clients: HashMap<String, LspClient>,
    config: LspConfig,
}
```

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
- Communication errors return `LspError::Connection`
