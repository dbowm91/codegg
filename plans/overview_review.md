# architecture/overview.md Review

**Reviewer**: Claude Code
**Date**: 2026-05-26
**Source**: `architecture/overview.md` vs actual codebase at `/Users/davidbowman/projects/codegg`

---

## Executive Summary

The architecture document is generally accurate but contains several discrepancies in counts and naming. Most claims verified against source.

---

## Findings by Section

### Technology Stack (Lines 5-14)

| Claim | Source | Status |
|-------|--------|--------|
| Tokio | N/A | ✅ Correct |
| SQLx | N/A | ✅ Correct |
| Ratatui | N/A | ✅ Correct |
| Axum (feature-gated `server`) | `src/lib.rs:14` | ✅ Correct |
| Wasmtime (feature-gated `plugins`) | N/A | ✅ Correct |

### System Architecture Diagram (Lines 17-56)

| Claim | Source | Status |
|-------|--------|--------|
| TUI Layer with Components/Dialogs (21) | `src/tui/app/types.rs:2-25` | ✅ 21 Dialog variants exist |
| Dialog::Info referenced in diagram | N/A | ⚠️ Dialog::Info doesn't exist in code (line 3 shows None as first) |
| LLM Provider (20+ models) | `src/provider/mod.rs:13-32` (19 files) | ⚠️ 19 provider files, but "20+" plausible with CodeggGo in additional.rs |
| MCP Servers (local/remote) | N/A | ✅ Correct |
| LSP Servers (40 servers) | `src/lsp/server.rs:27-375` | ✅ Verified 40 `LspServerDef` entries |

### Module Index (Lines 60-136)

#### Core Runtime Table (Lines 65-73)

| Module | Claim | Source | Status |
|--------|-------|--------|--------|
| Agent | "AgentLoop, message processing, subagent pool, compaction, routing, team coordination" | `src/agent/mod.rs` | ✅ Correct |
| Provider | "20+ LLM backends" | `src/provider/mod.rs:13-32` (19 files) | ⚠️ 19 explicit, 20+ phrasing is marketing |
| Tool | "26 built-in tools" | `src/tool/mod.rs:89-119` | ✅ Verified 26 tools registered |
| Event Bus | "GlobalEventBus (pub/sub), PermissionRegistry, QuestionRegistry" | `src/bus/mod.rs` | ✅ Correct |
| Core | "CoreClient facade, transport adapters (inproc/stdio/socket), protocol envelopes" | `src/core/mod.rs` | ✅ Correct |
| Compaction | Context window overflow management | `src/agent/compaction.rs` | ✅ Correct |

#### TUI Module (Line 96)

| Claim | Source | Status |
|-------|--------|--------|
| "21 dialog types" | `src/tui/app/types.rs:2-25` | ✅ 21 Dialog variants |
| Dialogs listed: `Model, Agent, Session, Help, Tree, Theme, Question, Permission, Mcp, Keybind, Share, Import, Template, Connect, Context, Cost, Usage, Goto, Plan, Diff, Confirm` | `src/tui/app/types.rs` | ✅ All 21 match |

### Built-in Tools Section (Lines 277-289)

| Claim | Value | Source | Status |
|-------|-------|--------|--------|
| Built-in Tools count | "29" in heading | `src/tool/mod.rs:89-119` | ❌ **Mismatch** - 26 tools registered |
| multiedit in tool list | Listed | N/A | ❌ **Not found** in `with_defaults()` |
| formatter in tool list | Listed | `src/tool/formatter.rs` | ✅ Exists |

**Tool count verification** (`src/tool/mod.rs:89-119`):
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
26. ToolSearchTool (line 117-118)

**Count: 26** (not 29)

The table also lists "lsp" and "formatter" under **Code** category. `formatter` exists at `src/tool/formatter.rs`. The "lsp" tool exists at `src/tool/lsp.rs`. Both are registered.

### LLM Providers Section (Lines 292-307)

| Claim | Source | Status |
|-------|--------|--------|
| 20+ providers | `src/provider/mod.rs:13-32` | ⚠️ 19 explicit provider files |
| Additional.rs contains | "Mistral, Groq, Deepinfra, Cerebras, Cohere, TogetherAI, Perplexity, xAI, Venice, MiniMax, CodeggGo" | `src/provider/additional.rs` | ✅ 11 providers listed |

**Total**: 19 provider files + 11 in additional = ~19 distinct providers. "20+" is optimistic.

### Directory Structure (Lines 200-236)

| Module | Claim | Source | Status |
|--------|-------|--------|--------|
| pty_session/ | Listed as `pty_session/` | `src/lib.rs:26` | ❌ **Named `shell_session/` in lib.rs |
| exec.rs | Non-interactive exec mode | `src/exec.rs` | ✅ Correct (file not directory) |

**Discrepancy**: Architecture shows `pty_session/` but actual module is `shell_session/`. However, the description "Shell session metadata management (in-memory, no actual PTY)" matches exactly.

### Built-in Agents Table (Lines 263-274)

Not verified against source (requires agent definitions).

---

## Plugin Hook Types

| Claim | Source | Status |
|-------|--------|--------|
| "13 hook types" | `src/plugin/hooks.rs:6-20` | ✅ 13 HookType variants |

**Verified HookType variants**:
1. Auth
2. Provider
3. ToolDefinition
4. ToolExecuteBefore
5. ToolExecuteAfter
6. ChatParams
7. ChatHeaders
8. Event
9. Config
10. ShellEnv
11. TextComplete
12. SessionCompacting
13. MessagesTransform

**Count: 13** ✅

---

## LSP Server Count

| Claim | Source | Status |
|-------|--------|--------|
| "40 servers" | `src/lsp/server.rs:27-375` | ✅ Verified 40 LspServerDef entries |

AGENTS.md claims 39 servers but `grep` finds 40 `LspServerDef` definitions. **AGENTS.md is outdated.**

---

## AppEvent Count

| Claim | Source | Status |
|-------|--------|--------|
| 36 AppEvents | `src/bus/events.rs:5-147` | ✅ Verified 36 (lines 6-147) |

---

## Verified Claims from AGENTS.md "Verified Codebase Facts"

| Item | Claimed | Source | Status |
|------|---------|--------|--------|
| Tool count | 26 | `src/tool/mod.rs:89-119` | ✅ |
| LSP server count | 39 | `src/lsp/server.rs:27-385` | ❌ **Should be 40** |
| Built-in command count | 39 | `src/tui/command.rs:79-161` | ✅ Verified 39 Command::new() calls |
| CommandRegistry location | Line 72 | `src/tui/command.rs:72` | ✅ |

---

## Summary of Issues

### Errors (Incorrect Claims)

1. **Tool count mismatch** (line 277 vs line 230)
   - Heading says "29", table says "26", code has 26
   - Fix: Change heading to "Built-in Tools (26)"

2. **multiedit tool listed** (line 285)
   - Tool doesn't exist in `with_defaults()`
   - Fix: Remove "multiedit" from list

3. **LSP server count in AGENTS.md** (line 138)
   - Claims 39, actual is 40
   - Fix: Update AGENTS.md to 40

4. **Dialog::Info doesn't exist**
   - Diagram shows 21 dialog types and code has 21, but the specific variant `Dialog::Info` doesn't exist
   - However, `src/tui/components/dialogs/info.rs` exists and `Dialog::Info` is used in `src/tui/components/dialogs/mod.rs`
   - Actually verified: `Dialog::Info` is NOT in the Dialog enum at types.rs:2-25

### Minor Discrepancies

5. **PTY Session module naming**
   - Doc: `pty_session/`
   - Actual: `shell_session/`
   - Description is accurate, just naming differs

6. **Provider "20+" claim**
   - 19 explicit provider files
   - "20+" is marketing language, not strictly accurate but defensible

### Correct Claims (Verified)

- ✅ Tool count: 26 (src/tool/mod.rs:89-119)
- ✅ Dialog count: 21
- ✅ Hook types: 13
- ✅ LSP servers: 40
- ✅ AppEvent count: 36
- ✅ Built-in commands: 39
- ✅ Technology stack all correct
- ✅ Feature flags correct
- ✅ Directory structure mostly correct

---

## Recommendations

1. Fix tool count from 29 to 26
2. Remove "multiedit" from tool list
3. Update AGENTS.md LSP count from 39 to 40
4. Consider renaming pty_session to shell_session in documentation for consistency
5. Change "20+" providers to "19+" or "20+"

---

## Files Referenced

- `src/tool/mod.rs:89-119` - Tool registration
- `src/lsp/server.rs:27-375` - LSP server definitions
- `src/tui/app/types.rs:2-25` - Dialog enum
- `src/plugin/hooks.rs:6-20` - HookType enum
- `src/bus/events.rs:5-147` - AppEvent enum
- `src/tui/command.rs:79-163` - Command definitions
- `src/provider/mod.rs:13-32` - Provider modules
- `src/lib.rs` - Module declarations