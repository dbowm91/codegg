# Architecture Stale Items Identification

**Review Date**: 2026-05-26
**Phase**: 4 - Stale Item Identification Complete
**Phase**: 5 - Pruning Recommendations (embedded below)

---

## Phase 5: Pruning Recommendations

Based on Phase 4 findings, the following pruning actions are recommended:

### 1. Create Missing Architecture Documentation

| Action | Reason |
|--------|--------|
| **Create architecture/protocol.md** | Protocol module (`src/protocol/`) exists with CoreRequest, CoreResponse, TuiMessage types but has no dedicated architecture doc. These types are partially documented in core.md but deserve their own doc. |

### 2. Update Existing Architecture Documentation

| Action | File | Reason |
|--------|------|--------|
| Fix built-in modes table | `architecture/permission.md` | "skill" incorrectly listed in allowed_tools for all 3 modes |
| Clarify provider auto-registration | `architecture/provider.md` | SAP AI Core, Zenmux, Kilo, Vercal AI Gateway not auto-registered |
| Rename "Discovery Providers" section | `architecture/provider.md` | Misleading title - these don't auto-discover |
| Document CoreEvent mapping gaps | `architecture/core.md` | Many events dropped in `map_app_event_to_core_event()` |
| Clarify hook timeout distinction | `architecture/plugin.md` | 5s outer dispatch vs 30s inner WASM |
| Clarify backoff formula | `architecture/resilience.md` | Should mention jitter |
| Fix UiState field list | `architecture/tui.md` | Some documented fields missing from source |

### 3. Remove or Update Line Number References

Line numbers in architecture docs are fragile and frequently drift. Recommendations:
- Remove specific line number references (e.g., "loop.rs:1777")
- Use method names or describe behavior instead
- If line numbers must be kept, add a disclaimer that they may be stale

### 4. Cross-Reference Fixes Needed

| Issue | Location | Fix |
|-------|----------|-----|
| AgentLoop hooks | `architecture/plugin.md` | Add cross-reference showing exact AgentLoop integration points |
| Snapshot table schema | `architecture/snapshot.md` | Note it's defined in `session::schema::migrate_v13()` |
| ToolExecutor not integrated | `architecture/tool.md` | Known issue - already documented but could be more prominent |
| Protocol types | `architecture/core.md` | Note that CoreRequest/CoreResponse are in `src/protocol/` |

### 5. Documentation Structure Improvements

| Improvement | Reason |
|-------------|--------|
| Rename `stat_core.rs` to `metrics.rs` | Filename is misleading - contains metrics code, not file stats |
| Document `compaction` as agent submodule | Currently documented separately which implies it's a top-level module |
| Document `error` and `exec` as single files | Currently in module index as if they were directories |

---

## Module Existence Verification

### Modules in Architecture Index vs Source

| Architecture Doc | Source Location | Status |
|-----------------|-----------------|--------|
| agent | `src/agent/` | ✓ Exists as directory |
| bus | `src/bus/` | ✓ Exists as directory |
| client | `src/client/` | ✓ Exists as directory |
| command | `src/command/` | ✓ Exists as directory |
| compaction | `src/agent/compaction.rs` | ⚠️ Submodule of agent, documented separately |
| config | `src/config/` | ✓ Exists as directory |
| core | `src/core/` | ✓ Exists as directory |
| crypto | `src/crypto/` | ✓ Exists as directory |
| error | `src/error.rs` | ⚠️ Single file, not directory |
| exec | `src/exec.rs` | ⚠️ Single file, not directory |
| hooks | `src/hooks/` | ✓ Exists as directory |
| ide | `src/ide/` | ✓ Exists as directory |
| lsp | `src/lsp/` | ✓ Exists as directory |
| mcp | `src/mcp/` | ✓ Exists as directory |
| memory | `src/memory/` | ✓ Exists as directory |
| permission | `src/permission/` | ✓ Exists as directory |
| plugin | `src/plugin/` | ✓ Exists as directory |
| provider | `src/provider/` | ✓ Exists as directory |
| protocol | `src/protocol/` | ❌ No corresponding architecture file |
| pty_session | `src/pty_session/` | ✓ Exists as directory |
| resilience | `src/resilience/` | ✓ Exists as directory |
| security | `src/security/` | ✓ Exists as directory |
| server | `src/server/` | ✓ Exists as directory |
| session | `src/session/` | ✓ Exists as directory |
| skills | `src/skills/` | ✓ Exists as directory |
| snapshot | `src/snapshot/` | ✓ Exists as directory |
| storage | `src/storage/` | ✓ Exists as directory |
| tool | `src/tool/` | ✓ Exists as directory |
| tts | `src/tts/` | ✓ Exists as directory |
| upgrade | `src/upgrade/` | ✓ Exists as directory |
| util | `src/util/` | ✓ Exists as directory |
| worktree | `src/worktree/` | ✓ Exists as directory |

**Summary**: 32 architecture docs, 30 source modules. `protocol` module has no corresponding architecture doc. `error` and `exec` are single files, not directories.

---

## Line Number Drift Issues

Line numbers in architecture docs frequently drift. The following specific references were found to be incorrect:

### agent.md
- Line 296: ToolExecuteBefore hook references "loop.rs:1777 and 1814" - actual lines are 1770 and 1814

### bus.md
- No line number issues found - event counts accurate

### core.md
- TurnSubmit documentation incomplete (missing fields)

### permission.md
- Built-in modes table (lines 198-202) shows "skill" in allowed_tools but not in source

### provider.md
- SAP AI Core, Zenmux, Kilo, Vercal AI Gateway listed as auto-registered but aren't
- "Discovery Providers" section title misleading

### plugin.md
- Hook dispatch timeout documentation misleading (5s outer vs 30s inner)

### resilience.md
- Backoff formula description ambiguous (should mention jitter)

### session.md
- checkpoint.rs compute_checksum uses SHA256, snapshot uses MD5 (inconsistency)

---

## Cross-Reference Issues

### Missing architecture files:
1. **protocol/** - No architecture/protocol.md exists, but `src/protocol/` module exists with CoreRequest, CoreResponse, TuiMessage types. These are partially documented in core.md.

### Incorrect cross-references:
1. **AgentLoop hooks not cross-referenced** - plugin.md mentions hooks but doesn't show exact AgentLoop integration points
2. **Snapshot table schema** - Defined in session module, not snapshot module
3. **ToolExecutor** - Documented but not integrated (known but could be clearer)

---

## Documentation vs Implementation Summary

| Module | Documentation Quality | Issues |
|--------|----------------------|--------|
| agent | Good | Line numbers drift |
| bus | Good | Event system accurate |
| core | Good | Incomplete CoreEvent mapping |
| command | Good | 41 commands verified |
| compaction | Good | Sync vs async behavior differs |
| permission | Good | Built-in modes table incorrect |
| security | Excellent | All claims verified |
| crypto | Excellent | All claims verified |
| session | Good | Hash algorithm inconsistency |
| storage | Excellent | All claims verified |
| memory | Good | Minor line off-by-one |
| snapshot | Good | AgentLoop integration is pseudocode |
| tui | Good | UiState missing fields |
| client | Good | handle_remote_event location |
| ide | Good | Accurate |
| server | Good | Permission routes table minor issue |
| mcp | Excellent | Known issues documented |
| lsp | Excellent | 39 servers verified |
| exec | Excellent | Complete coverage |
| plugin | Good | Fuel leak bugs |
| skills | Excellent | Perfect match |
| hooks | Good | Could use more line refs |
| upgrade | Good | CLI behavior clarification |
| provider | Good | Auto-registration misleading |
| tool | Good | 26 tools verified |
| resilience | Good | Backoff formula unclear |
| util | Good | stat_core.rs filename misleading |
| tts | Good | macOS-only verified |
| pty_session | Good | Correct |
| worktree | Good | is_locked/is_main not implemented |

---

## Counts That Have Drifted

| Item | Doc Claims | Actual | Module |
|------|------------|--------|--------|
| LSP servers | 39 or 40 | 39 | lsp |
| Tools | 26 | 26 | tool |
| Built-in commands | 41 | 41 | command |
| Event variants | 36 | 36 | bus |

---

## Recommendations for Stale Item Pruning

### High Priority
1. **Create architecture/protocol.md** - Protocol module exists but has no dedicated architecture doc
2. **Update permission built-in modes table** - Remove "skill" from allowed_tools
3. **Fix provider auto-registration claims** - Clarify which providers are auto-registered vs config-only

### Medium Priority
4. **Remove specific line numbers** - Use method names instead
5. **Clarify "Discovery Providers" section** - Rename or clarify
6. **Document CoreEvent mapping completeness** - Note which events are dropped

### Low Priority
7. **Rename stat_core.rs to metrics.rs** - Filename misleading
8. **Add protocol module coverage** - CoreRequest/CoreResponse/TuiMessage types need documentation
