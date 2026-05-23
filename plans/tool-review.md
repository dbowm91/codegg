# Tool Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| Tool trait signature: `async fn execute(&self, input: serde_json::Value)` | VERIFIED | `src/tool/mod.rs:59` - matches exactly |
| ToolResult has fields: tool_name, input, output, success | VERIFIED | `src/tool/mod.rs:63-68` - matches exactly |
| ToolRegistry has tools HashMap and catalog ToolCatalog | VERIFIED | `src/tool/mod.rs:70-73` - matches |
| ToolCatalog has tools HashMap and deferred_load Vec | VERIFIED | `src/tool/catalog.rs:37-40` - matches |
| plan_enter and plan_exit are separate tools | VERIFIED | `src/tool/plan.rs` - PlanEnterTool and PlanExitTool are distinct structs |
| ToolExecutor provides retry with exponential backoff | VERIFIED | `src/tool/executor.rs:29-56` - backoff with jitter implemented |
| ToolError::is_retryable matches Io/Network/Timeout | VERIFIED | `src/error.rs:349-354` - matches exactly |
| Path validation checks symlinks and allowed_root | VERIFIED | `src/tool/util.rs:5-20` - validates symlinks then checks canonical starts with root |
| BashTool blocked patterns regex-based detection | VERIFIED | `src/tool/bash.rs:17-67` - comprehensive regex patterns |
| Subprocess PATH uses user's actual PATH | VERIFIED | `src/tool/bash.rs:372-375`, `git.rs:120-123`, `terminal.rs:286-289` - all use `std::env::var_os("PATH")` |
| Built-in tools count 33+ | VERIFIED | `src/tool/mod.rs:89-119` - 30 tools registered, plus team tools (5) = 35 total |
| Tool definition caching with version | VERIFIED | `mod.rs:148-157` definitions() uses mcp_tool_count as version proxy (see known limitation in AGENTS.md) |
| Built-in Tools Table (10 File Operations) | VERIFIED | read, write, edit, glob, grep, list, diff, replace, multiedit, apply_patch all exist |
| Built-in Tools Table (4 Shell Execution) | VERIFIED | bash, terminal, git, commit all exist |
| Built-in Tools Table (3 Code Operations) | VERIFIED | codesearch, review, lsp all exist |
| Built-in Tools Table (2 Web Operations) | VERIFIED | webfetch, websearch both exist |
| Built-in Tools Table (Task Management) | VERIFIED | task, todo, plan_enter, plan_exit all exist (5 tools including batch, skill, tool_search) |
| Security: SSRF protection in WebFetch | VERIFIED | `src/tool/webfetch.rs:90-103` - validates host IP and revalidates DNS |
| Security: BashTool blocked patterns | VERIFIED | `src/tool/bash.rs:17-67` - 40+ patterns including command injection vectors |
| ToolCatalog.search() returns tools matching name or description | VERIFIED | `src/tool/catalog.rs:66-76` - case-insensitive search on both fields |

### Unverified / Incorrec

| Claim | Status | Evidence |
|-------|--------|----------|
| Document says "33+ tools" | SLIGHTLY INACCURATE | Actually 30 built-in tools registered in with_defaults(), plus team tools makes 35 |
| ToolCatalog.deferred_load documented but deferred loading not fully wired | INCORRECT | ToolCatalog has defer_load field but nothing actually sets it to true; no actual deferred loading mechanism |

## Bugs Found

### Critical

1. **GrepTool: Missing permission check for denied_paths**
   - `src/tool/grep.rs:140-148` - Files are validated after WalkDir traversal, but there's no `allowed_paths` configuration like bash.rs has
   - Any tool with `allowed_paths` restriction is bypassed in grep

2. **GlobTool: Same issue - no allowed_paths enforcement**
   - `src/tool/glob.rs:128-129` - Only checks if path starts with canonical_search, but `allowed_paths` field doesn't exist on GlobTool
   - User can configure allowed_paths in Config but GlobTool ignores it

### High

3. **BashTool: allowlist bypass via path traversal in blocked check**
   - `src/tool/bash.rs:176-186` - Blocked command check uses `normalized.starts_with(blocked_cmd)` which can be bypassed with `echo "rm -rf /" #`
   - Comments after blocked command would bypass the check

4. **ReplaceTool: Reports "all" instead of actual match count**
   - `src/tool/replace.rs:195-198` - Always prints "Replaced all occurrence(s)" even when printing the hardcoded string "all" instead of actual count
   - `_matches_len` is computed but never used in output

5. **EditTool: Missing FileChanged event on edit failure**
   - `src/tool/edit.rs:143-148` - FileChanged event is published even if the edit failed (the spawn_blocking returned Ok but try_edit returned None)
   - Only publishes on success path

6. **ListTool: No symlink check in validate_path helper**
   - `src/tool/list.rs:46-67` - Custom validate_path doesn't call `check_path_for_symlinks()` unlike all other file tools
   - Symlink traversal protection inconsistent

### Medium

7. **WriteTool: Auto-formatting not actually called**
   - `src/tool/write.rs:131-132` - Reads file back after write but never calls Formatter
   - Comment says "Runs auto-formatting after write" but code just reads content

8. **WebFetchTool: Cloudflare retry uses wrong User-Agent**
   - `src/tool/webfetch.rs:142` - Retry uses Chrome UA but still gets blocked if original was Cloudflare-protected
   - Should use actual Chrome browser headers properly

9. **TerminalTool: Missing workdir validation against allowed_paths**
   - `src/tool/terminal.rs:297-299` - Sets workdir but no validation that it falls within allowed paths
   - Inconsistent with BashTool which validates workdir

10. **CodeSearchTool: Input sanitization removes too many characters**
    - `src/tool/codesearch.rs:60-66` - Removes single/double quotes which may be needed in code queries
    - Query `class "MyClass"` becomes `class MyClass` which changes semantics

## Improvement Suggestions

### Performance

1. **GrepTool: Batch processing could use bounded parallelism**
   - `src/tool/grep.rs:167-234` - Uses `join_all` on chunks which could spawn unlimited futures
   - Consider using `futures::stream` with bounded concurrency

2. **ToolRegistry::definitions() allocates on every call**
   - `src/tool/mod.rs:148-157` - Creates new Vec with ToolDefinitions each time
   - Should cache definitions and invalidate on tool registration changes

3. **ReadTool: spawn_blocking for all ops including small reads**
   - `src/tool/read.rs:211-301` - Even trivial reads go through spawn_blocking
   - Could use `tokio::fs` directly for async I/O

### Correctness

4. **ToolError::NotFound used inconsistently**
   - Some tools return `ToolError::NotFound("tool_name")` (string as arg)
   - Others return `ToolError::Execution("unknown tool: name")`
   - Should standardize NotFound variant usage

5. **WebSearchTool: API key env var names inconsistent**
   - `src/tool/websearch.rs:30` uses `EXA_API_KEY`
   - `src/tool/codesearch.rs:74-76` checks `EXA_API_KEY` then `EXA_CODE_API_KEY`
   - Should use consistent naming

6. **BatchTool: Tool name validation regex is too permissive**
   - `src/tool/batch.rs:86-93` allows alphanumeric, underscore, hyphen
   - But `ToolRegistry::get()` does exact match, so this is fine actually

### Maintainability

7. **Duplicated blocked_pattern regex in bash.rs and terminal.rs**
   - `src/tool/bash.rs:17-67` and `src/tool/terminal.rs:31-78` have nearly identical patterns
   - Should extract to shared `tool::util` module

8. **Duplicated truncate_output function in bash.rs and terminal.rs**
   - `src/tool/bash.rs:421-441` and `src/tool/terminal.rs:338-358` are identical
   - Should be shared utility

9. **Inconsistent error messages across tools**
   - Some tools say "missing 'X' parameter", others "X is required", others use different formats
   - Consider standardizing error message format

10. **Tool constructors inconsistent**
    - Some tools have `new()`, `with_*()` builder pattern (BashTool)
    - Others only have `new()` with Default (ReadTool)
    - Makes it hard to configure tools uniformly

## Priority Actions (top 5 items to fix)

1. **Fix replace.rs to report actual match count** (High) - Currently always prints "all" instead of the actual number
2. **Add allowed_paths to GlobTool and GrepTool** (Critical) - These tools bypass path restrictions configured by user
3. **Extract duplicated blocked_pattern regex to shared util** (Medium) - Maintenance issue, keep in sync
4. **Extract duplicated truncate_output to shared util** (Medium) - Maintenance issue, duplicate code
5. **Add symlink check to ListTool::validate_path** (Medium) - Inconsistent with other file tools

## Additional Observations

### Architecture Accuracy
The architecture document is largely accurate. The main discrepancies are:
- Tool count is actually 35 (including team tools), not "33+"
- deferred_load mechanism exists in catalog but is not actually used by any tool

### Security Notes
- Overall security is strong: symlink protection, SSRF protection, blocked patterns for bash
- Main gaps are in allowed_paths enforcement for glob/grep/list tools
- PATH handling correctly uses user's actual PATH in all shell-invoking tools

### Code Quality
- Good use of spawn_blocking for filesystem operations
- Consistent error handling with ToolError enum
- Good test coverage in executor.rs, apply_patch.rs
- Some tools (review, commit) call Config::load() directly which may not respect hot-reload