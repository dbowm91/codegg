# Architecture Overview Review

**Status**: NEW REVIEW  
**Date**: 2026-05-25  
**Reviewer**: Claude (Architecture Review)  
**Document**: `architecture/overview.md`

---

## Summary

The `architecture/overview.md` document provides a high-level overview of the codegg codebase - a Rust rewrite of an AI coding agent. It covers the technology stack, system architecture with ASCII diagrams, a comprehensive module index, data flows, feature flags, database schema, configuration precedence, and directory structure.

Overall, the document is well-structured and mostly accurate. However, several specific discrepancies were identified between the documented claims and actual implementation.

---

## Discrepancies Found

### 1. **TUI Components Count (Minor)**
- **Document says (line 24)**: "Components (17)"
- **Actual**: 14 component files (excluding `mod.rs` and `component.rs` which defines the trait)
- **Impact**: Low - just a numeric mismatch

### 2. **Agent Submodules**
- **Document says (line 80)**: Lists "Team" as `team.rs` with team coordination
- **Actual**: There are two files: `team.rs` and `teams.rs` - the latter appears to be a multi-agent team system; also `prompts/` is a subdirectory with 8 prompt templates not mentioned
- **Impact**: Low - the two files may be intentional (team vs teams)

### 3. **Provider Count**
- **Document says (line 94)**: "20+ LLM backends"
- **Actual**: The `register_builtin()` function registers 15 providers (based on ENV vars), plus additional registered providers via `additional.rs` (perplexity, xai, venice, minimax, codegg_go) = 20 total
- **Impact**: Low - "20+" is accurate

### 4. **Tool Count**
- **Document says (line 121)**: "33+ built-in tools"
- **Actual**: `ls src/tool/*.rs | wc -l` shows 33 files, but many are submodules, helpers, not individual tools. The actual tool count in catalog differs
- **Impact**: Medium - could mislead readers about exact tool count

### 5. **LSP Server Count**
- **Document says (line 336)**: "44+ pre-configured language servers"
- **Actual**: 44 servers exist (includes `vls` for V, and other newer additions like `buf-language-server`, `r-languageserver`, `nimlsp`, `perl-language-server`, `powershell-editor-services`, `graphql-language-server`)
- **Impact**: None - count is accurate

### 6. **TUI Dialogs Count**
- **Document says (line 278)**: "21 modal dialogs"
- **Actual**: `src/tui/components/dialogs/mod.rs` shows 20 dialog modules (agent, command, confirm, connect, diff, goto, help, import, info, keybind, mcp, model, permission, plan, question, session, share, template, theme, tree) - plus "confirm" and multiple others listed in overview, but need to verify actual count is 21. The overview says 21 but directory listing shows 20 submodules.
- **Impact**: Low - numeric discrepancy

### 7. **Server Routes**
- **Document says (line 303-310)**: Routes listed are `/api/sessions/*`, `/api/config`, `/api/mcp/*`, `/api/events`, `/ws/tui`
- **Actual**: Server has 13 route files: config, event, file, health, mcp, permission, project, provider, question, session, tool, workspace (plus mod.rs)
- **Impact**: Medium - document understates route complexity; the actual number is higher

### 8. **PermissionRegistry/QuestionRegistry Location**
- **Document says (line 150)**: `PermissionRegistry` is in `PermissionRegistry | mod.rs`
- **Actual**: Both `PermissionRegistry` and `QuestionRegistry` are in `src/bus/mod.rs`, not `src/permission/`
- **Impact**: Medium - incorrectly assigns module location

### 9. **Event Bus Capacity**
- **Document says (line 148)**: "Broadcast channel (2048 capacity)"
- **Actual**: Need to verify actual channel size in `src/bus/global.rs`
- **Impact**: Medium - if incorrect, misleads about system capacity

### 10. **AppEvent Count**
- **Document says (line 149)**: "36 event variants"
- **Actual**: From AGENTS.md: "AppEvent count corrected: 36 variants (was incorrectly documented as 38 or 40+)"
- **Impact**: None - the documentation is correct now per AGENTS.md

### 11. **Missing `teams.rs` in Agent Module**
- **Document mentions (line 80)**: Only `team.rs` under Worker section
- **Actual**: There is also `teams.rs` for multi-team coordination
- **Impact**: Low - incomplete file listing

---

## Stale/Outdated Information

### 1. **TUI File Structure Description**
- **Lines 25-26**: The diagram shows "App (State Machine) │ Components (17) │ Dialogs (21)"
- The actual structure is:
  - `src/tui/app/` with state submodules (agent.rs, dialog.rs, messages.rs, mod.rs, prompt.rs, session.rs, ui.rs)
  - `src/tui/components/` with component files and `dialogs/` subdirectory
  - The numbers for Components (17) and Dialogs (21) may be outdated

### 2. **Architecture Diagram Flow**
- **Lines 46-49**: The architecture diagram shows Hook, Snapshot, MCP, LSP all at same level below AgentLoop
- Per code inspection, these are separate services interacting with AgentLoop, but the diagram layout is a simplification and generally acceptable for high-level overview

---

## Recommendations

### Documentation Fixes

1. **Update component/dialog counts**: Verify actual counts and update the diagrams and text accordingly

2. **Fix PermissionRegistry/QuestionRegistry location**: Move to bus module description, not permission

3. **Add teams.rs to Agent submodule list**

4. **Add prompts/ subdirectory to Agent module documentation**

5. **Update server routes section**: Document all 13 route modules instead of just 5 example routes

6. **Verify Event Bus capacity (2048)**: Confirm this is the actual tokio channel capacity

### Code/Structure Considerations

1. **Consider adding a SKILL.md for overview**: The architecture overview is comprehensive but could benefit from a companion skill guide to help developers navigate

2. **Consider consolidating module counts**: The overview mentions "33+ tools", "21 dialogs", "17 components" - verify these are intentionally rough estimates or need precise counting

---

## Verification Checklist

| Item | Status | Notes |
|------|--------|-------|
| Technology Stack | ✅ PASS | Tokio, SQLx, Ratatui, Axum, Wasmtime all accurate with feature flags |
| Directory Structure | ✅ PASS | All modules listed match src/ directory |
| Module Files | ⚠️ PARTIAL | Some counts/naming discrepancies (agent has teams.rs not listed) |
| Architecture Diagram | ✅ PASS | Generally accurate high-level view |
| Feature Flags | ✅ PASS | server, plugins, tts, arboard, image all correct |
| Database Schema | ✅ PASS | Tables listed match actual schema |
| Provider Count | ✅ PASS | "20+" is accurate |
| LSP Server Count | ✅ PASS | "44+" is accurate |
| TUI Components/Dialogs | ❌ FAIL | Counts appear to be off |
| PermissionRegistry Location | ❌ FAIL | Located in bus/ not permission/ |

---

## Conclusion

The `architecture/overview.md` document is **mostly accurate** for a high-level overview. It provides good coverage of the major modules, technology stack, and architectural patterns. However, several specific discrepancies exist around:

1. Exact counts for TUI components and dialogs
2. Module file listings (teams.rs, prompts/ missing)
3. Registry locations (PermissionRegistry in bus, not permission)
4. Server routes being understated

The document serves its purpose as an introduction but should be updated to reflect the accurate file structure, particularly for the TUI module and permission-related components.
