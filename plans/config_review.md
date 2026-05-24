# Config Module Architecture Review

## Summary

Reviewed `architecture/config.md` against the actual implementation in `src/config/`. The documentation is **highly accurate** and reflects the actual implementation correctly. No critical bugs or discrepancies were found.

---

## Verified Items

### Config Struct (`schema.rs:22-64`)
- ✅ All ~45 fields present and match documentation
- ✅ Field ordering matches (with `schema` at line 23 as shown in skill)
- ✅ `ProviderConfig` at lines 165-180 matches documented structure

### ProviderConfig API Key Method (`schema.rs:183-205`)
- ✅ `api_key(&self, prefix: &str)` method exists with environment variable fallback
- ✅ Format correctly transforms provider name (e.g., "openai" → "OPENAI_API_KEY")

### Config::load() Flow (`schema.rs:528-553`)
- ✅ Step-by-step flow matches documented sequence
- ✅ `decrypt_provider_keys()` called at line 542 (skill says 542, arch doc says 508-509 - see note below)

### Config::save() (`schema.rs:555-585`)
- ✅ Correctly calls `encrypt_provider_keys()` before saving (line 569)

### Config::validate() (`schema.rs:587-773`)
- ✅ Produces warnings, not errors
- ✅ `medium_model` validation present (lines 628-635)
- ✅ Port validation >= 1024 (line 697)

### ConfigWatcher (`watcher.rs`)
- ✅ Struct fields match documentation (lines 12-21)
- ✅ `reload_config()` calls `decrypt_provider_keys()` at lines 157-158
- ✅ Content hash deduplication implemented correctly

### Encryption (`encryption.rs`)
- ✅ Master key lookup order: CODEGG_MASTER_KEY → CODEGG_ENCRYPTION_KEY → OPENCODE_ENCRYPTION_KEY (lines 5-10)
- ✅ `encrypt_provider_keys()` handles legacy v1 → v2 migration (lines 68-82)

### paths.rs Discovery Order (`paths.rs:12-39`)
- ✅ Matches documented order: CODEGG_TUI_CONFIG → system → global → project

### JSONC Comment Stripping (`paths.rs:103-162`)
- ✅ Line comments (`//`) handled correctly
- ✅ Block comments (`/* */`) handled correctly
- ✅ Strings with slashes preserved (tests at lines 340-345)

### merge_configs() Field Merge (`paths.rs:164-284`)
- ✅ HashMap fields (agents, mcp, commands, modes) use replace merge, not field-by-field
- ✅ Instructions are concatenated (lines 266-271)
- ✅ Provider configs use `ProviderConfig::merge()` for field-level merging (lines 225-231)

---

## Discrepancies Found

### 1. Line Number References in Skill vs Architecture Doc

**Skill line 247 vs Architecture doc line 223:**
- Skill says: `decrypt_provider_keys()` called at `schema.rs:542`
- Architecture doc says: `decrypt_provider_keys()` called at `schema.rs:508-509`

**Actual location:** Line 542 in `schema.rs` - skill is correct, arch doc is outdated.

The architecture doc references an older line number. The actual implementation has additional code (checking/middleware setup) before the decrypt call, moving it from 508-509 to 542.

### 2. Architecture Doc Missing `migrate()` Call

The architecture doc's "Loading Flow" diagram (lines 147-156) shows:
```
4. decrypt_provider_keys() → decrypt API keys if encrypted
5. migrate() → apply version migrations
```

But the actual flow at `schema.rs:540-550` is:
```rust
let mut config = crate::config::paths::merge_configs(&configs);
crate::config::encryption::decrypt_provider_keys(&mut config)  // line 542
    .map_err(|e| crate::error::ConfigError::Invalid(e.to_string()))?;
config.migrate();  // line 545
```

The flow is correct, but the architecture doc's line references are stale.

### 3. Skill References Deprecated `inline_script` Field

The skill at line 246 mentions "InlineScript is not implemented" but this is not a current issue - the field is marked `#[deprecated]` in the code at `schema.rs:111-116`. The skill is accurate.

---

## Minor Documentation Issues

### 1. ConfigWatcher::reload_config() Line Numbers

The architecture doc (line 219) says `reload_config()` is at `watcher.rs:153-154`. The actual code has it at lines 157-158 due to the addition of `config.migrate()` call in the middle of the function.

### 2. ServerConfig::merge() Not Documented

The `ServerConfig::merge()` method at `schema.rs:133-163` provides field-by-field merging for server config but is not documented in the architecture or skill.

---

## Recommendations

### For Documentation

1. **Update line references in `architecture/config.md`:**
   - Line 219: Change `watcher.rs:153-154` to `watcher.rs:157-158`
   - Lines 223-224: Change `schema.rs:508-509` to `schema.rs:542`

2. **Add `ServerConfig::merge()` to architecture doc:**
   - Document that `ServerConfig` has field-level merge behavior (like `ProviderConfig`)

### For Code

1. **No bugs found** - the implementation is correct and matches the documented design.

---

## Conclusion

The config module implementation is **correct and well-documented**. The architecture document is mostly accurate with only minor line number discrepancies. The known issues listed in the architecture doc (encrypted keys not decrypting, provider config merge, medium_model validation, dead code removal) have all been verified as fixed.

**Verified correct:**
- ✅ `decrypt_provider_keys()` called in `Config::load()`
- ✅ `decrypt_provider_keys()` called in `ConfigWatcher::reload_config()`
- ✅ `ProviderConfig::merge()` provides field-by-field merging
- ✅ `medium_model` is validated
- ✅ `find_tui_config()` and `load_tui_config()` removed (dead code)

No bugs or inconsistencies requiring fixes were found.
