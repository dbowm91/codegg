# Config Module Override

This file contains config-specific guidance and overrides root AGENTS.md.

## Config Loading Flow

1. `resolve_config_paths()` - Collect config file paths (CODEGG_TUI_CONFIG → system → global → project)
2. `load_config()` - Parse each file (JSONC → JSON5 with env var interpolation)
3. `merge_configs()` - Later files override earlier; HashMaps merge field-by-field
4. `decrypt_provider_keys()` - **Automatically decrypts API keys on load** (critical fix from bug review)
5. `migrate()` - Apply version migrations
6. `validate()` - Validate config values (warnings, not hard errors)

## Key Bug Fixes (2026-05-21)

### decrypt_provider_keys() called in Config::load()
**Location**: `src/config/schema.rs:506-507`

API keys encrypted via `save()` are now automatically decrypted when config is loaded. Previously they were never decrypted on load.

### ProviderConfig field-by-field merge
**Location**: `src/config/schema.rs:175-234` (merge method)

When merging configs with the same provider, fields are now merged individually rather than replacing the entire struct.

### medium_model validation
**Location**: `src/config/schema.rs:553-561`

Validates `provider/model` format for `medium_model` just like `model` and `small_model`.

### migrate_from_v0 guards version check
**Location**: `src/config/schema.rs:749-756`

Migration now only runs when `version == "0"`, not unconditionally.

## ConfigWatcher

Hot-reload with content hash deduplication:
- Uses `notify` crate with debouncing (default 500ms)
- Ignores patterns via glob matching
- Only triggers reload if content hash actually changed

## API Key Encryption

Master key lookup order: `CODEGG_MASTER_KEY` → `CODEGG_ENCRYPTION_KEY` → `OPENCODE_ENCRYPTION_KEY`

Legacy v1 ciphertexts are automatically migrated to v2 format on save.

## Validation

Validation failures produce **warnings**, not errors - the app starts with a partially invalid config.

Validated:
- `log_level`: `debug|info|warn|error|trace`
- `share`: `manual|auto|disabled`
- `model`/`small_model`/`medium_model`: `provider/model` format
- MCP server types: `local` requires `command`, `remote` requires `url`
- Agent `mode`: `subagent|primary|all`
- Agent `color`: hex color or theme color name