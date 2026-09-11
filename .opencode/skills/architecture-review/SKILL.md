---
name: architecture-review
description: Guide for reviewing architecture documentation against actual codebase in codegg
version: 2.0.0
process: parallel-subagent
---

# Architecture Review Skill

Systematic process for verifying architecture documentation against source code, identifying documentation bugs, code issues, and stale content.

## When to Use

- After significant code changes that may have drifted from documentation
- Before major releases to ensure docs are accurate
- When onboarding new contributors who need reliable documentation

## Count Source of Truth

Do NOT copy numeric claims between documents. `architecture/overview.md`
"Verified Counts" is the single source of truth for counts (tool
registrations, LSP servers, AppEvent variants, slash commands, agents,
DB tables, layout version, test files, guard scripts). Verify each count
against the `Source` column listed there, fix `overview.md` first if the
code moved on, then align every other document to it.

Snapshot verified 2026-09-11 (re-verify; do not trust blindly): 51 tool
registration statements (`src/tool/mod.rs::with_options()`), ~31 always
registered core, 39 LSP servers (`crates/egglsp/src/server.rs`), 53
AppEvent variants, 139 slash commands, 10 agents, 71 tables / layout 56,
189 integration tests, 77 architecture docs, 21 `check_*` guards, 15
env-var providers (+ config-defined ones).

## Review Process

### Phase 1: Batch Review (Parallel)

Launch subagents for each batch of related architecture files. Each subagent:

1. **Reads** the assigned architecture document(s) fully
2. **Searches** for referenced source files, types, counts, and line numbers in `src/`
3. **Verifies** every concrete claim (counts, field names, variant names, line references)
4. **Identifies** documentation errors, code bugs, improvement opportunities, stale content
5. **Writes** findings to `plans/review_<batch_name>.md`

### Batch Structure

Derive batches from the `architecture/overview.md` Module Map so every
document is covered. The table below covers all 77 docs as of 2026-09-11;
if `overview.md` lists documents not present here, extend the batches
rather than silently skipping them.

| Batch | Files | Focus |
|-------|-------|-------|
| 0 | overview.md | Meta-document, module table, verified counts |
| 1 | agent.md, agent-tool-surface.md, compaction.md, cache-aware-context.md, context-ledger.md, context-compaction-ownership.md, model_profile_task_state.md, goal.md, research.md | Agent context and execution |
| 2 | command_intent.md, command_planner.md, command_routing.md, exec.md, test_runner.md, python_scripting.md | Command pipeline and execution |
| 3 | tool.md, tool_broker.md, tool_programs.md, tool_program_language.md, deterministic_tools.md, preflight.md | Tool layer and programs |
| 4 | git.md, git_phase_f_handoff.md, git_polish_verification_handoff.md, worktree.md, snapshot.md, run_store.md | Git, worktree, run artifacts |
| 5 | session.md, storage.md, project_catalog.md, project_identity_storage.md, identity.md | Persistence and identity |
| 6 | core.md, client.md, server.md, acp.md, protocol.md, presence.md, collaboration.md | Core facade and transport |
| 7 | provider.md, model-adapters.md, resilience.md, error.md, config.md, codegg_core.md, native_crates.md | Providers, config, crates |
| 8 | permission.md, security.md, auth.md, crypto.md, authorization.md, audit.md | Security and authorization |
| 9 | mcp.md, lsp.md, lsp_disk_cache_threat_model.md, plugin.md, hooks.md, ide.md, search_backend.md | External integrations |
| 10 | tui.md, command.md, theme.md, human_shell.md, skills.md, bus.md, projection.md | TUI, commands, events |
| 11 | jobs.md, scheduler.md, workspace.md, workspace_services.md, memory.md, tts.md, upgrade.md, util.md, testing.md | Daemon services and support |

### Phase 2: Consolidation

Read all batch review files and produce `plans/review_consolidated.md`:
- Deduplicate findings across batches
- Rank by severity (HIGH > MEDIUM > LOW)
- Group into: documentation fixes, code issues, improvements, stale content
- Identify cross-module issues

### Phase 3: Stale Item Pruning

- Check for `src/` and `crates/` modules without architecture docs
- Check for architecture docs referencing non-existent modules
- Flag entirely stale documents

### Phase 4: Archive Working Papers

Review outputs (`plans/review_<batch_name>.md`, `plans/review_consolidated.md`)
are interim working papers, not permanent plans. Once their findings have
landed (or been explicitly rejected with rationale), move them under
`plans/archive/` preserving relative structure per the `planning` skill
archive workflow, and update any inbound links. Do not leave completed
review batches beside active planning state indefinitely.

## Verification Checklist

Each subagent must confirm:
- [ ] Read the full architecture document
- [ ] Located each referenced source file in `src/` or `crates/`
- [ ] Verified at least 3 concrete counts/numbers against code AND against
  the `overview.md` Verified Counts table (flag divergence either way)
- [ ] Checked line number references (flag if off by >5 lines)
- [ ] Verified enum variant counts by counting actual entries
- [ ] Checked for dead code references
- [ ] Noted any inconsistencies between doc and code
- [ ] Identified at least 1 improvement opportunity per module

## Common Issues Found

| Issue Type | Example | Fix |
|------------|---------|-----|
| Stale line numbers | Provider trait at lines 60-73, actual 74-87 | Update line references |
| Wrong counts | Tool count 27, actual 28 | Update count in doc |
| Missing fields | AgentLoop missing 9 fields | Add fields to struct listing |
| Wrong behavior | exec.md says questions not supported, actually waits 300s | Correct behavior description |
| Dead code references | ToolExecutor section referencing deleted file | Remove section |
| Phantom types | PermissionResponse referenced but doesn't exist | Remove reference |

## Output Template

```markdown
# Review: <Batch Name>

**Reviewed**: <date>
**Files**: <architecture files>

## Summary
<1-2 paragraph overview>

## Documentation Issues
| # | File | Line | Issue | Action |
|---|------|------|-------|--------|

## Code Issues Found
| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|

## Improvement Opportunities
| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|

## Stale Content to Prune
| # | File | Content | Reason |
|---|------|---------|--------|
```

## Key Counts to Verify

See `architecture/overview.md` "Verified Counts" — that table is
authoritative. Re-verify every entry against its listed source; the
snapshot in this skill's "Count Source of Truth" section is a stale-trip
alarm, not evidence. Pay special attention to historically drifting
claims: tool registration statements vs always-registered core tools,
LSP server definitions (struct definition is not a server entry),
`AppEvent` variants, the slash-command registry test, `CREATE TABLE`
names vs `STORAGE_LAYOUT_VERSION`, and env-var vs config-defined
providers.
