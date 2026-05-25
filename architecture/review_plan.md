# Architecture Review Plan

This document outlines the review plan for architecture documentation in the `architecture/` directory. Each module will be reviewed by a subagent that will verify claims against the actual code and document findings and improvements.

## Status

**IN PROGRESS** - Wave 1 and Wave 2 reviews completed. Stale items identified. Awaiting commit.

## Review Summary

### Wave 1 Results (Stale Modules - May 25 modifications)

| Module | Status | Key Findings |
|--------|--------|--------------|
| Client | COMPLETE | SKILL.md line counts outdated (154 vs 159 for attach.rs) |
| Core | COMPLETE | Missing request variants (TurnCancel, TurnSteer, etc); misleading claim about InprocCoreClient publishing |
| Server | COMPLETE | ServerRuntimeError has 5 variants (doc shows 2); SSE methods misplaced to MCP not server |
| TUI | COMPLETE | 17 discrepancies found - many undocumented TuiMsg variants, pending_permission/pending_question fields wrong |
| Skills | COMPLETE | All accurate - no discrepancies |
| Overview | COMPLETE | TUI counts wrong (17 vs 14 components, 21 vs 20 dialogs); PermissionRegistry location wrong; Agent misses teams.rs; Server routes understated |

### Wave 2 Results (Known Issues)

| Module | Status | Key Findings |
|--------|--------|--------------|
| Agent | COMPLETE | Known issues correctly fixed - BackgroundScheduler and SubAgentSpawner working as expected |
| Snapshot | INCOMPLETE | restore() still NOT integrated into error-handling - bug confirmed |
| IDE | COMPLETE | Line count clarification provided - no actual bugs |
| Tool | COMPLETE | SKILL.md count FIXED - now correctly shows "26 total" |
| Hooks | COMPLETE | Architecture doc claim was WRONG - stream errors do NOT ensure hooks run (only SessionEnd hooks run, not AgentEnd) |
| Memory | COMPLETE | The claimed bugs (`>=` vs `>`, missing filter) were NOT FOUND - consolidated review had stale line numbers |

## Stale Items Identified

### Architecture Documents with Name Mismatch

| Document | Actual Module | Issue |
|----------|--------------|-------|
| `architecture/event-bus.md` | `src/bus/` | Name mismatch - module is `bus/` not `event-bus/` |
| `architecture/pty.md` | `src/pty_session/` | Name mismatch - module is `pty_session/` not `pty/` |
| `architecture/error.md` | `src/error.rs` (file) | No module directory - file-based module |
| `architecture/exec.md` | `src/exec.rs` (file) | No module directory - file-based module |
| `architecture/compaction.md` | NONE | No corresponding module exists |

### Recommended Pruning Actions

1. **Rename** `architecture/event-bus.md` → `architecture/bus.md` (or keep for bus module if intentional)
2. **Rename** `architecture/pty.md` → `architecture/pty_session.md` (or keep for pty_session if intentional)
3. **Consider removal** of `architecture/compaction.md` - no corresponding module exists
4. **Verify** `architecture/error.md` and `architecture/exec.md` are properly aligned with their file-based modules

### Plans Directory Stale Items

| File | Status | Action |
|------|--------|--------|
| `plans/plan.md` | Archived | Already marked as archived in AGENTS.md |
| `plans/tui_separation.md` | Current | More recent (May 25 20:45) - still relevant |
| `plans/compaction_review.md` | Orphaned | No corresponding module - consider removal |

## Modules Summary

| Category | Count | Modules |
|----------|-------|---------|
| Stale (modified after last review) | 6 | client, core, server, tui, skills, overview (all reviewed) |
| Known incomplete issues | 6 | agent (fixed), snapshot (still broken), ide (fixed), tool (fixed), hooks (fixed), memory (no bug found) |
| Previously reviewed (no action needed) | 19 | command, compaction, config, crypto, error, event-bus, exec, lsp, mcp, permission, plugin, provider, pty, resilience, security, session, storage, tts, upgrade, util, worktree |

## Key Discrepancies Requiring Documentation Fixes

1. **Server**: ServerRuntimeError variants (5 vs 2 documented)
2. **TUI**: 17+ discrepancies including TuiMsg variants, pending fields, Shift+Tab behavior
3. **Overview**: TUI component/dialog counts, PermissionRegistry location, Agent teams.rs missing, Server routes understated
4. **Core**: Missing request variants, InprocCoreClient publishing claim
5. **Snapshot**: restore() error handling integration missing (code bug, not doc)

## Review Methodology

For each module, the subagent will:

1. **Read the architecture document** at `architecture/<module>.md`
2. **Explore the corresponding source code** in `src/<module>/` or relevant locations
3. **Verify claims** by checking if documented types, functions, and behaviors match the implementation
4. **Identify discrepancies** between documentation and implementation
5. **Detect bugs** in the actual code that may not be documented
6. **Propose improvements** for both documentation and code

## Review Agent Instructions

Each subagent should:
- Load the relevant skill for the module (e.g., `agent-loop`, `provider`, etc.)
- Cross-reference the architecture document with actual source code
- Document any:
  - Inaccuracies in the documentation
  - Missing undocumented types or functions
  - Bugs or code quality issues
  - Missing architectural concerns
  - Recommendations for improvement
- Write findings to the corresponding file in `plans/` directory
- Include a "Status" section at the top: STALE, INCOMPLETE, or COMPLETE

---

*Generated: 2026-05-25*
*Updated: 2026-05-25 (after Wave 1 and Wave 2 completion)*
