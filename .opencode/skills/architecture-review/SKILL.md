---
name: architecture-review
description: Guide for reviewing architecture documentation against actual codebase in codegg
version: 2.0.0
process: parallel-subagent
tags:
  - architecture
  - documentation
  - audit
  - review
---

# Architecture Review Skill

Systematic process for verifying architecture documentation against source code, identifying documentation bugs, code issues, and stale content.

## When to Use

- After significant code changes that may have drifted from documentation
- Before major releases to ensure docs are accurate
- When onboarding new contributors who need reliable documentation

## Count Source of Truth

Do NOT copy numeric claims between documents. `architecture/overview.md`
"Verified counts" is the single source of truth for counts (tool
registrations, LSP servers, AppEvent variants, slash commands, agents,
DB tables, layout version, test files, guard scripts). Verify each count
against the `Source` column listed there, fix `overview.md` first if the
code moved on, then align every other document to it.

Do not trust any snapshot, including this one. Re-measure with
`python3 scripts/measure_overview_counts.py`, which applies each row's
`Source` method from the `overview.md` table and prints one row per count.

Snapshot re-measured 2026-10-27 (a stale-trip alarm, not evidence): 67 tool
registrations in `src/tool/mod.rs::with_options()` (53 `register` + 14
`register_boxed`), 79 measured distinct production tool names (63 literal +
16 counted `eggsact` names, of which the 16th — `probe` — is
`#[cfg(test)]`-only, so true production is 78), 40 LSP server
definitions (`crates/egglsp/src/server.rs::server_definitions()`), 61
`AppEvent` variants, 169 built-in slash commands, 10 built-in agents, 100
`CREATE TABLE` statements / 96 tables / storage layout 91, 355
integration test files, 92
architecture docs, 104 `check_*` guards of which CI runs 70.

**Scope the production-tool row to the whole `src/` tree.** It previously
measured `src/tool/` + `src/context/` only, which hid `python_script` — a
production tool registered unconditionally by `with_options()` that lives in
`src/python_script/`. A measure scoped to where you *expect* to find
something will not find the thing that lives somewhere else.

**A literal-name scan is not a name inventory.** The same row also missed the
15 production `eggsact` deterministic validators (`text_equal` …
`unit_convert`; a 16th counted name, `probe`, is test-only), whose
`impl Tool for EggsactTool` (`src/tool/deterministic.rs:72`)
returns `self.codegg_name` instead of a literal — so they are counted at
their `EggsactTool::new()` call sites, giving 79 measured (63 literal + 16
runtime-counted; true production 78). The same caution applies to a row whose *label* is narrower
than its measure: "CI guard scripts" was published as 76 (every
`scripts/check_*`) when CI ran 36 of them, and the row is now split into
"Guard scripts (all)" and "CI guard scripts".

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
document is covered. The table below covers all 92 docs as of 2026-10-23;
if `overview.md` lists documents not present here, extend the batches
rather than silently skipping them.

A batch holding one very large document (`lsp.md`, `tui.md`) may be split
into two sub-agents that own disjoint files. Splitting is preferred over
dropping a document: a subagent that reads many large files at once can hit
a token-per-minute rate limit, so instruct each to work **one document at a
time** and to edit as it goes rather than batching all edits to the end.

| Batch | Files | Focus |
|-------|-------|-------|
| 0 | overview.md, count_audit_history.md | Meta-document, module map, verified counts and their measurement history |
| 1 | agent.md, agent-tool-surface.md, compaction.md, cache-aware-context.md, context-ledger.md, context-compaction-ownership.md, model_profile_task_state.md, goal.md, research.md, work_orders.md, work_plan.md, blackboard_contract_matrix.md | Agent context and execution |
| 2 | command_intent.md, command_planner.md, command_routing.md, exec.md, test_runner.md, python_scripting.md, human_shell.md | Command pipeline and execution |
| 3 | tool.md, tool_broker.md, tool_programs.md, tool_program_language.md, deterministic_tools.md, preflight.md, tool-advisor.md, tool-advisor-framework-spike.md | Tool layer and programs |
| 4 | git.md, git_phase_f_handoff.md, git_polish_verification_handoff.md, worktree.md, snapshot.md, run_store.md, writer_permit.md | Git, worktree, run artifacts, writer permit |
| 5 | session.md, storage.md, project_catalog.md, project_identity_storage.md, identity.md, codegg_core.md, native_crates.md | Persistence and identity |
| 6 | core.md, client.md, server.md, acp.md, protocol.md, presence.md, collaboration.md, desktop.md | Core facade and transport |
| 7 | provider.md, model-adapters.md, resilience.md, retry.md, error.md, config.md, auth.md, crypto.md | Providers, config, crates |
| 8 | permission.md, security.md, authorization.md, audit.md, approval_reviewer.md, lsp_disk_cache_threat_model.md | Security and authorization |
| 9 | mcp.md, plugin.md, hooks.md, ide.md, search_backend.md, skills.md, windows-mxc-feasibility.md | External integrations |
| 10 | lsp.md | Language Server Protocol (authoritative `egglsp`) |
| 11 | tui.md, command.md, theme.md, document.md, bus.md, projection.md | TUI, commands, events |
| 12 | jobs.md, scheduler.md, workspace.md, workspace_services.md, memory.md, tts.md, upgrade.md, util.md, testing.md, process-tool-execution-ownership.md, build_admission.md, session_launch.md, workstream_claims.md | Daemon services and support |

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
landed (or been explicitly rejected with rationale), move them with `git mv`
into `plans/archive/reviews/`, preserving relative structure per the `planning`
skill archive workflow, and update any inbound links. Do not leave completed
review batches beside active planning state indefinitely.

`plans/archive/reviews/` already holds two archived rounds: the 2026-09-11
batch set and the 2026-09-13 set (archived 2026-10-06 after its findings were
re-derived from source — several were themselves wrong). Name a new round by
its review date to avoid collisions, and add the archival note the archive
README requires.

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

See `architecture/overview.md` "Verified counts" — that table is
authoritative. Re-verify every entry against its listed source; the
snapshot in this skill's "Count Source of Truth" section is a stale-trip
alarm, not evidence. Pay special attention to historically drifting
claims: tool registration call sites vs distinct production tool names,
LSP server definitions (struct definition is not a server entry),
`AppEvent` variants, the slash-command registry test, `CREATE TABLE`
names vs `STORAGE_LAYOUT_VERSION`, and env-var vs config-defined
providers.

## See Also

- `architecture/overview.md` — the authoritative module map and the
  `## Verified counts` table this skill's count snapshot mirrors
- `architecture/count_audit_history.md` — how each count was measured, and the
  reconciliation history that pinned them
- `architecture/testing.md` — the test taxonomy the integration-test-file count
  depends on
- `.opencode/skills/documentation/SKILL.md` — the doc-writing discipline that
  this review process feeds
- `scripts/measure_overview_counts.py` — the measurement tool the count
  snapshot must be re-derived with

## Source verification

Verified 2026-10-06 against `architecture/overview.md` (its `## Verified
counts` table and its `## Static guards` section), a full `architecture/*.md` listing diffed against the
batch table, and direct measurement (`src/tool/mod.rs`,
`crates/egglsp/src/server.rs`, `crates/codegg-core/src/bus/events.rs`,
`src/tui/command.rs`, `assets/agents/*.toml`,
`crates/codegg-core/src/session/schema.rs`,
`crates/codegg-core/src/storage/mod.rs::STORAGE_LAYOUT_VERSION`, `tests/*.rs`,
`scripts/check_*`). Corrected: the count snapshot (53 tool registrations /
53 `AppEvent` variants / 139 slash commands / 71 tables / layout 56 / 189
integration tests / 77 docs / 21 guards → 63 / 58 / 154 / 73 / 68 / 215 /
85 / 40), the batch coverage claim ("all 77 docs as of 2026-09-13" → all 85
as of 2026-10-06) with the eight uncovered documents added to the batches
`overview.md`'s Module Map assigns them to, and the section name
"Verified Counts" → "Verified counts". Dropped the "~31 always-registered
core" and "15 env-var providers" figures — neither is in the authoritative
table and neither has a traceable source. Claims without a traceable source
were removed rather than guessed.

Fourth pass (2026-10-23): re-ran `python3 scripts/measure_overview_counts.py`
against the live tree and confirmed **every figure** in this skill's "Count
Source of Truth" snapshot agrees with the authoritative `## Verified counts`
table — 67 tool registrations (54 `register` + 13 `register_boxed`), 62
distinct production tool names, 39 LSP server definitions, 61 `AppEvent`
variants, 168 built-in slash commands, 10 built-in agents, 91 tables /
storage layout 87, 321 integration test files, 92 architecture docs, 76
`check_*` guards (`ls scripts/check_* | wc -l` → 76). Also diffed the batch
table's document list against `architecture/*.md`: 92 covered, 0 missing,
0 extra. Corrected three stale line references that had drifted past the
5-line tolerance: the `## Verified counts` table moved to
`architecture/overview.md:298-336` (was `:263-292`); the built-in slash
command tripwire is the assertion `built_in_command_count_matches_release_docs`
at `src/tui/command.rs:1250` (was `:1104-1107`, which now points at
`CommandRegistry::commands()`); and `STORAGE_LAYOUT_VERSION` is at
`crates/codegg-core/src/storage/mod.rs:48` (was `:39`, which now points at the
const's doc comment). Line-number references to that table are fragile — the
table's position moves whenever a row is added — so the body now names the
section rather than pinning its range.

Fifth pass (2026-10-23, after the provider-reasoning-control milestone
landed on `main`): re-ran `python3 scripts/measure_overview_counts.py`
against the live tree. Three figures in the snapshot above had moved and
were corrected — **storage layout 87 -> 88** (the milestone's migration
was renumbered v84 -> v88 because `main` had already published v84
through v87), **guard scripts 76 -> 80**, and **CI guard scripts 36 -> 40**.
The fourth pass's pinned ranges are kept as the record of that pass and
are superseded: the `## Verified counts` table is now at
`architecture/overview.md:403-536`, the slash-command tripwire is at
`src/tui/command.rs:1217`, and `STORAGE_LAYOUT_VERSION` is at
`crates/codegg-core/src/storage/mod.rs:56`. The lesson stands: name the
section, and re-measure rather than trusting a snapshot — including this
one.

Sixth pass (2026-10-27, after the session-cwd + repository-build-admission
milestone `5194e543a` landed on `main`): re-ran
`python3 scripts/measure_overview_counts.py` against the live tree and
updated the snapshot above — **tool registrations 54+13 -> 53+14**,
**measured names 75 -> 79** (63 literal + 16 counted; true production 78
with the test-only `probe` excluded; new names `session_working_directory`,
`math_eval`, `unit_convert`), **LSP servers 39 -> 40**, **slash commands
168 -> 169** (tripwire now at `src/tui/command.rs:1277`), **tables 91 ->
100 statements / 96 tables / layout 91**, **test files 321 -> 355**,
**guard scripts 80 -> 104**, **CI guards 40 -> 70**. `overview.md` itself
was refreshed in the same pass (new reconciliation note, exact tool
table). Earlier passes' pinned ranges stay as their record and are
superseded the same way.

