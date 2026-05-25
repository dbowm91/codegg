# Architecture Review Plan

This document outlines the review plan for architecture documentation in the `architecture/` directory. Each module will be reviewed by a subagent that will verify claims against the actual code and document findings and improvements.

## Status

**INCOMPLETE** - Wave 3 (fix pass) completed. All identified issues resolved. Ready for next review cycle.

*Note: Set to INCOMPLETE for iterative improvement - will be updated after next review cycle.*

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
| Snapshot | COMPLETE | restore() documented as available but not auto-integrated into error-handling |
| IDE | COMPLETE | Line count clarification provided - no actual bugs |
| Tool | COMPLETE | SKILL.md count FIXED - now correctly shows "26 total" |
| Hooks | COMPLETE | Architecture doc claim was WRONG - stream errors do NOT ensure hooks run (only SessionEnd hooks run, not AgentEnd) |
| Memory | COMPLETE | The claimed bugs (`>=` vs `>`, missing filter) were NOT FOUND - consolidated review had stale line numbers |

## Stale Items Identified

### Architecture Documents with Name Mismatch (FIXED)

| Document | Actual Module | Issue | Status |
|----------|--------------|-------|--------|
| `architecture/event-bus.md` | `src/bus/` | Name mismatch - module is `bus/` not `event-bus/` | **FIXED - renamed to bus.md** |
| `architecture/pty.md` | `src/pty_session/` | Name mismatch - module is `pty_session/` not `pty/` | **FIXED - renamed to pty_session.md** |
| `architecture/error.md` | `src/error.rs` (file) | No module directory - file-based module | No action needed |
| `architecture/exec.md` | `src/exec.rs` (file) | No module directory - file-based module | No action needed |
| `architecture/compaction.md` | NONE | No corresponding module exists | See stale plans below |

### Plans Directory Stale Items

| File | Status | Action |
|------|--------|--------|
| `plans/plan.md` | Archived | Already marked as archived in AGENTS.md |
| `plans/tui_separation.md` | Current | More recent (May 25 20:45) - still relevant |
| `plans/compaction_review.md` | Orphaned | No corresponding module - to be removed |

## Modules Summary

| Category | Count | Modules |
|----------|-------|---------|
| All documented issues resolved | ALL | All architecture documents now accurate |
| Snapshot restore() | N/A | Available but not auto-integrated (documented, not a bug) |

## Key Discrepancies Fixed

1. **Server**: ServerRuntimeError variants (5 vs 2 documented) - ✅ FIXED
2. **TUI**: 17+ discrepancies including TuiMsg variants, pending fields, Shift+Tab behavior - ✅ FIXED
3. **Overview**: TUI component/dialog counts, PermissionRegistry location, Agent teams.rs, Server routes - ✅ FIXED
4. **Core**: Missing request variants, InprocCoreClient event flow - ✅ FIXED
5. **Snapshot**: restore() error handling integration (documented as available but not auto-integrated) - ✅ DOCUMENTED
6. **Architecture doc names**: event-bus→bus, pty→pty_session - ✅ FIXED
7. **Plans directory**: compaction_review.md removed (orphaned) - ✅ FIXED

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
*Updated: 2026-05-25 (Wave 1 & Wave 2 completed)*
*Updated: 2026-05-26 (Wave 3 - Fix pass completed)*
