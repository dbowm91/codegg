# IDE Module Override

This file contains IDE-specific guidance and overrides root AGENTS.md.

## Module Location
`src/ide/mod.rs`

## Key Functions

### Detection
- `is_vscode()` - Checks `VSCODE_IPC_HOOK`, `VSCODE_INJECTED_ENVIRONMENT`, `TERM_PROGRAM=vscode`
- `is_jetbrains()` - Checks `JETBRAINS_REMOTE`, `JB_PRODUCT_READINESS`, `IDEA_INITIAL_DIRECTORY`, `WEBCLBROWSER_HOST`
- `is_ide()` - Returns true if either IDE is detected

### Core Functions
- `open_diff(original, modified, original_lines, modified_lines)` - Opens IDE diff viewer with optional line range slicing
- `generate_unified_diff(old, new, path)` - Generates unified diff for TUI display
- `generate_side_by_side(old, new, path)` - Generates ANSI-colored side-by-side diff (currently unused)

## Implementation Notes

### Line Range Handling
When `original_lines`/`modified_lines` are provided, content is sliced using 1-indexed, end-inclusive line numbers. Both VS Code and JetBrains handlers now use temporary files to pass sliced content (fixed 2026-05-22).

### IDE Detection
- VS Code detection includes additional env vars beyond `VSCODE_IPC_HOOK`
- JetBrains detection now includes `JB_PRODUCT_READINESS`, `IDEA_INITIAL_DIRECTORY`, `WEBCLBROWSER_HOST`

### Windows Support
JetBrains handler now searches `%PROGRAMFILES%\JetBrains\` for Windows installs and uses `idea.bat`.

## Known Issues
- `generate_side_by_side` is currently dead code (never called from anywhere in codebase)