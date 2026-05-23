---
name: ide
description: IDE integration for VS Code and JetBrains diff viewing
version: 1.3.0
tags:
  - ide
  - diff
  - vscode
  - jetbrains
---

# IDE Integration Guide

This skill covers IDE integration for diff viewing in opencode-rs.

## Overview

Detects IDE presence and generates diffs for VS Code and JetBrains IDEs. Supports line range slicing for focused diff viewing.

## Detection Functions

```rust
use crate::ide::{is_vscode, is_jetbrains, is_ide};

if is_vscode() {
    // VS Code is running
}

if is_jetbrains() {
    // JetBrains IDE is running
}

if is_ide() {
    // Any supported IDE is running
}
```

## VS Code Detection

Checks for:
- `VSCODE_IPC_HOOK` - Set when VS Code IPC server is active
- `VSCODE_INJECTED_ENVIRONMENT` - Set in VS Code integrated terminal
- `TERM_PROGRAM=vscode` - Terminal program detection

## JetBrains Detection

Checks for:
- `JETBRAINS_REMOTE` - Set when JetBrains remote mode is active
- `JB_PRODUCT_READINESS` - JetBrains product readiness flag
- `IDEA_INITIAL_DIRECTORY` - JetBrains initial working directory
- `WEBCLBROWSER_HOST` - JetBrains web client host

## Opening Diff Views

```rust
use crate::ide::open_diff;

// Open full diff
open_diff("/path/to/original", "/path/to/modified", None, None)?;

// Open diff with line ranges (1-indexed, end-inclusive)
open_diff(
    "/path/to/original",
    "/path/to/modified",
    Some((10, 50)),  // original lines 10-50
    Some((10, 50)),  // modified lines 10-50
)?;
```

When line ranges are provided, content is sliced before opening in the IDE. Both VS Code and JetBrains handlers use temporary files for the sliced content.

**Important**: Temporary files persist until the IDE process exits (RAII pattern via `tempfile` crate). Files are flushed before passing paths to the IDE to ensure content is visible.

## Diff Generation (for TUI display)

```rust
use crate::ide::generate_unified_diff;

let diff = generate_unified_diff(old_content, new_content, "file.rs");
// Returns unified diff format or "(no changes)" if identical
```

```rust
use crate::ide::generate_side_by_side;

let diff = generate_side_by_side(old_content, new_content, "file.rs");
// Returns ANSI-colored side-by-side diff
```

## IDE-Specific Handlers

### VS Code
Uses `code --diff` CLI with temporary files.

### JetBrains
Uses `idea diff` or `idea.sh diff` CLI. Supports:
- `$JETBRAINS_TOOL` environment variable override
- Unix paths: `/opt/intellij/bin/idea.sh`, `/usr/local/bin/idea`
- Windows: `%PROGRAMFILES%\JetBrains\<product>\bin\idea.bat`

### Generic Fallback
If no IDE is detected, uses `std::env::split_paths` to search PATH for `code`/`code.exe` then `idea`/`idea.bat`. Creates temporary files with content (same as IDE-specific handlers).

## Implementation Details

- Uses `similar` crate for diff generation
- Uses `tempfile` crate for secure temporary file creation
- Temp file prefixes: `codegg_original_`, `codegg_modified_`
- Line ranges are 1-indexed and end-inclusive, with bounds checking via `saturating_sub` and `min`