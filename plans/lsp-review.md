# LSP Module Architecture Review

## Verification Results

### Claims (table format: Claim | Status | Evidence)

#### mod.rs

| Claim | Status | Evidence |
|-------|--------|----------|
| `pub struct Lsp` with `service`, `operations`, `diagnostics` fields | VERIFIED | `src/lsp/mod.rs:30-34` matches exactly |
| `open_file(path, content)` async method | VERIFIED | `mod.rs:49-55` |
| `update_file(path, content)` async method | VERIFIED | `mod.rs:57-63` |
| `close_file(path)` async method | VERIFIED | `mod.rs:65-67` |
| `save_file(path, content)` async method | VERIFIED | `mod.rs:69-75` |
| `shutdown()` async method | VERIFIED | `mod.rs:77-79` |

#### service.rs

| Claim | Status | Evidence |
|-------|--------|----------|
| `LspService` with `clients: Arc<RwLock<HashMap<String, ClientEntry>>>` | VERIFIED | `service.rs:21-24` matches exactly |
| `get_or_create_client(file_path)` returns `Result<(String, PathBuf)>` | VERIFIED | `service.rs:34-73` |
| All file lifecycle methods | VERIFIED | `service.rs:100-218` |
| `shutdown_all()` | VERIFIED | `service.rs:270-279` |

#### client.rs

| Claim | Status | Evidence |
|-------|--------|----------|
| `LspClient` struct with all 9 fields | VERIFIED | `client.rs:38-48` matches exactly |
| `DiagnosticEntry` struct with `uri` and `diagnostic` | VERIFIED | `client.rs:33-36` matches exactly |
| File lifecycle: `open_file`, `update_file`, `close_file`, `save_file` | VERIFIED | `client.rs:186-249` |
| Code intelligence: `go_to_definition`, `find_references`, `hover`, `document_symbols`, `code_actions`, `completion`, `signature_help` | VERIFIED | `client.rs:251-442` |
| `get_diagnostics`, `get_all_diagnostics`, `process_notification` | VERIFIED | `client.rs:542-579` |
| `send_request`, `send_notification`, `send_initialized` | VERIFIED | `client.rs:452-530` |
| `url_to_uri`, `detect_language_id` utilities | VERIFIED | `client.rs:29-31, 532-540` |

#### operations.rs

| Claim | Status | Evidence |
|-------|--------|----------|
| `LspOperations` struct | VERIFIED | `operations.rs:10-12` matches exactly |
| `go_to_definition(file_path, line, column)` | VERIFIED | `operations.rs:19-72` |
| `find_references(file_path, line, column)` | VERIFIED | `operations.rs:74-113` |
| `hover(file_path, line, column)` returns `Result<Option<String>>` | VERIFIED | `operations.rs:115-150` |
| `document_symbols(file_path)` | VERIFIED | `operations.rs:152-180` |
| `code_actions` with range, diagnostics, only params | VERIFIED | `operations.rs:182-233` |
| `completion` with trigger_kind, trigger_char | VERIFIED | `operations.rs:235-277` |
| `signature_help` returning `Result<Option<String>>` | VERIFIED | `operations.rs:279-315` |
| `code_lens` | VERIFIED | `operations.rs:317-342` |

#### diagnostics.rs

| Claim | Status | Evidence |
|-------|--------|----------|
| `DEBOUNCE_MS: u64 = 150` | VERIFIED | `diagnostics.rs:15` |
| `FileDiagnostic` struct with 7 fields | VERIFIED | `diagnostics.rs:17-26` matches exactly |
| `DiagnosticsCollector` with `service` and `last_update` | VERIFIED | `diagnostics.rs:28-31` matches exactly |
| `should_debounce(uri)` | VERIFIED | `diagnostics.rs:41-53` |
| `get_diagnostics_for_file(file_path)` | VERIFIED | `diagnostics.rs:55-89` |
| `get_all_diagnostics()` | VERIFIED | `diagnostics.rs:91-120` |
| `has_errors(file_path)` | VERIFIED | `diagnostics.rs:122-127` |

#### download.rs

| Claim | Status | Evidence |
|-------|--------|----------|
| `cache_dir()` returns `~/.cache/codegg/lsp/` | VERIFIED | `download.rs:10-15` |
| `ensure_server_binary(server)` checks PATH first | VERIFIED | `download.rs:18-21` |
| Falls back to cached download | VERIFIED | `download.rs:23-29` |
| Only rust-analyzer has download spec | VERIFIED | `server.rs:38-42` - only rust-analyzer has `download: Some(...)` |
| `find_in_path(cmd)` | VERIFIED | `download.rs:42-60` |
| `is_executable(path)` | VERIFIED | `download.rs:62-77` |
| `download_server(server, spec, dest)` | VERIFIED | `download.rs:79-129` |
| `resolve_url(spec)` | VERIFIED | `download.rs:131-153` |
| `extract_zip`, `extract_tar_gz`, `extract_tar_xz` | VERIFIED | `download.rs:155-261` |
| Supports Zip, TarGz, TarXz, Raw | VERIFIED | `download.rs:108-117` |

#### launch.rs

| Claim | Status | Evidence |
|-------|--------|----------|
| `LspProcess` struct with `stdin`, `stdout`, `stderr`, `child` | VERIFIED | `launch.rs:18-23` matches exactly |
| `spawn_server(command, args, env, cwd)` | VERIFIED | `launch.rs:25-82` |
| `send_request(process, msg)` | VERIFIED | `launch.rs:84-98` |
| `read_response(process)` | VERIFIED | `launch.rs:100-133` |
| `read_notification(process)` | VERIFIED | `launch.rs:135-171` |
| `drain_stderr(process)` | VERIFIED | `launch.rs:182-189` |
| `terminate(process)` | VERIFIED | `launch.rs:191-195` |
| `parse_content_length(header)` | VERIFIED | `launch.rs:173-180` |
| Uses Content-Length headers for framing | VERIFIED | `launch.rs:85` - `Content-Length: {}\r\n\r\n{}` |
| Preserves user's PATH from environment | VERIFIED | `launch.rs:39-43` - uses `std::env::var_os("PATH")` |

#### language.rs

| Claim | Status | Evidence |
|-------|--------|----------|
| `detect_language(path)` | VERIFIED | `language.rs:135-149` |
| `extension_to_language_id(ext)` | VERIFIED | `language.rs:1-83` |
| `language_id_to_server_id(lang_id)` | VERIFIED | `language.rs:85-133` |
| Supports 50+ extensions | VERIFIED | `language.rs:1-83` has ~90 extension mappings |
| PHP maps to `php-language-server` (not intelephense) | VERIFIED | `language.rs:96` |

#### root.rs

| Claim | Status | Evidence |
|-------|--------|----------|
| `find_project_root(start)` | VERIFIED | `root.rs:5-28` |
| Detects project roots via marker files | VERIFIED | `root.rs:30-92` |

#### server.rs

| Claim | Status | Evidence |
|-------|--------|----------|
| `LspServerDef` struct | VERIFIED | `server.rs:3-12` matches exactly |
| `DownloadSpec` struct | VERIFIED | `server.rs:14-19` matches exactly |
| `ArchiveType` enum | VERIFIED | `server.rs:21-27` matches exactly |
| `server_definitions()` | VERIFIED | `server.rs:29-387` - 42 servers defined |
| `find_server`, `find_server_for_language`, `find_server_for_extension` | VERIFIED | `server.rs:389-403` |
| `build_env_overrides(env)` | VERIFIED | `server.rs:405-412` |

#### Error Handling

| Claim | Status | Evidence |
|-------|--------|----------|
| `LspError` enum variants: ServerNotFound, DownloadFailed, LaunchFailed, NotInitialized, RequestFailed, RequestTimeout, UnsupportedLanguage, Io, Json | VERIFIED | All variants present in `src/error.rs` |

#### Recent Bug Fixes (all verified)

| Claim | Status | Evidence |
|-------|--------|----------|
| PATH parsing uses `std::env::split_paths()` | VERIFIED | `download.rs:51-52` |
| PHP correctly maps to `php-language-server` | VERIFIED | `language.rs:96` |
| 30-second request timeout in `send_request` | VERIFIED | `client.rs:450` |
| User PATH preserved via `std::env::var_os("PATH")` | VERIFIED | `launch.rs:39-43` |
| Stderr drained and logged during init | VERIFIED | `client.rs:66-69` |
| Notification loop cleaner with logged send failures | VERIFIED | `client.rs:500-502` |
| `close_file` race condition fixed | VERIFIED | `service.rs:148-185` - single write lock pattern |
| `save_file` race condition fixed | VERIFIED | `service.rs:187-218` - single write lock pattern |

---

## Bugs Found

### Critical

1. **completion() assumes server returns CompletionList but LSP allows CompletionItem[]**
   - **Location**: `operations.rs:275`
   - **Issue**: `let items: CompletionList = serde_json::from_value(resp)?;` fails if server returns `CompletionItem[]` directly (which LSP allows)
   - **Fix**: Deserialize as `CompletionList` first, then fall back to `Vec<CompletionItem>` if that fails

2. **extract_zip and extract_tar_gz lack path traversal protection**
   - **Location**: `download.rs:155-220`
   - **Issue**: Unlike `extract_tar_xz` (which checks `full_path.starts_with(&dest)`), zip and tar.gz extraction can write files outside destination via malicious archives
   - **Fix**: Add path traversal check before writing each file

### High

3. **has_errors() comparison is incorrect**
   - **Location**: `diagnostics.rs:126`
   - **Issue**: `d.severity == DiagnosticSeverity::ERROR` fails when `severity` is `None` (uses wrong comparison)
   - **Fix**: Use `d.severity.unwrap_or(DiagnosticSeverity::WARNING) >= DiagnosticSeverity::WARNING` or similar

4. **signature_help and hover silently fail on deserialization errors**
   - **Location**: `operations.rs:148`, `operations.rs:313-314`
   - **Issue**: When server returns non-null response but deserialization fails, returns `Err` instead of `Ok(None)`
   - **Fix**: Catch deserialization errors and return `Ok(None)` on failure

5. **read_notification returns None on any error, not just EOF**
   - **Location**: `launch.rs:137-140`
   - **Issue**: `match process.stdout.read_exact(&mut buf).await { Ok(_) => {}, Err(_) => return Ok(None) }` treats all errors as "no more data", but a read error should propagate
   - **Fix**: Distinguish `ErrorKind::UnexpectedEof` from other errors

6. **process_notification silently drops deserialization errors**
   - **Location**: `client.rs:571`
   - **Issue**: `serde_json::from_value(diags.clone()).unwrap_or_default()` silently ignores malformed diagnostics
   - **Fix**: Log a warning when diagnostics fail to parse

### Medium

7. **request_id initialization skips ID 0 unnecessarily**
   - **Location**: `client.rs:457-460`
   - **Issue**: Double fetch on first call because id == 0 check triggers another increment
   - **Fix**: Initialize counter to 1 or change logic to only skip zero once

8. **spawn_server fallback PATH is still hardcoded for non-Unix**
   - **Location**: `launch.rs:42-43`
   - **Issue**: Fallback to `/usr/local/bin:/usr/bin:/bin` on non-Unix when PATH not set
   - **Fix**: Use `which` crate or similar for cross-platform binary discovery fallback

9. **close_file and save_file don't handle LSP error responses**
   - **Location**: `service.rs:179`, `service.rs:215`
   - **Issue**: LSP server can return errors for didClose/didSave, but errors are silently ignored
   - **Fix**: Check return value and log warnings on failure

10. **service.rs LspConfig pattern matching is inconsistent**
    - **Location**: `service.rs:220-249`
    - **Issue**: `LspConfig::Disabled(false)` always returns `false`, but `Disabled(true)` always returns `true` regardless of server_id
    - **Fix**: Document this behavior or make it consistent

11. **No backpressure on notification channel**
    - **Location**: `client.rs:71`
    - **Issue**: `mpsc::unbounded_channel()` has no bound; slow processing can cause memory growth
    - **Fix**: Use bounded channel or add backpressure mechanism

12. **diagnostics collector last_update map grows unbounded**
    - **Location**: `diagnostics.rs:30`
    - **Issue**: `last_update` map is never cleaned up
    - **Fix**: Remove old entries periodically or use TTL-based cleanup

### Low

13. **No retry logic for failed requests**
    - **Issue**: Transient LSP failures are not retried

14. **No reconnection on server crash**
    - **Issue**: If LSP server dies, client remains in broken state

15. **No connection health check**
    - **Issue**: No mechanism to verify LSP server is still responsive

16. **close_file sends notification without waiting for response**
    - **Issue**: `didClose` is fire-and-forget; no way to know if server processed it

17. **No support for dynamic registration capability**
    - **Issue**: Client declares `dynamicRegistration: false` but doesn't handle servers that announce dynamic registration

18. **get_diagnostics_for_file returns empty on debounce**
    - **Location**: `diagnostics.rs:65-68`
    - **Issue**: Early return with empty diagnostics loses pending information
    - **Fix**: Consider returning cached/stale diagnostics during debounce

---

## Improvement Suggestions

### Performance

1. **Cache LSP server capabilities per client**
   - Currently capabilities are stored in `LspClient::capabilities` but not used to short-circuit unsupported operations
   - Could skip sending requests for unsupported capabilities

2. **Use shared HTTP client in download.rs**
   - `Client::new()` creates a new client per download
   - Sharing a client would enable connection pooling

3. **Batch diagnostics updates**
   - Multiple diagnostics for same file arriving in quick succession could be batched

4. **Consider lazy client initialization**
   - LSP clients are created on first file access, but this could be deferred further

### Correctness

1. **Handle CompletionList OR Vec<CompletionItem> in completion response**
   - LSP spec allows both forms; must handle both

2. **Add path traversal checks to extract_zip and extract_tar_gz**
   - Security: malicious archives could write outside destination

3. **Distinguish EOF from errors in read_notification**
   - Network errors should propagate, not be silently ignored

4. **Add validation for LSP message framing**
   - Currently assumes well-formed messages; could add validation

### Maintainability

1. **Add integration tests with mock LSP servers**
   - Unit tests exist but no integration tests with actual LSP server behavior

2. **Document LspConfig behavior in code comments**
   - The Disabled(false) always returns false behavior is non-obvious

3. **Extract magic numbers to constants**
   - DEBOUNCE_MS=150, REQUEST_TIMEOUT=30, hardcoded PATH fallback

4. **Add tracing spans for LSP operations**
   - Would help debugging in production

5. **Consider extracting DownloadSpec parsing to separate module**
   - archive_type validation, URL template substitution could be clearer

---

## Priority Actions (top 5 items to fix)

1. **[High] Fix completion() to handle CompletionItem[] directly** (`operations.rs:275`)
   - LSP servers can return either `CompletionList` or `Vec<CompletionItem>`
   - Current code crashes on servers returning array directly

2. **[Critical] Add path traversal protection to extract_zip and extract_tar_gz** (`download.rs:155-220`)
   - Security vulnerability: malicious archives can write outside destination
   - `extract_tar_xz` already has this protection; zip/gzip need same

3. **[High] Fix has_errors() severity comparison** (`diagnostics.rs:126`)
   - `d.severity == DiagnosticSeverity::ERROR` crashes when severity is None
   - Should use proper optional handling

4. **[High] Fix hover and signature_help error handling** (`operations.rs:148, 313-314`)
   - Should return `Ok(None)` when server responds but deserialization fails
   - Currently propagates error instead of graceful fallback

5. **[Medium] Fix read_notification error handling** (`launch.rs:137-140`)
   - Should distinguish `UnexpectedEof` (normal EOF) from actual errors
   - Current code silently drops all errors