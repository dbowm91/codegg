---
name: diff
description: Inline diff visualization with similar crate
tags: [diff, visualization, tui, widget]
---

Use the `/skill:diff` command to load context about diff visualization in the TUI.

## Overview

The DiffViewer widget displays file differences inline in the terminal, supporting both side-by-side and unified diff views with syntax highlighting.

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `d` | Open diff dialog |
| `↑/↓` or `j/k` | Scroll |
| `s` | Toggle inline/side-by-side mode |
| `PageUp/PageDown` | Scroll by 10 lines |
| `Home/End` | Jump to start/end |
| `Esc` | Close dialog |

## Module Structure

```
src/tui/components/
  diff.rs           # DiffViewer widget
  dialogs/diff.rs  # DiffDialog wrapper
```

## DiffViewer Widget

```rust
pub struct DiffViewer {
    old_text: String,
    new_text: String,
    mode: DiffMode,  // Inline or SideBySide
    scroll_offset: usize,
}
```

## Features

- **Color-coded lines**: Green for added, red for removed, yellow for modified
- **Line numbers**: Display line numbers for both old and new content
- **Scrolling**: Support for large diffs with centered scroll
- **Multiple modes**: Toggle between inline and side-by-side

## Usage

```rust
let viewer = DiffViewer::new()
    .with_old_text(&original)
    .with_new_text(&modified)
    .with_mode(DiffMode::Inline);
```

## Integration

- Opens via `TuiCommand::OpenDiffDialog`
- Uses existing dialog rendering infrastructure
- Isolated in `src/tui/components/` per Wave 4 requirements