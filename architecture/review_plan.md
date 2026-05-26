# Architecture Review Plan

This document outlines a systematic review process for all architecture documents in the `architecture/` directory. The goal is to verify claims against actual code, identify stale information, and create improvement plans for each module.

## Overview

- **Total architecture documents**: 34 (excluding this file)
- **Subagent batches**: 8 groups of 4-5 modules each
- **Output location**: Each subagent writes improvement plans to `plans/<module>_review.md`
- **Review scope**: Verify documentation claims, identify bugs, suggest improvements

## Staleness Check Process

Before launching subagents, a preliminary check identifies:
1. Architecture documents with no corresponding source module
2. Documents referencing non-existent file paths or line numbers
3. Outdated module names or deprecated patterns
4. Discrepancies between documented modules and actual `src/` structure

## Batch 1: Core Infrastructure
- [x] `core.md` - Core facade, transport adapters, protocol envelopes
- [x] `protocol.md` - CoreRequest/CoreResponse, TuiMessage protocol
- [x] `bus.md` - GlobalEventBus, PermissionRegistry, QuestionRegistry
- [x] `config.md` - Configuration loading, validation, encryption

## Batch 2: Agent & Session Management
- [x] `agent.md` - AgentLoop, subagent pool, team coordination
- [x] `session.md` - Session storage, database schema, checkpointing
- [x] `memory.md` - Persistent memory, session-to-session learning
- [x] `compaction.md` - Context window overflow management

## Batch 3: Execution & Security
- [x] `exec.md` - Non-interactive exec mode for CI/CD
- [x] `security.md` - SSRF, symlink protection, Landlock
- [x] `permission.md` - Mode system, PermissionChecker, DoomLoop
- [x] `crypto.md` - AES-256-GCM, Argon2id key derivation

## Batch 4: Provider & Resilience
- [x] `provider.md` - LLM provider implementations
- [x] `resilience.md` - Circuit breaker, FallbackProvider
- [x] `tool.md` - Tool trait, registration, execution flow
- [x] `command.md` - Slash command registry, templates

## Batch 5: Integration & Communication
- [x] `mcp.md` - Model Context Protocol client
- [x] `lsp.md` - Language Server Protocol support
- [x] `ide.md` - VS Code, JetBrains integration
- [x] `server.md` - HTTP/WebSocket server for remote TUI

## Batch 6: Client & UI
- [x] `client.md` - Remote TUI client, WebSocket
- [x] `tui.md` - Terminal UI, keyboard shortcuts
- [x] `hooks.md` - Hooks system for agent lifecycle
- [x] `error.md` - AppError, ProviderError, ToolError

## Batch 7: Plugins & Extensions
- [x] `plugin.md` - WASM plugin system
- [x] `skills.md` - Runtime skill loader
- [x] `snapshot.md` - File state capture and restore
- [x] `upgrade.md` - Self-upgrade via GitHub releases

## Batch 8: Utilities & Support Modules
- [x] `util.md` - Clipboard, fuzzy matching, metrics
- [x] `storage.md` - SQLite initialization, pooling
- [x] `worktree.md` - Git worktree management
- [x] `pty_session.md` - Shell session metadata
- [x] `tts.md` - Text-to-speech module

---

## Review Instructions for Subagents

Each subagent should:

1. **Read the architecture document** for their assigned modules

2. **Verify claims against source code**:
   - Check line numbers mentioned in docs against actual code
   - Verify struct field definitions match documented fields
   - Confirm function signatures, enum variants, and method names
   - Validate architecture claims about data flow and module interactions

3. **Identify stale information**:
   - Missing fields that were added to structs
   - Renamed functions or methods
   - Deprecated patterns no longer in use
   - Outdated file paths or line references

4. **Interrogate for improvements and bugs**:
   - Look for TODOs, FIXMEs, or HackMD notes in source
   - Identify missing error handling
   - Check for race conditions or concurrency issues
   - Verify security assumptions still hold

5. **Write improvement plan to `plans/<module>_review.md`**:
   - Summary of verification findings
   - List of stale items found
   - Bug reports with file:line references
   - Improvement suggestions (not direct code changes)

## Staleness Pruning Criteria

After all subagent reviews complete, consolidate findings to identify:

1. **Orphaned documents**: Architecture docs with no corresponding `src/` module
2. **Completely stale documents**: Docs where >50% of content is outdated
3. **Partially stale documents**: Specific sections that need updating
4. **Missing documents**: Modules in `src/` with no architecture doc

## Execution Order

1. **Phase 1**: Launch Batch 1-4 subagents in parallel (8 total subagents)
2. **Phase 2**: Launch Batch 5-8 subagents in parallel
3. **Phase 3**: Consolidate all `plans/*_review.md` files
4. **Phase 4**: Update this review plan with staleness findings
5. **Phase 5**: Create cleanup plan for stale items

## Expected Outputs

Review plans written by subagents to `plans/`:

- [x] `plans/core_infrastructure_review.md` - core, protocol, bus, config
- [x] `plans/agent_session_review.md` - agent, session, memory, compaction
- [x] `plans/exec_security_review.md` - exec, security, permission, crypto
- [x] `plans/provider_resilience_review.md` - provider, resilience, tool, command
- [x] `plans/integration_review.md` - mcp, lsp, ide, server
- [x] `plans/client_ui_review.md` - client, tui, hooks, error
- [x] `plans/plugin_extension_review.md` - plugin, skills, snapshot, upgrade
- [x] `plans/utility_support_review.md` - util, storage, worktree, pty_session, tts

## Staleness Findings Summary

### Critical Bugs Found
1. **snapshot.md**: Hash algorithm inconsistency - `collect_files_sync()` uses MD5 but SHA256 elsewhere (`src/snapshot/mod.rs:431` vs :143,:417)
2. **core.md/protocol.md**: Incorrectly claims subagent events NOT mapped in `map_app_event_to_core_event` - they ARE mapped
3. **skills.md**: Claims `.skills/` directory is loaded at runtime - it is NOT

### Stale Item Categories
| Category | Count | Modules Affected |
|----------|-------|------------------|
| Missing struct fields | 3 | agent, util |
| Wrong line numbers | 4 | pty_session, session, compaction |
| Wrong counts (commands, tests) | 3 | command, pty_session, provider |
| Outdated descriptions | 5 | bus, provider, client, plugin |
| Unused/dead code referenced | 4 | permission, tool |

### Potential Bugs in Code
| File | Issue |
|------|-------|
| `src/tool/executor.rs:8` | `ToolExecutor` exists but never used |
| `src/snapshot/mod.rs:431` | MD5 vs SHA256 inconsistency |
| `src/util/metrics.rs:122-124` | Histogram unbounded memory growth |
| `src/tui/app/state/ui.rs` | `render_panic_count` never incremented |
| `src/tui/app/state/ui.rs` | `dirty_regions` partial redraw optimization incomplete |
| `src/client/mod.rs` | `ClientError` lacks `is_retryable()` method |
| `src/memory/mod.rs` | `access_count` increments lost without explicit save |
| `src/security/sandbox.rs:237` | Static cache with no invalidation |

## Next Steps

1. **Fix critical bugs** in snapshot hashing, skills loading, and subagent event mapping
2. **Update stale documentation** in skills.md, bus.md, provider.md, client.md
3. **Audit dead code**: Remove or integrate `ToolExecutor`, `PermissionResponse`, `check_external_directory()`
4. **Address potential bugs** in metrics, TUI state, and memory persistence
5. **Update line numbers** in pty_session.md, session.md, compaction.md
6. **Correct command/test counts** in command.md, pty_session.md

## Notes

- This plan is for **review only** - subagents write improvement plans, not direct code changes
- Subagents should use the `general` agent type for thorough code analysis
- Each subagent prompt should specify exactly which modules they're reviewing
- Consolidation step identifies what needs to be pruned or updated in architecture/