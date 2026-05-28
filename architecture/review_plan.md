# Architecture Review Plan

**Status**: ACTIVE - Fresh review sweep starting 2026-05-28
**Objective**: Systematically review all architecture documentation, verify claims against code, identify bugs/improvements, and prune stale content
**Last Updated**: 2026-05-28

---

## Scope

All 35 architecture documents in `architecture/` directory (excluding `review_plan.md` itself). The `overview.md` file is reviewed in Batch 0 as a standalone pass since it serves as the index.

## Review Principles

1. **Verify before correcting** — Always read the actual source code before marking a claim as wrong. Counts, line numbers, and feature descriptions drift over time.
2. **Distinguish docs bugs from code bugs** — Not every documentation inconsistency is a code bug. Clearly separate the two.
3. **No direct code changes in plans** — Each subagent's output is an improvement *plan*, not a patch. The executing agent decides what to fix.
4. **Check for stale items** — After all batches complete, a final pass prunes outdated information and removes files documenting modules that no longer exist or have been superseded.
5. **Work within workspace only** — All subagent work stays in `/Users/davidbowman/projects/codegg/`.

---

## Batch Structure

Modules are grouped into 8 batches of 4-5 files each. Related modules are batched together to reduce cross-referencing overhead. Each batch is assigned a **Task agent** that reads the architecture docs, verifies claims against `src/`, and writes findings to `plans/<batch_name>.md`.

### Batch 0: Overview Index
**File**: `overview.md`
**Source modules**: N/A (meta-document)
**Verify**:
- Module table in `overview.md:48-80` — are all modules still in `src/`? Are any renamed/removed?
- Verified Counts table in `overview.md:109-118` — do counts still match source?
- Feature Gates table in `overview.md:120-127` — are feature flags still accurate?
- Navigation links in `overview.md:186-219` — do all target files still exist?
- Event Flow diagram — does it still reflect the actual architecture?
- Database Schema section — does it reflect current migrations?
- Key Types section — are agent/tool/event counts current?
**Output**: `plans/review_overview.md`

---

### Batch 1: Core Protocol and Agent Loop
**Files**: `protocol.md`, `agent.md`, `compaction.md`, `command.md`
**Rationale**: Protocol defines the message types that AgentLoop and Command consume; compaction is tightly coupled to agent loop context management.
**Verify**:
- `protocol.md`: CoreRequest/CoreResponse/CoreEvent/TuiMessage variant counts, field names, line references
- `agent.md`: AgentLoop lifecycle, subagent types, prompt templates, routing logic
- `compaction.md`: Compaction strategies, thresholds, trigger conditions
- `command.md`: Command count (should be 46), template system, markdown source files
**Output**: `plans/review_protocol_agent.md`

---

### Batch 2: Core Facade and Transport
**Files**: `core.md`, `server.md`, `client.md`, `exec.md`
**Rationale**: Core is the request/response layer; server and client are transport adapters; exec is a non-interactive variant.
**Verify**:
- `core.md`: InprocCoreClient fields, transport adapter types, CoreRequest handler coverage
- `server.md`: Axum routes, WebSocket handling, REST API, SSE, auth middleware
- `client.md`: WebSocket client, resume handshake, replay buffer
- `exec.md`: JSON I/O, question channel deadlock fix, exec mode constraints
**Output**: `plans/review_core_server.md`

---

### Batch 3: Provider, Resilience, and Error
**Files**: `provider.md`, `resilience.md`, `error.md`, `config.md`
**Rationale**: Providers use resilience (circuit breaker, fallback); error types span provider/tool/MCP; config drives provider registration.
**Verify**:
- `provider.md`: Provider count, auto-registration (codegg_go only), fallback logic, SSE parser references
- `resilience.md`: Circuit breaker states, backoff formula (`2^i`), FallbackProvider behavior
- `error.md`: AppError/ProviderError/ToolError/McpError/LspError variants, `is_retryable()` implementations
- `config.md`: Schema fields, encryption, file watcher, validation rules
**Output**: `plans/review_provider_resilience.md`

---

### Batch 4: Permission, Security, and Safety
**Files**: `permission.md`, `security.md`, `sandbox.md` (if exists, else note absence)
**Rationale**: Permission system controls tool access; security module handles SSRF, sandboxing; these are tightly coupled safety concerns.
**Verify**:
- `permission.md`: PermissionChecker, DoomLoop detection, mode system (Review/Debug/Docs), PermissionRegistry sync behavior, mode tool restrictions
- `security.md`: SSRF validation (RFC 6892), internal IP ranges, Landlock sandbox, `fc00::/7` vs `fc00::/8`
- Sandbox: If `sandbox.md` doesn't exist, note that Landlock info may be split between security.md and skill docs
- Check `CANONICAL_PATHS_CACHE` status — was TTL added?
**Output**: `plans/review_permission_security.md`

---

### Batch 5: Session, Storage, Snapshot, and Git
**Files**: `session.md`, `storage.md`, `snapshot.md`, `git.md`, `worktree.md`
**Rationale**: Session storage uses SQLite (storage); snapshots capture file state; git/worktree manage repo state — all persistence-layer modules.
**Verify**:
- `session.md`: Tables, schema, checkpoint, import/export
- `storage.md`: Migration count (should be v1-v15), WAL mode, connection pooling
- `snapshot.md`: SHA256 consistency, atomic restore, collect_files_sync hash
- `git.md`: GitSession, git info injection, worktree per session
- `worktree.md`: find_git_root, symlink detection, line number references (172/180, not 36/56)
**Output**: `plans/review_session_storage.md`

---

### Batch 6: MCP, LSP, and Plugin
**Files**: `mcp.md`, `lsp.md`, `plugin.md`, `hooks.md`
**Rationale**: All three are external system integration layers; hooks connect plugins to agent lifecycle.
**Verify**:
- `mcp.md`: Local vs remote clients, OAuth flow, dead code (connect_sse, run_socket), auto-reconnect, DNS re-validation
- `lsp.md`: Server count (should be 39), diagnostics, code operations, server_definitions array
- `plugin.md`: WASM execution, fuel tracking, hook_timeout vs WASM_HOOK_TIMEOUT, manifest format
- `hooks.md`: HookType variants (should be 13), lifecycle events, SessionCompacting dispatch
**Output**: `plans/review_mcp_lsp_plugin.md`

---

### Batch 7: TUI, Tool, and Skills
**Files**: `tui.md`, `tool.md`, `skills.md`, `tool-search.md` (if exists)
**Rationale**: TUI renders tool output; tool-search discovers tools; skills provide specialized tool sets.
**Verify**:
- `tui.md`: UiState fields (should be 26), Component trait (Send + Any), Dialog variants (including Stats), keyboard shortcuts, FocusManager
- `tool.md`: Tool count (should be 27), ToolCatalog::register(&dyn Tool), path validation, async command, ImageTool status
- `skills.md`: SkillIndex, activation via /skill:, .skills/ directory structure
- Check for `tool-search` doc or note absence
**Output**: `plans/review_tui_tool.md`

---

### Batch 8: Bus, Memory, Shell, and Remaining
**Files**: `bus.md`, `memory.md`, `shell_session.md`, `tts.md`, `upgrade.md`, `util.md`, `crypto.md`, `ide.md`
**Rationale**: These are smaller/singleton modules that don't warrant full batches; grouped for efficiency.
**Verify**:
- `bus.md`: AppEvent variants (should be 36), GlobalEventBus buffer (2048), PermissionRegistry/QuestionRegistry sync
- `memory.md`: Consolidation, namespace management, session-to-session learning
- `shell_session.md`: Metadata management, no PTY caveat
- `tts.md`: macOS `say` command, auto-stop on AgentFinished, toggle commands
- `upgrade.md`: GitHub releases, self-upgrade flow
- `util.md`: Clipboard, fuzzy matching, pricing.rs, metrics, truncation
- `crypto.md`: AES-256-GCM, Argon2id key derivation
- `ide.md`: VS Code IPC, JetBrains remote mode, diff viewing
**Output**: `plans/review_bus_memory_misc.md`

---

## Execution Workflow

### Phase 1: Parallel Subagent Review (Batches 0-8)

Launch 9 Task agents in parallel. Each agent:

1. **Reads** the assigned architecture document(s)
2. **Searches** for referenced source files, types, counts, and line numbers in `src/`
3. **Verifies** every concrete claim (counts, field names, variant names, line references, feature descriptions)
4. **Identifies**:
   - Documentation errors (wrong counts, stale line numbers, missing fields)
   - Code bugs surfaced during verification (dead code, inconsistencies, missing registrations)
   - Improvement opportunities (incomplete implementations, missing error handling, unclear APIs)
   - Stale content (historical notes, references to deleted files, outdated descriptions)
5. **Writes** findings to `plans/review_<batch_name>.md` using this template:

```markdown
# Review: <Batch Name>

**Reviewed**: <date>
**Files**: <architecture files reviewed>

## Summary

<1-2 paragraph overview of what was found>

## Documentation Issues

| # | File | Line | Issue | Action |
|---|------|------|-------|--------|
| 1 | architecture/X.md | L42 | "40 tools" should be "42 tools" | UPDATE |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | foo | Bar is dead code | src/foo/bar.rs:12 | LOW |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | foo | Add retry logic to bar | Reliability |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | architecture/X.md:200-210 | Historical note about removed feature | Feature removed in v2 |
```

### Phase 2: Consolidation

After all 9 batches complete:

1. **Read** all `plans/review_*.md` files
2. **Consolidate** into a single `plans/review_consolidated.md` that:
   - Deduplicates findings across batches
   - Ranks issues by severity (HIGH > MEDIUM > LOW)
   - Groups documentation fixes vs code fixes vs improvements
   - Identifies cross-module issues (e.g., inconsistent auth patterns between server and middleware)
3. **Update** `architecture/review_plan.md` with final status and consolidated summary

### Phase 3: Stale Item Pruning

Based on consolidated findings:

1. **Mark** documentation files or sections for removal/update in the consolidated plan
2. **Flag** any architecture documents that are entirely stale or superseded
3. **Note** any modules in `src/` that have no corresponding architecture document
4. **Note** any architecture documents that reference modules no longer in `src/`

### Phase 4: Final Commit

1. Commit all `plans/review_*.md` files and the updated `architecture/review_plan.md` to main

---

## Output File Map

| Batch | Output File | Modules |
|-------|-------------|---------|
| 0 | `plans/review_overview.md` | overview.md |
| 1 | `plans/review_protocol_agent.md` | protocol.md, agent.md, compaction.md, command.md |
| 2 | `plans/review_core_server.md` | core.md, server.md, client.md, exec.md |
| 3 | `plans/review_provider_resilience.md` | provider.md, resilience.md, error.md, config.md |
| 4 | `plans/review_permission_security.md` | permission.md, security.md |
| 5 | `plans/review_session_storage.md` | session.md, storage.md, snapshot.md, git.md, worktree.md |
| 6 | `plans/review_mcp_lsp_plugin.md` | mcp.md, lsp.md, plugin.md, hooks.md |
| 7 | `plans/review_tui_tool.md` | tui.md, tool.md, skills.md |
| 8 | `plans/review_bus_memory_misc.md` | bus.md, memory.md, shell_session.md, tts.md, upgrade.md, util.md, crypto.md, ide.md |
| — | `plans/review_consolidated.md` | All (consolidation output) |

---

## Verification Checklist (for each subagent)

Before writing findings, each subagent must confirm:

- [ ] Read the full architecture document (not just skimming)
- [ ] Located each referenced source file in `src/`
- [ ] Verified at least 3 concrete counts/numbers against code
- [ ] Checked line number references (flag if off by >5 lines)
- [ ] Verified enum variant counts by counting actual `Variant` entries
- [ ] Checked for dead code references (methods/types defined but unused)
- [ ] Noted any inconsistencies between doc and code
- [ ] Identified at least 1 improvement opportunity per module

---

*Review plan created: 2026-05-28*
*Supersedes previous review plan (2026-05-27)*
