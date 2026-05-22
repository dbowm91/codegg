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

Uses VS Code's `--diff` CLI argument with temporary files:

```rust
Command::new("code")
    .args(["--diff", original_path, modified_path])
```

## JetBrains Integration

Uses JetBrains `idea` or `idea.sh` CLI with `diff` subcommand. Supports:
- `$JETBRAINS_TOOL` environment variable override
- Unix paths: `/opt/intellij/bin/idea.sh`, `/usr/local/bin/idea`
- Windows: `%PROGRAMFILES%\JetBrains\<product>\bin\idea.bat`
- Falls back to `idea` in PATH

## See Also

- [tui.md](tui.md) - TUI that displays diffs