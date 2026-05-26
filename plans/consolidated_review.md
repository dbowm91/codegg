# Architecture Review - Consolidated Findings

**Date**: 2026-05-26
**Parent Review Agent**: Claude Code
**Status**: COMPLETE - All 34 modules reviewed

---

## Executive Summary

All 34 architecture documents have been reviewed via subagents across 7 batches. The documentation is **generally accurate** with several discrepancies requiring correction. Most issues are minor (line number drift, naming inconsistencies) but several represent real documentation errors.

---

## Critical Corrections Required

### 1. Module Naming Error (CRITICAL)
| File | Issue |
|------|-------|
| `architecture/pty_session.md` → `src/shell_session/` | Entire document uses wrong module name `pty_session` instead of `shell_session`. All struct names use `Pty` prefix but actual code uses `Shell` prefix. Module location is `src/shell_session/`, not `src/pty_session/`. |

### 2. Tool Count Error
| File | Issue |
|------|-------|
| `architecture/overview.md:277` | Claims "29" built-in tools. Actual count is **26**. Also lists `multiedit` which doesn't exist. |

### 3. LSP Server Count Error
| File | Issue |
|------|-------|
| `architecture/overview.md` + `AGENTS.md:138` | Claims 39 LSP servers. Actual count is **40** (cmake-language-server added). |
| `architecture/lsp.md:229` | Same issue - claims 39, should be 40. |

### 4. Permission Mode Error (MEDIUM)
| File | Issue |
|------|-------|
| `architecture/permission.md:202` | `docs` mode table incorrectly lists `write` as an allowed tool. Actual code: `write` is in `restricted_tools` for docs mode (`modes.rs:171`). |

---

## Discrepancies by File

### `architecture/protocol.md`
| Issue | Description |
|-------|-------------|
| CoreEvent count | Claims 20, actual is **21** |
| Turn events count | Claims 5, actual is **7** (missing TurnReasoningDelta and TurnCompleted) |
| TuiMessage Server→Client | Claims 9, actual is **10** (ResyncRequired miscategorized) |

### `architecture/command.md`
| Issue | Description |
|-------|-------------|
| Function line numbers | Lines 203-205 don't match actual (178-185 offset) |
| Bugs table | Stale - contradicts Historical Implementation Notes which documents fixes |
| `.opencode/docs/command/AGENTS.override.md` | File doesn't exist |

### `architecture/client.md`
| Issue | Description |
|-------|-------------|
| Backoff description | "1s, 2s, 4s" ambiguous - actual is `2^attempt` formula |
| ClientError location | Doc says inline, actual is `src/error.rs:504` |

### `architecture/agent.md`
| Issue | Description |
|-------|-------------|
| Line numbers | Minor drift (expected) |
| SubAgentSpawner | Documented in passing but not shown in detail |

### `architecture/skills.md`
| Issue | Description |
|-------|-------------|
| Skill count | 44 skill directories exist, not explicitly documented |
| `resources` field | SkillTool returns `resources` not documented |

### `architecture/provider.md`
| Issue | Description |
|-------|-------------|
| catalog.rs DashMap | Claims DashMap, but uses HashMap. DashMap is in cache.rs. |

### `architecture/plugin.md`
| Issue | Description |
|-------|-------------|
| event_bus.rs | Exists but undocumented |
| Feature flag | Claims `wasmtime-cache` in Cargo.toml but it's not there |
| MarketplaceService | Says TODO but returns empty Vec |
| Additional methods | `get()`, `list()`, `enabled_plugins()` not documented |

### `architecture/server.md`
| Issue | Description |
|-------|-------------|
| mDNS module | 384 lines, completely undocumented |
| rpc module | Public but not documented |
| routes module | Public but not documented |
| RenderFrame direction | Listed as Server→Client but actually Client→Server |

### `architecture/resilience.md`
| Issue | Description |
|-------|-------------|
| State transition diagram | Uses "recovery_timeout" but actual trigger is `last_failure.elapsed() >= timeout_secs` |
| call() method | Missing HalfOpen timeout check (actual lines 114-127) |

### `architecture/compaction.md`
| Issue | Description |
|-------|-------------|
| select_compaction_strategy threshold | ">6 messages" phrasing ambiguous (should clarify ≥7) |

### `architecture/util.md`
| Issue | Description |
|-------|-------------|
| stat_core.rs | References non-existent file, actual is `metrics.rs` |
| Histogram memory | Unbounded growth not documented (known issue) |

### `architecture/core.md`
| Issue | Description |
|-------|-------------|
| InprocCoreClient fields | Doc doesn't show `Option<Arc<T>>` wrapping |
| CoreEvent mapping | SnapshotSession, SnapshotWorkspace, SnapshotModels NOT mapped via `map_app_event_to_core_event` |
| SessionUpdated/FileChanged | NOT mapped either |

### `architecture/memory.md`
| Issue | Description |
|-------|-------------|
| `/memory-list` dual namespace | Claims dual-namespace behavior that doesn't exist in code |

### `architecture/config.md`
| Issue | Description |
|-------|-------------|
| compaction_threshold | Field name should be `compaction.threshold` |

### `architecture/ide.md`
| Issue | Description |
|-------|-------------|
| IdeServer line numbers | run_stdio at 78-119, run_socket at 121-144 (doc shows 125-130, 138-149) |

### `architecture/mcp.md`
| Issue | Description |
|-------|-------------|
| server_type field | JSON field is `type`, not `server_type` (Rust field is server_type) |

### `architecture/tui.md`
| Issue | Description |
|-------|-------------|
| UiState field count | Claims 21, actual is **25** |
| source code bug | `theme.rs:8` says "31 built-in themes" but actual is 33 |

### `architecture/snapshot.md`
| Issue | Description |
|-------|-------------|
| Hash algorithm | Uses MD5 for non-empty files, SHA256 elsewhere (not documented) |
| Integration line numbers | Off by ~1400 lines |
| Missing skill guide | `.opencode/skills/snapshot/SKILL.md` doesn't exist |

### `architecture/exec.md`
| Issue | Description |
|-------|-------------|
| Question tool timeout | SKILL.md claims 300s timeout but exec mode has NO question response handling |

### `architecture/error.md`
| Issue | Description |
|-------|-------------|
| Line numbers | Off by ~50% throughout |
| ServerRuntimeError IntoResponse | Not documented |

---

## Items Documented as Correct

The following were verified correct across all reviews:

| Module | Status | Notes |
|--------|--------|-------|
| Bus | ✅ Accurate | All counts and structures correct |
| Session | ✅ Accurate | Minor helper export issue only |
| Upgrade | ✅ Accurate | All claims verified |
| Hooks | ✅ Accurate | All 13 hook types, integration points correct |
| Worktree | ✅ Accurate | Minor gaps in See Also |
| Security | ✅ Accurate | All security flows verified |
| LSP (mostly) | ⚠️ Count error | Otherwise accurate |
| Tool | ✅ Accurate | 26 tools verified, ToolExecutor correctly noted as unused |
| Error | ✅ Accurate | All error variants and mappings correct |
| TTS | ✅ Accurate | Minor code simplification acceptable |
| Crypto | ✅ Accurate | All claims verified |
| Storage | ✅ Accurate | All pragmas, migrations verified |

---

## Known Issues Already Documented

These issues are already noted in AGENTS.md and/or exist as known limitations:

| Issue | Location | Status |
|-------|----------|--------|
| Snapshot hash inconsistency | AGENTS.md, `snapshot/mod.rs:431` | Known - MD5 vs SHA256 |
| ToolExecutor unused | AGENTS.md, `tool/executor.rs` | Known |
| CANONICAL_PATHS_CACHE never clears | AGENTS.md, `security/sandbox.rs:237` | Known |
| TTS stop() returns Ok on failure | AGENTS.md, `tts/mod.rs` | Known |
| TTS init() ignores providers | AGENTS.md, `tts/mod.rs` | Known |
| Histogram unbounded memory | AGENTS.md, `util/metrics.rs` | Known |
| Worktree symlink detection | AGENTS.md, `worktree/mod.rs` | Known |
| OAuth replay protection TOCTOU | AGENTS.md, `mcp/auth.rs` | Known |

---

## Summary Statistics

| Metric | Value |
|--------|-------|
| Total modules reviewed | 34 |
| Reviews with critical errors | 3 (overview, pty_session, permission) |
| Reviews with medium errors | 4 (protocol, core, memory, mcp) |
| Reviews mostly accurate | 27 |
| Total specific issues identified | ~45 |

---

## Recommended Actions

### Must Fix (Critical)
1. Rename `architecture/pty_session.md` to `architecture/shell_session.md` and fix all struct references
2. Fix tool count in `architecture/overview.md` (26 not 29, remove multiedit)
3. Update LSP server count to 40 in `architecture/overview.md`, `AGENTS.md`, and `architecture/lsp.md`
4. Fix `docs` mode `write` tool in `architecture/permission.md`

### Should Fix
5. Fix CoreEvent count (20→21) and Turn events (5→7) in `architecture/protocol.md`
6. Add mDNS documentation to `architecture/server.md`
7. Fix state transition diagram in `architecture/resilience.md` (recovery_timeout → last_failure.elapsed())
8. Add missing HalfOpen timeout check to `call()` method docs in `architecture/resilience.md`
9. Fix UiState field count in `architecture/tui.md` (25 not 21)

### Nice to Fix
10. Update function line numbers in `architecture/command.md`
11. Create `.opencode/skills/snapshot/SKILL.md` or remove reference
12. Fix RenderFrame direction in `architecture/server.md`
13. Update catalog.rs description (HashMap not DashMap) in `architecture/provider.md`

---

*Generated 2026-05-26 by architecture review process per `architecture/review_plan.md`*