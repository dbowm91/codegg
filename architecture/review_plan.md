# Architecture Review Plan

**Status**: Phase 3 Complete (2026-05-25)
**Created**: 2026-05-25
**All 33 subagent reviews**: Completed

---

## Summary

Review all architecture documents (33 modules), verify claims against code, identify bugs and improvements, then prune stale content.

## Phase 1: Subagent Reviews Complete

All 33 subagent reviews completed. See `plans/review/` for individual reports.

## Phase 2: Stale Item Detection

### Modules Verified Accurate (No fixes needed)
- `06_pty_session.md` - All claims verified correct
- `12_bus.md` - All claims verified correct
- `21_resilience.md` - Accurate, one missing detail (half_open_start_time)
- `24_worktree.md` - All claims verified correct
- `25_crypto.md` - All claims verified correct
- `27_exec.md` - All claims verified correct
- `29_memory.md` - Main doc accurate, skill doc has stale path
- `30_error.md` - All claims verified correct
- `31_storage.md` - All claims verified correct
- `33_upgrade.md` - All claims verified correct

### Modules Needing Corrections

| Module | Issues Found |
|--------|-------------|
| `01_overview.md` | Dialog count 21→22/23, LSP 44+→43+, Tools 33+→27+, Hook types 10→13 |
| `02_tui.md` | Theme count inconsistency (42 vs 31), DialogState classification wrong |
| `03_snapshot.md` | AgentLoop integration code example stale (line numbers wrong) |
| `04_server.md` | Missing FromRef implementations, rate limit headers |
| `05_mcp.md` | Missing Clone impl, validate_url_host location, run_stdio async I/O |
| `07_permission.md` | check_with_args missing, write tool lookup inconsistency |
| `08_lsp.md` | Missing code_lens(), send_initialized(), skill says 42 servers (should be 39) |
| `09_config.md` | Missing schema field, api_key() signature incomplete, line refs off |
| `10_core.md` | CoreRequest incomplete (~20 missing), CoreEvent undocumented, bugs in handlers |
| `11_agent.md` | Missing ToolResult variant, PartData::ToolCall not documented |
| `13_command.md` | Line 115 says 36 commands (should be 41), find_command_files not truly async |
| `14_hooks.md` | PreAgentRun/PostAgentRun don't exist, skill uses wrong YAML format |
| `15_skills.md` | Missing list()/get() methods, find_matching behavior undocumented |
| `16_client.md` | handle_remote_event() mislocated (in tui/app not client) |
| `17_security.md` | Missing IPv6 link-local, spurious [security] config section |
| `18_tool.md` | LspTool and TeamTools undocumented |
| `19_tts.md` | stop() description misleading |
| `20_plugin.md` | dispatch_to_plugin referenced but removed, fuel leaks on errors |
| `22_provider.md` | ProviderCache.store should be cache, missing OpenAiToolState, ResponseFormat, ModelVariant |
| `23_compaction.md` | 7 items needing correction, conflations, missing fields/functions |
| `26_ide.md` | Code examples don't match actual implementations |
| `28_session.md` | Module exports overstated, checkpoints table schema wrong |

### Skill Docs with Stale Content

| Skill | Issue |
|-------|-------|
| `.opencode/skills/hooks/SKILL.md` | Uses YAML map format, actual is TOML array |
| `.opencode/skills/memory/SKILL.md` | Wrong path `project/{hash}/conventions/MEMORY.md` (should be `project/{hash}/MEMORY.md`) |
| `.opencode/skills/lsp/SKILL.md` | Says 42 servers (should be 39) |
| `.opencode/skills/error/SKILL.md` | (review incomplete - may have issues) |

## Phase 3: Pruning

### Items to Remove/Archive

1. **Stale HookEvent references**: `PreAgentRun` and `PostAgentRun` in `architecture/hooks.md` - these events do not exist in codebase
2. **Spurious `[security]` config section**: `architecture/security.md` lines 200-206 - `ssrf_protection` not used anywhere
3. **Dead `dispatch_to_plugin` reference**: `architecture/plugin.md` references removed function

### Items to Investigate Further

1. **Core handlers bug**: `Initialize`, `TurnCancel`, `TurnSteer`, `AgentSelect`, `ModelSelect` defined but not handled (silently return `Ack`)
2. **Plugin fuel leaks**: `execute_wasm_hook()` has fuel leaks on error paths
3. **Server permission filtering**: `permission.rs` and `question.rs` session filtering may not work as intended

---

## Verification Commands

```bash
# Lint and typecheck
cargo clippy --all-targets --all-features 2>&1 | head -50

# Test suite
cargo test --all-features 2>&1 | tail -30
```

---

*Plan execution: All 33 subagents completed in 3 batches*