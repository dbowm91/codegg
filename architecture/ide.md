# IDE Module

The `ide` module provides integration with VS Code and JetBrains IDEs for diff viewing and detection.

## Overview

**Location**: `src/ide/`

**Key Responsibilities**:
- IDE detection (VS Code, JetBrains)
- Opening diff views in IDEs
- Unified and side-by-side diff generation

## Key Functions

### is_vscode()

```rust
pub fn is_vscode() -> bool {
    std::env::var("VSCODE_IPC_HOOK").is_ok()
        || std::env::var("VSCODE_INJECTED_ENVIRONMENT").is_ok()
        || std::env::var("TERM_PROGRAM").is_ok_and(|v| v == "vscode")
}
```

### is_jetbrains()

```rust
pub fn is_jetbrains() -> bool {
    std::env::var("JETBRAINS_REMOTE").is_ok()
        || std::env::var("JB_PRODUCT_READINESS").is_ok()
        || std::env::var("IDEA_INITIAL_DIRECTORY").is_ok()
        || std::env::var("WEBCLBROWSER_HOST").is_ok()
}
```

### is_ide()

```rust
pub fn is_ide() -> bool {
    is_vscode() || is_jetbrains()
}
```

### open_diff()

```rust
pub fn open_diff(
    _original: &str,
    _modified: &str,
    original_lines: Option<(usize, usize)>,
    modified_lines: Option<(usize, usize)>,
) -> Result<(), String>
```

Opens the IDE's diff viewer with two files. When line ranges are provided, the content is sliced before opening in the IDE. Uses temp files for JetBrains and VS Code diffs.

### generate_unified_diff()

```rust
pub fn generate_unified_diff(old: &str, new: &str, path: &str) -> String
```

Generates a unified diff string (--- a/path, +++ b/path format).

### generate_side_by_side()

```rust
pub fn generate_side_by_side(old: &str, new: &str, path: &str) -> String
```

Generates a side-by-side diff view with ANSI color codes.

## VS Code Integration

Uses VS Code's `--diff` CLI argument with temporary files. Files are flushed before passing to VS Code to ensure content is visible. Temporary files are dropped before invoking the IDE to ensure paths are valid:

```rust
let mut original_file = original_temp.as_file();
original_file.write_all(original_content.as_bytes())?;
original_file.flush()?;
drop(original_temp);  // Release file handle before IDE reads

let output = Command::new("code")
    .args(["--diff", original_path, modified_path])
    .output()?;

if !output.status.success() {
    return Err(format!(
        "vscode diff failed (exit {}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    ));
}
```

Error messages include exit status and stderr output for debugging.

## JetBrains Integration

Uses JetBrains `idea` or `idea.sh` CLI with `diff` subcommand. Supports:
- `$JETBRAINS_TOOL` environment variable override
- Unix paths: `/opt/intellij/bin/idea.sh`, `/usr/local/bin/idea`
- Windows: `%PROGRAMFILES%\JetBrains\<product>\bin\idea.bat`
- Falls back to `idea` in PATH

## Generic Fallback (No IDE Detected)

When no IDE is detected, `open_diff_generic()` searches PATH using `std::env::split_paths()` for `code` or `idea` binaries. Unlike IDE-specific handlers that use the original file paths, the generic fallback creates temporary files with the content (applying line range slicing if provided) and passes those to the IDE.

## MCP IdeServer (`src/mcp/ide_server.rs`)

The `IdeServer` struct provides MCP server functionality for IDE communication with two transport modes:

### `run_stdio()` - Standard I/O Transport

Uses tokio async I/O for stdio-based communication:

```rust
pub async fn run_stdio(&self) -> Result<(), McpError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (reader, mut writer) = (tokio::io::stdin(), tokio::io::stdout());
    // ...
}
```

### `run_socket()` - Unix Socket Transport

Uses Unix socket for network-based communication:

```rust
pub async fn run_socket(&self, socket_path: &str) -> Result<(), McpError> {
    let listener = UnixListener::bind(socket_path)?;
    loop {
        tokio::select! {
            biased;
            _ = self.shutdown_notify.notified() => break,
            result = listener.accept() => {
                // Handle incoming connections
            }
        }
    }
}
```

The `run_socket()` method uses async I/O via tokio's `UnixListener`, allowing multiple IDE connections. Each connection is handled in a spawned task via `handle_connection()`.

## See Also

- [tui.md](tui.md) - TUI that displays diffs
- [mcp.md](mcp.md) - MCP client/server system including IdeServer