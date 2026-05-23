# LSP Module Architecture Review

## Verified Claims

### mod.rs (Lsp struct)
- `Lsp` struct with `service`, `operations`, `diagnostics` fields - **MATCHES**
- Methods `open_file`, `update_file`, `close_file`, `save_file`, `shutdown` - **MATCHES**

### service.rs (LspService)
- `LspService` struct with `clients: Arc<RwLock<HashMap<String, ClientEntry>>>` and `config: LspConfig` - **MATCHES**
- Methods: `get_or_create_client`, `open_file`, `update_file`, `close_file`, `save_file`, `shutdown_all` - **MATCHES**

### client.rs (LspClient)
- `LspClient` struct with all 9 fields - **MATCHES**
- `request_id: AtomicU64` (documentation showed `AtomicI64` - **FIXED**)
- `DiagnosticEntry` struct - **MATCHES**
- Methods: `open_file`, `update_file`, `close_file`, `save_file`, `go_to_definition`, `find_references`, `hover`, `document_symbols`, `code_actions`, `completion`, `signature_help`, `get_diagnostics`, `get_all_diagnostics`, `process_notification`, `send_request`, `send_notification`, `send_initialized`, `url_to_uri`, `detect_language_id` - **ALL MATCH**

### operations.rs (LspOperations)
- Struct with `service: Arc<LspService>` - **MATCHES**
- All method signatures match: `go_to_definition`, `find_references`, `hover`, `document_symbols`, `code_actions`, `completion`, `signature_help`, `code_lens` - **ALL MATCH**

### diagnostics.rs (DiagnosticsCollector, FileDiagnostic)
- `DEBOUNCE_MS: u64 = 150` - **MATCHES**
- `FileDiagnostic` struct with all 8 fields - **MATCHES**
- `DiagnosticsCollector` with `service` and `last_update` - **MATCHES**
- Methods: `should_debounce`, `get_diagnostics_for_file`, `get_all_diagnostics`, `has_errors` - **ALL MATCH**

### download.rs
- `cache_dir()` returns `PathBuf` - **MATCHES**
- `ensure_server_binary` - **MATCHES**
- `find_in_path`, `is_executable`, `download_server`, `resolve_url`, `extract_zip`, `extract_tar_gz`, `extract_tar_xz` - **ALL MATCH**
- Uses `std::env::split_paths()` for PATH parsing (was fixed) - **MATCHES**

### launch.rs (LspProcess)
- `LspProcess` struct with `stdin`, `stdout`, `stderr: BufReader<ChildStderr>`, `child` - **MATCHES**
- `spawn_server` signature with `command`, `args`, `env`, `cwd` - **MATCHES**
- `send_request`, `read_response`, `read_notification`, `drain_stderr`, `terminate`, `parse_content_length` - **ALL MATCH**
- Preserves user's PATH from environment - **MATCHES**

### language.rs
- `detect_language`, `extension_to_language_id`, `language_id_to_server_id` - **ALL MATCH**
- 50+ extensions supported - **MATCHES**

### root.rs
- `find_project_root` function - **MATCHES**
- Marker files list matches implementation - **MATCHES**

### server.rs (LspServerDef, DownloadSpec, ArchiveType)
- `LspServerDef` with all fields - **MATCHES**
- `DownloadSpec` with `url_template`, `archive_type`, `binary_name` - **MATCHES**
- `ArchiveType` enum with `Zip`, `TarGz`, `TarXz`, `Raw` - **MATCHES**
- `server_definitions()`, `find_server`, `find_server_for_language`, `find_server_for_extension`, `build_env_overrides` - **ALL MATCH**
- 42 servers supported - **MATCHES**

### Error Handling
- `LspError` enum with all 9 variants - **MATCHES**
- `is_retryable()` method - **MATCHES**

### Bug Fixes (documented in "Recent Bug Fixes" section)
- PATH parsing with `std::env::split_paths()` - **FIXED**
- PHP mapping to `php-language-server` - **FIXED**
- Request timeout (30s) - **FIXED**
- Hardcoded PATH preserved - **FIXED**
- Stderr logging - **FIXED**
- Notification loop - **FIXED**
- close_file race condition - **FIXED**
- save_file race condition - **FIXED**

## Bugs/Discrepancies Found

### 1. `request_id` type mismatch (MEDIUM)
**Location**: `client.rs:42` vs `architecture/lsp.md:66`

**Issue**: Documentation shows `request_id: AtomicI64` but actual implementation uses `AtomicU64` (line 17 and 42 in client.rs).

**Impact**: Documentation is outdated and could cause confusion for developers reading the spec.

### 2. `build_env_overrides` defined but unused (LOW)
**Location**: `server.rs:405-411`

**Issue**: Function is defined but never called anywhere in the codebase. The `LspService::get_env_overrides` method (lines 237-250 in service.rs) implements its own logic instead of using this helper.

**Impact**: Dead code - not a bug but indicates possible refactoring opportunity or incomplete integration.

### 3. Documentation missing undocumented public methods (LOW)
**Location**: `service.rs`

**Issue**: Several public methods are not documented in the architecture:
- `get_diagnostics_for_key` (line 281)
- `get_all_diagnostics_for_key` (line 293)
- `send_request` (line 304)
- `client_keys` (line 317)

**Impact**: Incomplete documentation - developers using these methods won't have guidance.

## Improvement Suggestions

### High Priority

1. **Fix `build_env_overrides` integration or remove it**
   - The function in `server.rs:405-411` is defined but never used.
   - `LspService::get_env_overrides` duplicates its logic.
   - Either integrate it properly or remove the dead code.

2. **Document undocumented public methods in LspService**
   - `get_diagnostics_for_key`
   - `get_all_diagnostics_for_key`
   - `send_request`
   - `client_keys`

### Medium Priority

3. **Update `request_id` type in documentation**
   - Change from `AtomicI64` to `AtomicU64` in architecture/lsp.md line 66.
   - Reason: The implementation uses `AtomicU64` to avoid signed overflow issues (wrap-around concern for request IDs).

4. **Add LspService close_file/save_file method signatures to docs**
   - These methods have complex index-based lookup logic that isn't explained.
   - Documentation should clarify the "find client by opened file" pattern.

### Low Priority

5. **Add code_lens to supported operations list**
   - The operations.rs includes `code_lens()` method (line 333-358) but the client.rs documentation in the arch doc only lists `go_to_definition`, `find_references`, `hover`, `document_symbols`, `code_actions`, `completion`, `signature_help`.
   - `code_lens` should be added to match implementation.

6. **Clarify download.rs behavior for non-rust-analyzer servers**
   - The doc says "Only rust-analyzer has download specification currently" which is accurate, but could note that other servers should ideally have download specs too.

7. **Add examples to operations.rs method signatures**
   - Complex operations like `code_actions` take many parameters; examples in docstrings would help users.

## Summary

The LSP module implementation is largely correct and well-documented. The "Recent Bug Fixes" section accurately describes fixes that have been applied. The main issues are:

1. One type mismatch (`AtomicI64` vs `AtomicU64`)
2. One dead function (`build_env_overrides`)
3. Missing documentation for 4 public methods in `LspService`

All actual bugs mentioned in the documentation have been verified as fixed in the implementation.