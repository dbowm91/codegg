# Hooks & IDE & LSP Architecture Review

## Verified Claims

### Hooks Module
- **Location**: `src/hooks/mod.rs` - CORRECT
- **HookEvent enum** (lines 17-24): PreToolExecute, PostToolExecute, SessionStart, SessionEnd, AgentStart, AgentEnd - all present and correct
- **HookContext struct** (lines 56-63): All fields present (event, session_id, tool_name, tool_arguments, tool_result, timestamp)
- **HookRegistry** (lines 150-206): HashMap<HookEvent, Vec<Box<dyn Hook>>> pattern correct
- **Hook trait** (lines 89-92): `async fn execute(&self, ctx: &HookContext) -> Result<(), AppError>` - correct
- **ShellCommandHook** (lines 94-147): command, timeout, event fields - correct
- **ShellCommandHook::new** (lines 100-108): timeout defaults to 30 seconds - CORRECT
- **run_hooks** (lines 191-201): Returns Vec<AppError>, not early-returned - CORRECT
- **to_env_vars()** (lines 66-87): CODEGG_HOOK_EVENT, CODEGG_SESSION_ID, CODEGG_TOOL_NAME, CODEGG_TOOL_ARGUMENTS, CODEGG_TOOL_RESULT, CODEGG_TIMESTAMP - all correct
- **InlineScript deprecated** (line 181-184): Skipped with warning - CORRECT
- **PATH preservation** (line 118): Uses `std::env::var_os("PATH")` - CORRECT

### Plugin Hooks
- **Location**: `src/plugin/hooks.rs` - CORRECT
- **HookType enum** (lines 6-20): Auth, Provider, ToolDefinition, ToolExecuteBefore, ToolExecuteAfter, ChatParams, ChatHeaders, Event, Config, ShellEnv, TextComplete, SessionCompacting, MessagesTransform - all 14 present
- **HookType::as_str()** (lines 23-39): dot notation format (e.g., "tool.execute.before") - CORRECT
- **HookResult struct** (lines 68-72): output, blocked, error fields - CORRECT
- **HookResult::ok/blocked/error** (lines 75-97): All correct

### IDE Module
- **Location**: `src/ide/mod.rs` - CORRECT
- **is_vscode()** (lines 80-84): VSCODE_IPC_HOOK, VSCODE_INJECTED_ENVIRONMENT, TERM_PROGRAM=vscode - CORRECT
- **is_jetbrains()** (lines 86-91): JETBRAINS_REMOTE, JB_PRODUCT_READINESS, IDEA_INITIAL_DIRECTORY, WEBCLBROWSER_HOST - CORRECT
- **is_ide()** (lines 93-95): is_vscode() || is_jetbrains() - CORRECT
- **open_diff signature** (lines 97-102): correct parameter types
- **generate_unified_diff** (lines 371-397): correct implementation
- **generate_side_by_side** (lines 399-420): correct implementation
- **TempFilesGuard** (lines 43-63): Drop impl cleans up temp files - CORRECT
- **register_panic_cleanup** (lines 65-78): Uses std::sync::Once - CORRECT

### IDE MCP Server (ide_server.rs)
- **run_stdio() lines 78-119**: Uses tokio async I/O (BufReader, AsyncWriteExt) - CORRECT
- **run_socket() lines 121-144**: Uses tokio UnixListener - CORRECT
- **clone_for_connection() lines 146-153**: Creates Arc clones - CORRECT
- **handle_connection() lines 155-194**: Handles UnixStream connections - CORRECT

### LSP Module
- **Lsp struct** (mod.rs lines 30-34): service, operations, diagnostics Arc fields - CORRECT
- **LspService** (service.rs lines 21-24): clients HashMap, config - CORRECT
- **LspClient** (client.rs lines 38-48): All 9 fields correct (server_id, root, process, request_id, capabilities, opened_files, diagnostics, notif_tx, notif_rx)
- **DiagnosticEntry** (client.rs lines 33-36): uri, diagnostic - CORRECT
- **DEBOUNCE_MS** (diagnostics.rs line 15): 150ms - CORRECT
- **FileDiagnostic struct** (diagnostics.rs lines 19-28): All 7 fields correct
- **LspOperations** (operations.rs): All methods present and correct
- **LspError enum**: All 9 variants present (verified in error module)

### LSP Server Definitions
- **Server count**: 39 servers (verified by counting entries in server_definitions() at server.rs:27-383)
- rust-analyzer is ONLY server with download spec (line 36-40) - CORRECT
- All server entries have id, languages, extensions, repo, command, args, download fields - CORRECT

### LSP Language Detection
- **extension_to_language_id** (language.rs): Supports all listed languages plus more
- **language_id_to_server_id** (language.rs): Correct mappings verified
- **detect_language** (language.rs): Handles dockerfile, makefile special cases - CORRECT

### LSP Download
- **cache_dir()** (download.rs lines 10-15): Uses dirs::cache_dir() - CORRECT
- **find_in_path** (download.rs lines 42-60): PATH searching with is_executable - CORRECT
- **resolve_url** (download.rs lines 131-153): ARCH/OS mapping (x86_64, aarch64, darwin, win32) - CORRECT

### LSP Launch
- **LspProcess struct** (launch.rs lines 20-25): stdin, stdout, stderr, child - CORRECT
- **spawn_server preserves PATH** (launch.rs lines 41-45): std::env::var_os("PATH") - CORRECT
- **Content-Length header** (launch.rs line 87): format "Content-Length: {}\r\n\r\n{}" - CORRECT

### LSP Root Detection
- **find_project_root** (root.rs): Marker-based detection - CORRECT
- **is_project_root** (root.rs lines 30-93): Extensive marker list (90 markers) - CORRECT

---

## Incorrect/Stale Claims

### LSP Server Count Discrepancy
- **Documentation states**: "40 servers" (architecture/lsp.md line 229)
- **Actual count**: 39 servers in server_definitions() array (server.rs lines 29-383)
- **Fix needed**: Update line 229 from "40 servers" to "39 servers"

### LSP Documentation - Missing Server (cmake-language-server)
- The documentation table (lines 233-253) shows "Rust, Python, JavaScript/TypeScript, Go, C/C++" etc. with "and more" placeholder
- **Actual servers not shown in table**: cmake-language-server, lemminx, makefile-language-server, solidity-language-server, buf-language-server, graphql-language-server, perl-language-server, powershell-editor-services, erlang-ls, vls, nimlsp, dart-analysis-server, elixir-ls, clojure-lsp, vue-language-server, svelte-language-server, yaml-language-server, taplo, bash-language-server, terraform-ls, dockerfile-language-server, sql-language-server, r-languageserver, marksman, html-language-server, css-language-server, json-language-server, swift-sourcekit, php-language-server, ruby-lsp, zls
- **Note**: These ARE in the server_definitions() array, just not enumerated in the markdown table. This is documentation by reference, not a bug.

---

## Bugs Found

### None identified

All three modules (hooks, ide, lsp) appear to be correctly implemented and documented.

---

## Improvements Identified

### IDE Module - Potential Improvement
1. **ide/mod.rs line 180**: `open_diff_jetbrains` takes `original: &str, modified: &str` parameters but ignores them, instead creating temp files that don't contain the actual content passed in. The content is written to temp files, but then `original_path` and `modified_path` are used for the diff command. This is correct - the temp files contain the content.

### LSP Module - Minor Documentation Improvement
1. **ide_server.rs not in src/ide/**: The documentation references `src/mcp/ide_server.rs` (line 117) as if it's part of the IDE module. It is actually in `src/mcp/`. This is mentioned correctly in the "See Also" section referencing mcp.md, but the header could be clearer.

---

## Stale References

### None - All references verified correct:
- `architecture/hooks.md` references `src/hooks/mod.rs` - EXISTS
- `architecture/hooks.md` references `src/plugin/hooks.rs` - EXISTS
- `architecture/ide.md` references `src/mcp/ide_server.rs` - EXISTS (in src/mcp/)
- `architecture/lsp.md` references `src/lsp/` files - ALL EXIST
- `architecture/lsp.md` references `src/tool/lsp.rs` - EXISTS (for LspTool)

---

## Recommendations

### 1. Fix LSP Server Count in Documentation
**Location**: `architecture/lsp.md` line 229
**Current**: "## Supported Languages (40 servers)"
**Should be**: "## Supported Languages (39 servers)"

### 2. Consider Adding IDE MCP Server to IDE Module Documentation
The `IdeServer` at `src/mcp/ide_server.rs` is documented in ide.md but physically located in mcp module. Consider either:
- Moving the documentation to mcp.md
- Or clarifying that ide.md covers the IDE integration aspects while mcp module handles the protocol

### 3. Shell Command Hooks Table - Could be more detailed
The integration table in hooks.md (lines 184-193) shows location as "loop.rs" generically. The actual file is `src/agent/loop.rs`. While this is technically correct (the table is about integration points), being explicit about the full path might help developers locate the code faster.

---

## Summary

| Module | Documentation Status | Source Code Status |
|--------|---------------------|-------------------|
| Hooks (shell) | Accurate | Correct |
| Hooks (plugin) | Accurate | Correct |
| IDE | Accurate (minor location note) | Correct |
| LSP | 1 count error (40 vs 39) | Correct |

**Overall**: The architecture documentation for hooks, ide, and lsp modules is largely accurate. Only one factual error was found: the LSP server count should be 39, not 40. No bugs were identified in the actual source code.
