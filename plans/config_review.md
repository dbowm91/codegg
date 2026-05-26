# Config Architecture Review

## Summary
The config architecture document is well-maintained and accurate. All key types, methods, and integration points match the source code. The "Known Issues Fixed" section accurately documents bugs that have been resolved.

## Verified Correct
- Config struct at `src/config/schema.rs:22-64` matches doc's field listing exactly
- ProviderConfig at lines 167-180 matches doc (lines 69-83)
- ProviderConfig::api_key() method at lines 183-205 checks env vars first (format `{PREFIX}_API_KEY`), then api_key field, then encrypted_api_key - matches doc
- ServerConfig::merge() at lines 134-162 performs field-by-field merging as documented
- ConfigWatcher struct at `src/config/watcher.rs:12-21` matches doc
- Master key lookup order at `src/config/encryption.rs:5-10`: CODEGG_MASTER_KEY → CODEGG_ENCRYPTION_KEY → OPENCODE_ENCRYPTION_KEY matches doc
- Config::load() flow at lines 529-553: resolve paths → load → merge → decrypt → migrate → validate matches doc at lines 158-166
- Decrypt on hot-reload at `watcher.rs:163` matches doc "Known Issues" section at line 229
- Decrypt on load at `schema.rs:542` matches doc at line 233
- ProviderConfig::merge() at lines 207-244 with field-by-field merging matches doc at line 237
- medium_model validation at `schema.rs:628-635` matches doc at line 241
- Discovery order in paths.rs: CODEGG_TUI_CONFIG → system → global → project matches doc at lines 106-110
- Encrypted keys not decrypting on hot-reload bug fix documented at line 227-229 is accurate
- Encrypted keys not decrypting on load bug fix documented at line 231-233 is accurate
- Provider config fields lost during merge bug fix documented at line 235-237 is accurate
- medium_model not validated bug fix documented at line 239-241 is accurate

## Discrepancies Found
- **Dead tui_config code section (lines 243-245)**: The doc claims `find_tui_config()` and `load_tui_config()` were "Removed from paths.rs and mod.rs to clean up dead code (2026-05-22)". This is accurate - grep found no matches for these functions. However, the removal date suggests this was done recently and the doc should note this is already done (not pending).
- **File reference typo**: Doc at line 94 says "ServerConfig has a `merge()` method at `schema.rs:134-162`" - this is correct and line numbers are accurate.

## Bugs Identified
- No bugs found - all documented behaviors match implementation

## Improvement Suggestions
- **Stale "Known Issues Fixed" section**: The issues at lines 227-241 are documented as "fixes" but should be considered historical documentation. The "Dead tui_config code removed" note at 243-245 is the most recent (2026-05-22) and should be marked as such. Consider adding a date to each "fix" note to track when they were resolved.
- **Add additional validation checks**: ServerConfig validates port >= 1024 (schema.rs:697), tool_timeout_seconds (schema.rs:701-707), and max_parallel_tools (schema.rs:709-716) but these aren't documented in the Validation section (lines 168-179)

## Stale Items in Architecture Doc
- **Validation section incomplete**: Lines 172-179 document validated fields but miss several validations that exist in code:
  - `tool_timeout_seconds` cannot be 0 or exceed 3600 (schema.rs:701-707)
  - `max_parallel_tools` cannot be 0 or exceed 100 (schema.rs:709-716)
  - Compaction threshold must be 0.1-1.0 and max_tokens at least 1000 (schema.rs:719-733)