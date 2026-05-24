# Architecture Review Plan

This document outlines the review plan for architecture documentation in the `architecture/` directory. Each module will be reviewed by a subagent that will verify claims against the actual code and document findings and improvements.

## Modules to Review

| # | Module | Document | Review Agent Output |
|---|--------|----------|---------------------|
| 1 | Agent | `architecture/agent.md` | `plans/agent_review.md` |
| 2 | Client | `architecture/client.md` | `plans/client_review.md` |
| 3 | Command | `architecture/command.md` | `plans/command_review.md` |
| 4 | Compaction | `architecture/compaction.md` | `plans/compaction_review.md` |
| 5 | Config | `architecture/config.md` | `plans/config_review.md` |
| 6 | Crypto | `architecture/crypto.md` | `plans/crypto_review.md` |
| 7 | Error | `architecture/error.md` | `plans/error_review.md` |
| 8 | Event Bus | `architecture/event-bus.md` | `plans/event-bus_review.md` |
| 9 | Exec | `architecture/exec.md` | `plans/exec_review.md` |
| 10 | Hooks | `architecture/hooks.md` | `plans/hooks_review.md` |
| 11 | IDE | `architecture/ide.md` | `plans/ide_review.md` |
| 12 | LSP | `architecture/lsp.md` | `plans/lsp_review.md` |
| 13 | MCP | `architecture/mcp.md` | `plans/mcp_review.md` |
| 14 | Memory | `architecture/memory.md` | `plans/memory_review.md` |
| 15 | Permission | `architecture/permission.md` | `plans/permission_review.md` |
| 16 | Plugin | `architecture/plugin.md` | `plans/plugin_review.md` |
| 17 | Provider | `architecture/provider.md` | `plans/provider_review.md` |
| 18 | PTY | `architecture/pty.md` | `plans/pty_review.md` |
| 19 | Resilience | `architecture/resilience.md` | `plans/resilience_review.md` |
| 20 | Security | `architecture/security.md` | `plans/security_review.md` |
| 21 | Server | `architecture/server.md` | `plans/server_review.md` |
| 22 | Session | `architecture/session.md` | `plans/session_review.md` |
| 23 | Skills | `architecture/skills.md` | `plans/skills_review.md` |
| 24 | Snapshot | `architecture/snapshot.md` | `plans/snapshot_review.md` |
| 25 | Storage | `architecture/storage.md` | `plans/storage_review.md` |
| 26 | Tool | `architecture/tool.md` | `plans/tool_review.md` |
| 27 | TTS | `architecture/tts.md` | `plans/tts_review.md` |
| 28 | TUI | `architecture/tui.md` | `plans/tui_review.md` |
| 29 | Upgrade | `architecture/upgrade.md` | `plans/upgrade_review.md` |
| 30 | Util | `architecture/util.md` | `plans/util_review.md` |
| 31 | Worktree | `architecture/worktree.md` | `plans/worktree_review.md` |

## Review Methodology

For each module, the subagent will:

1. **Read the architecture document** at `architecture/<module>.md`
2. **Explore the corresponding source code** in `src/<module>/` or relevant locations
3. **Verify claims** by checking if documented types, functions, and behaviors match the implementation
4. **Identify discrepancies** between documentation and implementation
5. **Detect bugs** in the actual code that may not be documented
6. **Propose improvements** for both documentation and code

## Review Agent Instructions

Each subagent should:
- Load the relevant skill for the module (e.g., `agent-loop`, `provider`, etc.)
- Cross-reference the architecture document with actual source code
- Document any:
  - Inaccuracies in the documentation
  - Missing undocumented types or functions
  - Bugs or code quality issues
  - Missing architectural concerns
  - Recommendations for improvement

## Execution

Subagents will be launched in parallel groups to maximize efficiency. Each will write their findings to the corresponding file in the `plans/` directory.

---
*Generated: 2026-05-24*