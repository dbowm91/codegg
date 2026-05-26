# IDE Architecture Review

## Summary
The IDE architecture document is mostly accurate but has outdated code reference line numbers, incomplete documentation of some functions, and missing details about what happens when no IDE is detected.

## Verified Correct
- **is_vscode()** (`src/ide/mod.rs:80-84`) - Checks `VSCODE_IPC_HOOK`, `VSCODE_INJECTED_ENVIRONMENT`, `TERM_PROGRAM==vscode` - matches doc exactly
- **is_jetbrains()** (`src/ide/mod.rs:86-91`) - Checks `JETBRAINS_REMOTE`, `JB_PRODUCT_READINESS`, `IDEA_INITIAL_DIRECTORY`, `WEBCLBROWSER_HOST` - matches doc exactly
- **is_ide()** (`src/ide/mod.rs:93-95`) - Returns `is_vscode() || is_jetbrains()` - matches doc exactly
- **generate_unified_diff** (`src/ide/mod.rs:371-397`) - Returns unified diff with `--- a/path`, `+++ b/path` format - matches doc
- **generate_side_by_side** (`src/ide/mod.rs:399-420`) - Uses ANSI color codes - doc says it does, code confirms
- **open_diff function signature** (`src/ide/mod.rs:97-102`) - Parameters `_original`, `_modified`, `original_lines`, `modified_lines` - matches doc
- **IDE detection via temp files** - VS Code and JetBrains both use temp files as documented
- **run_command_with_timeout error format** (`src/ide/mod.rs:27`) - Returns `"{} failed (exit {})"` format - matches doc
- **JetBrains Windows path** (`src/ide/mod.rs:228-243`) - Uses `%PROGRAMFILES%\JetBrains\<product>\bin\idea.bat` - matches doc
- **JetBrains tool fallback** (`src/ide/mod.rs:245`) - Falls back to `idea` in PATH - matches doc获

## Discrepancies Found
- **Line 78-89 code example** - Architecture doc shows code snippet for VS Code integration with `run_command_with_timeout`, but the actual implementation details differ in that:
  - `original_file.flush()` happens inside a block at `src/ide/mod.rs:149`
  - Temp files are not individually passed to the guard until after creation (`src/ide/mod.rs:165-166`)
  - Not a bug but the code example in doc is simplified and not exact

## Bugs Identified
- **Indentation bug in generic fallback** (`src/ide/mod.rs:257-369`) - The `open_diff_generic` function has inconsistent indentation around `let _output = run_command_with_timeout` at lines 302-311, and the guard drop placement looks potentially incorrect visually, though the logic may still work
- **IDE integration detail** - Doc claims "Temporary files are dropped before invoking the IDE to ensure paths are valid" (`architecture/ide.md:77`) but this is misleading. The temp files are dropped AFTER the command invocation returns (lines 168-169 for VS Code, line 253 for JetBrains). The files need to exist while the diff command runs.

## Improvement Suggestions
- **Missing function: register_panic_cleanup** - This function (`src/ide/mod.rs:65-78`) is not documented and handles cleanup of temp files on panic - could be documented
- **Missing function: TempFilesGuard** - Not documented, implements Drop to clean up temp files - could be documented
- **Indentation** - `open_diff_generic` at lines 302-311 has questionable indentation that makes the code harder to read
- **Generic fallback check** (`src/ide/mod.rs:260-261`) - The check looks for `code` but only proceeds if `_output.is_ok()` - code variable is unused (prefixed with `_`)

## Stale Items in Architecture Doc
- **MCP IdeServer section** (`architecture/ide.md:105-141`) - References correct file `src/mcp/ide_server.rs` but shows code at lines 114-118 for `run_stdio()` and 126-137 for `run_socket()`, which are accurate per actual source at `src/mcp/ide_server.rs:78-119` and `121-144`
- **run_stdio implementation detail** - Doc says "Uses tokio async I/O for stdio-based communication" which matches actual code at `src/mcp/ide_server.rs:79`
- **run_socket implementation detail** - Doc correctly notes it uses tokio's `UnixListener`
