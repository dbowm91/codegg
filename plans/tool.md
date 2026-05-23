# Tool Architecture Review

## Architecture Document
- Path: architecture/tool.md

## Source Code Location
- src/tool/

## Verification Summary
Pass (with minor issues)

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| Tool trait signature (`execute(&self, input: serde_json::Value)`) | Pass | Matches implementation exactly |
| ToolResult struct with `tool_name`, `input`, `output`, `success` fields | Pass | Matches implementation in mod.rs:62-68 |
| ToolRegistry struct with `tools: HashMap` and `catalog: ToolCatalog` | Pass | Matches mod.rs:70-73 |
| ToolRegistry::with_defaults() registers 26 tools | Pass | 26 tools registered in mod.rs:89-119 |
| ToolCatalog struct with `tools: HashMap` and `deferred_load: Vec<String>` | Pass | Matches catalog.rs:37-40 |
| ToolExecutor with retry logic (exponential backoff with jitter) | Pass | Implemented correctly in executor.rs |
| ToolError enum with 8 variants | Pass | Matches error.rs:326-350 |
| ToolError::is_retryable() returns true for Io, Network, Timeout | Pass | Matches error.rs:352-358 |
| Path validation via validate_path() | Pass | Implemented in util.rs with symlink check |
| Symlink protection via check_path_for_symlinks() | Pass | Implemented in util.rs:32-51 |
| Plan tools split: plan_enter and plan_exit are separate | Pass | Two separate structs in plan.rs |
| tool_search registered with catalog for on-demand discovery | Pass | mod.rs:117-118 |
| BashTool blocked patterns regex | Pass | Implemented in bash.rs |
| Subprocess PATH uses user's actual PATH | Pass | Verified in bash.rs:372-376, git.rs, commit.rs, review.rs, formatter.rs |
| SSRF protection in WebFetch | Pass | Uses validate_url_host in webfetch.rs |

## Issues Found

### Bugs

1. **teams.rs not mentioned in documentation**: The `teams.rs` module defines `TeamTools` struct with 5 sub-tools (team_create, send_message, list_messages, team_status, list_teams) that are NOT documented in architecture/tool.md. These tools are registered via `register_all()` method but never appear in the Built-in Tools table.

2. **lsp.rs not mentioned in documentation**: The `lsp.rs` tool is completely undocumented in architecture/tool.md. It exists as a real tool ("lsp") with operations like goToDefinition, findReferences, hover, etc.

3. **formatter.rs not mentioned in documentation**: The `formatter.rs` module provides file formatting capabilities but is not listed as a tool. It appears to be used internally by write.rs for auto-formatting rather than being a standalone tool.

### Inconsistencies

1. **Tool count mismatch**: Architecture claims "26 tools in `with_defaults()`" but the table shows 28 entries (including lsp, formatter, teams which are not actually in with_defaults). Actual count is 26 as verified from code.

2. **teams.rs tools not in registry by default**: Team tools (team_create, send_message, list_messages, team_status, list_teams) require explicit registration via `TeamTools::register_all()` and are not part of the default 26 tools.

3. **ToolSearchTool constructor takes Arc<ToolCatalog>**: Architecture doesn't document the ToolSearchTool's dependency on ToolCatalog.

### Missing Documentation

1. **lsp tool**: Full LSP tool with 11 operations undocumented.

2. **teams tools**: 5 team-related tools completely missing from documentation.

3. **formatter module**: Auto-formatting integration with write tool not documented.

4. **ToolCatalog::search()**: Documents case-insensitive search but implementation uses to_lowercase() which may have Unicode implications.

5. **ToolCatalog::defer_load field**: The `defer_load` field on ToolMetadata is set to `false` always (from_tool method), making deferred loading infrastructure present but unused.

6. **ToolExecutor::calculate_delay()**: Documents "exponential backoff with jitter" but the jitter calculation is `capped_ms / 2` which is deterministic (not random jitter).

### Improvement Opportunities

1. **ToolExecutor jitter is deterministic**: The "jitter" in executor.rs:54 is actually just an extra delay component (`capped_ms / 2`), not random jitter. This should be clarified in documentation or changed to use actual random jitter.

2. **Tool definition caching**: Architecture mentions "Cache key includes version for proper invalidation" but I did not find caching logic in the src/tool/ directory - this appears to be in the provider layer.

3. **teams.rs tools are not part of default registry**: The TeamTools require separate registration and a TeamManager instance, suggesting they are optional/conditional. This should be documented.

4. **Configuration schema not loaded**: The architecture shows a `[tools]` config section but I did not verify if this configuration actually affects tool behavior (path_rules, allowed/denied tools).

## Recommendations

1. Update architecture/tool.md to include lsp, teams, and formatter tools in the Built-in Tools table.
2. Correct tool count claim to reflect actual 26 tools in with_defaults() and clarify that team tools require separate registration.
3. Document ToolSearchTool's ToolCatalog dependency.
4. Clarify whether ToolExecutor uses true random jitter or deterministic delay doubling.
5. Document the defer_load field purpose or remove unused infrastructure.
6. Add section explaining team tools are optional and require TeamManager.
7. Verify and document [tools] configuration schema implementation status.
