# Tool Architecture Review

## Summary
The tool architecture document is largely accurate but has one tool count discrepancy (claims 27, actual is 26) and could better reflect the actual `ToolExecutor` usage pattern in the codebase.

## Verified Correct
- Tool trait definition at `src/tool/mod.rs:54-60` matches doc
- ToolResult struct at `src/tool/mod.rs:62-68` matches doc
- ToolRegistry::new() at `src/tool/mod.rs:82-87` matches doc
- ToolRegistry::register/get/list/filter_out/definitions at lines 122-157 match doc
- ToolCatalog struct and impl at `src/tool/catalog.rs:37-106` matches doc
- ToolCatalog::deferred_tools() at `catalog.rs:83-89` and is_deferred() at `catalog.rs:96-99` match doc
- validate_path in `src/tool/util.rs:5-20` matches doc
- check_path_for_symlinks in `util.rs:32-51` matches doc
- ToolExecutor with retry logic at `src/tool/executor.rs:8-57` matches doc
- TeamTools registration via `teams.rs:28-37` matches doc

## Discrepancies Found
- **Tool count**: Doc states "27 tools in `with_defaults()`" at line 11, but actual count is **26 tools** (mod.rs:89-119):
  - bash, read, edit, write, glob, grep, list, task, webfetch, websearch, codesearch, question, todo, skill, apply_patch, diff, replace, review, batch, terminal, git, commit, PlanEnterTool, PlanExitTool, invalid, tool_search
- **ToolExecutor not used**: Doc shows ToolExecutor struct and describes retry logic at lines 169-202, but `ToolExecutor` is defined in `executor.rs` and has tests but is **not actually used** by any tool in the registry. The retry logic exists but isn't integrated into the tool execution flow.

## Bugs Identified
- None found - implementation is solid

## Improvement Suggestions
1. **Update tool count**: Change line 11 from "27 tools" to "26 tools"
2. **Document actual retry usage**: Either integrate ToolExecutor into the tool execution flow, or document that retry logic exists but is available for future use via `ToolExecutor::execute_with_retry()`
3. **add_blocked_patterns helper**: BashTool and TerminalTool have significant duplicate code for blocked pattern checking - could extract common functionality to `util.rs`
4. **TeamTools note in tables**: The tables show team tools but don't clearly indicate they're registered via `TeamTools::register_all()` separately, not via `with_defaults()`

## Stale Items in Architecture Doc
- Line 11: "27 tools" should be "26 tools"
- The ToolExecutor section (lines 169-202) describes a struct that exists but isn't used in the actual tool execution path - could be marked as "available but not integrated" or removed if not part of the plan