# Tool Module Architecture Review

## Verified Claims

### Tool Trait (mod.rs:54-60)
- `#[async_trait]` pattern matches documentation
- `name()`, `description()`, `parameters()`, `execute()` signatures all match
- Tool receives only `serde_json::Value` (NOT `ToolContext`) - **documentation is correct**

### ToolResult (mod.rs:62-68)
- Fields: `tool_name`, `input`, `output`, `success` - **matches documentation**

### ToolRegistry (mod.rs:70-157)
- `tools: HashMap<String, Box<dyn Tool>>` - **matches**
- `catalog: catalog::ToolCatalog` - **matches**
- All documented methods exist and have correct signatures:
  - `new()`, `with_defaults()`, `register()`, `get()`, `list()`, `definitions()`, `filter_out()`, `catalog()`

### ToolCatalog (catalog.rs)
- `tools: HashMap<String, ToolMetadata>` - **matches**
- `deferred_load: Vec<String>` - **matches**
- Methods match: `register()`, `search()`, `get()`, `list()`
- Additional undocumented methods found: `deferred_tools()`, `is_deferred()`

### ToolExecutor (executor.rs:8-56)
- Fields: `max_attempts`, `base_delay`, `max_delay` - **matches**
- `execute_with_retry()` signature and retry logic - **matches**
- Exponential backoff with jitter calculation - **matches**

### ToolError (error.rs:323-356)
- All 8 variants match documentation: `NotFound`, `Execution`, `Timeout`, `Permission`, `Format`, `Disabled`, `Io`, `Network`
- `is_retryable()` implementation - **matches** (returns true for `Io`, `Network`, `Timeout`)

### Built-in Tools (mod.rs:89-119)
- 26 tools registered in `with_defaults()` - **BUT documentation claims 33+**
- `plan_enter` and `plan_exit` are **separate tools** - **matches** (lines 113-114)

### Path Validation (util.rs)
- `validate_path()` - **matches** documentation
- Symlink check via `check_path_for_symlinks()` - **matches**
- `canonicalize_path()` - **matches**

### Security Features
- BashTool BLOCKED_PATTERN regex - **matches** "Regex-based blocked patterns" claim
- BashTool uses `std::env::var_os("PATH")` (line 372) - **matches** "Subprocess PATH" claim
- WebFetchTool uses `validate_url_host()`, `validate_host_ip()`, `revalidate_dns()` for SSRF protection - **matches**

### plan.rs
- `PlanEnterTool` and `PlanExitTool` are separate structs - **matches documentation**
- `detect_plan_mode_change()` function exists and returns `PlanModeChange` enum - **undocumented but correct**

---

## Bugs/Discrepancies Found

### 1. Tool Definition Caching Claim is Inaccurate (medium priority)

**File**: architecture/tool.md line 251
> "**Tool definition caching**: Cache key includes version for proper invalidation"

**Actual**: Looking at `ToolRegistry::definitions()` (mod.rs:148-157), there is no explicit caching mechanism visible. The tool definitions are generated on demand from `self.tools.values()`. While the skill mentions "proper invalidation", the actual implementation does not show version-based cache keys.

### 2. Skill doc shows `deferred_tools()` but architecture doesn't mention it (low priority)

**File**: .opencode/skills/tool/SKILL.md line 129
```rust
pub fn deferred_tools(&self) -> Vec<&ToolMetadata>;
```

**Actual**: `ToolCatalog` has `deferred_tools()` method (catalog.rs:84-89) but `architecture/tool.md` only shows 4 methods (`register`, `search`, `get`, `list`) without mentioning deferred loading functionality.

### 3. Documentation lists 33+ tools but only 26 exist (medium priority - MISMATCH)

**File**: architecture/tool.md line 11
> "Built-in tool implementations (33+ tools)"

**Actual**: `ToolRegistry::with_defaults()` registers **exactly 26 tools**:
1. BashTool
2. ReadTool
3. EditTool
4. WriteTool
5. GlobTool
6. GrepTool
7. ListTool
8. TaskTool
9. WebFetchTool
10. WebSearchTool
11. CodeSearchTool
12. QuestionTool
13. TodoTool
14. SkillTool
15. ApplyPatchTool
16. DiffTool
17. ReplaceTool
18. ReviewTool
19. BatchTool
20. TerminalTool
21. GitTool
22. CommitTool
23. PlanEnterTool
24. PlanExitTool
25. InvalidTool
26. ToolSearchTool

Note: The architecture's Built-in Tools table also doesn't list `tool_search`, `invalid`, `codesearch`, `terminal`, or `lsp` (which exists but isn't registered).

### 4. `lsp` tool documented but NOT registered (medium priority - MISMATCH)

**File**: architecture/tool.md line 75
> "| **lsp** | `lsp.rs` | LSP tool wrapper |"

**Actual**: `LspTool` exists in `lsp.rs` with full `Tool` implementation, BUT it is **NOT registered** in `ToolRegistry::with_defaults()`. The tool requires an `LspService` in its constructor, so it must be instantiated separately with proper service configuration. This appears to be intentional (LSP needs service injection) but the architecture lists it as a built-in tool.

### 5. `terminal` tool in architecture table (line 65) matches but with wrong name context

**File**: architecture/tool.md line 65
> "| **terminal** | `terminal.rs` | Terminal operations |"

**Actual**: `TerminalTool` is registered and works, but it's for executing terminal commands similar to `BashTool`. The description "Terminal operations" is vague.

### 6. Architecture doesn't document `ToolSearchTool` (medium priority)

**File**: architecture/tool.md (general)

**Actual**: `ToolSearchTool` (registered at mod.rs:117) provides on-demand tool discovery. It's not listed in the Built-in Tools table under External Integrations (which only shows `question`, `skill`, `batch`, `tool_search`).

### 7. `InvalidTool` not documented (low priority)

**File**: architecture/tool.md (general)

**Actual**: `InvalidTool` (registered at mod.rs:115) handles invalid tool call requests. Not listed in the Built-in Tools table.

---

## Improvement Suggestions

### High Priority

1. **Update tool count claim**: Change "33+ tools" to accurate count (~26) or "26 built-in tools" and update the table to be exhaustive.

2. **Document `lsp` tool availability**: Either register `lsp` in `ToolRegistry::with_defaults()` if it should be available, or remove it from the architecture table.

### Medium Priority

3. **Add `deferred_tools()` and `is_deferred()` to architecture doc**: These methods exist in `ToolCatalog` but aren't documented.

4. **Investigate tool definition caching**: Either remove the caching claim from the architecture or implement proper version-based cache invalidation.

5. **Document `ToolSearchTool`**: Add this tool to the External Integrations table since it provides on-demand tool discovery.

### Low Priority

6. **Document `InvalidTool`**: Add to table or explain its purpose.

7. **Clarify `terminal` tool description**: "Terminal operations" is vague. Consider "Execute terminal commands (interactive)" similar to bash.

8. **Add `detect_plan_mode_change()` and `PlanModeChange` to documentation**: These are public API elements used by other parts of the system but not documented in the architecture.

9. **Update Skill doc**: The skill doc at `.opencode/skills/tool/SKILL.md` is more complete (includes `deferred_tools()`) - sync these improvements back to `architecture/tool.md`.

---

## Summary

The core architecture is **accurate** - the Tool trait, ToolRegistry, ToolCatalog, ToolExecutor, ToolError, and security features all match the implementation. Main issues are:

- Tool count is overstated (33+ vs 26)
- `lsp` tool listed but not registered
- `deferred_tools()`/`is_deferred()` methods undocumented
- `ToolSearchTool` and `InvalidTool` not in documentation

The codebase is well-structured and the documentation is generally accurate for the parts that are documented.