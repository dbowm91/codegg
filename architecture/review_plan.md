# Architecture Review Plan

Generated: 2026-05-23
Last Updated: 2026-05-26

## Status: INCOMPLETE

This plan is maintained for iterative improvement. See below for review completion status.

## Overview

This plan orchestrates parallel review of all architecture documents (excluding this file). Each module is assigned to a dedicated subagent that will:
1. Read the architecture document
2. Verify claims against source code
3. Identify bugs, inconsistencies, and improvement opportunities
4. Write findings to `plans/<module>.md`

## Review Completion Status

| Module | Status | Notes |
|--------|--------|-------|
| agent | Pass | Minor line number updates needed |
| client | Partial | RenderFrame table placement fixed |
| command | Partial | Duplicate table removed, alias format fixed |
| compaction | New Doc | architecture/compaction.md created |
| config | Pass | Minor WatcherConfig field naming |
| crypto | Pass | - |
| error | Pass | - |
| event-bus | Pass | Other Events count fixed (9→8) |
| exec | Pass | PROVIDER_NOT_FOUND added |
| hooks | Pass | - |
| ide | Pass | Line range slicing bug fixed |
| lsp | Pass | - |
| mcp | Partial | Missing documentation items |
| memory | Pass | - |
| permission | Pass | PermissionResponse clarification |
| plugin | Pass | - |
| provider | Pass | - |
| pty | Pass | Location path fixed |
| resilience | Pass | - |
| security | Pass | - |
| server | Partial | SSE ResyncRequired issue noted |
| session | Pass | - |
| skills | Pass | - |
| snapshot | Partial | Restore flow documentation |
| storage | Pass | - |
| tool | Partial | Missing teams/lsp tools |
| tts | Pass | TTS bugs fixed, skill updated |
| tui | Pass | Theme count fixed |
| upgrade | Pass | - |
| util | Pass | - |
| worktree | Pass | - |

## Modules and Assignments

| Module | Architecture Doc | Review Output |
|--------|------------------|---------------|
| agent | architecture/agent.md | plans/agent.md |
| client | architecture/client.md | plans/client.md |
| command | architecture/command.md | plans/command.md |
| compaction | architecture/compaction.md | plans/compaction.md |
| config | architecture/config.md | plans/config.md |
| crypto | architecture/crypto.md | plans/crypto.md |
| error | architecture/error.md | plans/error.md |
| event-bus | architecture/event-bus.md | plans/event-bus.md |
| exec | architecture/exec.md | plans/exec.md |
| hooks | architecture/hooks.md | plans/hooks.md |
| ide | architecture/ide.md | plans/ide.md |
| lsp | architecture/lsp.md | plans/lsp.md |
| mcp | architecture/mcp.md | plans/mcp.md |
| memory | architecture/memory.md | plans/memory.md |
| permission | architecture/permission.md | plans/permission.md |
| plugin | architecture/plugin.md | plans/plugin.md |
| provider | architecture/provider.md | plans/provider.md |
| pty | architecture/pty.md | plans/pty.md |
| resilience | architecture/resilience.md | plans/resilience.md |
| security | architecture/security.md | plans/security.md |
| server | architecture/server.md | plans/server.md |
| session | architecture/session.md | plans/session.md |
| skills | architecture/skills.md | plans/skills.md |
| snapshot | architecture/snapshot.md | plans/snapshot.md |
| storage | architecture/storage.md | plans/storage.md |
| tool | architecture/tool.md | plans/tool.md |
| tts | architecture/tts.md | plans/tts.md |
| tui | architecture/tui.md | plans/tui.md |
| upgrade | architecture/upgrade.md | plans/upgrade.md |
| util | architecture/util.md | plans/util.md |
| worktree | architecture/worktree.md | plans/worktree.md |

## Execution Instructions

Launch subagents in parallel batches (5-6 at a time) to review modules. Each subagent should:

1. **Read the architecture document** for their assigned module
2. **Locate corresponding source code** in `src/<module>/`
3. **Verify claims**: Check each statement in the doc against implementation
4. **Identify issues**:
   - Outdated descriptions
   - Missing undocumented features
   - Bugs or anti-patterns
   - Missing error handling
   - Performance concerns
   - Security issues
5. **Write improvement plan** to `plans/<module>.md` with:
   - Summary of verification findings
   - List of issues with file:line references
   - Specific improvement recommendations

## Batch Execution

### Batch 1
- agent, client, command, compaction, config

### Batch 2
- crypto, error, event-bus, exec, hooks

### Batch 3
- ide, lsp, mcp, memory, permission

### Batch 4
- plugin, provider, pty, resilience, security

### Batch 5
- server, session, skills, snapshot, storage

### Batch 6
- tool, tts, tui, upgrade, util, worktree

## Review Output Template

```markdown
# <Module> Architecture Review

## Architecture Document
- Path: architecture/<module>.md
- Last Updated: <date from doc>

## Source Code Location
- `src/<module>/`

## Verification Summary
<Pass/Fail/Partial> - <brief description>

## Verified Claims
| Claim | Status | Notes |
|-------|--------|-------|
| ... | ... | ... |

## Issues Found

### Bugs
- ...

### Inconsistencies
- ...

### Missing Documentation
- ...

### Improvement Opportunities
- ...

## Recommendations
1. ...
```