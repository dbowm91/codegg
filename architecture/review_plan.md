# Architecture Review Plan

This plan outlines a systematic review of all architecture documents in the `architecture/` directory. Each module will be reviewed by a subagent that will verify claims against the implementation, identify bugs, and propose improvements.

## Modules to Review

| Module | Architecture Doc | Review Output |
|--------|------------------|---------------|
| agent | `architecture/agent.md` | `plans/agent-review.md` |
| client | `architecture/client.md` | `plans/client-review.md` |
| command | `architecture/command.md` | `plans/command-review.md` |
| config | `architecture/config.md` | `plans/config-review.md` |
| crypto | `architecture/crypto.md` | `plans/crypto-review.md` |
| error | `architecture/error.md` | `plans/error-review.md` |
| event-bus | `architecture/event-bus.md` | `plans/event-bus-review.md` |
| exec | `architecture/exec.md` | `plans/exec-review.md` |
| hooks | `architecture/hooks.md` | `plans/hooks-review.md` |
| ide | `architecture/ide.md` | `plans/ide-review.md` |
| lsp | `architecture/lsp.md` | `plans/lsp-review.md` |
| mcp | `architecture/mcp.md` | `plans/mcp-review.md` |
| memory | `architecture/memory.md` | `plans/memory-review.md` |
| permission | `architecture/permission.md` | `plans/permission-review.md` |
| plugin | `architecture/plugin.md` | `plans/plugin-review.md` |
| provider | `architecture/provider.md` | `plans/provider-review.md` |
| pty | `architecture/pty.md` | `plans/pty-review.md` |
| resilience | `architecture/resilience.md` | `plans/resilience-review.md` |
| security | `architecture/security.md` | `plans/security-review.md` |
| server | `architecture/server.md` | `plans/server-review.md` |
| session | `architecture/session.md` | `plans/session-review.md` |
| skills | `architecture/skills.md` | `plans/skills-review.md` |
| snapshot | `architecture/snapshot.md` | `plans/snapshot-review.md` |
| storage | `architecture/storage.md` | `plans/storage-review.md` |
| tool | `architecture/tool.md` | `plans/tool-review.md` |
| tts | `architecture/tts.md` | `plans/tts-review.md` |
| tui | `architecture/tui.md` | `plans/tui-review.md` |
| upgrade | `architecture/upgrade.md` | `plans/upgrade-review.md` |
| util | `architecture/util.md` | `plans/util-review.md` |
| worktree | `architecture/worktree.md` | `plans/worktree-review.md` |

## Review Methodology

For each module, the subagent should:

1. **Read the architecture document** to understand the intended design
2. **Read the source code** in `src/<module>/` (or relevant paths)
3. **Verify claims** - check that documented types, methods, fields match implementation
4. **Identify bugs** - find any discrepancies, missing implementations, or errors
5. **Propose improvements** - suggest optimizations, additional features, or fixes

## Subagent Tasks

Subagents will run concurrently. Each should output a review file with:
- Summary of verified claims
- List of bugs/discrepancies found
- Improvement suggestions with priority (high/medium/low)

## Execution

Launch subagents for all modules in parallel groups:

**Group 1:** agent, client, command, config, crypto, error
**Group 2:** event-bus, exec, hooks, ide, lsp, mcp
**Group 3:** memory, permission, plugin, provider, pty, resilience
**Group 4:** security, server, session, skills, snapshot
**Group 5:** storage, tool, tts, tui, upgrade, util, worktree