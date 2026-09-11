# Review: batch9 integrations

**Reviewed**: 2026-09-11
**Files**: mcp.md, lsp.md, plugin.md, hooks.md, skills.md, search_backend.md, ide.md, collaboration.md, presence.md

## Summary

Nine architecture documents covering external integrations (MCP, LSP, plugins, hooks, skills, search backend), IDE support, and collaboration/presence were reviewed against source code. The docs are generally accurate in describing behavior and architecture, but have accumulated stale line-number references as source files have grown, and several count claims are wrong. The collaboration doc has the most significant count errors (CoreRequest/Response/Event variants). The MCP and search_backend docs have the most stale line references due to schema.rs growth (~53-line offset). No phantom types or dead references were found; all referenced files and types exist.

## Documentation Issues

| # | File | Line | Issue | Severity | Suggested fix |
|---|------|------|-------|----------|---------------|
| 1 | lsp.md | 810 | `server_definitions()` claimed to return 40 servers; actual count is 39 | HIGH | Update count to 39 |
| 2 | skills.md | 143 | `SourceKind` claimed to have "9 variants"; actual count is 10 (includes `Plugin = 35` which is missing from the precedence table) | HIGH | Add `Plugin` to the precedence table (rank 35) and update count to 10 |
| 3 | collaboration.md | 49 | Claims "12 CoreRequests" for chat/presence; actual count is 18 (3 Presence + 15 Chat including M003 actions) | HIGH | Update to 18 |
| 4 | collaboration.md | 49 | Claims "8 CoreResponses" for chat/presence; actual count is 13 (3 Presence + 10 Chat including M003) | HIGH | Update to 13 |
| 5 | collaboration.md | 49 | Claims "4 CoreEvents" for chat/presence; actual count is 6 (1 Presence + 5 Chat including `ChatActionUpdated`) | HIGH | Update to 6 |
| 6 | mcp.md | 216 | `McpService` line reference `:109`, actual is `:130` (off by 21) | MEDIUM | Update to `:130` |
| 7 | mcp.md | 218 | `McpClientType` line reference `:91`, actual is `:112` (off by 21) | MEDIUM | Update to `:112` |
| 8 | mcp.md | 219 | `McpExposurePolicy` line reference `:122`, actual is `:144` (off by 22) | MEDIUM | Update to `:144` |
| 9 | mcp.md | 230 | `OAuthManager` line reference `:109`, actual is `:142` (off by 33) | MEDIUM | Update to `:142` |
| 10 | mcp.md | 231 | `TokenSet` line reference `:76`, actual is `:68` (off by -8) | MEDIUM | Update to `:68` |
| 11 | mcp.md | 236 | `parse_mcp_tool_server` line reference `:164`, actual is `:185` (off by 21) | MEDIUM | Update to `:185` |
| 12 | mcp.md | 287 | `McpEntry` config line reference `:881`, actual is `:934` (off by 53) | MEDIUM | Update to `:934` |
| 13 | mcp.md | 288 | `McpServerConfig` config line reference `:889`, actual is `:942` (off by 53) | MEDIUM | Update to `:942` |
| 14 | mcp.md | 289 | `McpReconnectConfig` config line reference `:906`, actual is `:959` (off by 53) | MEDIUM | Update to `:959` |
| 15 | mcp.md | 290 | `McpOAuthConfig` config line reference `:914`, actual is `:969` (off by 55) | MEDIUM | Update to `:969` |
| 16 | mcp.md | 222 | `McpServerStatus` line reference `:73`, actual is `:74` (off by 1) | LOW | Update to `:74` |
| 17 | mcp.md | 224 | `McpResource` line reference `:39`, actual is `:40` (off by 1) | LOW | Update to `:40` |
| 18 | mcp.md | 225 | `McpResourceContent` line reference `:47`, actual is `:48` (off by 1) | LOW | Update to `:48` |
| 19 | mcp.md | 233 | `McpCommand` line reference `:184`, actual is `:185` (off by 1) | LOW | Update to `:185` |
| 20 | mcp.md | 229 | `ConnectionState` line reference `:19`, actual is `:20` (off by 1) | LOW | Update to `:20` |
| 21 | mcp.md | 83 | `serverInfo/version` local.rs reference `:163`, actual is `:164` (off by 1) | LOW | Update to `:164` |
| 22 | ide.md | 25 | `is_vscode()` line reference `:83`, actual is `:81` (off by 2) | LOW | Update to `:81` |
| 23 | ide.md | 30 | `is_jetbrains()` line reference `:89`, actual is `:87` (off by 2) | LOW | Update to `:87` |
| 24 | ide.md | 36 | `is_ide()` line reference `:96`, actual is `:94` (off by 2) | LOW | Update to `:94` |
| 25 | ide.md | 40 | `open_diff()` line reference `:100`, actual is `:98` (off by 2) | LOW | Update to `:98` |
| 26 | ide.md | 77 | `generate_unified_diff()` line reference `:392`, actual is `:390` (off by 2) | LOW | Update to `:390` |
| 27 | ide.md | 81 | `generate_side_by_side()` line reference `:420`, actual is `:418` (off by 2) | LOW | Update to `:418` |
| 28 | ide.md | 161 | `shutdown()` line reference `:300`, actual is `:316` (off by 16) | MEDIUM | Update to `:316` |
| 29 | ide.md | 180 | `open_diff_handler()` line reference `:345`, actual is `:361` (off by 16) | MEDIUM | Update to `:361` |
| 30 | ide.md | 181 | `parse_file_reference()` line reference `:372`, actual is `:388` (off by 16) | MEDIUM | Update to `:388` |
| 31 | ide.md | 68 | `TempFilesGuard` struct line reference `:46`, actual is `:42` (off by 4) | LOW | Update to `:42` (struct) or `:46` (impl) |
| 32 | ide.md | 71 | `register_panic_cleanup()` line reference `:68`, actual is `:66` (off by 2) | LOW | Update to `:66` |
| 33 | hooks.md | 46 | `HookEvent` line reference `:15`, actual is `:16` (off by 1) | LOW | Update to `:16` |
| 34 | hooks.md | 72 | `ShellCommandHook` line reference `:94`, actual is `:93` (off by 1) | LOW | Update to `:93` |
| 35 | hooks.md | 79 | `HookRegistry` line reference `:151`, actual is `:170` (off by 19) | MEDIUM | Update to `:170` |
| 36 | hooks.md | 84 | `from_config()` line reference `:167`, actual is `:185` (off by 18) | MEDIUM | Update to `:185` |
| 37 | hooks.md | 85 | `run_hooks()` line reference `:193`, actual is `:211` (off by 18) | MEDIUM | Update to `:211` |
| 38 | plugin.md | 205 | `PluginService` line reference `:20`, actual is `:24` (off by 4) | LOW | Update to `:24` |
| 39 | plugin.md | 211 | `PluginError` line reference `:530`, actual is `:625` (off by 95) | MEDIUM | Update to `:625` |
| 40 | plugin.md | 156 | `PluginRuntimeSpec` line reference `:46`, actual is `:49` (off by 3) | LOW | Update to `:49` |
| 41 | plugin.md | 163 | `PluginCapability` line reference `:75`, actual is `:78` (off by 3) | LOW | Update to `:78` |
| 42 | plugin.md | 133 | `PluginManager` line reference `:201`, actual is `:256` (off by 55) | MEDIUM | Update to `:256` |
| 43 | skills.md | 124 | `EffectiveSkill` line reference `:32`, actual is `:33` (off by 1) | LOW | Update to `:33` |
| 44 | skills.md | 148 | `AssetDiscoveryConfig` line reference `:84`, actual is `:91` (off by 7) | MEDIUM | Update to `:91` |
| 45 | search_backend.md | 264 | `SearchConfig` config line reference `:463`, actual is `:516` (off by 53) | MEDIUM | Update to `:516` |
| 46 | search_backend.md | 265 | `SearchBackendConfig` line reference `:557`, actual is `:610` (off by 53) | MEDIUM | Update to `:610` |
| 47 | search_backend.md | 266 | `EggsearchConfig` line reference `:568`, actual is `:621` (off by 53) | MEDIUM | Update to `:621` |
| 48 | search_backend.md | 267 | `ToolTimeoutKind` line reference `:584`, actual is `:637` (off by 53) | MEDIUM | Update to `:637` |
| 49 | search_backend.md | 269 | `StructuredSearchResult` line reference `:47`, actual is `:56` (off by 9) | MEDIUM | Update to `:56` |
| 50 | search_backend.md | 270 | `EggsearchCallResult` line reference `:389`, actual is `:399` (off by 10) | MEDIUM | Update to `:399` |
| 51 | search_backend.md | 271 | `BootstrapReport` line reference `:275`, actual is `:306` (off by 31) | MEDIUM | Update to `:306` |
| 52 | search_backend.md | 272 | `CrossProcessLockGuard` line reference `:23`, actual is `:25` (off by 2) | LOW | Update to `:25` |

## Code Issues Found

No code bugs were identified during this review. All referenced types, functions, and modules exist and behave as documented.

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | skills.md | The `Plugin` source kind (rank 35) is not mentioned anywhere in the skills doc despite being a real variant that participates in precedence resolution. Adding it would prevent confusion when plugins contribute skills. | Accuracy |
| 2 | collaboration.md | The collaboration doc's protocol variant counts ("12 CoreRequests, 8 CoreResponses, 4 CoreEvents") are stale since M003 actions were added. Consider generating these counts from code or using a CI check script to prevent drift. | Maintenance |
| 3 | mcp.md | The consistent ~53-line offset in `codegg-config/src/schema.rs` references suggests the schema file has grown significantly since the doc was written. Consider dropping exact line references for config types (which are frequently edited) and instead referencing struct names only. | Readability |
| 4 | ide.md | The `shutdown()`, `open_diff_handler()`, and `parse_file_reference()` line references are off by 16 lines, suggesting code was inserted above these functions. These functions are internal helpers — the line refs could be dropped in favor of function names only. | Readability |
| 5 | lsp.md | The doc is very long (~1000+ lines) and could benefit from splitting the Phase 4 typed DTOs section into a separate `lsp_dto.md` or a dedicated section in the operations module doc. | Maintainability |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | mcp.md | Config type line references (`:881`, `:889`, `:906`, `:914`) | `codegg-config/src/schema.rs` has grown ~53 lines; these will drift again on next schema change |
| 2 | search_backend.md | Config type line references (`:463`, `:557`, `:568`, `:584`) | Same `schema.rs` growth issue |
| 3 | plugin.md | `PluginError` at `:530` and `PluginManager` at `:201` | Both drifted significantly (95 and 55 lines respectively); use struct names only |
