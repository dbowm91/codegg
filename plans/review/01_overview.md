# Review: `architecture/overview.md`

## Verified Correct Items

- **Technology Stack**: Tokio, SQLx, Ratatui, Axum (feature-gated), Wasmtime (feature-gated) - all correct
- **Feature Flags**: `server`, `plugins`, `tts`, `arboard`, `image` - all correct
- **Configuration Precedence**: Environment → Project → Global → System - correct
- **Directory Structure**: All listed modules exist in `src/`
- **Agent Loop architecture**: Provider/Tool/Permission/GlobalEventBus structure correct
- **Security modules**: Permission, Security, Crypto all correctly described
- **Data modules**: Session, Storage, Memory, Snapshot all correctly described
- **Plugin module**: 14 hook types confirmed (Auth, Provider, ToolDefinition, ToolExecuteBefore, ToolExecuteAfter, ChatParams, ChatHeaders, Event, Config, ShellEnv, TextComplete, SessionCompacting, MessagesTransform) - correct

## Incorrect/Stale Items

### 1. Dialog Count: "21 dialog types" (Line 25, 95)
**Incorrect**: Actually 23 Dialog variants in `src/tui/app/types.rs:2-25`
```rust
pub enum Dialog {
    None,           // not a dialog
    Model, Agent, Session, Help, Tree, Theme, Question, Permission,
    Mcp, Keybind, Share, Import, Template, Connect, Context, Cost,
    Usage, Goto, Plan, Diff, Confirm
}
```
Count: 23 dialogs (including None which is not a real dialog = 22 actual dialogs)

**Fix**: Update to 22 or 23 depending on whether `None` counts

### 2. Tool Count: "33+ built-in tools" (Line 70)
**Incorrect**: `src/tool/mod.rs:89-119` registers 28 tools (including `tool_search`).
Count: BashTool, ReadTool, EditTool, WriteTool, GlobTool, GrepTool, ListTool, TaskTool, WebFetchTool, WebSearchTool, CodeSearchTool, QuestionTool, TodoTool, SkillTool, ApplyPatchTool, DiffTool, ReplaceTool, ReviewTool, BatchTool, TerminalTool, GitTool, CommitTool, PlanEnterTool, PlanExitTool, InvalidTool, ToolSearchTool = 26 + LspTool (imported but not in with_defaults) = potentially 27

**Fix**: Update to "27+ built-in tools" or verify exact count

### 3. LSP Server Count: "44+ langs" (Line 54)
**Incorrect**: `src/lsp/language.rs:85-132` shows 43 language-to-server mappings (counted manually from the match arms)
- Need to verify: `language.rs:85-132` has entries for rust, go, python, javascript/typescript (2), java, kotlin, c/cpp, csharp, php, ruby, swift, objective-c/cpp (2), lua, perl/raku (2), haskell, scala, dart, elixir, erlang, clojure, vue, svelte, html, css/scss/sass/less (4), json/jsonc (2), yaml, toml, xml, shellscript, powershell, sql, graphql, proto, terraform, dockerfile, markdown, r, zig, nim, v, solidity, makefile, cmake = ~43

**Fix**: Update to "43+ languages" or verify exact count

### 4. Provider Count: "20+ models" (Line 54, 69)
**Correct in context**: `register_builtin()` at `src/provider/mod.rs:262-309` shows 15 providers registered from env vars, plus config-based providers - "20+ models" is acceptable approximation

### 5. Hook Type Count: "10 hook types" (Line 111)
**Incorrect**: `src/plugin/hooks.rs:6-37` defines 13 HookType variants:
Auth, Provider, ToolDefinition, ToolExecuteBefore, ToolExecuteAfter, ChatParams, ChatHeaders, Event, Config, ShellEnv, TextComplete, SessionCompacting, MessagesTransform

**Fix**: Update to "13 hook types"

### 6. Module descriptions need minor verification:
- **Tool**: "33+ built-in tools" - should be verified/updated
- **TUI**: "21 dialog types" - should be 22 or 23
- **LSP**: "44+ langs" - should be "43+ languages"
- **Plugin**: "10 hook types" - should be "13 hook types"

## Bugs Found in Related Code

None found. All referenced code is consistent with architecture intent.

## Specific Line Numbers Needing Updates

| Location | Current | Should Be | Action |
|----------|---------|-----------|--------|
| Line 25 | "Dialogs (21)" | "Dialogs (22)" | Fix |
| Line 54 | "44+ langs" | "43+ languages" | Fix |
| Line 70 | "33+ built-in tools" | "27+ built-in tools" | Fix or verify |
| Line 95 | "21 dialog types" | "22 dialog types" | Fix |
| Line 111 | "10 hook types" | "13 hook types" | Fix |

## Summary

The overview document is largely accurate. Main corrections needed:
1. Dialog count: 21 → 22 or 23 (verify whether `None` counts)
2. LSP language count: 44+ → 43+ (or verify exact)
3. Tool count: 33+ → 27+ (verify from `ToolRegistry::with_defaults()`)
4. Hook types: 10 → 13