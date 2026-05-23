# Implementation Plan - Documentation Review Consolidation

**Status**: COMPLETED ✅
**Last Updated**: 2026-05-27
**Goal**: Historical record of May 2026 architecture review consolidation

---

## Summary

All items from the consolidated documentation review plan have been completed. This file serves as a historical record for future agents.

### Verification Results (2026-05-27)

| Category | Items | Status |
|----------|-------|--------|
| Wave 0 (DOC-1 to DOC-6) | Quick documentation fixes | ✅ ALL VERIFIED |
| Wave 1 (BUG-6) | FocusManager pop_dialog index bug | ✅ FIXED |
| Wave 2 (DOC-8, DOC-9) | compaction + hooks SKILL.md created | ✅ COMPLETE |
| Wave 3 (DOC-11, DOC-12) | Snapshot restore path validation + resilience docs | ✅ COMPLETE |
| Wave 4 (DOC-10,13,15,16) | Architecture corrections | ✅ COMPLETE |
| Wave 5 (DOC-17,18,20) | Skills documentation | ✅ COMPLETE |
| Wave 6 (DOC-25) | Histogram/MetricsSnapshot documented | ✅ COMPLETE |

**Total: 19 items PASSED, 0 items FAILED**

---

## Notes for Future Agents

These implementation notes were verified during the May 2026 review sessions:

### Critical Implementation Notes

1. **Architecture docs vs actual code**: When implementing fixes, always verify against actual source. Many "bugs" from reviews were actually already fixed in prior sessions.

2. **Module naming**: Skills follow module naming (e.g., `compaction` skill at `.opencode/skills/compaction/SKILL.md`)

3. **TTS is macOS-only**: Uses hardcoded `say` command

4. **Permissions are synchronous**: `PermissionRegistry::respond()` is `fn`, not `async fn`. Do NOT use `await` when calling these.

5. **WASM plugins use fuel tracking**: `return_fuel()` uses `MAX_PLUGIN_FUEL_BUDGET`

6. **FocusManager dialog stack**: Uses `VecDeque`, `pop_dialog()` uses idx directly (was BUG-6, now fixed)

7. **PermissionRegistry location**: Located in `src/bus/mod.rs`, not `src/permission/`

8. **MCP reconnect wired up**: Heartbeat failures trigger reconnect via `reconnect_needed` Notify

9. **SSE not fully integrated**: `connect_sse()` etc. exist but are not automatically called during remote connection setup

10. **Snapshot restore() path validation**: Validates paths don't escape project_root

11. **Crypto FORMAT_V2_PREFIX**: Public constant `pub const FORMAT_V2_PREFIX: &str = "v2:"` at `src/crypto/mod.rs:10`

### Known Issues (Lower Priority)

- **SSE support not fully integrated**: `connect_sse()` and `connect_sse_stream()` exist but are not automatically called during remote connection setup. SSE events are collected but not yet processed by the agent.
- **Tool definition cache staleness**: Using `mcp_tool_count` as proxy means if MCP tool identities change without count changing, cache may be stale. MCP service would need to expose a version/hash for more precise invalidation.

---

## Historical Context

### Branch/Commit History (May 2026)

| Branch | Commit | Description |
|--------|--------|-------------|
| wave1-bugfix | 0f77c89 | FocusManager pop_dialog fix + compaction SKILL.md |
| wave3-bugfix | 9ef45f5 | Snapshot restore path validation + resilience docs |
| wave4-docs-corrections | 47b549a | Snapshot capture flow, LSP request_id, SSE methods, IdeServer::run_socket |
| wave5-skills-docs | ab2b274 | Teams/lsp/formatter tools, list_skill_resources, FORMAT_V2_PREFIX |
| wave6-final-docs | bb554b5 | Histogram 1000-element limit and MetricsSnapshot |

All merged to main: 84ec942 (later a953103 for FORMAT_V2_PREFIX fix)

### Original Plan Files

The following original plan files have been consolidated into this document:
- This file (`plans/plan.md`) is the single source of truth
- All other plan files in `plans/` directory have been removed

---

## Completed Verification Checklist

The following items were verified as correctly implemented (not bugs):

- **BUG-1**: IDE line range slicing - ✅ `open_diff_generic()` at src/ide/mod.rs:91
- **BUG-2**: external_directory in PERMISSION_TYPES - ✅ Not present
- **BUG-3**: TTS duplicate store - ✅ No duplicate `speaking.store(false)`
- **BUG-4**: TTS stop() guard - ✅ Checks `is_speaking()` first
- **BUG-5**: TTS init() - ✅ Exhaustive match on `TtsProvider::None`
- **PTY location**: ✅ Module is `src/pty_session/`

*(Last updated: 2026-05-27 - All items verified complete)*