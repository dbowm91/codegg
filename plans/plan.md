# Implementation Plan - Code Review Consolidation

**Status**: PLANNED (Not Started)
**Last Updated**: 2026-05-24
**Goal**: Consolidate all review findings into actionable implementation plan

---

## Summary of Review Findings

| Category | Count | Notes |
|----------|-------|-------|
| Critical Bugs (Compilation) | 0 | Codebase compiles |
| High Priority Bugs | 4 | Memory superseding, plugin fuel dead code, snapshot restore not wired |
| Medium Priority Bugs | 5 | Memory access_count, plugin event_log, snapshot restore failure handling, TTS stop |
| Documentation Updates | 50+ | Various arch docs and skills need corrections |

---

## Implementation Phases (Waves)

### Wave 1: Critical Fixes (High Priority Bugs)

#### W1-1: Memory Module Fixes
| Item | File | Line | Issue |
|------|------|------|-------|
| 1 | `src/memory/mod.rs` | 247 | Change `>=` to `>` for superseding threshold |
| 2 | `src/memory/mod.rs` | 270 | Add `.filter(\|m\| m.superseded_by.is_none())` before sorting in `get_memory_summary()` |

**Context for agents:**
- The superseding logic at line 247 prevents new memories from superseding existing ones when scores are close. Changing `>=` to `>` allows superseding when new score is meaningfully higher.
- The `get_memory_summary()` currently includes superseded memories in the output, which is confusing for users since those memories are no longer active.

#### W1-2: Plugin Module Fixes
| Item | File | Line | Issue |
|------|------|------|-------|
| 3 | `src/plugin/loader.rs` | 24-41 | Remove dead `check_and_reset_fuel_budget()` function |
| 4 | `src/plugin/loader.rs` | 15-17 | Remove unused `PLUGIN_FUEL_BUDGET` and `PLUGIN_FUEL_LAST_RESET` globals |
| 5 | `src/plugin/event_bus.rs` | 67 | Either use `event_log` or remove `get_event_log()` method |

**Context for agents:**
- The `check_and_reset_fuel_budget()` function at lines 24-41 is never called, making the global fuel budget auto-reset dead code.
- The `event_log` in `event_bus.rs` is populated but never consumed. Either wire it up or remove the dead code.
- The per-plugin fuel budget system in `module_cache` is the actual active mechanism.

#### W1-3: Snapshot Module - High Priority
| Item | File | Line | Issue |
|------|------|------|-------|
| 6 | `src/snapshot/mod.rs` | 291-292 | Add failure flag to stop processing on error in `restore()` |
| 7 | `src/snapshot/mod.rs` | - | Consider implementing error-triggered restore or document it doesn't exist |

**Context for agents:**
- The `restore()` function continues processing even after a write failure, potentially causing partial restoration without clear indication.
- Architecture doc shows error-restore flows that don't exist in code - need to either implement or remove from docs.

---

### Wave 2: Documentation Corrections

#### W2-1: Agent Module
| Item | File | Issue |
|------|------|-------|
| 8 | `architecture/agent.md` | Add `run_with_prompt()`, `drain_follow_up()`, `capture_snapshot_if_needed()`, `drain_file_change_events()` |
| 9 | `architecture/agent.md` | Document `ToolDefCache` type alias at loop.rs:60-67 |
| 10 | `.opencode/skills/subagent/SKILL.md` | Sync with current API after spawner deduplication |

#### W2-2: Client Module
| Item | File | Issue |
|------|------|-------|
| 11 | `architecture/client.md` | Clarify RenderFrame "received and logged" not "unused" |
| 12 | `architecture/client.md` | Document 3 retries with exponential backoff (2s, 4s) |
| 13 | `architecture/client.md` | Add note about `catch_unwind` in event handling |
| 14 | `.opencode/skills/client/SKILL.md` | Document `new_remote()` at line 492, `handle_remote_event()` at line 686 |

#### W2-3: Command Module
| Item | File | Issue |
|------|------|-------|
| 15 | `architecture/command.md` | Change "36 commands" to correct count at lines 52, 115 |
| 16 | `.opencode/skills/command/SKILL.md` | Fix line refs at 173-174 |

#### W2-4: Compaction Module
| Item | File | Issue |
|------|------|-------|
| 17 | `architecture/compaction.md` | Add two-phase pruning explanation (prune_tool_outputs vs truncate_tool_outputs) |
| 18 | `architecture/compaction.md` | Add `auto_compact()`, `auto_compact_sync()`, `compact_messages()` to functions table |
| 19 | `architecture/compaction.md` | Clarify that `TruncateToolOutputs` uses 500-char truncation |
| 20 | `architecture/compaction.md` | Add note about `SummarizeOldTurns` sync fallback behavior |

#### W2-5: Config Module
| Item | File | Issue |
|------|------|-------|
| 21 | `architecture/config.md` | Fix line number refs: line 219 `watcher.rs:153-154` → `157-158`, lines 223-224 `schema.rs:508-509` → `542` |
| 22 | `architecture/config.md` | Add `ServerConfig::merge()` method documentation |

#### W2-6: Crypto Module
| Item | File | Issue |
|------|------|-------|
| 23 | `architecture/crypto.md` | Document `FORMAT_V2_PREFIX` constant at line 10 |
| 24 | `.opencode/skills/crypto/SKILL.md` | Sync with implementation |

#### W2-7: Error Module
| Item | File | Issue |
|------|------|-------|
| 25 | `architecture/error.md` | Add `ProviderError::Auth(_)` to `is_retryable()` match pattern at lines 106-115 |
| 26 | `.opencode/skills/error/SKILL.md` | Add `ProviderError::Auth(_)` to `is_retryable()` pattern at lines 40-50 |

#### W2-8: Event Bus Module
| Item | File | Issue |
|------|------|-------|
| 27 | `AGENTS.md` | Correct AppEvent count from "38 variants" to actual count (36) |
| 28 | `architecture/event-bus.md` | Clarify `PermissionChoice` is defined in `src/permission/mod.rs` |

#### W2-9: Hooks Module
| Item | File | Issue |
|------|------|-------|
| 29 | `architecture/hooks.md` | Remove/correct false claim at line 191 about stream errors breaking loop ensuring hooks run - they DON'T run |
| 30 | `architecture/hooks.md` | Clarify shell hooks use underscore notation vs plugin hooks use dot notation |
| 31 | `architecture/hooks.md` | Add note that `InlineScript` hook type is deprecated |

---

### Wave 3: Documentation Corrections (Continued) - ✅ COMPLETED

#### W3-1: LSP Module
| Item | File | Issue | Status |
|------|------|-------|--------|
| 32 | `architecture/lsp.md` | Change "42 servers" to "44 servers" at line 227 | ✅ Done |
| 33 | `architecture/lsp.md` | Remove `build_env_overrides` from docs or implement it | ✅ Done |

#### W3-2: MCP Module
| Item | File | Issue | Status |
|------|------|-------|--------|
| 34 | `architecture/mcp.md` | Remove `heartbeat_task: Arc<AtomicBool>` field (doesn't exist in code) at line ~117 | ✅ Done |
| 35 | `.opencode/skills/mcp/SKILL.md` | Document SSE integration gap (connect_sse not hooked into main flow) | ✅ Done |

#### W3-3: Memory Module (Documentation)
| Item | File | Issue | Status |
|------|------|-------|--------|
| 36 | `.opencode/skills/memory/SKILL.md` | Change path `projects/{hash}/conventions` to `project/{hash}` | ✅ Done |
| 37 | `.opencode/skills/memory/SKILL.md` | Add `set_auto_save(&self, enabled: bool)` to API section | ✅ Done |
| 38 | `.opencode/skills/memory/SKILL.md` | Show negation scoring calculation explicitly | ✅ Done |

#### W3-4: Plugin Module (Documentation)
| Item | File | Issue | Status |
|------|------|-------|--------|
| 39 | `architecture/plugin.md` | Update WASM path construction to show actual `plugins_dir().join(plugin_name).join("plugin.wasm")` | ✅ Done |
| 40 | `architecture/plugin.md` | Add `BuiltinPlugin` struct documentation | ✅ Done |
| 41 | `.opencode/skills/plugin/SKILL.md` | Update `execute_wasm_hook()` example with actual error handling | ✅ Done |
| 42 | `.opencode/skills/plugin/SKILL.md` | Update timeout error format to `format!("{}: hook timeout: {}", ...)` | ✅ Done |

#### W3-5: Permission Module
| Item | File | Issue | Status |
|------|------|-------|--------|
| 43 | `.opencode/skills/permission/SKILL.md` | Change docs mode `default: "allow"` to `default: "ask"` at lines 150-157 | ✅ Done |

#### W3-6: Provider Module
| Item | File | Issue | Status |
|------|------|-------|--------|
| 44 | `architecture/provider.md` | Add `is_openai: bool` field to SseParser struct | ✅ Done |
| 45 | `architecture/provider.md` | Add `ProviderError::Auth(_)` to `is_retryable()` match pattern | ✅ Done |
| 46 | `architecture/provider.md` | Clarify `codegg_go` registration path (config-based, not `register_builtin()`) | ✅ Done |
| 47 | `architecture/provider.md` | Document `parse_anthropic_buffer_with_state()` at sse_parser.rs:500-519 | ✅ Done |

#### W3-7: PTY Module
| Item | File | Issue | Status |
|------|------|-------|--------|
| 48 | `.opencode/skills/pty/SKILL.md` | Change `src/pty/` to `src/pty_session/` at line 20 | ✅ Done |
| 49 | `architecture/pty.md` | Document that `PtyManager` implements `Default` trait | ✅ Done |

#### W3-8: Resilience Module
| Item | File | Issue | Status |
|------|------|-------|--------|
| 50 | `architecture/resilience.md` | Fix line number refs for `record_success()` and `record_failure()` at lines 123-131 | ✅ Done |

---

### Wave 4: Documentation Corrections (Final Batch) ✅ COMPLETED

#### W4-1: Server Module
| Item | File | Issue | Status |
|------|------|-------|--------|
| 51 | `architecture/server.md` | Remove claim "event_bus field was removed" - it still exists at line 70-71 | ✅ Done |
| 52 | `architecture/server.md` | Remove incorrect SSE handler description at lines 194-198 | ✅ Done |
| 53 | `.opencode/skills/server/SKILL.md` | Remove "Dead EventBus Struct" claim at line 210 | ⚠️ SKIP - file doesn't exist |

#### W4-2: Snapshot Module (Documentation)
| Item | File | Issue | Status |
|------|------|-------|--------|
| 54 | `architecture/snapshot.md` | Remove error-restore flows at lines 97-119, 139-156 (not implemented) | ✅ Done |
| 55 | `architecture/snapshot.md` | Document that `restore()` exists but not integrated into agent | ✅ Done |
| 56 | `architecture/snapshot.md` | Document atomic write pattern in `restore_to_path()` | ✅ Done |
| 57 | `architecture/snapshot.md` | Document `collect_files_sync()` exclusions and limits | ✅ Done |
| 58 | `.opencode/skills/snapshot/SKILL.md` | Add `delete_snapshot()` and `delete_all_for_session()` to API | ✅ Done (already documented) |
| 59 | `.opencode/skills/snapshot/SKILL.md` | Add note about atomic write in `restore_to_path()` | ✅ Done |
| 60 | `.opencode/skills/snapshot/SKILL.md` | Add config integration details | ✅ Done |

#### W4-3: Tool Module
| Item | File | Issue | Status |
|------|------|-------|--------|
| 61 | `architecture/tool.md` | Remove `[tools]` TOML config with `path_rules` - not implemented | ✅ Done |
| 62 | `.opencode/skills/tool/SKILL.md` | Change "25+ total" to "26 total" at line 32 | ✅ Done |
| 63 | `.opencode/skills/tool/SKILL.md` | Add note about `unrestricted` mode availability | ✅ Done |

#### W4-4: TTS Module
| Item | File | Issue | Status |
|------|------|-------|--------|
| 64 | `architecture/tts.md` | Document `stop()` method and `pkill say` implementation | ✅ Done |
| 65 | `architecture/tts.md` | Document `is_speaking()` method signature | ✅ Done |
| 66 | `architecture/tts.md` | Fix `speaking` type to `Mutex<AtomicBool>` not just `AtomicBool` | ✅ Done |
| 67 | `architecture/tts.md` | Clarify `[tts]` config not implemented at all | ✅ Done |
| 68 | `architecture/tts.md` | Document `init()` only handles `TtsProvider::None` | ✅ Done |
| 69 | `architecture/tts.md` | Document empty string validation in `speak()` | ✅ Done |

#### W4-5: TUI Module
| Item | File | Issue | Status |
|------|------|-------|--------|
| 70 | `architecture/tui.md` | Add missing UiState fields: `sidebar_visible`, `auto_scroll`, `show_thinking`, `show_timestamps`, `timeline_visible`, `timeline_selected`, `tts_enabled`, `fullscreen`, `dirty_regions` | ✅ Done |
| 71 | `architecture/tui.md` | Add Timeline as a render layer | ✅ Done |
| 72 | `architecture/tui.md` | Add CommandPalette field in DialogState | ✅ Done |
| 73 | `architecture/tui.md` | Document `busy_spinner` in App struct | ✅ Done |
| 74 | `architecture/tui.md` | Document `pending_*` fields in DialogState | ✅ Done |
| 75 | `architecture/tui.md` | Document ClickTarget enum | ✅ Done |
| 76 | `.opencode/skills/tui/SKILL.md` | Consider adding Timeline feature documentation | ✅ Done |

#### W4-6: Worktree Module
| Item | File | Issue | Status |
|------|------|-------|--------|
| 77 | `architecture/worktree.md` | Add note: `remove_worktree()` does not support `force` parameter | ✅ Done (already present) |
| 78 | `.opencode/skills/worktree/SKILL.md` | Add same note for consistency | ✅ Done (already present) |

#### W4-7: Upgrade Module
| Item | File | Issue | Status |
|------|------|-------|--------|
| 79 | `main.rs` | Change printed install command from `cargo install --git ...` to `curl -fsSL https://codegg.ai/install.sh` | ✅ Done |

---

### Wave 5: Low Priority / Optional

| Item | Module | File | Issue | Status |
|------|--------|------|-------|--------|
| 80 | Snapshot | `tests/snapshot.rs:15-22` | Remove dead `create_test_manager()` function | DONE |
| 81 | TTS | `src/tts/mod.rs:98-101` | Consider returning error when `pkill say` fails | SKIPPED (current behavior reasonable - pkill may legitimately fail if say not running) |
| 82 | Memory | `src/memory/mod.rs:169-177` | Document `get()` increments access_count but only persists if auto_save enabled | DONE |
| 83 | LSP | `architecture/lsp.md` | Consider documenting `completion` handles both `CompletionList` and `Vec<CompletionItem>` responses | DONE |
| 84 | MCP | `architecture/mcp.md` | Consider integrating SSE support into main connection flow | SKIPPED (gap already noted at lines 305-309) |
| 85 | Security | `architecture/security.md` | Consider adding note about Landlock config loading | DONE |

---

## Parallelization Strategy

### Wave 1 (High Priority Bugs) - Sequential
Must fix memory and plugin bugs first before documentation changes to ensure accuracy.

### Wave 2-4 (Documentation) - Can parallelize
Each sub-wave (W2-1 through W2-8, W3-1 through W3-8, W4-1 through W4-7) can be done in parallel by different agents.

### Wave 5 (Optional) - Low priority
Can be done at end or skipped.

---

## Verification Steps

After implementing changes, run:
```bash
cargo check
cargo test
```

Ensure:
1. No compilation errors
2. All existing tests pass
3. Documentation builds without broken links

---

*Last updated: 2026-05-24*