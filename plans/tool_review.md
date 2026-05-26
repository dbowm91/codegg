# Tool Module Architecture Review

**Reviewed**: 2026-05-26
**Source**: `architecture/tool.md` vs `src/tool/` source code

---

## Summary

**Status**: Mostly ACCURATE with minor discrepancies noted below.

---

## Verified Items

### Tool Count

| Claim | Actual | Status |
|-------|--------|--------|
| "26 tools in `with_defaults()`" | 26 tools registered (lines 91-119 in `mod.rs`) | VERIFIED |

**Actual tool list (26 total):**
1. bash (BashTool)
2. read (ReadTool)
3. edit (EditTool)
4. write (WriteTool)
5. glob (GlobTool)
6. grep (GrepTool)
7. list (ListTool)
8. task (TaskTool)
9. webfetch (WebFetchTool)
10. websearch (WebSearchTool)
11. codesearch (CodeSearchTool)
12. question (QuestionTool)
13. todo (TodoTool)
14. skill (SkillTool)
15. apply_patch (ApplyPatchTool)
16. diff (DiffTool)
17. replace (ReplaceTool)
18. review (ReviewTool)
19. batch (BatchTool)
20. terminal (TerminalTool)
21. git (GitTool)
22. commit (CommitTool)
23. plan_enter (PlanEnterTool)
24. plan_exit (PlanExitTool)
25. invalid (InvalidTool)
26. tool_search (ToolSearchTool)

### Tool Trait

**Location**: `src/tool/mod.rs:54-60`

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError>;
}
```

**Claim** (line 29): "tools do NOT receive a `ToolContext` struct. They receive only `serde_json::Value` as input."
**Verdict**: VERIFIED

### ToolResult

**Location**: `src/tool/mod.rs:62-68`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: String,
    pub success: bool,
}
```

**Verdict**: VERIFIED

### ToolRegistry Fields

**Location**: `src/tool/mod.rs:70-73`

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    catalog: catalog::ToolCatalog,
}
```

**Verdict**: VERIFIED. The doc claims fields are `tools: HashMap<...>` and `catalog: ToolCatalog` - matches exactly.

### ToolRegistry Methods

| Claimed Method | Location | Verdict |
|----------------|----------|----------|
| `new()` | `mod.rs:82-87` | VERIFIED |
| `with_defaults()` | `mod.rs:89-120` | VERIFIED |
| `register()` | `mod.rs:122-126` | VERIFIED |
| `get()` | `mod.rs:128-130` | VERIFIED |
| `list()` | `mod.rs:136-138` | VERIFIED |
| `definitions()` | `mod.rs:148-157` | VERIFIED |
| `filter_out()` | `mod.rs:140-146` | VERIFIED |
| `catalog()` | `mod.rs:132-134` | VERIFIED |

**Note**: `definitions()` returns `Vec<crate::provider::ToolDefinition>` not `Vec<ToolDefinition>` as documented. Minor inconsistency - doc doesn't specify full path.

### ToolCatalog

**Location**: `src/tool/catalog.rs`

**Struct** (lines 37-40):
```rust
pub struct ToolCatalog {
    tools: HashMap<String, ToolMetadata>,
    deferred_load: Vec<String>,
}
```

**Methods all verified** (lines 42-100):
- `new()`, `register()`, `search()`, `get()`, `list()`, `deferred_tools()`, `is_deferred()`

### ToolCatalog Methods Missing from Documentation

The architecture doc shows (lines 147-155):
```rust
impl ToolCatalog {
    pub fn register(&mut self, tool: &dyn Tool);
    pub fn search(&self, query: &str) -> Vec<&ToolMetadata>;
    pub fn get(&self, name: &str) -> Option<&ToolMetadata>;
    pub fn list(&self) -> Vec<&ToolMetadata>;
    pub fn deferred_tools(&self) -> Vec<&ToolMetadata>;  // List tools marked for deferred loading
    pub fn is_deferred(&self, name: &str) -> bool;  // Check if a tool is deferred
}
```

This matches the actual implementation exactly. VERIFIED.

### ToolExecutor

**Location**: `src/tool/executor.rs`

**Struct** (lines 8-12):
```rust
pub struct ToolExecutor {
    max_attempts: usize,
    base_delay: Duration,
    max_delay: Duration,
}
```

**Verdict**: VERIFIED

**Claim** (line 205): "ToolExecutor exists with retry logic but is **not currently integrated** into the tool registry."
**Verification**: Confirmed - `ToolExecutor` is defined in `executor.rs` but is NOT used anywhere in `mod.rs:89-119` tool registration. VERIFIED.

**Claim** (line 200): retry uses "Exponential backoff with jitter"
**Actual**: `executor.rs:48-56` uses `2^attempt * base_ms` with jitter = `capped_ms / 2`. VERIFIED.

### ToolError

**Location**: `src/error.rs:326-350`

**Variants verified**:
- `NotFound(String)` - line 328
- `Execution(String)` - line 331
- `Timeout(String)` - line 334
- `Permission(String)` - line 337
- `Format(String)` - line 340
- `Disabled(String)` - line 343
- `Io(String)` - line 346
- `Network(String)` - line 349

**Verdict**: Minor formatting difference - the doc shows them on fewer lines but content is identical.

**is_retryable()** (lines 352-358):
```rust
impl ToolError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ToolError::Io(_) | ToolError::Network(_) | ToolError::Timeout(_)
        )
    }
}
```
**Verdict**: VERIFIED

### Path Validation

**Location**: `src/tool/util.rs:5-51`

**validate_path** function signature matches (line 5):
```rust
pub fn validate_path(path: &Path, allowed_root: &Path) -> Result<PathBuf, ToolError>
```

**check_path_for_symlinks** verified at line 32.

**Verdict**: VERIFIED

### Plan Tools Split

**Claim** (line 271): "plan_enter and plan_exit are separate tools, not one `plan` tool"
**Verdict**: VERIFIED - `src/tool/plan.rs` defines both `PlanEnterTool` (line 12) and `PlanExitTool` (line 63) as separate structs.

### Security Notes

| Claim | Verification |
|-------|-------------|
| "Tool path validation" | `validate_path()` in util.rs:5 |
| "Symlink protection" | `check_path_for_symlinks()` in util.rs:32 |
| "BashTool blocked patterns" | Not verified in tool module - likely in bash.rs |
| "Subprocess PATH" | Not verified in tool module - likely in bash.rs |

### Team Operations

**Claim** (line 97-103): Team tools defined in `teams.rs`
**Actual**: `src/tool/teams.rs` defines `TeamTools` struct (lines 9-15) which wraps 5 team tools. Actual tool implementations are in `src/agent/teams.rs`.

**Note**: The doc says "(TeamTools registered separately via `TeamTools::register_all()`)" - there is no such method in `src/agent/teams.rs`. Let me check...

**Verdict**: PARTIALLY CORRECT. The `TeamTools::register_all()` method exists at `src/tool/teams.rs:28-37` - it IS in the tool module, but the actual tool types (TeamCreateTool, etc.) are defined in `src/agent/teams.rs`.

---

## Discrepancies

### 1. ToolCatalog Method: `register` signature mismatch

**Doc** (line 148): `pub fn register(&mut self, tool: &dyn Tool);`
**Actual** (`catalog.rs:52`): `pub fn register(&mut self, tool: &dyn Tool)`

These match - doc just shows it differently on one line.

### 2. ToolCatalog `list()` return type

**Doc** (line 151): `pub fn list(&self) -> Vec<&ToolMetadata>;`
**Actual** (`catalog.rs:92-93`): Returns `Vec<&ToolMetadata>`

**Verdict**: VERIFIED

### 3. Missing ToolCatalog note in Known Implementation Notes

The doc does not mention that `ToolCatalog::register()` takes `&dyn Tool` as parameter (not `Box<dyn Tool>`). Minor omission.

---

## File Organization

**Files in `src/tool/`**: 33 .rs files

| Category | Files |
|----------|-------|
| Core | mod.rs, catalog.rs, executor.rs, util.rs, formatter.rs |
| File Operations | read.rs, write.rs, edit.rs, glob.rs, grep.rs, list.rs, diff.rs, replace.rs, multiedit.rs, apply_patch.rs |
| Shell Execution | bash.rs, terminal.rs, git.rs, commit.rs |
| Code Operations | codesearch.rs, review.rs, lsp.rs |
| Web Operations | webfetch.rs, websearch.rs |
| Task Management | task.rs, todo.rs, plan.rs |
| External Integrations | question.rs, skill.rs, batch.rs, tool_search.rs, invalid.rs |
| Misc | teams.rs |

---

## Conclusion

The architecture documentation for the tool module is **largely accurate**. Key findings:

1. **Tool count**: 26 tools - VERIFIED
2. **ToolTrait**: Receives only `serde_json::Value` - VERIFIED
3. **ToolExecutor exists but unused**: VERIFIED
4. **Plan tools split**: VERIFIED
5. **TeamTools location**: Implementation details partially documented - actual tools in agent/teams.rs, wrapper in tool/teams.rs

Minor discrepancies are informational only and do not indicate bugs.
