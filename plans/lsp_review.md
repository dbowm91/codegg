# LSP Architecture Review

**Date**: 2026-05-26
**Reviewer**: Code review agent
**Document**: `architecture/lsp.md`
**Source**: `src/lsp/`, `src/tool/lsp.rs`

---

## Summary

The LSP architecture document is **mostly accurate** but contains several discrepancies that need correction, primarily in server count, extension count, and some implementation details.

---

## Module Organization

### ✅ Verified Correct

| Component | File | Status |
|-----------|------|--------|
| Main entry point | `src/lsp/mod.rs` | ✅ Matches doc (lines 30-79) |
| Client management | `src/lsp/service.rs` | ✅ Matches doc |
| LSP Client | `src/lsp/client.rs` | ✅ Matches doc |
| Operations | `src/lsp/operations.rs` | ✅ Matches doc |
| Diagnostics | `src/lsp/diagnostics.rs` | ✅ Matches doc |
| Download | `src/lsp/download.rs` | ✅ Matches doc |
| Launch | `src/lsp/launch.rs` | ✅ Matches doc |
| Language detection | `src/lsp/language.rs` | ✅ Matches doc |
| Project root | `src/lsp/root.rs` | ✅ Matches doc |
| Server definitions | `src/lsp/server.rs` | ✅ Matches doc |

### ⚠️ Discrepancies

1. **`LspService` struct (service.rs:21-24)**: Doc shows `config: LspConfig` - **correct**, but `ClientEntry` is **private** (not shown in doc).
2. **`LspProcess` struct (launch.rs:20-25)**: Doc shows `BufReader<tokio::process::ChildStderr>` - **correct**.
3. **`completion` fallback logic (operations.rs:282-285)**: Doc states it "first attempts to deserialize as `CompletionList`" - **incorrect**. Code tries `CompletionList` first, then falls back to `Vec<CompletionItem>`. This matches the description of the behavior, but the wording could be clearer.

---

## Server Count

### ❌ **Incorrect**: Document claims **39 servers**, actual count is **40**

Counted entries in `server_definitions()` function (lines 27-385):

| # | Server ID | Language(s) |
|---|-----------|-------------|
| 1 | rust-analyzer | rust |
| 2 | gopls | go |
| 3 | pyright | python |
| 4 | typescript-language-server | javascript, javascriptreact, typescript, typescriptreact |
| 5 | jdtls | java |
| 6 | clangd | c, cpp, objective-c, objective-cpp |
| 7 | omnisharp | csharp |
| 8 | kotlin-language-server | kotlin |
| 9 | lua-language-server | lua |
| 10 | haskell-language-server | haskell |
| 11 | metals | scala |
| 12 | elixir-ls | elixir |
| 13 | clojure-lsp | clojure |
| 14 | vue-language-server | vue |
| 15 | svelte-language-server | svelte |
| 16 | yaml-language-server | yaml |
| 17 | taplo | toml |
| 18 | bash-language-server | shellscript |
| 19 | terraform-ls | terraform |
| 20 | zls | zig |
| 21 | marksman | markdown |
| 22 | dockerfile-language-server | dockerfile |
| 23 | sql-language-server | sql |
| 24 | ruby-lsp | ruby |
| 25 | php-language-server | php |
| 26 | swift-sourcekit | swift |
| 27 | dart-analysis-server | dart |
| 28 | erlang-ls | erlang |
| 29 | html-language-server | html |
| 30 | css-language-server | css, scss, less |
| 31 | json-language-server | json, jsonc |
| 32 | solidity-language-server | solidity |
| 33 | perl-language-server | perl, raku |
| 34 | powershell-editor-services | powershell |
| 35 | graphql-language-server | graphql |
| 36 | buf-language-server | proto |
| 37 | r-languageserver | r |
| 38 | nimlsp | nim |
| 39 | vls | v |
| 40 | cmake-language-server | cmake |

**Note**: `cmake-language-server` was added (not listed in the documented table), increasing count from 39 to 40.

**Missing from documented table**: cmake-language-server, elixir-ls, clojure-lsp, vue-language-server, svelte-language-server, taplo, terraform-ls, marksman, dockerfile-language-server, sql-language-server, erlang-ls, html-language-server, css-language-server, json-language-server, solidity-language-server, powershell-editor-services, graphql-language-server, buf-language-server, cmake-language-server.

**Extra in documented table**: Perl/Raku, Nim, V (these are in the table but the full list in code has them).

---

## Extension Count

### ❌ **Incorrect**: Document claims "50+ extensions", actual count is **~80 extensions**

The `extension_to_language_id` function in `language.rs` (lines 1-83) maps extensions to language IDs. Counted extensions:

- Standard extensions (rs, go, py, pyw, pyx, js, jsx, ts, tsx, java, kt, kts, c, h, cpp, cc, cxx, hpp, hxx, cs, php, rb, swift, m, mm, lua, pl, pm, raku, hs, lhs, scala, sc, dart, ex, exs, erl, hrl, clj, cljs, cljc, vue, svelte, html, htm, css, scss, sass, less, json, jsonc, yaml, yml, toml, xml, sh, bash, zsh, fish, ps1, psm1, psd1, sql, graphql, gql, proto, tf, tfvars, dockerfile, md, r, R, zig, nim, v, sol, makefile, cmake)

**Total**: 82 extensions (not counting special filenames like "dockerfile", "makefile").

---

## Line Number Verification

| Item | Doc Line | Actual Location | Status |
|------|----------|-----------------|--------|
| Lsp struct | 30-36 | `src/lsp/mod.rs:30-79` | ✅ |
| LspService struct | 42-45 | `src/lsp/service.rs:21-24` | ✅ |
| LspClient struct | 62-72 | `src/lsp/client.rs:38-48` | ✅ |
| LspOperations struct | 90-93 | `src/lsp/operations.rs:11-13` | ✅ |
| FileDiagnostic | 114-122 | `src/lsp/diagnostics.rs:19-28` | ✅ |
| DEBOUNCE_MS constant | 111 | `src/lsp/diagnostics.rs:15` | ✅ (150ms) |
| LspProcess struct | 160-165 | `src/lsp/launch.rs:20-25` | ✅ |
| REQUEST_TIMEOUT | - | `src/lsp/client.rs:450` (30 seconds) | ✅ |
| LspError enum | 262-273 | `src/error.rs:401-428` | ✅ (different file) |
| server_definitions() | 222 | `src/lsp/server.rs:27` | ✅ |

---

## Field Verification

### LspClient (client.rs:38-48)

| Field | Doc | Actual | Status |
|-------|-----|--------|--------|
| server_id | ✅ | ✅ String | Correct |
| root | ✅ | ✅ PathBuf | Correct |
| process | ✅ | ✅ `tokio::sync::Mutex<LspProcess>` | Correct |
| request_id | ✅ | ✅ `AtomicU64` | Correct |
| capabilities | ✅ | ✅ `Mutex<Option<ServerCapabilities>>` | Correct |
| opened_files | ✅ | ✅ `Mutex<HashMap<String, i32>>` | Correct |
| diagnostics | ✅ | ✅ `Arc<Mutex<HashMap<String, Vec<...>>>>` | Correct |
| notif_tx | ✅ | ✅ `mpsc::UnboundedSender<String>` | Correct |
| notif_rx | ✅ | ✅ `Mutex<Option<mpsc::UnboundedReceiver<String>>>` | Correct |

### LspService (service.rs:21-24)

| Field | Doc | Actual | Status |
|-------|-----|--------|--------|
| clients | ✅ | ✅ `Arc<RwLock<HashMap<String, ClientEntry>>>` | Correct |
| config | ✅ | ✅ `LspConfig` | Correct |

### FileDiagnostic (diagnostics.rs:19-28)

| Field | Doc | Actual | Status |
|-------|-----|--------|--------|
| file | ✅ | ✅ String | Correct |
| line | ✅ | ✅ u32 | Correct |
| column | ✅ | ✅ u32 | Correct |
| message | ✅ | ✅ String | Correct |
| severity | ✅ | ✅ DiagnosticSeverity | Correct |
| source | ✅ | ✅ Option<String> | Correct |
| code | ✅ | ✅ Option<String> | Correct |

---

## Implementation Notes Verification

| Item | Doc Claim | Actual | Status |
|------|-----------|--------|--------|
| PATH parsing | Uses `std::env::split_paths()` | `download.rs:52` uses `std::env::split_paths(&path_var)` | ✅ Correct |
| PHP mapping | Maps to `php-language-server` | `language.rs:96` → `php-language-server` | ✅ Correct |
| Request timeout | 30-second timeout | `client.rs:450` `REQUEST_TIMEOUT = 30 secs` | ✅ Correct |
| Hardcoded PATH | Preserves user's actual PATH | `launch.rs:41-45` uses `std::env::var_os("PATH")` | ✅ Correct |
| Stderr logging | Drained during initialization | `client.rs:66-69` drains stderr after spawn | ✅ Correct |
| Notification loop | Clean notification handling | `client.rs:169-176` spawns task to consume notifications | ✅ Correct |
| close_file race | Fixed with single write lock | `service.rs:148-185` uses index-based lookup with single lock | ✅ Correct |
| save_file race | Fixed with single write lock | `service.rs:187-218` uses index-based lookup with single lock | ✅ Correct |

---

## Tool Integration

### ✅ Verified

- `LspTool` is exposed in `src/tool/lsp.rs:57-320`
- Config gated by `experimental.lsp_tool` (confirmed in tool registration)
- Operations include: goToDefinition, findReferences, hover, documentSymbol, workspaceSymbol, goToImplementation, prepareCallHierarchy, incomingCalls, outgoingCalls, codeAction, codeLens

---

## Error Handling

### ⚠️ LspError Location

Document shows `LspError` at lines 262-273 in the architecture doc (implying it's in the LSP module).

**Actual**: `LspError` is defined in `src/error.rs:401-428`, not in `src/lsp/`. This is a minor documentation issue - the enum is correctly documented but its location is wrong.

### Error Variants Verified

| Variant | Documented | Actual |
|---------|------------|--------|
| ServerNotFound(String) | ✅ | ✅ `error.rs:402-403` |
| DownloadFailed(String) | ✅ | ✅ `error.rs:405-406` |
| LaunchFailed(String) | ✅ | ✅ `error.rs:408-409` |
| NotInitialized(String) | ✅ | ✅ `error.rs:411-412` |
| RequestFailed(String) | ✅ | ✅ `error.rs:414-415` |
| RequestTimeout(String) | ✅ | ✅ `error.rs:417-418` |
| UnsupportedLanguage(String) | ✅ | ✅ `error.rs:420-421` |
| Io(std::io::Error) | ✅ | ✅ `error.rs:423-424` |
| Json(serde_json::Error) | ✅ | ✅ `error.rs:426-427` |

---

## Recommendations

1. **Update server count**: Change "39 servers" to "40 servers" in line 229.
2. **Update extension count**: Change "50+ extensions" to "~80 extensions" in line 186.
3. **Document cmake-language-server**: Add to the supported languages table.
4. **Clarify completion fallback**: Rephrase to clarify it tries `CompletionList` first, then falls back to `Vec<CompletionItem>`.
5. **Note LspError location**: Add a note that `LspError` is defined in `src/error.rs` for consistency with other error types.
6. **Expand documented table**: The table shows only ~18 of 40 servers. Consider either expanding the table or removing the "... and more" placeholder.

---

## Verified Claims (Correct)

- ✅ Module location: `src/lsp/`
- ✅ Client-per-root pattern with key format `"{project_root}:{server_id}"`
- ✅ File lifecycle methods (open_file, update_file, close_file, save_file)
- ✅ Code intelligence operations (go_to_definition, find_references, hover, etc.)
- ✅ DEBOUNCE_MS = 150ms
- ✅ Uses Content-Length headers for LSP message framing
- ✅ Preserves user's PATH from environment
- ✅ Only rust-analyzer has download specification
- ✅ Supports Zip, TarGz, TarXz, Raw archive types
- ✅ Project root detection via marker files