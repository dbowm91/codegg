# Tool Module Architecture Review (2026-05-27)

## Verified Correct Items

| Item | Location | Status |
|------|----------|--------|
| Tool trait signature | `src/tool/mod.rs:54-60` | ✅ Matches doc |
| ToolResult struct (tool_name, input, output, success) | `src/tool/mod.rs:62-68` | ✅ Matches doc |
| ToolCatalog struct and methods | `src/tool/catalog.rs:36-100` | ✅ Matches doc |
| ToolExecutor with retry (exponential backoff + jitter) | `src/tool/executor.rs:8-56` | ✅ Matches doc |
| ToolError enum variants (NotFound, Execution, Timeout, Permission, Format, Disabled, Io, Network) | `src/error.rs:326-350` | ✅ Matches doc |
| is_retryable() for Io/Network/Timeout | `src/error.rs:352-358` | ✅ Matches doc |
| Path validation (validate_path, check_path_for_symlinks) | `src/tool/util.rs:5-51` | ✅ Matches doc |
| 26 tools in with_defaults() | `src/tool/mod.rs:89-119` | ✅ Verified count |
| plan_enter / plan_exit are separate tools | `src/tool/mod.rs:113-114` | ✅ Matches doc |
| Tool definition caching with mcp_tool_count proxy | `src/agent/loop.rs:1029-1102` | ✅ Matches known limitation |
| BashTool blocked patterns regex | `src/tool/bash.rs:17-66` | ✅ Verified |
| Subprocess PATH via std::env::var_os("PATH") | `src/tool/bash.rs:372-375`, `git.rs:120-123` | ✅ Matches doc |
| SSRF protection via validate_url_host | `src/tool/webfetch.rs:90` | ✅ Matches doc |

## Incorrect / Stale Items

### 1. Missing Tools (Not in Default Registry)

**Issue**: The doc claims 26 tools in `with_defaults()` but omits tools that exist but are registered separately.

**`LspTool`** (`src/tool/lsp.rs`):
- Implements Tool trait with name "lsp"
- Not registered in `with_defaults()`
- Registered nowhere in codebase (appears dead code)
- **Doc action**: Add to "Code Operations" table as "lsp" with note "Not in default registry"

**TeamTools** (`src/tool/teams.rs`):
- Contains: TeamCreateTool, SendMessageTool, ListMessagesTool, TeamStatusTool, ListTeamsTool
- Registered via `TeamTools::register_all()` (separate from default registry)
- **Doc action**: Add new "Team Operations" category with these tools, note "Registered separately via TeamTools::register_all()"

### 2. formatter.rs is Not a Tool

**Issue**: `src/tool/formatter.rs` exists but is a `Formatter` utility struct, not a Tool.

**Doc action**: No change needed (not listed as tool in doc). May want to add note that formatter is a utility, not a tool.

### 3. Security Section Numbering Error

**Issue**: Doc says "7. **Subprocess PATH**" but lists only 6 numbered items before the unnumbered final paragraph.

**Location**: `architecture/tool.md:227-237`

**Doc action**: Fix numbering or merge "Snapshot before modify" and "Subprocess PATH" into single item.

## Minor Issues

### 4. ToolExecutor Constructor Not Documented

**Issue**: Doc shows `ToolExecutor` struct and `execute_with_retry()` but not the `new()` constructor.

**Location**: `src/tool/executor.rs:15-21`

**Doc action**: Add `pub fn new(max_attempts: usize) -> Self` to ToolExecutor section.

## Summary of Line Changes Needed

| Line(s) | Change |
|---------|--------|
| 69-75 | Add "lsp" to Code Operations table (or note it's external) |
| 83-90 | Add "Team Operations" category with team tools |
| 156-176 | Document ToolExecutor::new() constructor |
| 227-237 | Fix numbering in Security Considerations |
