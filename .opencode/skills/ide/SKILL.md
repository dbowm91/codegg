---
name: ide
description: IDE integration for VS Code and JetBrains diff viewing
version: 1.0.0
tags:
  - ide
  - diff
  - vscode
  - jetbrains
---

# IDE Integration Guide

This skill covers IDE integration for diff viewing in opencode-rs.

## Overview

Detects IDE presence and generates diffs for VS Code and JetBrains IDEs.

## VS Code Detection

```rust
use crate::ide::is_vscode;

if is_vscode() {
    // VS Code is running
}
```

## JetBrains Detection

```rust
use crate::ide::is_jetbrains;

if is_jetbrains() {
    // JetBrains IDE is running
}
```

## Diff Generation

```rust
use crate::ide::{generate_unified_diff, generate_side_by_side};

let diff = generate_unified_diff(old_content, new_content, "file.rs");
let side_by_side = generate_side_by_side(old_content, new_content);
```

## Environment Variables

- `VSCODE_IPC_HOOK` - Set when VS Code is running
- `JETBRAINS_REMOTE` - Set when JetBrains remote mode is active