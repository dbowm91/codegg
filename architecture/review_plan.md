# Architecture Review Plan

This plan organizes a systematic review of all architecture documents in the `architecture/` directory, using subagents to verify claims against actual code and identify improvements and bugs.

## Modules to Review (34 total, excluding review_plan.md)

| # | Module | File |
|---|--------|------|
| 1 | Overview | `architecture/overview.md` |
| 2 | Protocol | `architecture/protocol.md` |
| 3 | Command | `architecture/command.md` |
| 4 | Client | `architecture/client.md` |
| 5 | Bus | `architecture/bus.md` |
| 6 | Agent | `architecture/agent.md` |
| 7 | Skills | `architecture/skills.md` |
| 8 | Provider | `architecture/provider.md` |
| 9 | Plugin | `architecture/plugin.md` |
| 10 | Upgrade | `architecture/upgrade.md` |
| 11 | Session | `architecture/session.md` |
| 12 | Server | `architecture/server.md` |
| 13 | Resilience | `architecture/resilience.md` |
| 14 | Compaction | `architecture/compaction.md` |
| 15 | Util | `architecture/util.md` |
| 16 | Permission | `architecture/permission.md` |
| 17 | Core | `architecture/core.md` |
| 18 | Memory | `architecture/memory.md` |
| 19 | Config | `architecture/config.md` |
| 20 | IDE | `architecture/ide.md` |
| 21 | MCP | `architecture/mcp.md` |
| 22 | Hooks | `architecture/hooks.md` |
| 23 | Worktree | `architecture/worktree.md` |
| 24 | Security | `architecture/security.md` |
| 25 | LSP | `architecture/lsp.md` |
| 26 | PTY Session | `architecture/pty_session.md` |
| 27 | Exec | `architecture/exec.md` |
| 28 | TUI | `architecture/tui.md` |
| 29 | Tool | `architecture/tool.md` |
| 30 | Error | `architecture/error.md` |
| 31 | TTS | `architecture/tts.md` |
| 32 | Snapshot | `architecture/snapshot.md` |
| 33 | Crypto | `architecture/crypto.md` |
| 34 | Storage | `architecture/storage.md` |

## Review Process

### Phase 1: Subagent Batch Reviews (Parallel Execution)

Launch subagents in batches of 5-6 modules each. Each subagent will:

1. Read the architecture document for their assigned module(s)
2. Read the corresponding source code in `src/`
3. Verify claims in the documentation against actual code
4. Identify:
   - Stale/incorrect documentation claims
   - Bugs discovered during review
   - Improvement opportunities
   - Missing features that are documented as existing
   - Features documented but not implemented
5. Write findings to `plans/<module>_review.md`

### Batch Assignments

**Batch 1** (Modules 1-5): Overview, Protocol, Command, Client, Bus
- Task: `plans/overview_review.md`, `plans/protocol_review.md`, `plans/command_review.md`, `plans/client_review.md`, `plans/bus_review.md`

**Batch 2** (Modules 6-10): Agent, Skills, Provider, Plugin, Upgrade
- Task: `plans/agent_review.md`, `plans/skills_review.md`, `plans/provider_review.md`, `plans/plugin_review.md`, `plans/upgrade_review.md`

**Batch 3** (Modules 11-15): Session, Server, Resilience, Compaction, Util
- Task: `plans/session_review.md`, `plans/server_review.md`, `plans/resilience_review.md`, `plans/compaction_review.md`, `plans/util_review.md`

**Batch 4** (Modules 16-20): Permission, Core, Memory, Config, IDE
- Task: `plans/permission_review.md`, `plans/core_review.md`, `plans/memory_review.md`, `plans/config_review.md`, `plans/ide_review.md`

**Batch 5** (Modules 21-25): MCP, Hooks, Worktree, Security, LSP
- Task: `plans/mcp_review.md`, `plans/hooks_review.md`, `plans/worktree_review.md`, `plans/security_review.md`, `plans/lsp_review.md`

**Batch 6** (Modules 26-30): PTY Session, Exec, TUI, Tool, Error
- Task: `plans/pty_session_review.md`, `plans/exec_review.md`, `plans/tui_review.md`, `plans/tool_review.md`, `plans/error_review.md`

**Batch 7** (Modules 31-34): TTS, Snapshot, Crypto, Storage
- Task: `plans/tts_review.md`, `plans/snapshot_review.md`, `plans/crypto_review.md`, `plans/storage_review.md`

### Phase 2: Stale Item Detection

After all subagents complete, the parent agent will:

1. **Cross-reference all `plans/*_review.md` files**
   - Compile a list of all identified stale documentation items
   - Compile a list of all identified stale module files

2. **Check for stale items in architecture directory**:
   - Modules that exist in `architecture/` but have no corresponding source in `src/`
   - Modules that describe features that have been removed or significantly changed
   - Line numbers and field counts that no longer match source
   - File references that point to moved/renamed files

3. **Verify module organization**:
   - Compare `architecture/` listing against what modules actually exist in `src/`
   - Identify orphaned architecture files
   - Note any source modules without architecture documentation

### Phase 3: Review Summary (Parent Agent)

After all subagent reviews and stale detection:
1. Consolidate findings into a summary document at `plans/consolidated_review.md`
2. Produce final list of recommended:
   - Documentation corrections
   - Documentation files to remove (stale)
   - Architecture files to remove (stale)
   - Code improvements (for separate implementation)

## Subagent Instructions

Each subagent should:
- Work only in `/Users/davidbowman/projects/codegg`
- Verify every claim by checking actual source code
- Note exact line numbers for discrepancies
- Document what should be fixed but NOT make direct code changes
- Write findings to the specified output file in `plans/`

## Output Structure

Each `plans/<module>_review.md` should contain:

```markdown
# <Module> Architecture Review

## Documentation Accuracy
- [List verified accurate claims]
- [List stale/incorrect claims with corrections]

## Code Bugs Found
- [Bug description with location]

## Improvement Opportunities
- [Suggested improvements]

## Stale Items to Remove from Architecture Directory
- [Any items that should be pruned]

## Missing Documentation
- [Features with no docs or incomplete docs]
```

## Notes

- This is a research/review task only - no code changes should be made by subagents
- Subagents should verify using grep, glob, and Read tools against actual source
- All work stays within `/Users/davidbowman/projects/codegg`
