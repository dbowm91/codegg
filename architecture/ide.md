# IDE Module

The IDE module provides VS Code and JetBrains detection, diff viewing, and
an MCP server that exposes diff capabilities to IDE extensions.

## Purpose

Detect the host IDE, open native diff viewers with file content (or line
ranges), and serve IDE integration tools over MCP stdio or Unix socket
transport.

## Where It Lives

| Artifact | Path |
|----------|------|
| Detection + diff | `src/ide/mod.rs` (473 lines) |
| MCP server | `src/mcp/ide_server.rs` (424 lines) |

## How It Works

### IDE Detection

Three public functions check environment variables:

**`is_vscode()`** (`src/ide/mod.rs:83`):
- `VSCODE_IPC_HOOK` set
- `VSCODE_INJECTED_ENVIRONMENT` set
- `TERM_PROGRAM` equals `"vscode"`

**`is_jetbrains()`** (`src/ide/mod.rs:89`):
- `JETBRAINS_REMOTE` set
- `JB_PRODUCT_READINESS` set
- `IDEA_INITIAL_DIRECTORY` set
- `WEBCLBROWSER_HOST` set

**`is_ide()`** (`src/ide/mod.rs:96`): Returns `is_vscode() || is_jetbrains()`.

### Diff Viewing

**`open_diff()`** (`src/ide/mod.rs:100`):
1. Reads both files from disk.
2. Applies line-range slicing if `original_lines` or `modified_lines` are
   provided (1-indexed, inclusive end).
3. Dispatches to `open_diff_vscode()`, `open_diff_jetbrains()`, or
   `open_diff_generic()` based on detection.

**VS Code** (`open_diff_vscode`, line 134):
- Writes content to temp files with `codegg_original_`/`codegg_modified_`
  prefixes.
- Flushes and drops file handles before invoking `code --diff`.
- Uses `run_command_with_timeout()` with a 30-second deadline.

**JetBrains** (`open_diff_jetbrains`, line 188):
- Same temp file pattern.
- Tool resolution: `$JETBRAINS_TOOL` env var → `/opt/intellij/bin/idea.sh`
  → `/usr/local/bin/idea` → Windows `%PROGRAMFILES%\JetBrains\<product>\bin\idea.bat`
  → `idea` in PATH.
- Invokes `<tool> diff <original> <modified>`.

**Generic fallback** (`open_diff_generic`, line 270):
- Searches PATH for `code`, `code.exe`, `code.cmd`, `idea`, `idea.bat`,
  `idea.cmd`.
- Tries VS Code first; if that fails, tries IntelliJ.
- Returns `"no IDE diff tool found"` if neither is available.

### Temp File Safety

**`TempFilesGuard`** (`src/ide/mod.rs:46`): Implements `Drop` to remove
temp files on scope exit, including panics.

**`register_panic_cleanup()`** (`src/ide/mod.rs:68`): Registers a
one-time panic hook that removes all `codegg_*` temp files from the
system temp directory.

### Diff Generators

**`generate_unified_diff()`** (`src/ide/mod.rs:392`):
Produces `--- a/path` / `+++ b/path` unified diff format. Returns
`"(no changes)"` when no differences exist.

**`generate_side_by_side()`** (`src/ide/mod.rs:420`):
Produces ANSI-colored side-by-side diff with grouped operations (context
of 3 lines).

### Command Execution

**`run_command_with_timeout()`** (`src/ide/mod.rs:10`):
Spawns a process, polls with `try_wait()` every 50ms, returns
`Ok(())` on success or `Err(format)` on failure/timeout. Timeout is
30 seconds (`IDE_COMMAND_TIMEOUT`, line 8).

## MCP IdeServer (`src/mcp/ide_server.rs`)

The `IdeServer` struct provides MCP server functionality for IDE
communication.

### Structure

```rust
pub struct IdeServer {                    // line 50
    tools: HashMap<String, ToolHandler>,  // registered tool handlers
    pending: PendingRequests,             // in-flight request tracking
    shutdown: Arc<Mutex<bool>>,           // shutdown flag
    shutdown_notify: Arc<Notify>,         // shutdown signal
}
```

### Transport Modes

**`run_stdio()`** (line 79):
Reads JSON-RPC frames from stdin line-by-line via `BufReader`. Writes
responses to stdout. Manages an `initialized` flag; all methods except
`initialize` are rejected until initialized.

**`clone_for_connection()`** (line 123):
Creates a clone sharing `tools`, `pending`, `shutdown`, and
`shutdown_notify` via `Arc`. Used for per-connection state in socket mode.

**`handle_connection()`** (line 133):
Processes JSON-RPC requests on a `UnixStream` connection. Uses
`BufReader` for line-based reading.

Note: `run_socket()` is described in some references but is **not
implemented** in the current code. Only `run_stdio()` is available.

### MCP Protocol

The server implements the MCP protocol (version `2024-11-05`):

| Method | Behavior |
|--------|----------|
| `initialize` | Returns server info (`codegg-ide` v0.1.0) and capabilities |
| `notifications/initialized` | Acknowledged (no-op) |
| `tools/list` | Returns registered tools with schemas |
| `tools/call` | Dispatches to tool handler by name |
| anything else | Returns method-not-found error |

### Registered Tools

**`openDiff`** (line 318):
Opens the native IDE diff viewer. Schema:

```json
{
  "original": "string (file path or @file#L1-L99 syntax)",
  "modified": "string (file path or @file#L1-L99 syntax)"
}
```

Both `original` and `modified` are required.

**`parse_file_reference()`** (line 372):
Parses the `@file#L1-L99` syntax:
- `path@file#start-end` → path + line range
- `path@file` → path only
- `path` → path only
- Empty line spec defaults to `(1, usize::MAX)`

### Shutdown

`shutdown()` (line 300) sets the shutdown flag and notifies the shutdown
signal. In stdio mode, the loop breaks on EOF.

## Key Types & APIs

| Type / Function | Location | Purpose |
|----------------|----------|---------|
| `is_vscode()` | `src/ide/mod.rs:83` | Detect VS Code via env vars |
| `is_jetbrains()` | `src/ide/mod.rs:89` | Detect JetBrains via env vars |
| `is_ide()` | `src/ide/mod.rs:96` | Detect any supported IDE |
| `open_diff()` | `src/ide/mod.rs:100` | Open IDE diff viewer with optional line ranges |
| `generate_unified_diff()` | `src/ide/mod.rs:392` | Generate unified diff string |
| `generate_side_by_side()` | `src/ide/mod.rs:420` | Generate ANSI side-by-side diff |
| `run_command_with_timeout()` | `src/ide/mod.rs:10` | Spawn process with 30s timeout |
| `TempFilesGuard` | `src/ide/mod.rs:46` | RAII guard for temp file cleanup |
| `IdeServer` | `src/mcp/ide_server.rs:50` | MCP server for IDE integration |
| `IdeServer::run_stdio()` | `src/mcp/ide_server.rs:79` | Run MCP over stdio |
| `IdeServer::handle_connection()` | `src/mcp/ide_server.rs:133` | Handle a Unix socket connection |
| `IdeServer::shutdown()` | `src/mcp/ide_server.rs:300` | Signal shutdown |
| `open_diff_handler()` | `src/mcp/ide_server.rs:345` | MCP tool handler for openDiff |
| `parse_file_reference()` | `src/mcp/ide_server.rs:372` | Parse `@file#L1-L99` syntax |

## Configuration Surface

| Constant | Value | Location |
|----------|-------|----------|
| `IDE_COMMAND_TIMEOUT` | 30 seconds | `src/ide/mod.rs:8` |

No config file keys. IDE detection is purely environment-variable-based.
The MCP server exposes no configuration beyond transport selection.

## Invariants & Gotchas

- **Temp files are flushed and handles dropped before IDE invocation**: This
  ensures content is visible to the IDE process. The `TempFilesGuard`
  provides cleanup on normal exit and panics.
- **Line ranges are 1-indexed, inclusive end**: `open_diff()` converts to
  0-indexed internally (`start.saturating_sub(1)`).
- **Generic fallback tries VS Code first**: If both `code` and `idea` are
  in PATH, VS Code gets priority.
- **JetBrains tool resolution is platform-aware**: Checks `$JETBRAINS_TOOL`,
  hardcoded Unix paths, Windows `PROGRAMFILES`, then PATH.
- **`run_socket()` is not implemented**: The current code only supports
  stdio transport. Socket mode is referenced in docs but absent from the
  implementation.
- **MCP server requires `initialize` first**: All other methods return
  error -32002 until initialized.
- **`openDiff` is synchronous**: The tool handler blocks until the IDE
  command completes or times out.

## Testing

```bash
cargo test -p codegg --lib ide           # IDE detection and diff tests
cargo test -p codegg --lib mcp::ide_server  # MCP server tests (if any)
```

Inline tests (`src/ide/mod.rs:444-473`):
- `test_vscode_detection` — verifies `is_vscode()` returns false in test env
- `test_jetbrains_detection` — verifies `is_jetbrains()` returns false
- `test_no_changes` — unified diff with identical input
- `test_with_changes` — unified diff with changed lines

## Related Docs

- [mcp.md](mcp.md) — MCP client/server system
- [tui.md](tui.md) — TUI that may display diffs
- [tool.md](tool.md) — Tool registry including IDE tools
