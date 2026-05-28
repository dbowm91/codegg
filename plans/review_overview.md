# Architecture Overview Review - Findings

**File reviewed**: `architecture/overview.md` (219 lines)
**Reviewed against**: `src/` directory, `Cargo.toml`, `lib.rs`
**Date**: 2026-05-28

---

## Verified Counts

| Item | Doc Value | Actual Value | Status | Evidence |
|------|-----------|--------------|--------|----------|
| Tools (default registry) | 27 | **28** | UPDATE | `src/tool/mod.rs:90-122` - 28 `register()` calls including `tool_search` |
| LSP servers | 39 | 39 | CONFIRMED | `src/lsp/server.rs:27-384` - 39 entries in `server_definitions()` array |
| UiState fields | 26 | 26 | CONFIRMED | `src/tui/app/state/ui.rs:27-76` - 26 struct fields counted |
| AppEvent variants | 36 | 36 | CONFIRMED | `src/bus/events.rs:5-150` - 36 variants in enum |
| Built-in commands | 46 | 46 | CONFIRMED | `src/tui/command.rs:83-182` - 46 `Command::new()` calls |
| Built-in agents | 7 | 7 | CONFIRMED | `src/agent/mod.rs:147-271` - 7 agents in `builtin_agents()` |
| Migrations | 15 | 15 | CONFIRMED | `src/session/schema.rs:25-69` - versions 1-15 |
| GlobalEventBus buffer | 2048 | 2048 | CONFIRMED | `src/bus/global.rs:13` - `broadcast::channel(2048)` |
| Protocol version | 1 | 1 | CONFIRMED | `src/protocol/core.rs:3` - `PROTOCOL_VERSION: u32 = 1` |

---

## Module Table (overview.md:48-80)

**Status**: MOSTLY CONFIRMED with issues

All 33 modules listed in the table exist as directories under `src/`. However:

### Issue 1: `git/` is orphaned code (UPDATE)
The overview lists `git/` as a top-level module, but:
- `src/git/mod.rs` exists on disk but is **NOT declared in `lib.rs`** (no `pub mod git;`)
- No other module references `crate::git` anywhere in the codebase
- It contains `GitSession` and `GitStatus` structs that appear unused
- The tool module has its own `src/tool/git.rs` (a GitTool) which IS used

The `git/` entry in the module table is misleading. The actual git functionality lives in `src/tool/git.rs`.

### Issue 2: Single-file modules missing from table (NEW)
Two modules are declared in `lib.rs` but not listed in the module table:
- `exec` (file: `src/exec.rs`, has `architecture/exec.md` and navigation link)
- `error` (file: `src/error.rs`, has `architecture/error.md` and navigation link)

These should either be added to the module table or noted as single-file modules.

---

## Key Files References (overview.md:50-80)

| Module | Doc Lists | Actual Files | Status |
|--------|-----------|--------------|--------|
| agent/ | `loop.rs`, `worker.rs`, `compaction.rs`, `router.rs` | loop.rs, worker.rs, compaction.rs, router.rs, mention.rs, processor.rs, prompt.rs, task.rs, team.rs, teams.rs | CONFIRMED (listed files exist) |
| bus/ | `global.rs`, `events.rs`, `mod.rs` | global.rs, events.rs, mod.rs | CONFIRMED |
| config/ | `schema.rs`, `paths.rs`, `watcher.rs` | schema.rs, paths.rs, watcher.rs, encryption.rs, mod.rs | CONFIRMED |
| lsp/ | `server.rs`, `service.rs`, `operations.rs` | server.rs, service.rs, operations.rs, client.rs, diagnostics.rs, download.rs, language.rs, launch.rs, root.rs, mod.rs | CONFIRMED |
| mcp/ | `local.rs`, `remote.rs`, `auth.rs` | local.rs, remote.rs, auth.rs, cli.rs, ide_server.rs, mod.rs | CONFIRMED |
| provider/ | `mod.rs`, `anthropic.rs`, `fallback.rs` | mod.rs, anthropic.rs, fallback.rs, additional.rs, azure.rs, bedrock.rs, cache.rs, catalog.rs, cloudflare.rs, codegg_zen.rs, copilot.rs, discovery.rs, gitlab.rs, google.rs, models.rs, openai_compatible.rs, openai.rs, openrouter.rs, sse_parser.rs, text_tool_parser.rs, vertex.rs | CONFIRMED (listed files exist, but missing `additional.rs` which contains `codegg_go`) |
| tool/ | `mod.rs`, `bash.rs`, `read.rs`, etc. | 32 .rs files | CONFIRMED |

---

## Feature Gates (overview.md:120-127)

| Feature | Doc Name | Code Name | Status |
|---------|----------|-----------|--------|
| server | `server` | `server` | CONFIRMED |
| plugin | `plugin` | **`plugins`** | UPDATE - plural in code |
| image | `image` | `image` | CONFIRMED |

**Source**: `Cargo.toml:169-180`
```
[features]
default = ["arboard"]
plugins = ["wasmtime", "wasmtime-wasi"]  # plural
server = ["axum", "http", "tower-http", "tokio-tungstenite"]
image = ["dep:ratatui-image", "dep:image"]
```

Additionally, code has `arboard` (default feature) and `debug-logging` not mentioned in the doc.

---

## Navigation Links (overview.md:186-219)

All 34 architecture docs referenced in navigation links exist in `architecture/`:

- agent.md, bus.md, client.md, command.md, compaction.md, config.md, core.md, crypto.md, error.md, exec.md, git.md, hooks.md, ide.md, lsp.md, mcp.md, memory.md, permission.md, plugin.md, protocol.md, provider.md, resilience.md, security.md, server.md, session.md, shell_session.md, skills.md, snapshot.md, storage.md, tool.md, tts.md, tui.md, upgrade.md, util.md, worktree.md

**Status**: CONFIRMED - all targets exist.

Note: The navigation also includes `exec.md` and `error.md` which are not in the module table (see Issue 2 above).

---

## Line Number References

| Doc Reference | Actual Location | Delta | Status |
|---------------|-----------------|-------|--------|
| `tool/mod.rs:89-119` | `tool/mod.rs:90-122` | +1 start, +3 end | CLOSE (off by 1-3 lines) |
| `lsp/server.rs:27-383` | `lsp/server.rs:27-384` | +1 end | CLOSE |
| `tui/app/state/ui.rs:27-76` | `tui/app/state/ui.rs:27-76` | 0 | EXACT |
| `bus/events.rs:5-147` | `bus/events.rs:5-150` | +3 end | CLOSE |
| `tui/command.rs:79-182` | `tui/command.rs:82-182` | +3 start | CLOSE |
| `agent/mod.rs:147-262` | `agent/mod.rs:147-271` | +9 end | OFF BY 9 (borderline) |

**Status**: MOSTLY ACCURATE. `agent/mod.rs:147-262` is off by 9 lines (doc says 262, last agent ends at 271). All others are within 3 lines.

---

## Event Flow Diagram (overview.md:148-167)

The diagram shows:
```
User Input -> TUI Event Loop -> App::on_key() -> State Mutation -> Render
                                  |
                       CoreClient.request()
                                  |
                    AgentLoop, PermissionChecker, HookRegistry
                                  |
                    Provider <- ToolRegistry -> Tools
                                  |
                    GlobalEventBus::publish()
                                  |
                    CoreClient.subscribe() -> TUI updates
```

**Status**: CONFIRMED - matches the actual architecture. The TUI sends `CoreRequest` via `CoreClient`, which routes to `AgentLoop` (the main execution cycle). `AgentLoop` uses `Provider` for LLM calls, `ToolRegistry` for tool execution, and `PermissionChecker` for access control. Events are published to `GlobalEventBus` and the TUI subscribes for updates.

---

## Database Schema (overview.md:128-146)

Doc lists 7 tables: sessions, messages, parts, permissions, todos, usage, snapshots.

Actual tables in `src/session/schema.rs` (13 tables):
1. migration_version
2. project
3. session
4. message
5. part
6. todo
7. permission
8. session_share
9. cached_models
10. task
11. checkpoints
12. snapshot
13. usage

**Status**: UPDATE - doc lists 7 tables but actual schema has 13. Missing: migration_version, project, session_share, cached_models, task, checkpoints. Also, doc uses plural names (sessions, messages, etc.) but code uses singular (session, message, etc.).

---

## Provider Auto-Registration (overview.md:106-107)

Doc says: "Auto-registered: codegg_zen only"

Code shows:
- `register_builtin()` at `src/provider/mod.rs:279` registers only `codegg_zen`
- `register_builtin_with_config()` at `src/provider/mod.rs:390` registers both `codegg_zen` AND `codegg_go`

**Status**: UPDATE - `codegg_go` is also auto-registered when config is available. The doc only mentions `codegg_zen`.

---

## New Findings (not in doc)

### 1. Orphaned `src/git/mod.rs` (NEW)
- `src/git/mod.rs` exists but is not declared in `lib.rs` and not referenced anywhere
- Contains `GitSession` and `GitStatus` structs that appear unused
- Should be removed or properly integrated

### 2. Missing `arboard` and `debug-logging` feature flags (NEW)
- `Cargo.toml` defines `arboard` (default feature) and `debug-logging` (empty)
- Neither is mentioned in the Feature Gates table

### 3. `tool_search` not mentioned in Key Types (NEW)
- The doc says "27 built-in tools" but `tool_search` (ToolSearchTool) is registered at `tool/mod.rs:121-122`
- This brings the actual count to 28
- `tool_search` is a meta-tool for on-demand tool discovery and deserves mention

---

## Summary of Required Updates

| # | Location | Issue | Severity |
|---|----------|-------|----------|
| 1 | Line 75 | Tool count: 27 -> 28 | Low |
| 2 | Line 64 | Feature gate: `plugin` -> `plugins` | Low |
| 3 | Line 106 | Provider: "codegg_zen only" -> "codegg_zen and codegg_go" | Medium |
| 4 | Lines 130-146 | Database tables: 7 -> 13, fix plural names | Medium |
| 5 | Line 57 | `git/` module is orphaned code, not a real module | Medium |
| 6 | Lines 48-80 | Missing `exec` and `error` from module table | Low |
| 7 | Line 118 | `agent/mod.rs:147-262` off by 9 lines (should be 147-271) | Low |
| 8 | Lines 120-127 | Missing `arboard` and `debug-logging` features | Low |

## Improvement Opportunities

1. **Add exec and error to module table**: These are first-class modules declared in `lib.rs` with architecture docs. They should appear in the module map for completeness.

2. **Resolve orphaned `src/git/mod.rs`**: Either delete it (it's unused) or properly integrate it as a top-level module by adding `pub mod git;` to `lib.rs`.

3. **Document the `tool_search` meta-tool**: Since it brings the tool count to 28 and enables on-demand tool discovery, it deserves a mention in the Key Types section.

4. **Add `arboard` and `debug-logging` to feature gates**: These are real features defined in Cargo.toml that users may need to know about.
