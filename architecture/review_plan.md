# Architecture Review Plan

Generated: 2026-05-23

## Overview

This plan orchestrates parallel review of all 30 architecture documents. Each module is assigned to a subagent that will:
1. Read the architecture document
2. Read the corresponding source code
3. Verify claims against implementation
4. Identify bugs and improvements
5. Write findings to `plans/<module>-review.md`

## Module Assignments

### Batch 1: Core Infrastructure
| Module | Architecture Doc | Source Path | Review Output |
|--------|-----------------|-------------|---------------|
| agent | `architecture/agent.md` | `src/agent/` | `plans/agent-review.md` |
| event-bus | `architecture/event-bus.md` | `src/bus/` | `plans/event-bus-review.md` |
| error | `architecture/error.md` | `src/error/` | `plans/error-review.md` |
| config | `architecture/config.md` | `src/config/` | `plans/config-review.md` |
| storage | `architecture/storage.md` | `src/storage/` | `plans/storage-review.md` |

### Batch 2: Communication & Integration
| Module | Architecture Doc | Source Path | Review Output |
|--------|-----------------|-------------|---------------|
| provider | `architecture/provider.md` | `src/provider/` | `plans/provider-review.md` |
| client | `architecture/client.md` | `src/client/` | `plans/client-review.md` |
| server | `architecture/server.md` | `src/server/` | `plans/server-review.md` |
| mcp | `architecture/mcp.md` | `src/mcp/` | `plans/mcp-review.md` |
| command | `architecture/command.md` | `src/command/` | `plans/command-review.md` |

### Batch 3: Tooling & Execution
| Module | Architecture Doc | Source Path | Review Output |
|--------|-----------------|-------------|---------------|
| tool | `architecture/tool.md` | `src/tool/` | `plans/tool-review.md` |
| skills | `architecture/skills.md` | `src/skills/` | `plans/skills-review.md` |
| exec | `architecture/exec.md` | `src/exec/` | `plans/exec-review.md` |
| hooks | `architecture/hooks.md` | `src/hooks/` | `plans/hooks-review.md` |
| plugin | `architecture/plugin.md` | `src/plugin/` | `plans/plugin-review.md` |

### Batch 4: Security & Safety
| Module | Architecture Doc | Source Path | Review Output |
|--------|-----------------|-------------|---------------|
| security | `architecture/security.md` | `src/security/` | `plans/security-review.md` |
| permission | `architecture/permission.md` | `src/permission/` | `plans/permission-review.md` |
| crypto | `architecture/crypto.md` | `src/crypto/` | `plans/crypto-review.md` |
| resilience | `architecture/resilience.md` | `src/resilience/` | `plans/resilience-review.md` |

### Batch 5: UI & Output
| Module | Architecture Doc | Source Path | Review Output |
|--------|-----------------|-------------|---------------|
| tui | `architecture/tui.md` | `src/tui/` | `plans/tui-review.md` |
| ide | `architecture/ide.md` | `src/ide/` | `plans/ide-review.md` |
| tts | `architecture/tts.md` | `src/tts/` | `plans/tts-review.md` |
| lsp | `architecture/lsp.md` | `src/lsp/` | `plans/lsp-review.md` |

### Batch 6: Data & State
| Module | Architecture Doc | Source Path | Review Output |
|--------|-----------------|-------------|---------------|
| session | `architecture/session.md` | `src/session/` | `plans/session-review.md` |
| memory | `architecture/memory.md` | `src/memory/` | `plans/memory-review.md` |
| snapshot | `architecture/snapshot.md` | `src/snapshot/` | `plans/snapshot-review.md` |
| pty | `architecture/pty.md` | `src/pty/` | `plans/pty-review.md` |

### Batch 7: Utilities & Legacy
| Module | Architecture Doc | Source Path | Review Output |
|--------|-----------------|-------------|---------------|
| util | `architecture/util.md` | `src/util/` | `plans/util-review.md` |
| upgrade | `architecture/upgrade.md` | `src/upgrade/` | `plans/upgrade-review.md` |
| worktree | `architecture/worktree.md` | `src/worktree/` | `plans/worktree-review.md` |

## Subagent Prompt Template

Each subagent will receive this prompt pattern (filled with module-specific values):

```
Review the {module} architecture document at `architecture/{module}.md` and the 
implementation at `src/{module}/`. 

Your tasks:
1. Read the architecture document thoroughly
2. Read all source files in `src/{module}/`
3. For each claim/assertion in the architecture doc:
   - Verify it against the actual code
   - Mark as VERIFIED, INCORRECT, or UNABLE_TO_VERIFY
4. Identify bugs: logic errors, edge cases not handled, race conditions, etc.
5. Identify improvements: performance, maintainability, missing features
6. Write a detailed improvement plan to `plans/{module}-review.md`

Format of output file:
## Verification Results
### Claims (table format: Claim | Status | Evidence)

## Bugs Found
### Critical
### High
### Medium

## Improvement Suggestions
### Performance
### Correctness
### Maintainability

## Priority Actions (top 5 items to fix)
```

## Execution Order

1. **Phase 1**: Launch batches 1-4 in parallel (20 subagents)
2. **Phase 2**: Launch batches 5-7 after phase 1 completes (10 subagents)
3. **Phase 3**: Consolidate findings into summary document

## Review Criteria

- **Accuracy**: Does the doc match implementation?
- **Completeness**: Are all public APIs documented?
- **Correctness**: Are there bugs in the implementation?
- **Consistency**: Is naming consistent across modules?
- **Security**: Any SSRF, injection, or access control issues?
- **Performance**: Any obvious bottlenecks or inefficiencies?

## Progress Tracking

- [ ] Batch 1 (agent, event-bus, error, config, storage)
- [ ] Batch 2 (provider, client, server, mcp, command)
- [ ] Batch 3 (tool, skills, exec, hooks, plugin)
- [ ] Batch 4 (security, permission, crypto, resilience)
- [ ] Batch 5 (tui, ide, tts, lsp)
- [ ] Batch 6 (session, memory, snapshot, pty)
- [ ] Batch 7 (util, upgrade, worktree)