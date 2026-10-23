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
| Detection + diff | `src/ide/mod.rs` (471 lines) |
| MCP server | `src/mcp/ide_server.rs` (440 lines) |
| Agent-side diff tool | `src/tool/diff.rs` (a **separate** implementation — see "Diff Generators") |

## How It Works

### IDE Detection

Three public functions check environment variables:

**`is_vscode()`** (`src/ide/mod.rs:81`):
- `VSCODE_IPC_HOOK` set
- `VSCODE_INJECTED_ENVIRONMENT` set
- `TERM_PROGRAM` equals `"vscode"`

**`is_jetbrains()`** (`src/ide/mod.rs:87`):
- `JETBRAINS_REMOTE` set
- `JB_PRODUCT_READINESS` set
- `IDEA_INITIAL_DIRECTORY` set
- `WEBCLBROWSER_HOST` set

**`is_ide()`** (`src/ide/mod.rs:94`): Returns `is_vscode() || is_jetbrains()`.

### Diff Viewing

**`open_diff()`** (`src/ide/mod.rs:98`):
1. Reads both files from disk (`std::fs::read_to_string`).
2. Applies line-range slicing if `original_lines` or `modified_lines` are
   provided (1-indexed, inclusive end).
3. Dispatches to `open_diff_vscode()`, `open_diff_jetbrains()`, or
   `open_diff_generic()` based on detection.

**VS Code** (`open_diff_vscode`, line 132):
- Writes content to temp files with `codegg_original_`/`codegg_modified_`
  prefixes.
- Flushes and drops file handles before invoking `code --diff`.
- Uses `run_command_with_timeout()` with a 30-second deadline.

**JetBrains** (`open_diff_jetbrains`, line 186):
- Same temp file pattern.
- Tool resolution: `$JETBRAINS_TOOL` env var → `/opt/intellij/bin/idea.sh`
  → `/usr/local/bin/idea` → Windows `%PROGRAMFILES%\JetBrains\<product>\bin\idea.bat`
  → `idea` in PATH.
- Invokes `<tool> diff <original> <modified>`.

**Generic fallback** (`open_diff_generic`, line 268):
- Searches PATH for `code`, `code.exe`, `code.cmd`, `idea`, `idea.bat`,
  `idea.cmd`.
- Tries VS Code first; if that fails, tries IntelliJ.
- Returns `"no IDE diff tool found"` if neither is available.

### Temp File Safety

**`TempFilesGuard`** (`src/ide/mod.rs:42`): Implements `Drop` to remove
temp files on scope exit, including panics. Each opener registers its two
temp paths with the guard before dropping the `tempfile` handles.

**`register_panic_cleanup()`** (`src/ide/mod.rs:66`): Registers a
one-time panic hook that removes all `codegg_*` temp files from the
system temp directory. All three openers call it
(`src/ide/mod.rs:133`, `:187`, `:269`).

### Diff Generators

**`generate_unified_diff()`** (`src/ide/mod.rs:390`):
Produces `--- a/path` / `+++ b/path` unified diff format. Returns
`"(no changes)"` when no differences exist.

**`generate_side_by_side()`** (`src/ide/mod.rs:418`):
Produces ANSI-colored side-by-side diff with grouped operations (context
of 3 lines).

Neither is wired into a tool. The agent-facing diff tool is
`src/tool/diff.rs`, which carries its **own private**
`generate_unified_diff()` (`src/tool/diff.rs:300`) with an extra
`line_range` argument and a 1000-line window cap. The `src/ide/mod.rs`
copy has no caller outside its own unit tests, and
`generate_side_by_side()` has no caller at all.

### Command Execution

**`run_command_with_timeout()`** (`src/ide/mod.rs:10`):
Builds a `ManagedProcessRequest` and runs it through
`ManagedProcessService::run_blocking`, so the IDE command is a
**managed-process** launch, not a bare `std::process::Command`. The request
carries:

- `EnvironmentPolicy::inherited()` — the IDE command sees the user's
  environment (minus the always-stripped command-bearing variables), which
  is the opposite of the sanitized policy the shell hooks use;
- `request.timeout = Some(IDE_COMMAND_TIMEOUT)` (30s);
- `OutputPolicy::new(64 * 1024)`.

Returns `Ok(())` on a successful exit, `Err` on a non-zero exit, on a
`TerminationReason::TimedOut` (reported as `"<program> timed out after
30s"`), and on a spawn failure.


## MCP IdeServer (`src/mcp/ide_server.rs`)

The `IdeServer` struct provides MCP server functionality for IDE
communication.

### Structure

```rust
pub struct IdeServer {                    // line 51
    tools: HashMap<String, ToolHandler>,  // registered tool handlers
    pending: PendingRequests,             // #[allow(dead_code)] — never read
    shutdown: Arc<Mutex<bool>>,           // shutdown flag
    shutdown_notify: Arc<Notify>,         // shutdown signal
}
```

### Transport Modes

**`run_stdio()`** (line 80):
Reads JSON-RPC frames from stdin line-by-line via `BufReader`. Writes
responses to stdout. Manages an `initialized` flag; all methods except
`initialize` are rejected until initialized.

**`clone_for_connection()`** (line 124):
Creates a clone sharing `tools`, `pending`, `shutdown`, and
`shutdown_notify` via `Arc`.

**`handle_connection()`** (line 135):
Processes JSON-RPC requests on a `UnixStream` connection. Uses
`BufReader` for line-based reading.

**The socket transport is unreachable.** `run_socket()` does not exist, and
both socket helpers are `#[allow(dead_code)]` and have **no caller**: the
only reference to `IdeServer` outside this file is the `pub mod ide_server;`
declaration in `src/mcp/mod.rs`. Nothing constructs an `IdeServer` outside
the module, so `run_stdio()` is the only live entry point.

### MCP Protocol

The IDE server currently implements the legacy MCP server binding
(`2024-11-05`, `src/mcp/protocol.rs::LEGACY_PROTOCOL_VERSION`) for
compatibility. CodeGG's generic MCP clients negotiate modern `2026-07-28`
servers independently; this IDE server is not an authority for that client
policy:

| Method | Behavior |
|--------|----------|
| `initialize` | Returns server info (`codegg-ide` v0.1.0) and capabilities |
| `notifications/initialized` | Acknowledged (no-op) |
| `tools/list` | Returns registered tools with schemas |
| `tools/call` | Dispatches to tool handler by name |
| anything else | Returns method-not-found error |

Error codes returned by `handle_request()` (`src/mcp/ide_server.rs:176`):

| Code | Condition |
|------|-----------|
| `-32002` | A method other than `initialize` arrived before initialization |
| `-32601` | Unknown method, or `tools/call` named an unregistered tool |
| `-32603` | The tool handler returned `Err`, or its blocking task failed |

`tools/call` runs the (synchronous) handler through
`tokio::task::spawn_blocking`, so a handler that blocks up to 30 seconds
does not occupy a Tokio worker.

### Registered Tools

**`openDiff`** (schema at `src/mcp/ide_server.rs:343`):
Opens the native IDE diff viewer. Schema:

```json
{
  "original": "string (file path or @file#L1-L99 syntax)",
  "modified": "string (file path or @file#L1-L99 syntax)"
}
```

Both `original` and `modified` are required.

**`open_diff_handler()`** (line 363):
Reads `original`/`modified` as strings (missing → `Err("original file
required")` / `Err("modified file required")`), parses each with
`parse_file_reference()`, calls `crate::ide::open_diff`, and returns
`{"success": true, "message": "Opened diff: <original> vs <modified>"}`.

**`parse_file_reference()`** (line 390):
Parses the `@file#L1-L99` syntax. The parser splits at the **first** `@`:
| `path@file#start-end` | path `path@file`, plus the parsed line range |
| `path@file` | path `path` — the `file` part is **dropped** |
| `path` | path `path`, no line range |

**`parse_line_range()`** (line 420) accepts **bare numbers only**: an empty
spec is `(1, usize::MAX)`, `a-b` is `(a, b)`, and a single number `n` is
`(n, n)`. It does not strip an `L` prefix, so the `@file#L1-L99` form
advertised in the tool schema fails to parse with `"invalid start line"`.
The shape that works is `@file#1-99`.

### Shutdown

`shutdown()` (line 318) sets the shutdown flag and notifies the shutdown
signal. In stdio mode, the loop breaks on EOF.

## Key Types & APIs

| Type / Function | Location | Purpose |
|----------------|----------|---------|
| `is_vscode()` | `src/ide/mod.rs:81` | Detect VS Code via env vars |
| `is_jetbrains()` | `src/ide/mod.rs:87` | Detect JetBrains via env vars |
| `is_ide()` | `src/ide/mod.rs:94` | Detect any supported IDE |
| `open_diff()` | `src/ide/mod.rs:98` | Open IDE diff viewer with optional line ranges |
| `generate_unified_diff()` | `src/ide/mod.rs:390` | Generate unified diff string |
| `generate_side_by_side()` | `src/ide/mod.rs:418` | Generate ANSI side-by-side diff |
| `run_command_with_timeout()` | `src/ide/mod.rs:10` | Run the IDE command through the managed-process surface, 30s timeout |
| `TempFilesGuard` | `src/ide/mod.rs:42` | RAII guard for temp file cleanup |
| `IdeServer` | `src/mcp/ide_server.rs:51` | MCP server for IDE integration |
| `IdeServer::run_stdio()` | `src/mcp/ide_server.rs:80` | Run MCP over stdio (the only live transport) |
| `IdeServer::handle_connection()` | `src/mcp/ide_server.rs:135` | Handle a Unix socket connection (dead code) |
| `IdeServer::shutdown()` | `src/mcp/ide_server.rs:318` | Signal shutdown |
| `open_diff_handler()` | `src/mcp/ide_server.rs:363` | MCP tool handler for openDiff |
| `parse_file_reference()` | `src/mcp/ide_server.rs:390` | Parse the `@file#start-end` syntax |
| `parse_line_range()` | `src/mcp/ide_server.rs:420` | Parse a bare `start-end` line range |

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
  0-indexed internally (`start.saturating_sub(1)`), and the slice
  `lines[start_idx..end_idx]` yields lines `start..=end`. A range whose
  **start is greater than its end** (`#10-5`) is not guarded and slices
  out of bounds.
- **The IDE command inherits the user's environment**: `run_command_with_timeout`
  uses `EnvironmentPolicy::inherited()`, not the sanitized policy the shell
  hooks use — the IDE needs its own configuration to launch.
- **The IDE command's working directory is the process CWD**:
  `run_command_with_timeout` resolves it with `std::env::current_dir()`
  (`src/ide/mod.rs:15`). In the user-scoped daemon that is the launch
  directory, not the session's `workspace_root`.
  `scripts/check_daemon_cwd_usage.py` does not scan `src/ide/`.
- **Generic fallback tries VS Code first**: If both `code` and `idea` are
  in PATH, VS Code gets priority.
- **JetBrains tool resolution is platform-aware**: Checks `$JETBRAINS_TOOL`,
  hardcoded Unix paths, Windows `PROGRAMFILES`, then PATH.
- **`run_socket()` is not implemented**: The current code only supports
  stdio transport. Socket mode is referenced in docs but absent from the
  implementation — and the two socket helpers that do exist are
  `#[allow(dead_code)]` with no caller.
- **MCP server requires `initialize` first**: All other methods return
  error -32002 until initialized.
- **`openDiff` is synchronous in its handler**: The handler blocks until
  the IDE command completes or times out, but the server runs it through
  `tokio::task::spawn_blocking` so the Tokio worker is not held.
- **The `openDiff` schema advertises a syntax the parser rejects**: the
  schema says `@file#L1-L99`, but `parse_line_range` accepts bare numbers
  only (`@file#1-99`).
- **`src/ide/mod.rs`'s diff generators are not the agent's diff**:
  `src/tool/diff.rs` carries its own `generate_unified_diff()` with a
  line-range cap; the `src/ide/mod.rs` copy has no non-test caller.

## Testing

```bash
cargo test -p codegg --lib ide           # IDE detection and diff tests
```

Inline tests (`src/ide/mod.rs:441-471`):
- `test_vscode_detection` — verifies `is_vscode()` returns false in test env
- `test_jetbrains_detection` — verifies `is_jetbrains()` returns false
- `test_no_changes` — unified diff with identical input
- `test_with_changes` — unified diff with changed lines

**Coverage gaps**: `src/mcp/ide_server.rs` has **no** `#[cfg(test)]` module —
the MCP IDE server (protocol errors, `parse_file_reference`,
`parse_line_range`, `open_diff_handler`) is untested. The separate
`src/tool/diff.rs` tool *does* have inline tests, including
`line_range_past_end_reports_no_changes_without_panic`.

## Related Docs

- [mcp.md](mcp.md) — MCP client/server system
- [tui.md](tui.md) — TUI that may display diffs
- [tool.md](tool.md) — Tool registry including IDE tools

## Source verification

Verified 2026-10-06 against `src/ide/mod.rs` (471 lines) and
`src/mcp/ide_server.rs` (438 lines): `IDE_COMMAND_TIMEOUT` 30s
(`ide/mod.rs:8`), `run_command_with_timeout` (`:10`), `TempFilesGuard`
(`:42`), `register_panic_cleanup` (`:66`), `is_vscode` (`:81`),
`is_jetbrains` (`:87`), `is_ide` (`:94`), `open_diff` (`:98`),
`open_diff_vscode` (`:132`), `open_diff_jetbrains` (`:186`),
`open_diff_generic` (`:268`), `generate_unified_diff` (`:390`),
`generate_side_by_side` (`:418`), inline tests (`:442-471`);
`IdeServer` (`ide_server.rs:50`), `run_stdio` (`:79`),
`clone_for_connection` (`:123`), `handle_connection` (`:133`),
`shutdown` (`:316`), `open_diff_handler` (`:361`),
`parse_file_reference` (`:388`). Confirmed `run_socket` has no
implementation (the caveat stands). The prior `parse_file_reference`
line 372 in prose contradicted the correct 388 in the table below;
both now read 388.

Verified 2026-10-23 against `src/ide/mod.rs` (471 lines),
`src/mcp/ide_server.rs` (440 lines), `src/mcp/protocol.rs`, `src/mcp/mod.rs`,
`src/managed_process.rs`, and `src/tool/diff.rs`. Corrections made:

| Claim in the previous revision | Finding |
|---|---|
| `src/mcp/ide_server.rs` is 438 lines | It is 440 lines. |
| Every `src/mcp/ide_server.rs` line reference | All were one line low. Corrected: `IdeServer` `:50`→`:51`, `run_stdio` `:79`→`:80`, `clone_for_connection` `:123`→`:124`, `handle_connection` `:133`→`:135`, `shutdown` `:316`→`:318`, `open_diff_handler` `:361`→`:363`, `parse_file_reference` `:388`→`:390`. Added `handle_request` (`:176`), `get_tool_schema` (`:343`), `parse_line_range` (`:420`). |
| "Spawns a process, polls with `try_wait()` every 50ms" | Stale. `run_command_with_timeout` now builds a `ManagedProcessRequest` and runs it through `ManagedProcessService::run_blocking`, with `EnvironmentPolicy::inherited()`, `timeout = Some(IDE_COMMAND_TIMEOUT)`, and `OutputPolicy::new(64 * 1024)`. |
| `pending: PendingRequests` — "in-flight request tracking" | The field is `#[allow(dead_code)]` and is never read; nothing tracks in-flight requests. |
| "Socket mode … used for per-connection state" | `clone_for_connection` and `handle_connection` are both `#[allow(dead_code)]` with **no caller** anywhere in the workspace, and `IdeServer` is only referenced by `pub mod ide_server;` in `src/mcp/mod.rs`. The socket transport is unreachable, not merely unimplemented. |
| "`openDiff` is synchronous: the tool handler blocks" | Half right. The handler is synchronous, but the server runs it through `tokio::task::spawn_blocking`, so the Tokio worker is not held. |
| `parse_file_reference` bullets | Made precise: the parser splits at the **first** `@`; `path@file#start-end` yields path `path@file` **plus** a range, while `path@file` (no `#`) drops the `file` part and yields `path` with no range. |
| — | Added: the tool's error codes (`-32002` not initialized, `-32601` unknown method/tool, `-32603` handler error), the `open_diff_handler` behaviour, `parse_line_range` accepting **bare numbers only** (so the schema's advertised `@file#L1-L99` cannot parse), the `spawn_blocking` offload, and the fact that `src/tool/diff.rs` carries a separate private `generate_unified_diff()` (with a 1000-line window cap) while the `src/ide/mod.rs` copy has no non-test caller and `generate_side_by_side()` has none at all. |

Confirmed still accurate: every `src/ide/mod.rs` reference
(`IDE_COMMAND_TIMEOUT` `:8`, `run_command_with_timeout` `:10`,
`TempFilesGuard` `:42`, `register_panic_cleanup` `:66`, `is_vscode` `:81`,
`is_jetbrains` `:87`, `is_ide` `:94`, `open_diff` `:98`,
`open_diff_vscode` `:132`, `open_diff_jetbrains` `:186`,
`open_diff_generic` `:268`, `generate_unified_diff` `:390`,
`generate_side_by_side` `:418`, inline tests `:441-471`); the four IDE
detection environment-variable sets; the JetBrains tool-resolution order;
the generic fallback's VS Code-first order and its `"no IDE diff tool
found"` error; the `codegg_original_`/`codegg_modified_` temp prefixes;
the flush-and-drop-before-invoke ordering; the 1-indexed inclusive-end
line-range convention; and the `LEGACY_PROTOCOL_VERSION` value
`2024-11-05` (`src/mcp/protocol.rs:10`).

Verified 2026-10-23 (second pass) against `src/ide/mod.rs` (471 lines),
`src/mcp/ide_server.rs` (440 lines), `src/tool/diff.rs` (437 lines),
`src/managed_process.rs`, `src/mcp/protocol.rs`, `src/mcp/mod.rs`, and
`scripts/check_daemon_cwd_usage.py`. Corrections made:

| Claim in the previous revision | Finding |
|---|---|
| "cargo test -p codegg --lib mcp::ide_server # MCP server tests (if any)" | There are **none**. `src/mcp/ide_server.rs` has no `#[cfg(test)]` module, and no integration test targets it. The command was dropped and the gap is now recorded. |
| inline tests "`src/ide/mod.rs:442-471`" | The `#[cfg(test)]` attribute is at `:441` and `mod tests {` at `:442`; `:441-471` is the accurate span. |
| — | Added: the IDE command's **working directory** — `run_command_with_timeout` takes it from `std::env::current_dir()` (`src/ide/mod.rs:15`), i.e. the daemon's launch directory rather than the session `workspace_root`; recorded as an invariant and reported as a code issue (`scripts/check_daemon_cwd_usage.py` does not scan `src/ide/`). |

Re-confirmed as accurate: every line reference in both files
(`src/ide/mod.rs` `:8`/`:10`/`:42`/`:66`/`:81`/`:87`/`:94`/`:98`/`:132`/
`:186`/`:268`/`:390`/`:418`; `src/mcp/ide_server.rs` `:51`/`:80`/`:124`/
`:135`/`:176`/`:318`/`:343`/`:363`/`:390`/`:420`); the four VS Code and
four JetBrains detection variables; `open_diff`'s read → slice → dispatch
order and the unguarded `start > end` slice; the VS Code flush-and-drop
ordering; the JetBrains tool-resolution chain including the literal `idea`
PATH fallback; the generic fallback's VS Code-first search over
`code`/`code.exe`/`code.cmd` then `idea`/`idea.bat`/`idea.cmd` and its
`"no IDE diff tool found"` error; `EnvironmentPolicy::inherited()`; the
managed-process request's timeout and `OutputPolicy::new(64 * 1024)`; the
`initialize` response (`codegg-ide` v0.1.0, `LEGACY_PROTOCOL_VERSION`) and
the `-32002`/`-32601`/`-32603` error codes; `spawn_blocking`; the `openDiff`
schema and its `required` list; `parse_file_reference`'s first-`@` split
(`path@file#start-end` → `path@file` + range; `path@file` → `path`, range
dropped); `parse_line_range`'s bare-number-only grammar; that `IdeServer` has
no consumer outside `pub mod ide_server;`; and that `src/tool/diff.rs:300`
carries its own private `generate_unified_diff` with the 1000-line window
cap at `:318`.

