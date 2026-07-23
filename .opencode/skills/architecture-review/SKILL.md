---
name: architecture-review
description: Guide for reviewing architecture documentation against actual codebase in codegg
version: 1.0.0
process: parallel-subagent
---

# Architecture Review Skill

Systematic process for verifying architecture documentation against source code, identifying documentation bugs, code issues, and stale content.

## When to Use

- After significant code changes that may have drifted from documentation
- Before major releases to ensure docs are accurate
- When onboarding new contributors who need reliable documentation

## Review Process

### Phase 1: Batch Review (Parallel)

Launch subagents for each batch of related architecture files. Each subagent:

1. **Reads** the assigned architecture document(s) fully
2. **Searches** for referenced source files, types, counts, and line numbers in `src/`
3. **Verifies** every concrete claim (counts, field names, variant names, line references)
4. **Identifies** documentation errors, code bugs, improvement opportunities, stale content
5. **Writes** findings to `plans/review_<batch_name>.md`

### Batch Structure

| Batch | Files | Focus |
|-------|-------|-------|
| 0 | overview.md | Meta-document, module table, counts |
| 1 | protocol.md, agent.md, compaction.md, command.md | Core protocol and agent loop |
| 2 | core.md, server.md, client.md, exec.md | Core facade and transport |
| 3 | provider.md, resilience.md, error.md, config.md | Provider, resilience, error |
| 4 | permission.md, security.md | Permission, security, safety |
| 5 | session.md, storage.md, snapshot.md, git.md, worktree.md | Persistence layer |
| 6 | mcp.md, lsp.md, plugin.md, hooks.md | External integrations |
| 7 | tui.md, tool.md, skills.md | TUI, tools, skills |
| 8 | bus.md, memory.md, shell_session.md, tts.md, upgrade.md, util.md, crypto.md, ide.md | Remaining modules |

### Phase 2: Consolidation

Read all batch review files and produce `plans/review_consolidated.md`:
- Deduplicate findings across batches
- Rank by severity (HIGH > MEDIUM > LOW)
- Group into: documentation fixes, code issues, improvements, stale content
- Identify cross-module issues

### Phase 3: Stale Item Pruning

- Check for `src/` modules without architecture docs
- Check for architecture docs referencing non-existent modules
- Flag entirely stale documents

## Verification Checklist

Each subagent must confirm:
- [ ] Read the full architecture document
- [ ] Located each referenced source file in `src/`
- [ ] Verified at least 3 concrete counts/numbers against code
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

| Item | Expected | Source |
|------|----------|--------|
| Tool count (base) | ~38 | `src/tool/mod.rs:with_options()` |
| Tool count (all features) | ~51 | includes 8 always-visible + 5 deferred eggsact deterministic tools |
| LSP servers | 39 | `crates/egglsp/src/server.rs` |
| AppEvent variants | 45 | `crates/codegg-core/src/bus/events.rs` |
| Built-in commands | 108 | `src/tui/command.rs` (assertion at line 525) |
| Built-in agents | 9 | `assets/agents/*.toml` |
| DB tables | ~50 | `crates/codegg-core/src/session/schema.rs` |
| Native tool crates | 10 | `crates/` workspace |
