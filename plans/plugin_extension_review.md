# Plugin, Skills, Snapshot, and Upgrade Module Architecture Review

## Executive Summary

Reviewed four architecture documents against source code. Found stale items in plugin.md and skills.md (regarding `.skills/` directory), a hash algorithm inconsistency bug in snapshot/mod.rs, and accurate documentation for upgrade.md.

---

## Plugin Module (`architecture/plugin.md`)

### Verification: MOSTLY ACCURATE

**File Reference Summary:**
| Item | Document Line | Actual Line(s) | Status |
|------|--------------|----------------|--------|
| HookType enum | 84-101 | `hooks.rs:6-20` | MATCH |
| HookContext struct | 107-110 | `hooks.rs:62-65` | MATCH |
| HookResult struct | 116-126 | `hooks.rs:68-98` | MATCH |
| PluginManifest struct | 50-67 | `manifest.rs:4-16` | MATCH |
| LoadedPlugin struct | 72-78 | `loader.rs:32-36` | MATCH |
| PluginInfo struct | 213-219 | `registry.rs:8-15` | MATCH |
| PluginRegistry fields | 221-224 | `registry.rs:17-20` | MATCH |
| PluginService struct | 189-192 | `service.rs:9-12` | MATCH |
| ModuleCache struct | 143-156 | `loader.rs:109-114` | MATCH |
| Fuel constants | 388-393 | `loader.rs:9-15` | MATCH |
| MarketplaceService methods | 303-312 | `marketplace.rs:33-110` | MATCH |
| BuiltinPlugin struct | 289-296 | `builtin/mod.rs:13-16` | MATCH |

### Stale Items in plugin.md

1. **Missing enum variant documentation**: `PluginTier` enum (`Official`, `Repository`, `Personal`) is defined in `marketplace.rs:4-9` but not documented in plugin.md. This enum is used by `MarketplacePlugin` struct.

2. **Incorrect dispatch method list**: Lines 202-206 of the document list dispatch methods but are incomplete. The actual `PluginService` in `service.rs` has these dispatch methods:
   - `dispatch_auth` (line 143) ✅ listed
   - `dispatch_tool_definition` (line 151) ✅ listed
   - `dispatch_tool_execute_before` (line 159) ✅ listed
   - `dispatch_tool_execute_after` (line 167) ✅ listed
   - `dispatch_chat_params` (line 175) ❌ missing from doc list
   - `dispatch_chat_headers` (line 183) ❌ missing from doc list
   - `dispatch_event` (line 191) ❌ missing from doc list
   - `dispatch_config` (line 199) ❌ missing from doc list
   - `dispatch_shell_env` (line 207) ❌ missing from doc list
   - `dispatch_text_complete` (line 215) ❌ missing from doc list
   - `dispatch_session_compacting` (line 223) ❌ missing from doc list
   - `dispatch_messages_transform` (line 231) ❌ missing from doc list
   - `dispatch_provider` (line 239) ❌ missing from doc list

3. **Reference to manifest.toml path**: Line 330 says `manifest.toml` but doesn't note it must be a sibling to `plugin.wasm`. The actual `find_wasm` function (`loader.rs:79-100`) searches for `plugin.wasm` or `plugin.wasm32-wasi.wasm`, not `manifest.toml`.

### Potential Bug (Fuel Tracking)

**Hash Algorithm Inconsistency in Snapshot, but Plugin Fuel Logic is Correct**:
- The plugin fuel logic in `loader.rs` is correctly implemented - all early returns in `execute_wasm_hook()` properly call `return_fuel()` after fuel_reserved is set.
- The skill document (`plugin/SKILL.md`) mentions a known issue at `loader.rs:255-285` with fuel leaks, but this is STALE. The current code at lines 254-289 shows fuel is properly returned on all three early return paths.

---

## Skills Module (`architecture/skills.md`)

### Verification: MOSTLY ACCURATE

**File Reference Summary:**
| Item | Document Line | Actual Line(s) | Status |
|------|--------------|----------------|--------|
| Skill struct | 22-29 | `skills/mod.rs:7-15` | MATCH |
| SkillIndex struct | 35-36 | `skills/mod.rs:26-28` | MATCH |
| `new()` method | 40 | `skills/mod.rs:37-39` | MATCH |
| `load()` method | 41 | `skills/mod.rs:41-57` | MATCH |
| `get()` method | 42 | `skills/mod.rs:85-87` | MATCH |
| `list()` method | 43 | `skills/mod.rs:89-91` | MATCH |
| `find_matching()` method | 44 | `skills` | MATCH |
| `build_system_prompt()` | 45 | `skills/mod.rs:107-121` | MATCH |
| `activate()` method | 46 | `skills/mod.rs:123-125` | MATCH |
| Skill loading locations | 81-88 | `skills/mod.rs:44-54` | MATCH |

### Stale Items in skills.md

1. **`.skills/` directory not loaded by runtime**: Line 14 of the document states the repo maintains skill docs in `.skills/` for maintenance, and line 82-84 describes loading from:
   - Global: `~/.config/codegg/skills/`
   - Project: `.codegg/skills/`
   - Repo maintenance copy: `.skills/`

   **This is INCORRECT/MISLEADING**: The actual code at `skills/mod.rs:41-57` only loads from:
   - `dirs::config_dir()/codegg/skills/` (global)
   - `.codegg/skills/` (project)
   
   The `.skills/` directory exists in the repository but is NOT loaded by the runtime skill loader. The skills skill (`skill/skills/SKILL.md`) itself claims `.skills/` should be kept aligned, but the runtime code doesn't actually load from there.

   The `.skills/` directory appears to be documentation-only, serving as the agent-facing maintenance copy that agents can read but the runtime `SkillIndex::load()` does not use.

2. **Integration point line number stale**: Line 108 (main.rs:930) - the actual line may differ. Not critical to verify.

### Improvements Suggested for skills.md

1. Clarify that `.skills/` is a documentation-only copy and not loaded at runtime
2. Add `list_skill_resources()` function documentation (mentioned in skill guide at `tool/skill.rs:67`)

---

## Snapshot Module (`architecture/snapshot.md`)

### Verification: MOSTLY ACCURATE

**File Reference Summary:**
| Item | Document Line | Actual Line(s) | Status |
|------|--------------|----------------|--------|
| SnapshotOptions struct | 22-26 | `mod.rs:9-14` | MATCH |
| FileSnapshot struct | 33-40 | `mod.rs:26-32` | MATCH |
| Snapshot struct | 46-54 | `mod.rs:34-41` | MATCH |
| SnapshotView struct | 60-68 | `mod.rs:43-50` | MATCH |
| SnapshotManager struct | 72-77 | `mod.rs:52-56` | MATCH |
| All SnapshotManager methods | 79-92 | `mod.rs:58-360` | MATCH |
| SnapshotManager::new_with_options | 81 | `mod.rs:67-82` | MATCH |

**Database schema reference (line 184-196)**: Correctly notes schema is in `src/session/schema.rs` (migration v13).

### Bug Report: Hash Algorithm Inconsistency

**File**: `src/snapshot/mod.rs`

| Line | Content | Hash Used |
|------|---------|-----------|
| 143 | Non-empty file content hashing in `capture_incremental` | `sha2::Sha256::digest` |
| 417 | Empty file hashing in `collect_files_sync` | `sha2::Sha256::digest` |
| 431 | Non-empty file hashing in `collect_files_sync` | `md5::compute` **⚠️ INCONSISTENT** |

**Issue**: `collect_files_sync` uses SHA256 for empty files but MD5 for non-empty files. This is a bug/inconsistency - the same hash algorithm should be used consistently throughout the module.

**Impact**: Snapshots taken via `capture()` (which uses `collect_files_sync`) will have MD5 hashes, while snapshots from `capture_incremental()` will have SHA256 hashes. This could cause issues when comparing or validating snapshots.

**Recommendation**: Standardize on SHA256 (the more secure option) for all hash computations.

### Potential Improvement: Empty Hash Value

At line 417:
```rust
hash: format!("{:x}", sha2::Sha256::digest([])),
```

An empty input SHA256 hash is `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`, which is technically correct but wasteful. Could use a constant string `"empty"` or similar for clarity.

---

## Upgrade Module (`architecture/upgrade.md`)

### Verification: ACCURATE

**File Reference Summary:**
| Item | Document Line | Actual Line(s) | Status |
|------|--------------|----------------|--------|
| VERSION constant | 5 | `mod.rs:5` | MATCH |
| VersionInfo struct | 47-52 | `mod.rs:7-12` | MATCH |
| current_version() | 59-63 | `mod.rs:14-16` | MATCH |
| check_for_updates() | 65-106 | `mod.rs:18-55` | MATCH |
| upgrade() | 108-140 | `mod.rs:57-87` | MATCH |

### Stale Items in upgrade.md

1. **No significant stale items found**. The document accurately describes:
   - The `upgrade()` function exists but is not called by CLI
   - `autoupdate` config field is defined but not wired to upgrade module
   - Version checking via GitHub API
   - Installer script execution

2. **Minor**: The document references `VERSION` constant but actual code uses `VERSION` which is `env!("CARGO_PKG_VERSION")` (line 5). This is accurate.

### Observations

The `upgrade()` function is essentially a no-op from the CLI perspective. The `autoupdate` config option could be wired to automatically call `upgrade()`, but this is not currently implemented. This appears intentional based on the architecture decision to only report and let user manually upgrade.

---

## Summary Table of Issues

| Module | Severity | Type | Location | Description |
|--------|----------|------|----------|-------------|
| plugin | LOW | Stale Documentation | `plugin.md:202-206` | Missing dispatch method list entries |
| plugin | LOW | Missing Documentation | `plugin.md` | `PluginTier` enum not documented |
| skills | MEDIUM | Stale Documentation | `skills.md:82-84` | `.skills/` directory description misleading |
| snapshot | HIGH | Bug (Inconsistency) | `mod.rs:431` | MD5 used where SHA256 used elsewhere |
| upgrade | NONE | - | - | No significant issues |

---

## Improvement Suggestions (NOT Code Changes)

### Plugin Module
1. **Add `PluginTier` enum documentation** to the marketplace section
2. **Update dispatch method list** to include all 13 dispatch methods
3. **Clarify WASM plugin contract** regarding manifest.toml sibling requirement

### Skills Module  
1. **Clarify `.skills/` status**: Either mark as "loaded at agent review time" or "documentation only"
2. **Add reference to `list_skill_resources()`** function behavior
3. **Consider adding a loading location table** distinguishing runtime-loaded locations vs. documentation locations

### Snapshot Module
1. **Fix hash algorithm inconsistency**: Use SHA256 everywhere (or document why MD5 is preferred for incremental captures)
2. **Consider adding a constant** for empty content hash rather than recomputing
3. **Document the hash algorithm used** in the struct comments for FileSnapshot

### Upgrade Module
1. **Document the autoupdate limitation** more prominently as a TODO/recommended enhancement
2. **Add a section on planned integration** with the Config.autoupdate field

---

## Files Referenced

### Plugin Module Source Files
- `src/plugin/mod.rs` (main module exports)
- `src/plugin/loader.rs` (WASM loading, fuel tracking)
- `src/plugin/hooks.rs` (HookType, HookContext, HookResult)
- `src/plugin/registry.rs` (PluginRegistry, PluginInfo)
- `src/plugin/service.rs` (PluginService)
- `src/plugin/builtin/mod.rs` (BuiltinPlugin)
- `src/plugin/marketplace.rs` (MarketplaceService, PluginTier)
- `src/plugin/install.rs` (installation functions)
- `src/plugin/manifest.rs` (PluginManifest)
- `src/plugin/tui.rs` (TUI extensions)

### Skills Module Source Files
- `src/skills/mod.rs` (Skill, SkillIndex)
- `src/tool/skill.rs` (SkillTool)
- `.skills/skills/SKILL.md` (skill document)

### Snapshot Module Source Files
- `src/snapshot/mod.rs` (SnapshotManager, Snapshot, SnapshotView, FileSnapshot)
- `src/snapshot/diff.rs` (FileDiff, DiffHunk, DiffLine, DiffKind)

### Upgrade Module Source Files
- `src/upgrade/mod.rs` (VersionInfo, check_for_updates, upgrade)
- `src/config/schema.rs` (AutoupdateConfig)
