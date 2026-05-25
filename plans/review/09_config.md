# Config Module Architecture Review (2026-05-25)

## Verified Correct Items

| Item | Location | Notes |
|------|----------|-------|
| Config struct fields | schema.rs:22-64 | All 35+ fields match exactly, including `schema` field (was missing from doc) |
| ProviderConfig.api_key() signature | schema.rs:183 | Takes `&str` prefix, returns `Option<String>`, checks env vars first |
| ProviderConfig.merge() | schema.rs:207-244 | Field-by-field merge implemented correctly |
| ServerConfig.merge() | schema.rs:134-162 | Field-by-field merge implemented correctly |
| decrypt_provider_keys() called in Config::load() | schema.rs:542 | ✓ Fixed 2026-05-21 |
| decrypt_provider_keys() called in reload_config() | watcher.rs:163 | ✓ Fixed 2026-05-22 |
| medium_model validation | schema.rs:628-635 | ✓ Fixed 2026-05-21 |
| Master key lookup order | encryption.rs:5-10 | CODEGG_MASTER_KEY → CODEGG_ENCRYPTION_KEY → OPENCODE_ENCRYPTION_KEY |
| ConfigWatcher struct fields | watcher.rs:12-21 | All 7 fields match exactly |
| WatcherConfig fields | schema.rs:467-471 | `ignore` and `debounce_duration_ms` match exactly |
| merge_configs instructions concat | paths.rs:266-271 | Instructions are concatenated, not replaced |
| merge_configs provider field-by-field | paths.rs:222-234 | Uses `ProviderConfig::merge()` for existing keys |

## Incorrect/Stale Items

### 1. Config struct missing `schema` field (Line 23)
**Doc shows**: Field list starts at line 23 with `version`
**Actual at**: schema.rs:24-25 has `schema: Option<String>` before `version`
**Fix**: Add `pub schema: Option<String>,` as first field after `#[serde(default)]`

### 2. ProviderConfig.api_key() signature incomplete (Line 84)
**Doc says**: "checks environment variables first"
**Actual**: Takes `prefix: &str` parameter, constructs env var name via `format!("{}_API_KEY", prefix.to_uppercase().replace('-', "_"))` (schema.rs:184)
**Fix**: Update line 84 to: `ProviderConfig::api_key(&self, prefix: &str) -> Option<String>` that checks `${PREFIX}_API_KEY` env var first, then `api_key`, then `encrypted_api_key`

### 3. Known Issues section line references off (Lines 221-237)
**Line 229**: Doc says `ProviderConfig::merge()` at `schema.rs:175-212` - actual is **207-244**
**Line 233**: Doc says `medium_model` validation at `schema.rs:594-601` - actual is **628-635**
**Line 225**: Doc says `decrypt_provider_keys()` at `schema.rs:542` - confirmed correct
**Line 221**: Doc says `ConfigWatcher::reload_config()` calls `decrypt_provider_keys()` at `watcher.rs:163` - confirmed correct

## Minor Documentation Improvements

### 1. Config struct - explicit field order documentation
The doc shows fields in a specific order that mostly matches actual struct. The `schema` field is an addition. The order in schema.rs:
- schema (added 2026-05-22, was not in original doc)
- version
- log_level
- model / small_model / medium_model
- ...etc

### 2. ProviderConfig::api_key() master key fallback
The doc (line 84) doesn't document that `api_key()` method also checks `encrypted_api_key` with master key decryption as a third fallback (schema.rs:193-202). This is useful behavior worth documenting.

### 3. encryption.rs functions are async-safe
Both `encrypt_provider_keys` and `decrypt_provider_keys` are synchronous functions that do CPU-intensive crypto work. The architecture doc doesn't note this. For hot-reload paths (watcher.rs), crypto operations block the async receiver. This is fine given small config sizes.

## Summary

The architecture document is **largely accurate**. The main fixes needed:
1. Add `schema` field to Config struct documentation (line 23 area)
2. Correct line number references in "Known Issues Fixed" section (lines 229, 233)
3. Document `ProviderConfig::api_key(&self, prefix: &str)` signature more completely

No bugs found in the actual config implementation - all "known issues fixed" items verified correct.