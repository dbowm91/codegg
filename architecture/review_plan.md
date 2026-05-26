# Architecture Review Plan

**Status**: INCOMPLETE
**Created**: 2026-05-26
**Purpose**: Systematic in-depth review of all architecture documents with code verification

---

## Overview

This plan organizes a comprehensive review of 28 architecture modules. Each module will be assigned to a subagent that will:
1. Read the architecture document(s)
2. Verify all claims against actual source code
3. Interrogate code for bugs, inconsistencies, and improvement opportunities
4. Write findings to `plans/<module>.md`

After all subagent reviews complete, findings will be consolidated and stale items identified for pruning.

---

## Phase 1: Module Reviews (Subagent Tasks)

### Batch 1: Core Runtime (5 modules)
| Module | Architecture File | Review Focus |
|--------|-------------------|--------------|
| agent | `architecture/agent.md` | AgentLoop, message processing, subagent pool, team coordination |
| bus | `architecture/bus.md` | GlobalEventBus, PermissionRegistry, QuestionRegistry |
| core | `architecture/core.md` | CoreClient facade, transport adapters |
| command | `architecture/command.md` | Slash command registry, templates |
| compaction | `architecture/compaction.md` | Context overflow management |

### Batch 2: Security & Access Control (3 modules)
| Module | Architecture File | Review Focus |
|--------|-------------------|--------------|
| permission | `architecture/permission.md` | Access control, DoomLoop, mode system |
| security | `architecture/security.md` | SSRF, IP validation, Landlock sandboxing |
| crypto | `architecture/crypto.md` | AES-256-GCM, Argon2id key derivation |

### Batch 3: Data & State Management (4 modules)
| Module | Architecture File | Review Focus |
|--------|-------------------|--------------|
| session | `architecture/session.md` | SQLite storage, message history, checkpointing |
| storage | `architecture/storage.md` | SQLite initialization, connection pooling |
| memory | `architecture/memory.md` | Persistent memory, namespaces, consolidation |
| snapshot | `architecture/snapshot.md` | File state capture and restore |

### Batch 4: UI & Rendering (3 modules)
| Module | Architecture File | Review Focus |
|--------|-------------------|--------------|
| tui | `architecture/tui.md` | Ratatui components, dialogs, FocusManager |
| client | `architecture/client.md` | WebSocket remote TUI, resume/replay |
| ide | `architecture/ide.md` | VS Code, JetBrains integration, diff viewing |

### Batch 5: Integration Services (4 modules)
| Module | Architecture File | Review Focus |
|--------|-------------------|--------------|
| server | `architecture/server.md` | Axum HTTP, WebSocket, REST API, SSE |
| mcp | `architecture/mcp.md` | MCP client, local/remote, OAuth flow |
| lsp | `architecture/lsp.md` | LSP diagnostics, code operations |
| exec | `architecture/exec.md` | Non-interactive CI/CD mode |

### Batch 6: Extensibility (4 modules)
| Module | Architecture File | Review Focus |
|--------|-------------------|--------------|
| plugin | `architecture/plugin.md` | WASM plugins, hook types, fuel tracking |
| skills | `architecture/skills.md` | Skill loading, YAML frontmatter |
| hooks | `architecture/hooks.md` | Lifecycle hooks system |
| upgrade | `architecture/upgrade.md` | GitHub releases, self-upgrade |

### Batch 7: Provider & Utilities (5 modules)
| Module | Architecture File | Review Focus |
|--------|-------------------|--------------|
| provider | `architecture/provider.md` | LLM backends, streaming, model catalog |
| tool | `architecture/tool.md` | Tool registry, built-in tools |
| resilience | `architecture/resilience.md` | Circuit breaker, FallbackProvider |
| util | `architecture/util.md` | Clipboard, fuzzy matching, truncation |
| tts | `architecture/tts.md` | Text-to-speech (macOS) |
| pty_session | `architecture/pty_session.md` | Shell session metadata |
| worktree | `architecture/worktree.md` | Git worktree management |

---

## Phase 2: Subagent Instructions

Each subagent will receive this instruction template:

```
## Task: Review [MODULE_NAME] Architecture

1. Read `architecture/[module].md` thoroughly
2. Identify all claims (counts, line numbers, field names, behaviors)
3. For each claim:
   - Use code search (grep, glob) to find relevant source files
   - Read the actual source code
   - Compare documentation to implementation
   - Mark as VERIFIED, STALE, or INCORRECT
4. Interrogate code for:
   - Missing functionality documented as implemented
   - Implemented functionality missing from docs
   - Inconsistencies between modules
   - Potential bugs or edge cases
   - Outdated patterns or deprecated approaches
5. Write findings to `plans/[module].md` with structure:
   - ## Verified Claims
   - ## Stale Information  
   - ## Bugs Found
   - ## Improvements Suggested
   - ## Cross-Module Issues
6. Return summary of key findings
```

---

## Phase 3: Consolidation

After all batches complete:

1. Read all `plans/*.md` files
2. Identify cross-cutting issues affecting multiple modules
3. Document systemic patterns needing architectural fixes
4. Create `plans/consolidated.md` with master findings

---

## Phase 4: Stale Item Identification

### Step 4.1: Compare Module Index

Read `architecture/overview.md` Module Index section and verify each listed module:
- Still exists in `src/` as actual module
- Has corresponding architecture file
- Is referenced in AGENTS.md module table

### Step 4.2: Check Architecture File Validity

For each `.md` file in `architecture/`:
- Verify it documents an actual module in `src/`
- Verify claims are still accurate
- Check for duplicate/outdated information

### Step 4.3: Identify Stale Items

Create `plans/stale_items.md` documenting:
- Modules in architecture with no corresponding source
- Files referencing deprecated patterns
- Counts/line numbers that have drifted from reality
- Cross-references to other architecture files that are outdated

---

## Phase 5: Pruning Recommendations

Based on Phase 4 findings, recommend:
1. Remove `architecture/*.md` files for modules that no longer exist
2. Update `architecture/overview.md` module index
3. Fix cross-references between architecture files
4. Remove duplicate information across files

---

## Verification Commands

After any documentation changes:
```bash
cargo build --all-features
cargo clippy --all-features -- -D warnings
```

---

## File Outputs

| File | Purpose |
|------|---------|
| `plans/agent.md` | Agent module review findings |
| `plans/bus.md` | Bus module review findings |
| `plans/core.md` | Core module review findings |
| `plans/command.md` | Command module review findings |
| `plans/compaction.md` | Compaction module review findings |
| `plans/permission.md` | Permission module review findings |
| `plans/security.md` | Security module review findings |
| `plans/crypto.md` | Crypto module review findings |
| `plans/session.md` | Session module review findings |
| `plans/storage.md` | Storage module review findings |
| `plans/memory.md` | Memory module review findings |
| `plans/snapshot.md` | Snapshot module review findings |
| `plans/tui.md` | TUI module review findings |
| `plans/client.md` | Client module review findings |
| `plans/ide.md` | IDE module review findings |
| `plans/server.md` | Server module review findings |
| `plans/mcp.md` | MCP module review findings |
| `plans/lsp.md` | LSP module review findings |
| `plans/exec.md` | Exec module review findings |
| `plans/plugin.md` | Plugin module review findings |
| `plans/skills.md` | Skills module review findings |
| `plans/hooks.md` | Hooks module review findings |
| `plans/upgrade.md` | Upgrade module review findings |
| `plans/provider.md` | Provider module review findings |
| `plans/tool.md` | Tool module review findings |
| `plans/resilience.md` | Resilience module review findings |
| `plans/util.md` | Util module review findings |
| `plans/tts.md` | TTS module review findings |
| `plans/pty_session.md` | PTY Session module review findings |
| `plans/worktree.md` | Worktree module review findings |
| `plans/consolidated.md` | Master findings across all modules |
| `plans/stale_items.md` | Items identified for pruning |

---

## Notes

- **Context Limits**: Subagents may undergo compaction after ~2000 lines. Batch size is designed to stay within limits.
- **Line Numbers**: Always use code search to verify line numbers - docs may drift
- **Verification First**: Many "bugs" in docs turn out to be correctly implemented. Always verify claims against code.
- **No Direct Changes**: This plan is for REVIEW ONLY. No direct code changes should be specified for execution.

---

*End of plan*