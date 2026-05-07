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
    // Check VSCODE_INJECTED_ENVIRONMENT variable
    // Or check for .vscode-server directory
}
```

### is_jetbrains()

```rust
pub fn is_jetbrains() -> bool {
    // Check for JetBrains-specific env vars
    // JB_PRODUCT_READINESS
    // IDEA_INITIAL_DIRECTORY
}
```

### open_diff()

```rust
pub fn open_diff(original: &Path, modified: &Path, original_name: &str, modified_name: &str) -> Result<()>;
```

Opens the IDE's diff viewer with two files.

### generate_unified_diff()

```rust
pub fn generate_unified_diff(original: &str, modified: &str, original_name: &str, modified_name: &str) -> String;
```

Generates a unified diff string.

### generate_side_by_side()

```rust
pub fn generate_side_by_side(original: &str, modified: &str, width: usize) -> String;
```

Generates a side-by-side diff view.

## VS Code Integration

Uses VS Code's IPC mechanism:

```rust
#[derive(Serialize)]
struct VsCodeCommand {
    command: String,
    args: Vec<Value>,
}
```

Commands sent via stdio to VS Code.

## JetBrains Integration

Uses JetBrains' remote mode API:

```rust
#[derive(Serialize)]
struct JetBrainsRequest {
    action: String,
    params: Value,
}
```

HTTP requests to JetBrains gateway.

## See Also

- [tui.md](tui.md) - TUI that displays diffs
