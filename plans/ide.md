# IDE Architecture Review Findings

## Verified Claims

- **is_vscode()**: Checks `VSCODE_IPC_HOOK`, `VSCODE_INJECTED_ENVIRONMENT`, `TERM_PROGRAM==vscode` - confirmed at `src/ide/mod.rs:80-84`
- **is_jetbrains()**: Checks `JETBRAINS_REMOTE`, `JB_PRODUCT_READINESS`, `IDEA_INITIAL_DIRECTORY`, `WEBCLBROWSER_HOST` - confirmed at `src/ide/mod.rs:86-91`
- **is_ide()**: Returns `is_vscode() || is_jetbrains()` - confirmed at `src/ide/mod.rs:93-95`
- **open_diff signature**: `_original: &str, _modified: &str, original_lines: Option<(usize, usize)>, modified_lines: Option<(usize, usize)>` - confirmed at `src/ide/mod.rs:97-102`
- **generate_unified_diff**: Returns `--- a/path, +++ b/path` format - confirmed at `src/ide/mod.rs:371-397`
- **generate_side_by_side**: Generates ANSI-colored side-by-side diff - confirmed at `src/ide/mod.rs:399-420`
- **TempFilesGuard**: Defined at `src/ide/mod.rs:43-63` - implements Drop to clean up files
- **register_panic_cleanup**: Private function at `src/ide/mod.rs:65-78` - uses `std::sync::Once` and `std::panic::set_hook`
- **VS Code integration**: Uses `code --diff` with temp files, file handles released AFTER IDE invocation - confirmed at lines 168-175
- **JetBrains integration**: Supports `$JETBRAINS_TOOL` env var, Unix/Windows paths, falls back to `idea` in PATH - confirmed at lines 180-255
- **IDE command timeout**: 30 seconds - `IDE_COMMAND_TIMEOUT` at line 8
- **IdeServer::run_stdio()**: Uses `tokio::io::stdin()` and `tokio::io::stdout()` async I/O - confirmed at `src/mcp/ide_server.rs:78-119`
- **IdeServer::run_socket()**: Uses `UnixListener::bind()` - confirmed at `src/mcp/ide_server.rs:121-144`
- **IdeServer clone_for_connection**: Creates new IdeServer for each connection with Arc cloned fields - confirmed at `src/mcp/ide_server.rs:146-153`
- **open_diff_handler**: Handler function for MCP "openDiff" tool - confirmed at `src/mcp/ide_server.rs:367-392`

## Stale Information

- **TempFilesGuard line numbers**: Document says "defined at `src/ide/mod.rs:43-63`" - ACTUAL is lines 43-63, VERIFIED
- **register_panic_cleanup line numbers**: Document says "`src/ide/mod.rs:65-78`" - ACTUAL is lines 65-78, VERIFIED
- **run_stdio signature**: Document says returns `Result<(), McpError>` using tokio async I/O - VERIFIED

## Bugs Found

- **open_diff_generic implementation**: The generic fallback at `src/ide/mod.rs:257-369` searches PATH for `code` or `idea` but cannot actually open files passed as arguments since `_original` and `_modified` are not read in `open_diff_generic`. The generic function creates temp files but passes them to `code --diff` or `idea diff`. However, the issue is the function signature takes string content, not file paths - the docs say "creates temporary files with the content" which is correct.

## Improvements Suggested

- None identified

## Cross-Module Issues

- **IdeServer protocolVersion**: Reports "2024-11-05" at `src/mcp/ide_server.rs:207` - this is a hardcoded value that may need updating as MCP protocol evolves
- **Reference to tui.md and mcp.md**: Document correctly references cross-module architecture docs
