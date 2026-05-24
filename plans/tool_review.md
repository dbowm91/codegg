# Tool Module Architecture Review

**Date**: 2026-05-24  
**Reviewer**: Code review  
**Files Reviewed**:
- `architecture/tool.md` (262 lines)
- `src/tool/mod.rs` (158 lines)
- `src/tool/catalog.rs` (106 lines)
- `src/tool/executor.rs` (124 lines)
- `src/tool/util.rs` (51 lines)
- `src/tool/invalid.rs` (50 lines)
- `src/tool/plan.rs` (130 lines)
- `src/tool/bash.rs` (442 lines)
- `src/tool/glob.rs` (157 lines)
- `src/tool/grep.rs` (279 lines)
- `.opencode/skills/tool/SKILL.md` (535 lines)

---

## Summary

The tool module is well-implemented and generally well-documented. The architecture document and skill guide are mostly accurate, with only minor discrepancies found. The core Tool trait, ToolRegistry, ToolCatalog, ToolExecutor, and path validation are all correctly implemented.

**Verdict**: No significant bugs found. Documentation is accurate with minor inconsistencies noted below.

---

## Verified Correct Items

### 1. Tool Trait (mod.rs:54-60)
- `#[async_trait]` with `Send + Sync` bounds
- `name()`, `description()`, `parameters()`, `execute(input: Value)` - all correct
- **Confirmed**: Tools receive only `serde_json::Value`, NOT `ToolContext`

### 2. ToolResult (mod.rs:62-68)
```rust
pub struct ToolResult {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: String,
    pub success: bool,
}
```
- **Accurate** - matches code exactly

### 3. ToolRegistry (mod.rs:70-157)
- `tools: HashMap<String, Box<dyn Tool>>` - correct
- `catalog: catalog::ToolCatalog` - correct
- Methods: `new()`, `with_defaults()`, `register()`, `get()`, `catalog()`, `list()`, `filter_out()`, `definitions()` - all implemented correctly

### 4. ToolCatalog (catalog.rs:36-100)
- `tools: HashMap<String, ToolMetadata>` - correct
- `deferred_load: Vec<String>` - correct
- Methods: `new()`, `register()`, `search()`, `get()`, `deferred_tools()`, `list()`, `is_deferred()` - all implemented correctly

### 5. ToolExecutor (executor.rs:8-56)
- Uses exponential backoff with jitter
- `calculate_delay()`: `base_ms * 2^(attempt-1)` with `/2` jitter
- Correctly retries only `is_retryable()` errors
- **Verified**: base_delay=500ms, max_delay=30s

### 6. Path Validation (util.rs:5-51)
- `validate_path()` checks symlinks, canonicalizes, ensures path starts with allowed_root
- `canonicalize_path()` checks symlinks then canonicalizes
- `check_path_for_symlinks()` - checks each path component

### 7. ToolError (src/error.rs:326-358)
```rust
pub enum ToolError {
    NotFound(String),     // line 328
    Execution(String),   // line 331
    Timeout(String),     // line 334
    Permission(String),  // line 337
    Format(String),      // line 340
    Disabled(String),    // line 343
    Io(String),          // line 346
    Network(String),     // line 349
}
```
- **Accurate** - 8 variants
- `is_retryable()` matches `Io | Network | Timeout` - correct

### 8. Built-in Tools Count
- **Document claims 26 tools** in `with_defaults()`
- **Actual count**: 26 registers confirmed (bash, read, edit, write, glob, grep, list, task, webfetch, websearch, codesearch, question, todo, skill, apply_patch, diff, replace, review, batch, terminal, git, commit, plan_enter, plan_exit, invalid, tool_search) - **correct**

### 9. plan_enter and plan_exit (plan.rs)
- **Confirmed**: Two separate tools, not one `plan` tool
- `PlanEnterTool` at line 12, `PlanExitTool` at line 63
- Registered separately at mod.rs:113-114

### 10. invalid tool (invalid.rs)
- **Confirmed**: `InvalidTool` exists and is registered
- Handles malformed tool calls, not unregistered tools (which return None from `get()`)

### 11. Subprocess PATH Security
- All tools use `std::env::var_os("PATH")` after `env_clear()`:
  - bash.rs:372
  - commit.rs:38, 64, 170
  - formatter.rs:51
  - git.rs:120
  - review.rs:33
  - terminal.rs:286
- **Confirmed working** per AGENTS.md notes

### 12. Teams and LSP Tools
- `TeamTools` in teams.rs - exists, not in defaults (separate registration)
- `LspTool` in lsp.rs - exists, not in defaults (separate registration)
- Both correctly documented in SKILL.md as "Extended Tool Modules"

---

## Discrepancies Found

### Discrepancy 1: `ToolCatalog::search()` signature
- **Doc** (architecture/tool.md:136): `pub fn search(&self, query: &str) -> Vec<&ToolMetadata>;`
- **Skill** (.opencode/skills/tool/SKILL.md:126): Same signature
- **Code** (catalog.rs:66): `pub fn search(&self, query: &str) -> Vec<&ToolMetadata>`
- **Status**: Correct

### Discrepancy 2: Missing `InvalidInput` error variant
- **Architecture doc** (line 181-198): Shows `ToolError` variants, but `InvalidInput` is NOT listed
- **Skill doc** (lines 384-408): Same - does NOT list `InvalidInput`
- **Code** (src/error.rs:326-350): Does NOT have `InvalidInput` variant
- **Status**: Not a bug - architecture and skill are correct to not list `InvalidInput`
- **Note**: In `invalid.rs:42`, the code returns `ToolError::Execution("invalid tool input: ...")` for parse failures

### Discrepancy 3: Document says 26 tools in `with_defaults()`
- **Verified**: 26 tools registered (lines 91-118 in mod.rs)
- **Status**: Correct

### Discrepancy 4: `glob.rs` and `grep.rs` `unrestricted` handling
- **Architecture doc** mentions unrestricted mode for trusted environments
- **Skill doc** (line 491): "Unrestricted Mode - For trusted environments only; skips validation"
- **Code**: Both tools have `unrestricted: bool` field that skips path validation when true
- **Issue**: `with_defaults()` creates tools with `unrestricted=false` (glob.rs:23, grep.rs:31)
- **Status**: Correct implementation, just not exposed via config in defaults

---

## Minor Documentation Issues

### Issue 1: Architecture doc "Configuration" section (lines 241-249)
Shows TOML config for `[tools]` with `allowed`, `denied`, `path_rules` but this configuration is NOT actually loaded by the tool module. The tool registry just has `filter_out()` for denying tools, but no path_rules configuration exists in the tool module.

**Reference**: No `path_rules` configuration is loaded in `ToolRegistry::with_defaults()` or elsewhere

### Issue 2: Skill says "25+ total" tools (line 32)
- **Skill says**: "25+ total"
- **Actual**: 26 tools in `with_defaults()`
- **Severity**: Minor - should say "26 total"

### Issue 3: Architecture doc says ToolCatalog has `deferred_load` field (line 131)
- **Doc says**: `deferred_load: Vec<String>`
- **Code**: Correct - `deferred_load: Vec<String>` at catalog.rs:39
- **Status**: Correct

---

## No Bugs Found

The following items were verified as NOT bugs after review:

1. **Tool definition caching**: Cache key version noted correctly
2. **Plan tools split**: Verified two separate tools
3. **ToolCatalog metadata**: Correctly tracks tools separately from registry
4. **BashTool security patterns**: All regex patterns documented
5. **PATH handling**: All tools use `env_clear()` + user PATH

---

## Recommendations

### For Documentation

1. **Update architecture/tool.md "Configuration" section**: Either remove or document that `[tools]` path_rules configuration is NOT implemented. The actual configuration is in `permission` module.

2. **Update SKILL.md line 32**: Change "25+ total" to "26 total"

3. **Add note about `unrestricted` mode**: Document that `GlobTool::with_allowed_root()` and `GrepTool::with_allowed_root()` set `unrestricted=false`. The unrestricted mode is available but not exposed through the default registry.

### For Code

No code changes recommended. The implementation is correct.

---

## Conclusion

The tool module implementation is solid and matches the documentation well. The architecture document is mostly accurate with only minor inconsistencies in the Configuration section that should be addressed. The skill guide is comprehensive and up-to-date. No bugs or security issues were found.
