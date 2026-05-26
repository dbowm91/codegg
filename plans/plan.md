# Implementation Plan

**Status**: COMPLETED
**Last Updated**: 2026-05-26

---

## Overview

This plan was consolidated from 31 individual module review files and has been fully executed. All items have been verified against actual code implementation.

**Key Finding from Review**: Many "bugs" in review files were actually correctly implemented - always verify claims against code before implementing.

---

## Verification Summary

All items in this plan have been completed:

| Priority | Items | Status |
|----------|-------|--------|
| HIGH (H-1 to H-9) | 9 items | ✅ All Completed |
| MEDIUM (M-1 to M-15) | 15 items | ✅ All Completed |
| LOW (L-1 to L-4) | 4 categories | ✅ All Completed |

---

## Completed Items Reference

### H-1: TUI Architecture Documentation
- Line count updated to 5978 lines
- `fullscreen: bool` field documented in UiState
- SpinnerWidget reference corrected

### H-2: Tool Count
- Corrected to 26 tools in `with_defaults()` at `src/tool/mod.rs:89-119`

### H-3: LSP Server Count
- Corrected to 39 servers (was incorrectly listed as 42)
- AGENTS.md updated

### H-4: PermissionResponse Documentation
- Corrected to `{level: PermissionLevel, persist: bool}`
- Mode tables corrected
- `git` added to PERMISSION_TYPES
- `skill` tool added to mode tables

### H-5: Agent Hook Invocation Clarification
- Both hook systems (plugin service AND HookRegistry) correctly documented
- No longer references specific fragile line numbers

### H-6: Plugin Dead Code Removal
- `check_and_reset_fuel_budget()`, `PLUGIN_FUEL_BUDGET`, `PLUGIN_FUEL_LAST_RESET` removed
- Per-plugin fuel tracking via ModuleCache is the actual mechanism

### H-7: Provider ToolDefinition Documentation
- Stale "input_schema renamed to parameters" comment removed
- `register_builtin_with_config` documented as primary entry point

### H-8: Session Event Publishing Clarification
- Explicitly states `SessionCreated` and `MessageAdded` ARE published at `src/bus/events.rs:7,21`

### H-9: MCP Config Example Update
- Config example updated with full `McpEntry` schema

### M-1 through M-15: All Medium Priority Items
- All completed and verified against actual code

### L-1 through L-4: All Low Priority Items
- All formatting, line reference, and clarification items completed

---

## Verified Codebase Facts

These items were verified during review and are accurate:

| Item | Value | Location |
|------|-------|----------|
| Tool count | 26 | `src/tool/mod.rs:89-119` |
| LSP server count | 39 | `src/lsp/server.rs:27-385` |
| PermissionResponse | `{level: PermissionLevel, persist: bool}` | `src/permission/mod.rs:1142-1145` |
| UiState.fullscreen exists | `fullscreen: bool` | `src/tui/app/state/ui.rs:71` |
| ToolExecutor NOT integrated | Exists but unused | `architecture/tool.md:205` |
| Plugin fuel tracking | CORRECT - returns early when exhausted | `src/plugin/loader.rs:238` |
| Memory frequency_bonus | `(count - 1) * 2.0` | `src/memory/patterns.rs:232` |
| Session events published | SessionCreated, MessageAdded | `src/bus/events.rs:7,21` |
| InlineScript deprecated | `#[allow(deprecated)]` | `src/hooks/mod.rs:180` |
| Auth middleware | Allows requests without token when no token configured | `src/server/middleware/auth.rs:37-39` - intentional for dev mode |
| AppEvent count | 36 | `src/bus/events.rs:5-190` |
| CommandRegistry location | Line 72 | `src/tui/command.rs:72` |

---

## Testing Commands

After any changes, run:

```bash
# Build verification
cargo build --all-features

# Lint
cargo clippy --all-features -- -D warnings

# Test
cargo test --all-features

# TUI tests
cargo test tui::input
cargo test tui

# Specific module tests
cargo test --package codegg -- <module>_test_pattern
```

---

## Implementation Notes for Future Agents

1. **Batch processing**: Process 4-5 review files per subagent to avoid context compaction (~2000 line limit)
2. **Plan consolidation pattern**: Subagent reads batch → writes consolidated temp file → parent reads all temp files → creates final plan
3. **Subagent context limits**: Subagents undergo compaction after ~2000 lines
4. **Accurate status tracking**: Many items flagged as "pending" were already fixed - verify before implementing
5. **Line numbers fragile**: Always use code search to find exact locations, never trust line numbers in docs
6. **Verification before assumption**: Many "bugs" in review files turned out to be correctly implemented after direct inspection
7. **Implementation approach**: When implementing, read the current architecture doc first, then verify against actual source code, then make changes only if there's a real discrepancy

---

## See Also

- [AGENTS.md](../AGENTS.md) - Root index file with module quick reference
- [AGENTS.override.md](../AGENTS.override.md) - Override file with verified facts
- `architecture/` - Architecture documentation per module
- `.opencode/skills/` - Module-specific skill guides

*(End of file)*