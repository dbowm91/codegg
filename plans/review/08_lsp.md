# LSP Architecture Review (08_lsp.md)

## Verified Correct Items

### LspClient Fields (client.rs:38-48)
- ✅ 9 public fields: `server_id`, `root`, `process`, `request_id`, `capabilities`, `opened_files`, `diagnostics`, `notif_tx`, `notif_rx`
- ✅ `request_id` correctly uses `AtomicU64` (signed wrap-around fixed)
- ✅ `process` is wrapped in `tokio::sync::Mutex<LspProcess>` (not `std::sync::Mutex`)

### DiagnosticEntry (client.rs:33-36)
- ✅ 2 fields: `uri: String`, `diagnostic: lsp_types::Diagnostic`

### LspError Variants (error.rs:401-426)
- ✅ All 9 variants present: `ServerNotFound`, `DownloadFailed`, `LaunchFailed`, `NotInitialized`, `RequestFailed`, `RequestTimeout`, `UnsupportedLanguage`, `Io`, `Json`

### Request Timeout (client.rs:450)
- ✅ `const REQUEST_TIMEOUT: Duration = Duration::from_secs(30)`

### DEBOUNCE_MS (diagnostics.rs:15)
- ✅ `const DEBOUNCE_MS: u64 = 150`

### Bug Fixes Verified
- ✅ `completion()` handles both `CompletionList` and `Vec<CompletionItem>` (operations.rs:282-285)
- ✅ PHP maps to `php-language-server` (language.rs:96)
- ✅ `spawn_server()` preserves user's PATH (launch.rs:41-45)
- ✅ `drain_stderr()` called after spawn (client.rs:66-69)
- ✅ `send_request()` uses `tokio::time::timeout` with 30s (client.rs:472-511)
- ✅ `close_file` race condition fixed with single write lock (service.rs:148-185)
- ✅ `save_file` race condition fixed with single write lock (service.rs:187-218)

### Other Verified
- ✅ `Lsp` struct in mod.rs has 3 fields: `service`, `operations`, `diagnostics` (mod.rs:30-34)
- ✅ `LspService` struct: `clients: Arc<RwLock<HashMap<String, ClientEntry>>>`, `config: LspConfig` (service.rs:21-24)
- ✅ `LspOperations` struct: single `service: Arc<LspService>` field (operations.rs:11-13)
- ✅ `FileDiagnostic` struct (diagnostics.rs:19-28): `file`, `line`, `column`, `message`, `severity`, `source`, `code`
- ✅ `DiagnosticsCollector` struct (diagnostics.rs:30-33): `service`, `last_update`
- ✅ `LspProcess` struct (launch.rs:20-25): `stdin`, `stdout`, `stderr`, `child`
- ✅ `parse_content_length()` exists (launch.rs:176-183)
- ✅ `terminate()` exists (launch.rs:194-198)
- ✅ `build_env_overrides()` - **undocumented** function doesn't exist (not needed)
- ✅ `read_notification()` exists (launch.rs:137-174)

---

## Incorrect/Stale Items in architecture/lsp.md

### 1. Server Count: "39 servers" (line 229)
**Status:** CORRECT - There are exactly **39 servers**

**Actual count** from `server_definitions()` (server.rs:27-385):
1. rust-analyzer | 2. gopls | 3. pyright | 4. typescript-language-server | 5. jdtls
6. clangd | 7. omnisharp | 8. kotlin-language-server | 9. lua-language-server | 10. haskell-language-server
11. metals | 12. elixir-ls | 13. clojure-lsp | 14. vue-language-server | 15. svelte-language-server
16. yaml-language-server | 17. taplo | 18. bash-language-server | 19. terraform-ls | 20. zls
21. marksman | 22. dockerfile-language-server | 23. sql-language-server | 24. ruby-lsp | 25. php-language-server
26. swift-sourcekit | 27. dart-analysis-server | 28. erlang-ls | 29. html-language-server | 30. css-language-server
31. json-language-server | 32. solidity-language-server | 33. perl-language-server | 34. powershell-editor-services | 35. graphql-language-server
36. buf-language-server | 37. r-languageserver | 38. nimlsp | 39. vls

**Fix:** The architecture doc header says "39 servers" at line 229 which is CORRECT. The table only shows a subset (lines 231-253) with "... and more" - this is fine. The skill file at line 76 incorrectly says "42 server implementations" - should be "39 server implementations".

### 2. `code_lens` Missing from Operations Documentation (lines 89-104)
**Status:** INCORRECT - `code_lens()` IS implemented

The doc shows `LspOperations` with 8 methods but `code_lens()` exists at operations.rs:333-358.

**Fix:** Add `pub async fn code_lens(&self, file_path: &Path) -> Result<Vec<CodeLens>, LspError>` to the operations.rs documentation block (line 102).

### 3. `send_initialized()` Missing from Key Operations (line 84)
**Status:** INCORRECT - `send_initialized()` exists at client.rs:181-184

**Fix:** Add `send_initialized()` to the Key operations list at line 84.

### 4. ide/mod.rs NOT part of LSP module
**Status:** MISLEADING - The architecture doc doesn't mention ide/mod.rs at all

The `src/ide/mod.rs` is **not** the LSP module - it's the IDE integration module for diff viewing. It has its own `generate_unified_diff()` and `generate_side_by_side()` functions.

---

## Bugs Found in Related Code

### download.rs:51-52 - PATH Parsing Still Uses `MAIN_SEPARATOR`
**Status:** BUG NOT FIXED

```rust
// download.rs:51-52
let path_var = std::env::var("PATH").ok()?;
let paths = std::env::split_paths(&path_var);  // <-- Actually CORRECT now
```

Wait - this is actually CORRECT now! The AGENTS.md claim that download.rs uses `MAIN_SEPARATOR` is wrong - it was fixed and now uses `std::env::split_paths()`. But the skill doc at line 198-207 shows the "fixed" version as being in download.rs, which is correct.

However, the `find_in_path()` function at download.rs:42-60 has its own PATH parsing that uses `std::env::split_paths()` (line 52), which is correct.

**Verdict:** download.rs PATH parsing is FIXED, not broken.

---

## Missing/Undocumented Items

### 1. `build_env_overrides()` - Never Existed
The architecture doc shows `build_env_overrides()` as an undocumented function that doesn't exist. This is fine - it was a documentation error.

### 2. `client_keys()` Method Missing
`LspService::client_keys()` at service.rs:317-320 returns `Vec<String>` of client keys - useful for debugging but not essential.

### 3. `get_env_overrides()` and `get_init_opts()` Undocumented
These internal methods (service.rs:237-268) exist but aren't documented - they're feature flags for server-specific configuration.

---

## Line-Specific Fixes for architecture/lsp.md

| Line | Issue | Fix |
|------|-------|-----|
| 229 | "39 servers" | Change to "42 servers" |
| 102 | Missing `code_lens()` | Add to operations.rs documentation |
| 84 | Missing `send_initialized()` | Add to Key operations list |
| 186 | "50+ extensions" | Could be more precise - actually ~80 extensions in language.rs |

---

## Skill Synchronization Needed

The skill at `.opencode/skills/lsp/SKILL.md` is largely accurate but has one error:
- Line 28: Says "30+ server definitions" but should be "42 server definitions"

**Note:** The skill's "Bug Fixes Applied" section at lines 195-264 accurately describes the fixed bugs, but the PATH parsing fix is correctly in `download.rs` (lines 198-207 match actual code).

---

## Summary

The architecture/lsp.md is **mostly accurate** with the server count of 39 being correct. The only real issues are:
1. Missing `code_lens()` from operations documentation
2. Missing `send_initialized()` from key operations
3. Skill file incorrectly says "42 server implementations" (should be 39)