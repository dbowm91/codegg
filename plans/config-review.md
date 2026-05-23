# Config Module Architecture Review

## Verified Claims

### Config struct fields
All documented Config fields match implementation (schema.rs lines 22-64):
- version, log_level, model, small_model, medium_model, auto_route_models, default_agent, username, share, autoupdate, server, provider, disabled_providers, enabled_providers, agent, mcp, permission, compaction, subagent, skills, commands, templates, instructions, layout, tools, formatter, lsp, watcher, snapshot, snapshot_config, plugin, enterprise, experimental, mode, keybinds, vim_mode, hooks, notifications, catalog ✓

### ProviderConfig struct
All documented ProviderConfig fields match implementation (schema.rs lines 167-180):
- api_key, encrypted_api_key, encrypted, base_url, enterprise_url, set_cache_key, timeout, chunk_timeout, whitelist, blacklist, models, options ✓

### ProviderConfig.merge() method
Field-by-field merge implemented at schema.rs:207-244 ✓

### ProviderConfig.api_key() method
Environment variable fallback implemented at schema.rs:183-205 ✓

### Config::load() flow
Correctly calls: resolve_config_paths → load_config → merge_configs → decrypt_provider_keys → migrate → validate (schema.rs:529-553) ✓

### Config::save() flow
Correctly encrypts keys before saving (schema.rs:555-585) ✓

### Config::validate() checks
- log_level validation (schema.rs:590-598) ✓
- share validation (schema.rs:600-608) ✓
- model format validation (schema.rs:610-617) ✓
- small_model format validation (schema.rs:619-626) ✓
- medium_model format validation (schema.rs:628-635) ✓
- port >= 1024 validation (schema.rs:696-699) ✓
- agent mode validation (schema.rs:737-745) ✓
- agent color validation (schema.rs:746-764) ✓
- MCP server type validation (schema.rs:656-685) ✓

### ConfigWatcher struct
All documented fields match implementation (watcher.rs lines 12-21):
- watcher, rx, tx, watched_paths, started, debounce_duration, last_hash, ignore_patterns ✓

### ConfigWatcher methods
- new() with 500ms default debounce (watcher.rs:24-36) ✓
- with_config(&WatcherConfig) (watcher.rs:38-46) ✓
- start() (watcher.rs:48-90) ✓
- recv() with hash deduplication (watcher.rs:92-115) ✓
- reload_now() (watcher.rs:117-119) ✓
- reload_config() calls decrypt_provider_keys (watcher.rs:140-161) ✓

### Encryption functions
- get_master_key() order: CODEGG_MASTER_KEY → CODEGG_ENCRYPTION_KEY → OPENCODE_ENCRYPTION_KEY (encryption.rs:5-10) ✓
- decrypt_provider_keys() (encryption.rs:12-42) ✓
- encrypt_provider_keys() (encryption.rs:44-107) ✓

### Known bugs fixed (verified)
- decrypt_provider_keys in Config::load() (schema.rs:542) ✓
- decrypt_provider_keys in reload_config() (watcher.rs:157) ✓
- medium_model validation (schema.rs:628-635) ✓
- find_tui_config/load_tui_config removed ✓

### Discovery order
CODEGG_TUI_CONFIG → system → global → project (paths.rs:12-39) ✓

### merge_configs behavior
- Simple fields override via merge_option! macro (paths.rs:166-202) ✓
- server uses ServerConfig::merge() (paths.rs:203-208) ✓
- provider uses ProviderConfig::merge() for same provider (paths.rs:222-235) ✓
- watcher merges only non-None fields (paths.rs:209-221) ✓
- instructions concatenates (paths.rs:266-271) ✓

---

## Bugs/Discrepancies Found

### 1. Config struct missing `schema` field (Low)
**Location**: architecture/config.md lines 22-62

The architecture doc shows Config starting with `pub version: Option<String>` but the actual struct has:
```rust
#[serde(rename = "$schema")]
pub schema: Option<String>,  // <-- MISSING FROM DOC
pub version: Option<String>,
```

**Impact**: Documentation is slightly incomplete but not misleading.

### 2. Agent config merge is NOT field-by-field (Medium)
**Location**: architecture/config.md line 107

The doc states "HashMaps merge field-by-field" but for agents, the implementation does a full replace:
```rust
// paths.rs:236-244
if let Some(ref agents) = config.agent {
    match &mut merged.agent {
        Some(ref mut existing) => {
            for (k, v) in agents {
                existing.insert(k.clone(), v.clone()); // Full replace, not field-by-field
            }
        }
```

Compare to provider merge (paths.rs:222-235):
```rust
for (k, v) in providers {
    if let Some(existing) = existing.get_mut(k) {
        existing.merge(v);  // Field-by-field merge
    } else {
        existing.insert(k.clone(), v.clone());
    }
}
```

**Impact**: Users expecting agent config merging behavior similar to provider merging will be surprised. This is a documentation inaccuracy—the doc implied field-by-field merge for all HashMaps.

### 3. Skill doc references wrong line numbers (Low)
**Location**: .opencode/skills/config/SKILL.md line 248

States `decrypt_provider_keys()` is called at `schema.rs:508-509` but actual is at line 542.

**Impact**: Minor confusion when reading source code vs docs.

### 4. Skill doc uses wrong constant name (Low)
**Location**: .opencode/skills/config/SKILL.md line 168

States `CRYPTO_V2_PREFIX: &str = "v2:"` but actual is `FORMAT_V2_PREFIX` imported from `crate::crypto`.

**Impact**: Developer trying to use this constant will fail.

### 5. Architecture doc references wrong line numbers for watcher fix (Low)
**Location**: architecture/config.md line 219

States fix at `watcher.rs:153-154` but actual is at lines 157-158.

**Impact**: Minor confusion when debugging.

---

## Improvement Suggestions

### Priority: Medium

1. **Document agent merge behavior accurately**
   - Update architecture/config.md to clarify that agent configs are replaced wholesale (not field-by-field merged)
   - Or consider implementing field-by-field merge for agents to match provider behavior

2. **Add `schema` field to Config documentation**
   - Update architecture/config.md to include `#[serde(rename = "$schema")] pub schema: Option<String>`

### Priority: Low

3. **Fix skill line numbers**
   - Update .opencode/skills/config/SKILL.md line 248 from `schema.rs:508-509` to `schema.rs:542`

4. **Fix CRYPTO_V2_PREFIX reference**
   - Update .opencode/skills/config/SKILL.md line 168 to reference `FORMAT_V2_PREFIX` from `crate::crypto`

5. **Fix architecture doc line reference**
   - Update architecture/config.md line 219 from `watcher.rs:153-154` to `watcher.rs:157-158`

---

## Summary

The config module implementation is solid and well-tested. The architecture document is mostly accurate but has minor discrepancies in:
- Missing `schema` field in Config struct
- Incorrect description of agent merge behavior (full replace vs field-by-field)
- Stale line number references in both architecture doc and skill doc

The "Known Issues Fixed" section accurately documents the bug fixes that were made. No actual bugs in the implementation were found—the issues are purely documentation-related.