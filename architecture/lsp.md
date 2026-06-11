# LSP Module

The `lsp` module provides Language Server Protocol support for IDE-like features. It implements a **client-side LSP integration** that spawns and manages external LSP server processes.

**Location**: `src/lsp/` (Codegg-side thin re-exports) and `crates/egglsp/` (full implementation; see [native_crates.md](native_crates.md))

## Key Responsibilities

- LSP server lifecycle management (download, launch, initialize)
- Diagnostics collection via publishDiagnostics notifications
- Code operations (goto definition, find references, hover, document symbols, workspace symbols, diagnostics)
- Preview-only semantic edits (renamePreview, formatPreview, sourceActionPreview) — returns unified-diff patches, never writes files
- Temporary overlays (semanticCheckPreview) — accepts full content or a single-file unified diff patch, applies it in memory via OverlaySession, collects diagnostics/symbols, restores disk view, never writes files
- Compact semantic context packets (semanticContext) — combines source excerpt, diagnostics, symbols, optional definition/reference/overlay information into a bounded pre-edit/pre-review context packet
- Security context packets (securityContext) — security-review context packet with deterministic risk markers, security-relevant diagnostics/symbols, optional call hierarchy, and optional overlay diagnostics
- Language detection from file extensions
- Project root detection
- Shallow call/type hierarchy queries (`callHierarchy`, `typeHierarchy`) — read-only, bounded, non-recursive relationship summaries for the symbol at a target position.
- Compact agent-facing output DTOs (not raw LSP JSON)

## Architecture

The full LSP implementation lives in the `egglsp` workspace crate
(`crates/egglsp/`). Codegg-side `src/lsp/mod.rs` is a thin wrapper
that re-exports `egglsp::*` and bridges:

- `crate::config::schema::LspConfig` → `egglsp::LspConfig` (via `From` impl in the wrapper)
- `egglsp::LspError` → `crate::error::LspError` (delegates to the existing codegg-side error variant)

The crate uses a client-per-root pattern: `LspService` maintains a `HashMap<String, ClientEntry>` where the key is `"{project_root}:{server_id}"`.

## Components

### src/lsp/mod.rs - Codegg-side thin wrapper

```rust
pub struct Lsp {
    pub service: Arc<LspService>,
    pub operations: Arc<LspOperations>,
    pub diagnostics: Arc<DiagnosticsCollector>,
}

impl Lsp {
    pub async fn open_file(&self, path: &Path, content: &str) -> Result<(), LspError>
    pub async fn update_file(&self, path: &Path, content: &str) -> Result<(), LspError>
    pub async fn close_file(&self, path: &Path) -> Result<(), LspError>
    pub async fn save_file(&self, path: &Path, content: Option<&str>) -> Result<(), LspError>
    pub async fn shutdown(&self)
}
```

### service.rs - Client Management

```rust
pub struct LspService {
    clients: Arc<RwLock<HashMap<String, ClientEntry>>>,
    config: LspConfig,
}

impl LspService {
    pub async fn get_or_create_client(&self, file_path: &Path) -> Result<(String, PathBuf), LspError>
    pub async fn get_or_create_client_for_file(&self, file_path: &Path) -> Result<(String, PathBuf), LspError>
    pub async fn ensure_file_open_from_disk(&self, file_path: &Path) -> Result<(String, PathBuf), LspError>
    pub async fn find_existing_client_for_root_hint(&self, root_hint: Option<&Path>, server_id: Option<&str>) -> Result<(String, PathBuf), LspError>
    pub async fn open_file(&self, file_path: &Path, text: &str) -> Result<(), LspError>
    pub async fn update_file(&self, file_path: &Path, text: &str) -> Result<(), LspError>
    pub async fn close_file(&self, file_path: &Path) -> Result<(), LspError>
    pub async fn save_file(&self, file_path: &Path, text: Option<&str>) -> Result<(), LspError>
    pub async fn shutdown_all(&self)
}
```

### client.rs - LSP Client

Manages JSON-RPC communication with a single LSP server process. A dedicated background reader task owns stdout and routes responses via the `pending` map while independently dispatching notifications (e.g. `publishDiagnostics`):

```rust
pub struct LspClient {
    pub server_id: String,
    pub root: PathBuf,
    pub process: tokio::sync::Mutex<LspProcess>,
    pub request_id: AtomicU64,
    pub capabilities: Mutex<Option<ServerCapabilities>>,
    pub opened_files: Mutex<HashMap<String, i32>>,
    pub last_opened_at: Mutex<HashMap<String, Instant>>,
    pub diagnostics: Arc<Mutex<HashMap<String, Vec<lsp_types::Diagnostic>>>>,
    pub pending: PendingMap,
    _reader_task: tokio::task::JoinHandle<()>,
}

pub struct DiagnosticEntry { ... } // internal
```

**Key operations**:
- File lifecycle: `open_file()`, `update_file()`, `close_file()`, `save_file()`, `ensure_file_open_from_disk()`
- Code intelligence: `go_to_definition()`, `find_references()`, `hover()`, `document_symbols()`, `code_actions()`, `completion()`, `signature_help()`, `code_lens()` (internal), plus preview-only `rename_preview()` / `format_preview()` (see edit.rs)
- Diagnostics: `get_diagnostics()`, `get_all_diagnostics()`, `diagnostics_may_still_be_warming()`
- Communication: `send_request()`, `send_notification()`, `send_initialized()`
- Utilities: `url_to_uri()`, `detect_language_id()`, `classify_json_rpc_message`, `dispatch_notification`

`notif_tx`/`notif_rx` and direct `read_response`/`read_notification` paths have been removed; stdout is exclusively owned by the background reader.

### operations.rs - High-Level Operations

```rust
pub struct LspOperations {
    service: Arc<LspService>,
}

impl LspOperations {
    pub async fn go_to_definition(&self, file_path: &Path, line: u32, column: u32) -> Result<Vec<LocationLink>, LspError>
    pub async fn find_references(&self, file_path: &Path, line: u32, column: u32) -> Result<Vec<Location>, LspError>
    pub async fn hover(&self, file_path: &Path, line: u32, column: u32) -> Result<Option<String>, LspError>
    pub async fn document_symbols(&self, file_path: &Path) -> Result<Vec<DocumentSymbol>, LspError>
    pub async fn code_actions(&self, file_path: &Path, start_line: u32, start_col: u32, end_line: u32, end_col: u32, diagnostics: Vec<Diagnostic>, only: Option<Vec<CodeActionKind>>) -> Result<Vec<CodeActionOrCommand>, LspError>
    pub async fn completion(&self, file_path: &Path, line: u32, column: u32, trigger_kind: Option<CompletionTriggerKind>, trigger_char: Option<String>) -> Result<Vec<CompletionItem>, LspError>
    pub async fn signature_help(&self, file_path: &Path, line: u32, column: u32) -> Result<Option<String>, LspError>
    pub async fn code_lens(&self, file_path: &Path) -> Result<Vec<CodeLens>, LspError>  // internal, not model-facing
    pub async fn prepare_rename(&self, file_path: &Path, line: u32, column: u32) -> Result<Option<PrepareRenameResponse>, LspError>
    pub async fn rename_preview(&self, file_path: &Path, line: u32, column: u32, new_name: &str, allowed_root: Option<&Path>) -> Result<WorkspaceEditPreview, LspError>
    pub async fn format_preview(&self, file_path: &Path, allowed_root: Option<&Path>) -> Result<WorkspaceEditPreview, LspError>
    pub async fn source_action_preview(&self, file_path: &Path, action: SourceActionPreviewKind, allowed_root: Option<&Path>) -> Result<WorkspaceEditPreview, LspError>
    pub async fn semantic_check_preview(&self, file_path: &Path, content: &str, allowed_root: Option<&Path>) -> Result<SemanticCheckPreview, LspError>
}
```

**Note**: The `LspOperations::completion` method handles both LSP response types - `CompletionList` (a structured list with `isIncomplete` flag) and plain `Vec<CompletionItem>`. It first attempts to deserialize as `CompletionList`, and if that fails, falls back to parsing as a `Vec<CompletionItem>`. This fallback is handled at the operations layer; the lower-level `LspClient::completion` only handles `CompletionList`.

### diagnostics.rs - Diagnostics Collection

```rust
const DEBOUNCE_MS: u64 = 150;

#[derive(Debug, Clone)]
pub struct FileDiagnostic {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub source: Option<String>,
    pub code: Option<String>,
}

pub struct DiagnosticsCollector {
    service: Arc<LspService>,
    last_update: Arc<Mutex<HashMap<String, Instant>>>,
}

impl DiagnosticsCollector {
    pub async fn should_debounce(&self, uri: &str) -> bool
    pub async fn get_diagnostics_for_file(&self, file_path: &Path) -> Result<Vec<FileDiagnostic>, LspError>
    pub async fn get_all_diagnostics(&self) -> Result<HashMap<String, Vec<FileDiagnostic>>, LspError>
    pub async fn has_errors(&self, file_path: &Path) -> Result<bool, LspError>
}
```

### download.rs - Binary Download

```rust
pub async fn ensure_server_binary(server: &LspServerDef) -> Result<PathBuf, LspError>
pub fn cache_dir() -> PathBuf

async fn find_in_path(cmd: &str) -> Option<PathBuf>
async fn is_executable(path: &Path) -> bool
async fn download_server(server: &LspServerDef, spec: &DownloadSpec, dest: &Path) -> Result<PathBuf, LspError>
fn resolve_url(spec: &DownloadSpec) -> String
fn extract_zip(data: &[u8], dest: &Path, binary_name: &str) -> Result<PathBuf, LspError>
fn extract_tar_gz(data: &[u8], dest: &Path, binary_name: &str) -> Result<PathBuf, LspError>
fn extract_tar_xz(data: &[u8], dest: &Path, binary_name: &str) -> Result<PathBuf, LspError>
```

1. First checks PATH for binary
2. Falls back to cached download in `$HOME/.cache/codegg/lsp/`
3. Only rust-analyzer has download specification currently
4. Supports Zip, TarGz, TarXz, and Raw archive types

### launch.rs - Process Spawning

```rust
pub struct LspProcess {
    pub stdin: tokio::process::ChildStdin,
    pub stdout: tokio::process::ChildStdout,
    pub stderr: Option<BufReader<tokio::process::ChildStderr>>,
    pub child: tokio::process::Child,
}

pub async fn spawn_server(command: &str, args: &[&str], env: &[(String, String)], cwd: Option<&Path>) -> Result<LspProcess, LspError>
pub async fn send_request(process: &mut LspProcess, msg: &str) -> Result<(), LspError>
pub fn spawn_stderr_drain(server_id: &str, stderr: tokio::process::ChildStderr)
pub async fn terminate(process: &mut LspProcess)
fn parse_content_length(header: &str) -> Option<usize>
```

Uses Content-Length headers for LSP message framing. Preserves user's PATH from environment. Stderr is drained in a background task (capped at 64KB) to prevent blocking initialization. `read_response` and `read_notification` have been removed; stdout is exclusively owned by the background reader task in `client.rs`.

### language.rs - Language Detection

```rust
pub fn detect_language(path: &str) -> Option<&'static str>
pub fn extension_to_language_id(ext: &str) -> Option<&'static str>
pub fn language_id_to_server_id(lang_id: &str) -> Option<&'static str>
```

Supports ~80 extensions including Rust, Python, JavaScript/TypeScript, Go, Java, C/C++, C#, Ruby, Kotlin, Scala, Dart, Swift, Haskell, Lua, PHP, Perl/Raku, and more.

### root.rs - Project Root Detection

```rust
pub fn find_project_root(start: &Path) -> Option<PathBuf>
```

Detects project roots by looking for marker files like `.git`, `Cargo.toml`, `package.json`, etc.

### server.rs - Server Definitions

```rust
pub struct LspServerDef {
    pub id: &'static str,
    pub languages: &'static [&'static str],
    pub extensions: &'static [&'static str],
    pub repo: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub download: Option<DownloadSpec>,
}

pub struct DownloadSpec {
    pub url_template: &'static str,
    pub archive_type: ArchiveType,
    pub binary_name: &'static str,
}

pub enum ArchiveType {
    Zip,
    TarGz,
    TarXz,
    Raw,
}

pub fn server_definitions() -> &'static [LspServerDef]
pub fn find_server(id: &str) -> Option<&'static LspServerDef>
pub fn find_server_for_language(lang: &str) -> Option<&'static LspServerDef>
pub fn find_server_for_extension(ext: &str) -> Option<&'static LspServerDef>

```

## Supported Languages (40 servers)

| Language | Server | Command |
|----------|--------|---------|
| Rust | rust-analyzer | rust-analyzer |
| Python | pyright | pyright-langserver --stdio |
| JavaScript/TypeScript | typescript-language-server | typescript-language-server --stdio |
| Go | gopls | gopls |
| C/C++ | clangd | clangd |
| Java | jdtls | jdtls |
| C# | omnisharp | OmniSharp |
| Ruby | ruby-lsp | ruby-lsp |
| Kotlin | kotlin-language-server | kotlin-language-server |
| Scala | metals | metals |
| Dart | dart-analysis-server | dart language-server --client-id codegg |
| Swift | swift-sourcekit | sourcekit-lsp |
| Haskell | haskell-language-server | haskell-language-server-wrapper --lsp |
| Lua | lua-language-server | lua-language-server |
| PHP | php-language-server | php-language-server |
| Perl/Raku | perl-language-server | perl-language-server |
| Zig | zls | zls |
| V | vls | vls |
| Nim | nimlsp | nimlsp |
| R | r-languageserver | R --slave -e library(languageserver) |
| ... and more | | |

## Tool Integration

LSP is exposed via `LspTool` in `src/tool/lsp.rs`. The tool returns compact agent-facing summaries, not raw LSP JSON.

### Exposed Operations

Only these operations are model-facing:

| Operation | LSP Request | Output Shape |
|-----------|-------------|--------------|
| `goToDefinition` | `textDocument/definition` | `Vec<LocationSummary>` |
| `findReferences` | `textDocument/references` | `Vec<LocationSummary>` (capped at 100) |
| `hover` | `textDocument/hover` | `HoverSummary` (capped at 2000 chars) |
| `documentSymbol` | `textDocument/documentSymbol` | `Vec<SymbolSummary>` (capped at 300) |
| `workspaceSymbol` | `workspace/symbol` | Compact summary list |
| `diagnostics` | (via DiagnosticsCollector) | `Vec<DiagnosticSummary>` (plus warming flag) |
| `renamePreview` | `textDocument/rename` (after ensure open + optional prepareRename) | `WorkspaceEditPreview` (unified diff patches + metadata; preview-only) |
| `formatPreview` | `textDocument/formatting` | `WorkspaceEditPreview` (unified diff patches; preview-only) |
| `sourceActionPreview` | `textDocument/codeAction` (filtered to `source.organizeImports`; full-document range computed from synced file contents) | `WorkspaceEditPreview` (unified diff patches; preview-only) |
| `semanticCheckPreview` | `textDocument/didChange` (OverlaySession + restore) + `textDocument/documentSymbol` | `SemanticCheckPreview` (diagnostics + symbols + error fields; accepts `content` or single-file `patch`, preview-only, no disk writes) |
| `semanticContext` | (combines multiple LSP requests) | `SemanticContextPacket` (source excerpt + diagnostics + symbols + optional definitions/references/overlay + optional source-action hints + optional call/type hierarchy; read-only, never writes files) |
| `securityContext` | (combines multiple LSP requests + risk marker scanning) | `SecurityContextPacket` (source excerpt + risk markers + security-relevant diagnostics/symbols + optional definitions/references/call hierarchy + optional overlay; read-only, never writes files) |
| `callHierarchy` | `textDocument/prepareCallHierarchy` + `callHierarchy/incomingCalls` + `callHierarchy/outgoingCalls` | `CallHierarchySummary` (items, incoming, outgoing, errors, truncated) |
| `typeHierarchy` | `textDocument/prepareTypeHierarchy` + `typeHierarchy/supertypes` + `typeHierarchy/subtypes` | `TypeHierarchySummary` (items, supertypes, subtypes, errors, truncated) |

`codeLens` is intentionally not exposed in the model-facing schema (remains available in `egglsp::operations` only).

**LSP edit previews are strictly read-only**: `renamePreview`/`formatPreview` (and any future preview ops) return bounded unified-diff patches via `WorkspaceEditPreview` (title, per-file original_hash + TextEditPreview + patch). They never write files. Actual mutation requires the separate mutating `apply_patch` tool (or equivalent). The `lsp` tool remains `ToolCategory::ReadOnly`.

### Preview-only edits

`renamePreview`, `formatPreview`, and `sourceActionPreview` request semantic edits from the language server, convert them into `WorkspaceEditPreview`, and return unified diff patches. They never write files. `sourceActionPreview` currently supports only `source.organizeImports` (with aliases `organizeImports` and `organize_imports`); arbitrary code actions and command execution are intentionally rejected. `CodeAction` values with `command: Some(_)` but `edit: None` are classified as command-only and rejected (command execution is disabled for safety). `format_preview` enforces `allowed_root` at the crate layer — paths outside the root are rejected with `LspError::PathOutsideRoot`. Large patches are structurally marked via `FileEditPreview.patch_omitted` (not by string matching). Applying a preview requires the existing mutating `apply_patch` tool and therefore follows normal Codegg permission handling. `semanticContext` can also include source-action hints (currently limited to `source.organizeImports`) when `include_source_actions` is true, reusing the same preview-only semantics described above.

Hidden operations (in `egglsp::operations` for internal use only, not model-facing): `completion`, `signatureHelp`, `codeAction` (arbitrary code actions), `codeLens`, and `goToImplementation`. The `source.organizeImports` source action is the only source action exposed to the model via `sourceActionPreview`.

### Temporary overlays

`semanticCheckPreview` accepts either full proposed `content` or a single-file unified diff `patch`. The patch is applied in memory against `file_path` via `OverlaySession`, gathers diagnostics/symbols, then restores the LSP view back to the current disk content. This allows pre-apply semantic checks without writing files. Multi-file patches are unsupported in this pass. The operation is read-only from Codegg's filesystem permission perspective.

`OverlaySession::apply_overlay(file_path, proposed_text)` reads disk content, opens the file in LSP, sends `didChange` with the proposed content, and returns an `OverlayRestoreToken` capturing the original text, file path, key, and URI. `OverlaySession::restore(token)` sends `didChange` restoring the original disk content. The proposed content is never written to disk; patch input follows the same in-memory path after being expanded against `file_path`.

The overlay flow:
1. `OverlaySession::apply_overlay` reads disk content and sends `didChange` with proposed content
2. Wait 250ms for diagnostics debounce
3. Collect diagnostics and document symbols
4. `OverlaySession::restore` sends `didChange` restoring disk content
5. Return results (diagnostics, symbols, `restored_disk_view` flag, error fields)

Operation-level root enforcement: `semantic_check_preview` accepts `allowed_root: Option<&Path>` and rejects files outside the root with `LspError::PathOutsideRoot`.

Restore runs even if diagnostics or symbol collection fails. Restore failures are logged and surfaced via `restore_error: Option<String>` in the response (and `restored_disk_view: false`). `SemanticCheckPreview` also includes `diagnostics_error: Option<String>` and `symbols_error: Option<String>` — each is non-None when the corresponding LSP request fails, replacing previously swallowed empty-vector fallbacks. `diagnostics_may_still_be_warming` indicates the LSP server may not have fully processed the overlay yet. Diagnostics may be warming or stale (publishDiagnostics is async). The operation is single-file in the first pass; multi-file overlays are unsupported in this pass.

### Hierarchy operations

`callHierarchy` and `typeHierarchy` are read-only code-intelligence operations. They require `file_path`, `line`, and `column`. Both operations default to `direction="both"`.

`callHierarchy` maps:
- `incoming` → callers of the target symbol
- `outgoing` → calls made by the target symbol

`typeHierarchy` maps:
- `incoming` → supertypes
- `outgoing` → subtypes

The first implementation is shallow and non-recursive. It prepares the target hierarchy item and requests immediate relationships only. Unsupported language servers may return empty sections or per-section error fields.

Hierarchy `from_ranges` truncation (capped at `MAX_HIERARCHY_RANGES = 32` per call) is now included in the summary `truncated` flag. The `truncated` field is `true` when items, edges, or ranges exceed their caps.

Hierarchy prepare operations use `ensure_file_open_from_disk` to open/sync the file from disk before sending the prepare request, ensuring position-sensitive behavior against a document view known to the server.

### Semantic context packets

`semanticContext` is the preferred agent-facing pre-edit/pre-review context operation. It combines a bounded source excerpt with current diagnostics, document symbols, optional definition/reference information, optional overlay diagnostics for proposed content or a single-file patch, optional source-action hints, and optional call/type hierarchy information. It is read-only and never applies changes.

Input parameters:
- `file_path` (required): file to analyze
- `line`, `column` (optional, both-or-neither): 1-indexed target position for definitions/references and hierarchy
- `radius` (optional, default 40, max 120): lines above/below target for source excerpt
- `include_references` (optional, default true when line+column): include findReferences results
- `include_definitions` (optional, default true when line+column): include goToDefinition results
- `include_overlay` (optional, default true when content/patch provided): include overlay diagnostics
- `include_source_actions` (optional, default false): include source-action hints (e.g. `source.organizeImports`) in the packet; each hint is a `SemanticSourceActionHint` with `action`, `available`, `preview` (optional `WorkspaceEditPreview`), and `error` (optional); failures are per-hint and do not fail the whole packet
- `include_call_hierarchy` (optional, default false): include call hierarchy information (requires line+column); requests without a target position are rejected with a validation error
- `include_type_hierarchy` (optional, default false): include type hierarchy information (requires line+column); requests without a target position are rejected with a validation error
- `content` / `patch` (optional, mutually exclusive): proposed content for overlay diagnostics

All output sections are bounded:
- Diagnostics: capped at 100
- Symbols: capped at 120
- References: capped at 80
- Overlay diagnostics: capped at 100 (included in `overlay_diagnostics_truncated` limit)
- Source excerpt: capped at 32KB text

The operation gathers existing read-only semantic facts, optionally runs an overlay semantic check, and returns a stable JSON DTO. All sections are best-effort: individual failures do not prevent the rest of the packet from being returned. Per-section errors are surfaced as `definitions_error: Option<String>` and `references_error: Option<String>` (non-None when the corresponding LSP request fails). `result_count` includes overlay diagnostics and overlay symbols in addition to the base counts. Source excerpt truncation is UTF-8-safe — it cuts at character boundaries using `truncate_to_byte_limit_on_char_boundary`, avoiding replacement characters or partial-codepoint corruption. `execute_structured` checks both `/results/restore_error` and `/results/overlay/restore_error` for success detection.

### Security context packets

`securityContext` is a read-only context-gathering operation for security review. It is not a vulnerability scanner and does not produce vulnerability verdicts. It never writes proposed content to disk; patch/content input is applied only in memory through the existing semantic overlay path.

It combines:
- bounded source excerpt (configurable radius, default 80, max 200);
- deterministic risk markers via pattern matching (11 categories);
- security-relevant diagnostics and symbols (filtered by keyword matching and proximity to risk markers);
- definitions and references when a target position is supplied;
- shallow call hierarchy when a target position is supplied;
- optional overlay diagnostics for proposed full content or a single-file patch.

**Supported risk marker categories:** `auth`, `crypto`, `filesystem`, `network`, `process`, `unsafe`, `serialization`, `sql`, `secrets`, `path_traversal`, `concurrency`

**Limits:**

| Section | Default | Max |
|---------|---------|-----|
| risk markers | 80 | 200 |
| excerpt radius | 80 lines | 200 lines |
| security diagnostics | 80 | 80 |
| security symbols | 80 | 80 |
| references | 80 | 80 |

**Input parameters:**

| Parameter | Type | Default | Notes |
|-----------|------|---------|-------|
| `file_path` | string | required | Target file |
| `line` | number | optional | 1-indexed line; both line and column required together |
| `column` | number | optional | 1-indexed column |
| `radius` | number | 80 | Excerpt radius (max 200) |
| `content` | string | optional | Proposed file content for overlay (mutually exclusive with patch) |
| `patch` | string | optional | Single-file unified diff for overlay (mutually exclusive with content) |
| `security_categories` | string[] | all | Filter risk marker categories |
| `max_risk_markers` | number | 80 | Max risk markers (max 200) |
| `include_call_hierarchy` | bool | true when position | Include call hierarchy when line+column provided |

**Risk marker categories:** `auth`, `crypto`, `filesystem`, `network`, `process`, `unsafe`, `serialization`, `sql`, `secrets`, `path_traversal`, `concurrency`

**Output shape:**

- `risk_markers` — deterministic pattern-matched markers with category, label, line, column, matched_text, rationale
- `security_relevant_symbols` — symbols filtered for security relevance (keyword matching + proximity to risk markers)
- `security_relevant_diagnostics` — diagnostics filtered for severity (error/warning) and proximity to risk markers
- `definitions` / `references` — when line+column provided
- `call_hierarchy` — when include_call_hierarchy=true and line+column provided
- `overlay` — when content or patch provided
- `notes` — human-readable context notes including unavailable section errors
- `limits` — truncation flags per section (precise: flags reflect filtered counts, not raw counts)

**Read-only contract:** `securityContext` never writes files. Patch-based overlay is applied in memory only and restored after diagnostics collection.

**Error visibility:** Nonfatal LSP subrequest failures (diagnostics, document symbols, definitions, references) are surfaced in the `notes` array rather than failing the whole packet. This allows partial results when individual LSP operations fail.

**Implementation:** Risk marker scanning, pattern tables, and security-relevant filtering helpers live in `src/tool/lsp_security.rs`. The scanner collects all markers then caps, ensuring precise truncation flags. Diagnostics and symbols are filtered for security relevance before capping, so relevant items after many irrelevant ones are not dropped.

### Position Convention

Model-facing line and column are **1-indexed**. The wrapper converts to LSP 0-indexed via `to_lsp_position()`. Missing required fields return clear `ToolError::Execution` messages.

### Compact DTOs

All output is wrapped in `LspToolOutput<T>` with `operation`, `file_path`, `result_count`, `truncated`, and `results` fields. Individual results use `LocationSummary`, `DiagnosticSummary`, `SymbolSummary`, or `HoverSummary` with 1-indexed positions and file paths (not URIs). Additionally, `SemanticContextPacket` wraps a bounded source excerpt (`SourceExcerpt` with `start_line`, `end_line`, `text`), diagnostics, symbols, definitions, references, optional per-section error fields (`definitions_error`, `references_error`), optional `source_actions` array of `SemanticSourceActionHint` objects (`action`, `available`, `preview`, `error`), and a `SemanticContextLimits` struct tracking truncation (including `overlay_diagnostics_truncated`).

### Diagnostics

The `diagnostics` operation is first-class. It reads from the shared diagnostics cache populated by `publishDiagnostics` notifications. Diagnostics use 1-indexed line/column in output. If no diagnostics have arrived yet, an empty list is returned.

### Backend config (MCP fallback semantics)

The `lsp` tool's registration is decided by `[tool_backends.lsp]` in
the loaded `Config`. The matrix is applied by `ToolRegistry::with_options`
and mirrored exactly by `ToolRegistry::backend_report(...)`:

| `[tool_backends.lsp]` setting | Registered tool | `backend_report` status |
|-------------------------------|-----------------|-------------------------|
| `backend = "native"` (default) or `"builtin"` | real `LspTool` wrapper around `egglsp::LspService` | `ready` |
| `backend = "mcp", fallback_to_native = true` (default for `mcp`) | real `LspTool` wrapper (the live path is the native crate, not an MCP server) | `fallback-native` |
| `backend = "mcp", fallback_to_native = false` | hidden `DisabledTool` stub — model never sees `lsp` | `unavailable` (`ConfiguredButUnavailable`) regardless of MCP server connectivity |
| `backend = "disabled"` | hidden `DisabledTool` stub — model never sees `lsp` | `disabled` |

The `DisabledTool` stub is registered (callable for diagnostics) but
filters itself out of the model-facing catalog via
`Tool::expose_in_definitions() == false`. Production session
construction uses `ToolRegistry::with_session_config_defaults(&config,
...)` so the resolved config is preserved; the legacy
`with_session_defaults(...)` is documented as a footgun for
config-aware paths.

## Error Handling

Overlay-specific behavior: `semanticCheckPreview` restore failures are logged and surfaced via `restore_error: Option<String>` in the response (alongside `restored_disk_view: false`) rather than returning `LspError`. `diagnostics_error` and `symbols_error` are similarly non-None when their respective LSP requests fail, rather than silently returning empty vectors. The original disk content is never written by this operation, so a restore failure leaves the LSP in-memory state stale but the filesystem untouched. The wrapper's `execute_structured` sets `success=false` when `restore_error` is present.

```rust
pub enum LspError {
    ServerNotFound(String),
    DownloadFailed(String),
    LaunchFailed(String),
    NotInitialized(String),
    RequestFailed(String),
    RequestTimeout(String),
    UnsupportedLanguage(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    UnsupportedEdit(String),
    PathOutsideRoot(String),
    Utf16Position(String),
    OverlappingEdits,
    UnsupportedSourceAction(String),
    CommandOnlySourceAction(String),
    NoEditForSourceAction(String),
    AmbiguousSourceAction(String, String),
}
```

`HierarchyDirection` parsing is available via `HierarchyDirection::parse(direction)` — accepts `"incoming"`, `"outgoing"`, `"both"`, or omitted (defaults to `"both"`). Invalid values return an error.

## Implementation Notes

- **PATH parsing**: Uses `std::env::split_paths()` for correct cross-platform PATH handling
- **PHP mapping**: Correctly maps to `php-language-server`
- **Request timeout**: 30-second timeout in `send_request()` with `LspError::RequestTimeout`
- **Hardcoded PATH**: Preserves user's actual PATH from environment
- **Stderr handling**: Background task drains stderr (capped at 64KB) to prevent blocking initialization
- **Notification handling**: Notifications received during request/response matching are parsed through `parse_publish_diagnostics` and update the shared diagnostics cache
- **Diagnostics parser**: `parse_publish_diagnostics` is a pure function that parses `publishDiagnostics` JSON-RPC notifications, testable without a real LSP server
- **Compact output**: Model-facing output uses DTOs (`LocationSummary`, `DiagnosticSummary`, etc.) with 1-indexed positions, not raw LSP JSON
- **Position conversion**: `to_lsp_position()` converts 1-indexed model input to 0-indexed LSP positions exactly once at the wrapper boundary
- **Client routing**: `workspaceSymbol` resolves client via `get_or_create_client_for_file` or `get_or_create_client_for_root_hint`, not arbitrary first-key selection
- **Doctor subsystem**: `codegg doctor --subsystem lsp` provides non-mutating LSP diagnostics

## See Also

- [.opencode/skills/lsp/SKILL.md](../.opencode/skills/lsp/SKILL.md) - LSP skill guide
- [tool.md](tool.md) - LSP tool wrapper
