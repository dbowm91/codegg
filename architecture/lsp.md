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
    initializing: Arc<Mutex<HashMap<String, InitSlot>>>,
    active_init_tasks: Arc<Mutex<HashMap<u64, InitTaskControl>>>,
    document_owners: Arc<RwLock<HashMap<String, String>>>,
    lifecycle: Arc<RwLock<LifecycleState>>,
    lifecycle_tx: watch::Sender<LifecycleState>,
    config: LspConfig,
}
```

`InitTaskControl` holds the authoritative terminal completion primitive for each spawned initialization task:

- `attempt_id: u64` — unique per-attempt monotonic counter
- `cancellation: CancellationToken` — cooperative cancellation signal
- `abort_handle: tokio::task::AbortHandle` — forced-abort primitive for stragglers
- `completion: oneshot::Receiver<InitTaskExit>` — **authoritative** terminal signal owned by the wrapper task

The completion receiver is the only authoritative source of truth for "the wrapper task has terminated". The wrapper task owns the paired `Sender` and is required to either send exactly one `InitTaskExit` (`Completed`, `Panicked(String)`, or `Cancelled`) before exiting, or be dropped (which closes the channel and resolves the receiver with `Err`). Shutdown never wraps the real `JoinHandle` in a forwarding task — the receiver is the completion primitive.

Lock ordering: the clients map lock must be acquired before any client-level lock.
Documented on the struct for future contributors.

```rust
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

**`save_file` freshness tracking**: When `save_file` is called with text content (`text: Some(...)`), it updates the `last_content_change_at` timestamp for the file, marking diagnostics as potentially stale since the server may recompute diagnostics for the new content. A bare save (`text: None`) sends the `didSave` notification without affecting freshness.

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

### SemanticContextCollector

**Location:** `src/lsp/semantic_context.rs`

A collector/builder that assembles the shared semantic read model for `semanticContext`. It produces `egglsp::semantic_context::SemanticContextResponse` by collecting the shared evidence needed for source excerpt, diagnostics, symbols, definitions, references, and hierarchy summaries from LSP services. Source-action hints and overlay translation are not part of the collector — they remain handler-local.

```rust
pub struct SemanticContextCollector {
    service: Arc<LspService>,
    operations: Arc<LspOperations>,
    diagnostics: Arc<DiagnosticsCollector>,
    allowed_root: PathBuf,
}

impl SemanticContextCollector {
    pub fn new(service, operations, diagnostics, allowed_root) -> Self;
    pub async fn collect(&self, request: SemanticContextRequest)
        -> Result<SemanticContextResponse, String>;
}
```

The collector handles:
- Source excerpt construction (file reading + byte-limited truncation)
- Diagnostic snapshot collection with freshness metadata
- Document symbol flattening and capping
- Definition/reference gathering with capability gating
- Source-action preview hints
- Call/type hierarchy summaries (capability-gated)
- Per-section truncation metadata
- Structured unavailable metadata via `LspCapabilitySnapshot`

Overlay resolution stays handler-local because patch/content expansion is tool-specific; the shared semantic read model carries the resulting overlay summary when the handler chooses to attach one.

Unit tests use fake/static inputs and do not require live LSP servers. Hierarchy flag wiring tests (`semantic_context_request_sets_call_hierarchy_flag`, etc.) are unit-level: they verify request construction and `SemanticContextPacket::from_semantic_response` adapter behavior with static `SemanticContextResponse` fixtures. Root composite tests in `tests/lsp_composite_stdio.rs` exercise the real `SemanticContextCollector` against a fake LSP server end-to-end, covering the full workflow, capability gating, and failure degradation paths. Production preview conversion (rename, format, source-action) is tested through the same composite harness, confirming that `WorkspaceEditPreview` and `FileEditPreview` round-trip correctly through the production `LspClient`/`LspOperations`/`LspService` stack.

## Supported Languages (39 servers)

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
| `hunkSourceContext` | (combines diff parsing + semantic context) | `HunkSourceNavigationResponse` (per-hunk evidence with enclosing symbols, diagnostics, definitions, references; read-only, bounded) |
| `callHierarchy` | `textDocument/prepareCallHierarchy` + `callHierarchy/incomingCalls` + `callHierarchy/outgoingCalls` | `CallHierarchySummary` (items, incoming, outgoing, errors, truncated) |
| `typeHierarchy` | `textDocument/prepareTypeHierarchy` + `typeHierarchy/supertypes` + `typeHierarchy/subtypes` | `TypeHierarchySummary` (items, supertypes, subtypes, errors, truncated) |

`codeLens` is intentionally not exposed in the model-facing schema (remains available in `egglsp::operations` only).

**LSP edit previews are strictly read-only**: `renamePreview`/`formatPreview` (and any future preview ops) return bounded unified-diff patches via `WorkspaceEditPreview` (title, per-file original_hash + TextEditPreview + patch). They never write files. Actual mutation requires the separate mutating `apply_patch` tool (or equivalent). The `lsp` tool remains `ToolCategory::ReadOnly`.

### Preview-only edits

`renamePreview`, `formatPreview`, and `sourceActionPreview` request semantic edits from the language server, convert them into `WorkspaceEditPreview`, and return unified diff patches. They never write files. `sourceActionPreview` currently supports only `source.organizeImports` (with aliases `organizeImports` and `organize_imports`); arbitrary code actions and command execution are intentionally rejected. `CodeAction` values with `command: Some(_)` but `edit: None` are classified as command-only and rejected (command execution is disabled for safety). `format_preview` enforces `allowed_root` at the crate layer — paths outside the root are rejected with `LspError::PathOutsideRoot`. Large patches are structurally marked via `FileEditPreview.patch_omitted` (not by string matching). Applying a preview requires the existing mutating `apply_patch` tool and therefore follows normal Codegg permission handling. `semanticContext` can also include source-action hints (currently limited to `source.organizeImports`) when `include_source_actions` is true, reusing the same preview-only semantics described above. Source-action hints are collected handler-locally by `LspTool::collect_source_action_hints`, not by the shared `SemanticContextCollector`, because they produce `WorkspaceEditPreview` payloads that are preview-rich and tool-specific.

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

`semanticContext` can include hierarchy sections with `include_call_hierarchy=true` or `include_type_hierarchy=true`. These flags require `line` and `column`; requests without a target position are rejected.

`securityContext` requests shared call hierarchy from `SemanticContextCollector` when `include_call_hierarchy` is enabled and a target position is supplied; type hierarchy is not currently part of security context. Both `semanticContext` and `securityContext` gate hierarchy calls through `LspCapabilitySnapshot`; unsupported operations are skipped and surfaced as notes or empty sections rather than failing the packet.

### Semantic context packets

`semanticContext` is the preferred agent-facing pre-edit/pre-review context operation. It combines a bounded source excerpt with current diagnostics, document symbols, optional definition/reference information, optional overlay diagnostics for proposed content or a single-file patch, optional source-action hints, and optional call/type hierarchy information. It is read-only and never applies changes.

The shared semantic read model is assembled by `SemanticContextCollector`. Overlay translation remains handler-local by design: patch/content expansion is tool-specific (the handler resolves the overlay via `semanticCheckPreview` and attaches the resulting summary), so the collector never handles overlay requests or responses. `securityContext` reuses the same diagnostic freshness evidence and capability snapshot, but filters results into a security-specific packet instead of a general semantic summary.

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

> **Architecture note:** `SemanticContextPacket` is a tool-local presentation type. `SemanticContextCollector` assembles the shared semantic read model, and `SemanticContextPacket::from_semantic_response()` adapts that response into the tool-local packet. Overlay resolution stays handler-local.

### Security context packets

`securityContext` is a read-only context-gathering operation for security review. It is not a vulnerability scanner and does not produce vulnerability verdicts. It never writes proposed content to disk; patch/content input is applied only in memory through the existing semantic overlay path.

Risk markers are deterministic keyword/identifier/context matches with rationale strings. They are prompts for review, not evidence of a confirmed vulnerability.

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
| `security_preset` | string | none | Optional preset: rust_server, rust_cli, web_backend, dependency_review, unsafe_review |

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

`securityContext` reuses the same freshness metadata and capability gating used by the semantic-context path. When diagnostics are stale or unavailable, the packet keeps that evidence visible in notes and metadata instead of turning the gap into a clean bill of health.

**Error visibility:** Nonfatal LSP subrequest failures (diagnostics, document symbols, definitions, references) are surfaced in the `notes` array rather than failing the whole packet. This allows partial results when individual LSP operations fail.

**Implementation:** Risk marker scanning, pattern tables, and security-relevant filtering helpers live in `src/tool/lsp_security.rs`. The scanner collects all markers then caps, ensuring precise truncation flags. Diagnostics and symbols are filtered for security relevance before capping, so relevant items after many irrelevant ones are not dropped.

### Security call expansion

`securityContext` can optionally include a bounded call expansion with `call_depth`. This is separate from the shared compact call hierarchy collected by `SemanticContextCollector`: the shared hierarchy provides only immediate incoming/outgoing relationships, while call expansion performs its own recursive BFS expansion handler-locally via `build_call_expansion_summary`. The default is `0`, which disables recursive expansion. Supported depths are `1` and `2`; higher depths are rejected with a clear error. Expansion is breadth-first, dedupes repeated nodes, preserves edges to already-seen nodes, and is capped by `max_call_nodes` (default 32, max 64) and internal edge/range limits (`MAX_CALL_EDGES = 128`, `MAX_HIERARCHY_RANGES = 32`). When caps are reached, expansion prefers returning a partial graph with `truncated=true` rather than failing the entire packet. `call_expansion.truncated` is true when nodes, edges, or per-edge ranges are dropped due to configured or internal caps.

This is not whole-program analysis. It is a shallow LSP-backed neighborhood around the target symbol for review triage.

**Input fields:**

| Field | Type | Default | Max | Description |
|-------|------|---------|-----|-------------|
| `call_depth` | number | 0 (off) | 2 | Call expansion depth. Requires `line`+`column`. |
| `max_call_nodes` | number | 32 | 64 | Maximum nodes in expansion graph. |
| `call_direction` | string | "both" | — | `"incoming"` (callers), `"outgoing"` (callees), or `"both"`. |

**Validation rules:**

- `call_depth > 2` → `ToolError::Execution` (rejected, not clamped)
- `call_depth > 0` without `line`+`column` → `ToolError::Execution`
- `max_call_nodes > 64` → clamped to 64
- Invalid `call_direction` → `ToolError::Execution`

**Read-only boundary:** Call expansion only sends LSP hierarchy requests (`prepareCallHierarchy`, `callHierarchy/incomingCalls`, `callHierarchy/outgoingCalls`). It never writes files or executes code.

**Error handling:** Expansion errors are nonfatal and collected in `call_expansion.errors`. A failure in one child request does not abort the entire expansion. The packet still returns risk markers, diagnostics, and other context even if expansion fails.

**Presets:** No preset enables call expansion by default. All presets keep `call_depth = 0`. Expansion is only activated through explicit `call_depth > 0`.

### SecurityContextPacket fields

| Field | Type | Description |
|-------|------|-------------|
| `file` | string | File path |
| `target` | object/null | Target position (line, column) |
| `excerpt` | object | Source excerpt |
| `risk_markers` | array | Security risk markers |
| `security_relevant_symbols` | array | Security-relevant symbols |
| `security_relevant_diagnostics` | array | Security-relevant diagnostics |
| `definitions` | array | Go-to-definition results |
| `references` | array | Find-references results |
| `call_hierarchy` | object/null | Shallow call hierarchy summary |
| `call_expansion` | object/null | Bounded recursive call expansion (when `call_depth > 0`) |
| `overlay` | object/null | Overlay diagnostics (when content/patch provided) |
| `preset` | string/null | Applied security preset name |
| `notes` | array | Informational notes |
| `limits` | object | Truncation flags |

### Security context presets

`securityContext` supports optional presets through `security_preset`. Presets tune default risk categories, excerpt radius, marker count, and call-hierarchy inclusion. Explicit input fields override preset defaults.

| Preset | Use case | Categories | Radius | Max markers | Call hierarchy |
|--------|----------|------------|--------|-------------|----------------|
| `rust_server` | Rust services/APIs/daemons | auth, network, serialization, filesystem, process, secrets, sql, path_traversal, crypto, unsafe, concurrency | 120 | 120 | true when positioned |
| `rust_cli` | CLI/local automation | process, filesystem, secrets, path_traversal, serialization, crypto, unsafe, concurrency | 100 | 100 | true when positioned |
| `web_backend` | Web handlers/auth/database | auth, network, serialization, sql, secrets, filesystem, path_traversal, crypto | 120 | 120 | true when positioned |
| `dependency_review` | manifests/build/dependency-sensitive files | secrets, filesystem, process, network, serialization, crypto | 80 | 80 | false by default |
| `unsafe_review` | unsafe/FFI/concurrency review | unsafe, concurrency, filesystem, process | 160 | 120 | true when positioned |

Preset defaults are retrieval defaults, not vulnerability policies. They do not change the read-only contract or add external scanners. Explicit user inputs (`security_categories`, `radius`, `max_risk_markers`, `include_call_hierarchy`) always override preset defaults.

### Security review workflow

The security agent uses `securityContext` as evidence-gathering input for defensive code review. It follows this loop:

1. **Target discovery** — Uses `egggit` diff APIs to identify changed files and hunks. Binary/deleted files are skipped. Generated/vendor paths (`target/`, `node_modules/`, etc.) are excluded. Async discovery reuses `build_security_review_targets` and `build_file_level_security_review_target` for consistent positioned targets (`column: Some(1)`).
2. **Preset selection** — Each file is classified into a `securityContext` preset (`rust_server`, `rust_cli`, `web_backend`, `dependency_review`, `unsafe_review`) based on path heuristics and optional content hints.
3. **Preflight checks** — Filename-hint scans (`secret_filename_hint_scan`, `unsafe_filename_hint_scan`) run on target file names (not contents).
4. **Context gathering** — `securityContext` is requested around changed hunks with bounded settings. Call expansion is opt-in (depth 0 by default, escalated to 1 only for high-risk targets via `choose_security_context_escalation`).
5. **Prompt synthesis** — Risk markers always become *review prompts*, never findings. Planned target prompts use `source: changed_hunk` evidence; risk-marker prompts use `source: securityContext.risk_marker` evidence.
6. **Evidence-based synthesis** — `synthesize_evidence_based_findings()` groups evidence by file/line bucket, applies the eligibility gate (2+ dimensions required), and emits findings for eligible groups. Marker-only evidence never creates findings. Findings are heuristic defensive review outputs, not proof of exploitability.
7. **Output** — Review prompts, findings, and parsed hunk refs (`SecurityReviewOutput.hunks`) are returned. The hunk refs carry line-level diff detail for TUI display. The `/security-review` command and `run_security_review_workflow` orchestrator produce all three.

Key types live in `src/security/workflow/` (split into submodules: `mod.rs`, `types.rs`, `diff.rs`, `preflight.rs`, `evidence.rs`, `context.rs`, `report.rs`, `enrichment.rs`). The workflow is read-only and never mutates files.

#### Orchestrator

`run_security_review_workflow(root, base, options)` is an async entry point that runs the full pipeline (discover targets → build prompts → preflight checks → evidence-based synthesis → assemble output). It does NOT execute `securityContext` LSP requests — those are deferred to a subsequent phase. Content preflight uses `root.join(p)` for repo-root-relative reads, so it works correctly when launched from any working directory. `SecurityReviewWorkflowOptions` controls which stages run and caps output counts.

#### LSP enrichment (optional)

`run_security_review_workflow_with_lsp_enrichment(root, base, options, executor)` extends the deterministic stage-1 review with an optional LSP enrichment pass. When `enable_lsp_enrichment` is true, it:

1. Runs deterministic stage-1 review.
2. Calls `run_security_context_enrichment()` which filters escalation plans to non-None levels, caps requests at `max_lsp_requests`, and executes each via a `SecurityContextExecutor` trait with per-request timeout (`lsp_request_timeout_ms`).
3. Converts responses to enriched prompts via `prompts_from_security_context()` and structured evidence via `evidence_from_security_context()` (extracting risk markers, diagnostics, call graph summaries, truncation notices).
4. Reruns synthesis via `synthesize_evidence_based_findings_with_extra_evidence()` with enriched CallPath/Diagnostic/TruncationNotice evidence injected into eligible findings.

Failures, timeouts, and truncation are recorded as notes — they never fail the whole review. Dedicated note helpers handle enrichment status: `note_lsp_enrichment_unavailable` (enrichment requested but no executor available), `note_lsp_enrichment_no_eligible_targets` (no targets met escalation policy), and `note_lsp_enrichment_executed` (reports executed request count). The `SecurityContextExecutor` trait enables mockable testing via `NoopSecurityContextExecutor` (always errors) and `FixtureSecurityContextExecutor` (pre-configured responses). A real adapter `LspSecurityContextExecutor` (in `src/security/lsp_executor.rs`) wraps `LspTool` to delegate `securityContext` operations. It validates requests via `validate_security_context_request()`, injects the operation field, and parses the JSON string response. The `SecurityContextExecutorProvider` trait and `run_security_review_command_with_executor()` enable executor injection at the command level; `run_security_review_command()` delegates to the executor-aware runner with `None`. In local mode the TUI creates a shared `LspTool` at startup (`App.lsp_tool`) and passes a `LspSecurityContextExecutor` to the command handler for `--enrich`. In socket/remote mode `lsp_tool` is `None` and `--enrich` falls back to deterministic stage-1 with an unavailable note.

The TUI dispatches `/security-review` asynchronously so the render thread is never blocked. The handler spawns a tokio task and publishes a `TuiCommand::SecurityReviewRun { id, root, args, lsp_tool }` variant (carrying a `SecurityReviewRunId` newtype and a cloned `Arc<LspTool>`) which is consumed in the `cmd_rx` arm of `run_event_loop` in `src/tui/mod.rs` by a new `async fn handle_security_review_run(...)`. That handler invokes the new `pub async fn run_security_review_background(root: PathBuf, args: SecurityReviewCommandArgs, lsp_tool: Option<Arc<LspTool>>) -> Result<SecurityReviewReceipt, String>` in `src/security/workflow/report.rs`, which owns its inputs (no borrowed `&self` across the await) and constructs the `LspSecurityContextExecutor` internally when `lsp_tool` is `Some`. In remote/socket mode `lsp_tool` is `None` and the call falls back to deterministic stage-1 with `note_lsp_enrichment_unavailable`. A reentrancy guard, `App.security_review_running: Option<SecurityReviewTaskState>` (holding `{ id, abort_handle }`, defined in `src/security/workflow/receipt.rs:301`), is set on dispatch and cleared in both success and failure paths; a second `/security-review` issued while the guard is set is rejected with a warning toast ("Security review already running. Wait for it to finish or cancel it."). On success the full report is pushed to the message timeline as a `UIMessage` with `MessageRole::Assistant` and a `[Security Review]` label, plus a brief success toast; the structured `SecurityReviewReceipt` is stored on `App.latest_security_review` via `App::set_latest_security_review` (`src/tui/app/mod.rs:914`) for later reopening. On failure an error toast is shown. The local-mode `LspSecurityContextExecutor` and the remote/socket deterministic fallback are both preserved.

The completion handler in `src/tui/mod.rs:2205` (`handle_security_review_finished`) guards against stale completions by comparing the incoming `id` against `app.security_review_running.id` via `App::security_review_run_id`; mismatches are silently dropped. `/security-review-cancel` aborts the running task via `App::cancel_security_review` (`src/tui/app/mod.rs:936`) which calls `AbortHandle::abort()` and clears the guard; cancellation is best-effort — if the spawned task is in a non-cancellable section (e.g. inside a blocking syscall), its completion may still arrive and is dropped by the id-mismatch guard. `/security-show` reopens `Dialog::SecurityReview` (a master/detail panel at `src/tui/components/dialogs/security_review.rs` with keybindings `j/k`, `PgUp/PgDn`, `f` cycle filter (including `HunkBacked` to show only items with hunk context), `n` notes, `p` prompts, `h` jump to hunk section, `H` copy hunk text to clipboard, `]`/`[` next/previous hunk-backed item, `Enter` opens a read-only source preview dialog for the finding's file (root-scoped via `resolve_security_review_item_path` in `receipt.rs`; shows "Security Review Finding/Prompt" origin label; falls back to clipboard if the file cannot be opened)), `Esc/q` close) from the in-memory receipt without rerunning the review. When a finding or prompt has a matching hunk (derived from the reviewed diff, not live files), the detail section renders hunk context with added/removed/context line styling. If no receipt exists yet, `/security-review-show` surfaces a "No security review result available yet." warning toast. Receipt persistence is in-memory only; the `--panel` flag on `/security-review` auto-opens the result panel on completion.

The `/security-review --enrich` command flag opts into enrichment. The `--panel` flag auto-opens the result panel on completion. Without these flags, behavior is unchanged (deterministic, no LSP execution; report goes to timeline only).

The legacy entry point `plan_security_review_from_diff(diff, repo_root)` remains available. It parses changed hunks via `parse_changed_hunks`, applies path exclusions (`is_security_review_excluded_path` — excludes `vendor/`, `third_party/`, `target/`, `dist/`, `build/`, `node_modules/`, `*.min.js`; notably does NOT exclude `Cargo.toml`, `Cargo.lock`, `build.rs`), selects presets via `select_security_preset`, builds `securityContext` request payloads via `build_security_context_request`, converts risk markers to review prompts via `prompts_from_security_context`, and assembles reports with an explicit "not confirmed findings" note via `assemble_security_review_report`.

#### Escalation policy

`choose_security_context_escalation(target, finding, prompt)` maps risk signals to `SecurityContextEscalationLevel` (None, Basic, CallDepth1, CallDepth2). `build_escalated_security_context_request(target, level)` builds the `securityContext` payload with the chosen depth. `plan_security_context_escalations(targets, ...)` returns a `SecurityContextEscalationPlan` DTO — a policy output that recommends escalation levels per target without executing LSP requests. The plan is a recommendation, not an execution. Escalation is read-only, bounded (max depth 2), and never writes files.

### Hunk/source navigation

`hunkSourceContext` is a read-only context-gathering operation that provides hunk-aware evidence for code review, edit planning, and navigation. It consumes a unified diff (patch) and maps changed hunks to enclosing symbols, nearby diagnostics, definitions, references, and hierarchy data.

**Input parameters:**

| Parameter | Type | Default | Notes |
|-----------|------|---------|-------|
| `file_path` | string | required | Target file |
| `patch` | string | optional | Unified diff text (mutually exclusive with hunks) |
| `include_definitions` | bool | true | Include definitions intersecting hunks |
| `include_references` | bool | true | Include references intersecting hunks |
| `include_call_hierarchy` | bool | false | Include call hierarchy for enclosing symbols |
| `include_type_hierarchy` | bool | false | Include type hierarchy for enclosing symbols |
| `radius` | number | 40 | Excerpt radius for source context |
| `max_hunks` | number | 20 | Maximum hunks to process |

**Output shape:**

- `file_path` — target file path
- `hunks` — per-hunk evidence (enclosing symbol, related symbols, diagnostics, definitions, references, call/type hierarchy, source excerpt, diagnostic freshness). When multiple hunks are present, semantic context is collected centered on the first hunk; definitions, references, and hierarchy are shared across all hunks.
- `limits` — truncation flags (hunks_truncated, symbols_truncated, diagnostics_truncated, references_truncated, excerpt_truncated)
- `notes` — informational notes
- `truncated` — whether output was capped

**Note:** The response does NOT include the full `SemanticContextResponse`. Hunk evidence is derived from a single semantic collection centered on the first hunk; definitions, references, and hierarchy from that collection are distributed to all hunks via range matching.

**Key properties:**

- Read-only: never writes files; patch is parsed in memory
- Pure navigator: `HunkSourceNavigator` consumes `SemanticContextResponse` and does not call LSP directly
- Bounded: per-hunk caps on symbols, diagnostics, references; global cap on hunk count
- Diagnostic freshness is preserved per hunk from the semantic response
- Evidence is best-effort and bounded; not proof of correctness or security

**Implementation:** Diff parsing (`parse_unified_diff`) produces `HunkDescriptor` values. Range primitives (`hunk_nav_ranges`) handle overlap, containment, and symbol/diagnostic matching. `HunkSourceNavigator` assembles per-hunk evidence. `HunkSourceNavigationCollector` coordinates parsing + semantic collection.

#### Hunk evidence routing policy

`HunkSourceContextPolicy` (in `src/lsp/hunk_nav_policy.rs`) controls when `hunkSourceContext` is invoked. The policy is conservative by default: definitions and references are on, hierarchy is off, and multi-file / oversized patches are skipped.

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `true` | Master switch |
| `max_patch_bytes` | 64 KB | Maximum patch size before skipping |
| `max_hunks` | 20 | Maximum hunk count per file before skipping |
| `include_definitions` | `true` | Include definitions intersecting hunks |
| `include_references` | `true` | Include references intersecting hunks |
| `include_call_hierarchy` | `false` | Include call hierarchy |
| `include_type_hierarchy` | `false` | Include type hierarchy |

`decide_hunk_source_context(policy, patch, file_path)` returns `HunkSourceContextDecision::Use` or `Skip { reason }`. Skip reasons include: disabled policy, no file path, unsupported file extension (checked against `LSP_LIKELY_EXTENSIONS` — ~25 extensions including rs, py, ts, js, go, java, c, cpp, rb, swift, kt), no extension, oversized patch, zero hunks, or too many hunks. The decision is explicit and testable; skip reasons are logged, never silently swallowed.

#### Compact summary formatter

`format_hunk_source_context_summary(response)` (in `src/lsp/hunk_nav_prompt.rs`) formats a `HunkSourceNavigationResponse` into a compact, bounded text summary suitable for agent-facing review/edit-planning prompts. The summary format is deterministic but the underlying evidence is best-effort and server-dependent. Output includes: file path, diagnostic freshness metadata, per-hunk focus range, enclosing symbol, related symbols (capped at 5), diagnostics in hunk (capped at 5 messages), nearby diagnostics count, definitions count, references count, call/type hierarchy summaries, truncation flags, and per-hunk notes. Does not dump raw JSON.

#### Security review workflow integration

`SecurityReviewWorkflowOptions.enable_hunk_source_context` (default `false`) opts into best-effort `hunkSourceContext` execution during `run_security_review_workflow`. When enabled and an executor is available:

1. `collect_hunk_source_context_all_files()` groups `ChangedHunk`s by file path, processes files in deterministic sorted order, and invokes the `HunkSourceContextPolicy` per file using actual per-file patch data. It returns a `HunkSourceContextCollectionResult` with evidence, summaries, notes, and `HunkSourceContextExecutionStats` (tracking files_considered, files_policy_skipped, requests_attempted/succeeded/failed/timed_out, evidence_items_emitted). Policy evaluation (Option B) happens before request-cap check, keeping skip statistics complete. `files_considered` counts files whose policy was evaluated (within file cap, before any request-cap break). `evidence_items_emitted` is assigned post-loop from `all_evidence.len()` (not incrementally accumulated). Request caps count actual executor calls, not loop position. The `HunkSourceContextExecutor` trait (`src/security/workflow/context.rs`) defines the boundary; `LspHunkSourceContextExecutor` (`src/security/lsp_executor.rs`) is the real adapter that calls `LspTool::execute_hunk_source_context_typed()` directly with a typed `HunkSourceNavigationRequest` — no JSON round-trip. The adapter uses an internal `TypedHunkSourceContextTarget` trait (production: `LspTool`) with a `#[cfg(test)]` recording target for forwarding verification without a live LSP server. The model-facing tool schema remains patch-only; internal pre-parsed hunk descriptors are used via the typed API.
2. `evidence_from_hunk_source_context()` converts `HunkSourceNavigationResponse` into `StructuredSecurityEvidence` items with kind `HunkNavigation` (enclosing symbols, definitions, reference counts) or `Diagnostic` (in-hunk and nearby diagnostics). Only real `HunkSourceNavigationResponse` produces `HunkNavigation` evidence — policy skip decisions are routing metadata, never security evidence.
3. Evidence is injected into `synthesize_evidence_based_findings_with_extra_evidence()` for eligibility gating. The tightened gate requires `HunkNavigation` to appear alongside `RiskMarker` or `Preflight` (or other supporting dimensions) — `ChangedHunk + HunkNavigation` alone is not finding-eligible.

Multi-file diffs are processed one file at a time (capped at 8 files). The workflow is the `/security-review --hunk-context` flag path, not model-initiated.

Fail-open: per-file errors are noted (appended to output `notes`) and never block the workflow.

#### HunkNavigation evidence kind

`SecurityEvidenceKind::HunkNavigation` (in `src/security/workflow/types.rs`) represents evidence from `hunkSourceContext`: enclosing symbols, definitions intersecting changed ranges, and reference counts. Each item carries `file_path`, `line`, `summary`, and `detail` (hunk id). `HunkNavigation` is not standalone finding-eligible — it requires `RiskMarker`, `Preflight`, or another supporting dimension to form a finding. Policy skip decisions never produce `HunkNavigation` evidence.

#### ChangedHunk → HunkDescriptor conversion

`ChangedHunk::to_hunk_descriptor(hunk_index)` (in `src/security/workflow/types.rs`) converts a security-workflow `ChangedHunk` into an egglsp `HunkDescriptor` for the typed internal execution path. The `old_range` and `new_range` are computed from the hunk's start/count fields. The `hunk_index` parameter provides the deterministic hunk id prefix. These pre-parsed descriptors are passed directly to `LspTool::execute_hunk_source_context_typed()` via the `HunkSourceContextExecutor` trait — the model-facing tool schema remains patch-only.

#### Fail-open behavior

All hunk source context operations are fail-open: errors during policy evaluation, semantic collection, or evidence conversion are recorded as notes in the output and do not prevent the rest of the security review from completing. LSP results remain server-dependent and fail-open. Policy skip reasons are logged at debug level.

#### Default caps

- `max_patch_bytes`: 64 KB (patch size limit; policy uses actual per-file patch data)
- `max_hunks`: 20 (per-file hunk count limit)

#### Known limitations

- **Single-file hunk context only**: `hunkSourceContext` processes one file's hunks at a time. The security review workflow groups multi-file patches by file path and processes them independently in deterministic sorted order.
- **First-hunk-centered semantic collection**: Semantic context (definitions, references, hierarchy) is collected centered on the first hunk's position. Results are distributed to all hunks via range matching. Hunks far from the first may have less precise context.
- **LSP results are server-dependent**: LSP results remain server-dependent and fail-open. Policy skips and LSP errors produce notes, never block the caller.

### Position Convention

Model-facing line and column are **1-indexed**. The wrapper converts to LSP 0-indexed via `to_lsp_position()`. Missing required fields return clear `ToolError::Execution` messages.

### Compact DTOs

All output is wrapped in `LspToolOutput<T>` with `operation`, `file_path`, `result_count`, `truncated`, and `results` fields. Individual results use `LocationSummary`, `DiagnosticSummary`, `SymbolSummary`, or `HoverSummary` with 1-indexed positions and file paths (not URIs). Additionally, `SemanticContextPacket` wraps a bounded source excerpt (`SourceExcerpt` with `start_line`, `end_line`, `text`), diagnostics, symbols, definitions, references, optional per-section error fields (`definitions_error`, `references_error`), optional `source_actions` array of `SemanticSourceActionHint` objects (`action`, `available`, `preview`, `error`), and a `SemanticContextLimits` struct tracking truncation (including `overlay_diagnostics_truncated`).

### Diagnostics

The `diagnostics` operation is first-class. It reads from the shared diagnostics cache populated by `publishDiagnostics` notifications. Diagnostics use 1-indexed line/column in output. If no diagnostics have arrived yet, an empty list is returned.

The `diagnostics` tool output includes freshness metadata (`freshness`, `source`, `age_ms`, `usable_evidence`) so callers can judge diagnostic reliability. `age_ms` is the age in milliseconds since diagnostics were received from the language server. Freshness is classified as `Fresh`, `PossiblyStale`, `Stale`, or `Unavailable`. See the Diagnostics Cache Lifecycle section below for details.

### Capability-Gated Operations

The `semanticContext` and `securityContext` handlers check `LspCapabilitySnapshot` before making optional expensive LSP calls (definitions, references, call hierarchy, type hierarchy). When a capability is unsupported:

- **definitions**: `definitions_error` is set to `"definition not supported by server"` and no LSP request is made.
- **references**: `references_error` is set to `"references not supported by server"` and no LSP request is made.
- **call hierarchy** (semanticContext): the `call_hierarchy` field is `None` (no request made).
- **call hierarchy** (securityContext): a note `"call hierarchy not supported by server"` is appended.
- **call expansion** (securityContext): a note `"call expansion not supported by server (call hierarchy required)"` is appended and `call_expansion` is `None`.
- **type hierarchy** (semanticContext): the `type_hierarchy` field is `None` (no request made).

When no capability snapshot is available (e.g., server not yet initialized), operations default to attempting the call (fail-open). This ensures degraded-but-functional behavior when capabilities cannot be determined.

### Capability Discovery and Normalization

`LspCapabilitySnapshot` provides a normalized boolean view of a server's capabilities after initialization. Each boolean field corresponds to a specific LSP feature or operation, derived from the `ServerCapabilities` reported by the server during the `initialize` handshake.

```rust
pub struct LspCapabilitySnapshot {
    pub publish_diagnostics: bool,
    pub document_symbols: bool,
    pub workspace_symbols: bool,
    pub goto_definition: bool,
    pub find_references: bool,
    pub hover: bool,
    pub completion: bool,
    pub call_hierarchy: bool,
    pub type_hierarchy: bool,
    pub semantic_tokens: bool,
    pub code_actions: bool,
    pub formatting: bool,
    pub rename: bool,
    pub signature_help: bool,
}
```

`LspSemanticOperation` enumerates the semantic operations available through the tool interface:

```rust
pub enum LspSemanticOperation {
    Diagnostics,
    DocumentSymbols,
    WorkspaceSymbols,
    Definition,
    References,
    Hover,
    Completion,
    CallHierarchy,
    TypeHierarchy,
    SemanticTokens,
    SecurityContext,
}
```

`LspUnavailable` is a structured fallback response returned when an operation is not supported by the server:

```rust
pub struct LspUnavailable {
    pub operation: LspSemanticOperation,
    pub reason: String,
    pub server_id: String,
}
```

The `capabilities` LspTool operation returns the snapshot for the server associated with a given file path. Capability detection uses actual initialized server capabilities where available; if the server has not yet initialized, the snapshot reflects the server definition's known defaults. The snapshot carries real `server_name` and `language_id` metadata from the initialized server, not placeholders. `SecurityContext` is always treated as available — it is a composite operation that relies on multiple underlying LSP requests and risk marker scanning, not a single capability.

### Diagnostics Cache Lifecycle

`DiagnosticCacheEntry` (in `crates/egglsp/src/client.rs`) stores per-file diagnostics with `received_at`, `source`, and `content_version` metadata. The cache is updated asynchronously when `publishDiagnostics` notifications arrive from the LSP server.

`LspClient::diagnostic_snapshot()` classifies freshness based on these fields:

`age_ms` is zero for unavailable snapshots and elapsed diagnostic age for all cached diagnostic snapshots, including stale cached snapshots.

`LspDiagnosticSnapshot` represents a point-in-time view of diagnostics for a single file:

```rust
pub struct LspDiagnosticSnapshot {
    pub file_path: String,
    pub diagnostics: Vec<DiagnosticSummary>,
    pub age_ms: i64,
    pub source: LspDiagnosticSource,
    pub freshness: LspDiagnosticFreshness,
}
```

`LspDiagnosticFreshness` indicates how current the cached diagnostics are:

```rust
pub enum LspDiagnosticFreshness {
    Fresh,
    PossiblyStale,
    Stale,
    Unavailable,
}
```

`LspDiagnosticSource` tracks how diagnostics were obtained:

```rust
pub enum LspDiagnosticSource {
    Pushed,
    Pulled,
    Unknown,
}
```

**Invalidation rules:**

- Diagnostics transition to `PossiblyStale` on file content changes (the server has not yet republished after the change).
- Diagnostics transition to `Stale` on server restart (the cache is cleared and repopulated asynchronously).
- `Unavailable` indicates no diagnostics have been received for the file.

`PossiblyStale` and `Stale` diagnostics should not be treated as high-confidence evidence for code analysis or security findings. The freshness field allows consumers to make informed decisions about diagnostic reliability.

`DiagnosticsCollector::get_diagnostic_snapshot_for_file()` is the primary API for obtaining a snapshot. It ensures the file is open from disk, then delegates to `LspService::get_diagnostic_snapshot_for_key()` which consults the client's diagnostic cache.

`DiagnosticsCollector::get_all_diagnostic_snapshots()` returns a `HashMap<String, LspDiagnosticSnapshot>` for freshness-aware bulk diagnostics. `get_all_diagnostics()` is a legacy freshness-blind view that returns raw diagnostics without freshness metadata.

`LspDiagnosticSnapshot::diagnostics_may_still_be_warming()` is a derived method that returns `true` when freshness is `PossiblyStale` and diagnostics are empty, indicating the server may still be processing.

### Diagnostic Evidence in Context Packets

Both `SemanticContextPacket` and `SecurityContextPacket` include an optional `diagnostic_evidence` field carrying freshness metadata:

```rust
struct DiagnosticEvidenceMeta {
    freshness: LspDiagnosticFreshness,
    source: LspDiagnosticSource,
    age_ms: i64,
    usable_evidence: bool,
}
```

The `age_ms` field is the age in milliseconds since diagnostics were received from the language server, not an absolute generation timestamp. The `usable_evidence` field is `true` when freshness is `Fresh` or `PossiblyStale`. Consumers should treat stale/unavailable diagnostic evidence as low-confidence. The `securityContext` handler appends notes when diagnostics are stale or unavailable:

- `"diagnostics stale: treating diagnostics as low-confidence evidence"` (for `Stale`)
- `"diagnostics unavailable: no LSP diagnostic evidence available"` (for `Unavailable`)

This allows the security review workflow to make informed decisions about diagnostic reliability when synthesizing findings.

## Shared Semantic Context API

The shared semantic context API provides domain-agnostic DTOs for assembling LSP evidence. `SemanticContextResponse` is the **internal semantic read model** for `semanticContext`; `securityContext` reuses the shared diagnostic evidence and capability snapshot but assembles its own security-filtered packet.

### SemanticContextRequest

Describes what the caller wants to know:

```rust
pub struct SemanticContextRequest {
    pub file_path: String,
    pub line: Option<u32>,          // 1-indexed
    pub column: Option<u32>,        // 1-indexed
    pub intent: SemanticContextIntent,
    pub max_symbols: usize,
    pub max_references: usize,
    pub max_diagnostics: usize,
    pub call_depth: u8,
    pub include_overlay: bool,
    pub include_source_actions: bool,
    pub include_definitions: bool,
    pub include_references: bool,
    pub excerpt_radius: u32,
}
```

### SemanticContextResponse

The assembled semantic context. This is the internal read model that `semanticContext` and `securityContext` adapt from:

```rust
pub struct SemanticContextResponse {
    pub file_path: String,
    pub symbol: Option<SemanticSymbolSummary>,        // First symbol (backward-compatible)
    pub all_symbols: Vec<SemanticSymbolSummary>,      // All document symbols (flattened, capped)
    pub diagnostics: Vec<FileDiagnostic>,             // 0-indexed diagnostics
    pub definitions: Vec<SemanticLocation>,
    pub references: Vec<SemanticLocation>,
    pub call_hierarchy: Option<SemanticCallGraphSummary>,
    pub type_hierarchy: Option<SemanticTypeGraphSummary>,
    pub source_excerpt: Option<SemanticSourceExcerpt>,
    pub diagnostic_evidence: Option<SemanticDiagnosticEvidence>,
    pub overlay: Option<SemanticOverlay>,
    pub source_actions: Vec<SemanticSourceActionHint>,
    pub section_truncations: Vec<SemanticSectionTruncation>,
    pub limits: SemanticContextLimits,
    pub notes: Vec<String>,
    pub truncated: bool,
    pub unavailable: Vec<LspUnavailable>,
}
```

### Supporting DTOs

- `SemanticSourceExcerpt`: Source text excerpt with `start_line`, `end_line`, `text`, `truncated`
- `SemanticDiagnosticEvidence`: Freshness metadata (`freshness`, `source`, `age_ms`, `usable_evidence`)
- `SemanticOverlay`: Overlay diagnostics/symbols from proposed content preview
- `SemanticSectionTruncation`: Per-section truncation metadata (`section`, `original_count`, `emitted_count`, `limit`)
- `SemanticContextLimits`: Truncation flags per section
- `SemanticSymbolSummary`, `SemanticLocation`: Compact symbol/location summaries (1-indexed)
- `SemanticCallGraphSummary`, `SemanticTypeGraphSummary`: Hierarchy summaries

All line/column values in shared DTOs are **1-indexed** for consistency with the presentation layer.

### Conversion

The `semanticContext` handler follows this flow:

```rust
let request = SemanticContextRequest::from_tool_input(...)?;
let response = collector.collect(request).await?;
let packet = SemanticContextPacket::from_semantic_response(response, target, overlay, source_actions, limits);
serialize(packet)
```

`SemanticContextPacket::from_semantic_response()` is the adapter that converts the shared response into the tool-local presentation packet, handling 0→1-indexed diagnostic conversion, excerpt adaptation, and note→error field mapping.

### Remote/Core Ownership Model (Phase 7)

In the headless-core architecture:

- The **headless core** owns all LSP server processes, capability snapshots, diagnostics caches, and file synchronization state. LSP servers are spawned and managed exclusively by the core.
- **Frontends** (TUI, web, IDE extensions) request semantic context over the core protocol (`CoreRequest::SemanticContext` or equivalent). They never start their own LSP server processes for the same workspace unless explicitly configured as local-only.
- All requests pass through **root authorization** — the core enforces that requested file paths fall within an allowed root directory before dispatching to LSP.
- A remote frontend that connects to a headless core with no LSP support for the requested language receives a structured `LspUnavailable` response rather than an opaque error. The response includes the server ID and a human-readable reason.
- When the core has no LSP server for the file's language (e.g., unsupported language, no server configured), the `SemanticContextResponse.unavailable` field contains one or more `LspUnavailable` entries. The frontend can render these as informational notes.
- Diagnostics cache ownership remains with the core. Frontends receive `LspDiagnosticSnapshot` with freshness metadata and can display staleness indicators.

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

## Protocol Peer Hardening (Phase 1)

Codegg's LSP runtime operates as a **bidirectional JSON-RPC peer**, not merely a client that sends requests and consumes diagnostics. The server can send requests back to the client (e.g. `workspace/configuration`, `workspace/workspaceFolders`, `client/registerCapability`), and Codegg answers them correctly.

### Incoming Message Taxonomy

The `classify_json_rpc_message` function classifies incoming JSON-RPC messages using strict structural analysis:

| Shape | Classification |
|-------|---------------|
| `id` + `method` | Server request |
| `id` + valid error object (with numeric `code` and string `message`) | Error response |
| `id` + `result` field present | Success response |
| `method` without `id` | Notification |
| Otherwise (including id-only objects, malformed errors) | Unknown |

The classifier is strict: an `id` without `method`, without a valid error object, and without a `result` field is classified as `Unknown`, not as a response. Malformed error objects (e.g., missing `code`, non-numeric `code`, missing `message`) also fall through to `Unknown`.

`JsonRpcId` preserves both numeric (`Number(i64)`) and string (`String(String)`) IDs per JSON-RPC spec. Client-originated IDs are tracked in the `pending` map; server-originated IDs are answered but never inserted into `pending`.

### Supported Server-Originated Requests

Codegg handles these server requests via `dispatch_server_request` in `server_request.rs`:

| Method | Behavior |
|--------|----------|
| `workspace/configuration` | Returns configuration values scoped to the server/root; `null` for unknown sections |
| `workspace/workspaceFolders` | Returns the current root as a single-element workspace folder array |
| `client/registerCapability` | Records registration in `DynamicRegistrationState` (bounded at 256); acknowledges with `null` |
| `client/unregisterCapability` | Removes registration by ID; tolerates unknown IDs |
| `window/workDoneProgress/create` | Acknowledges with `null` |
| `workspace/applyEdit` | **Always rejected** as an application-level result with `applied: false` and a `failureReason` string — not a JSON-RPC error. Codegg does not permit implicit language-server edits. |
| Unknown methods | Returns JSON-RPC error `-32601` (Method not found) |
| Malformed params | Returns `-32602` (Invalid params) |

### Dynamic Registration

`DynamicRegistrationState` tracks server-requested capability registrations bounded at 256 entries. Recording a registration does **not** mean Codegg claims operational support for that feature — `LspCapabilitySnapshot` is derived from `ServerCapabilities` (the `initialize` response) only.

`client/registerCapability` processes the full `registrations` array via `register_batch()`, which pre-checks capacity before any mutation: all entries are validated first (rejecting the entire request if any entry is malformed), deduplicated by ID (last-write-wins within a single request), then applied. This atomic batch approach prevents partial application when a batch exceeds the capacity limit. Replacements of existing IDs bypass the 256 cap; only new IDs are counted against it.

`client/unregisterCapability` accepts either the `unregisterations` array (LSP spec), the `unregistrations` compat spelling, or a single `id` field for backward compatibility. Unknown IDs are silently tolerated.

### Shared Serialized Writer

`LspWriter` (`writer.rs`) provides a shared, `Arc<Mutex<...>>`-wrapped writer for all protocol output. Both client requests/notifications and the background server-request dispatcher use the same writer, ensuring serialized writes without interleaving frames. Content-Length framing uses UTF-8 byte length.

### Timeout Cancellation

On request timeout:
1. The pending entry is removed from the map
2. A best-effort `$/cancelRequest` notification is sent to the server with the original request ID
3. If the cancel write fails, `fail_transport()` marks the transport failed and drains any remaining pending requests
4. The timeout error is returned to the caller

Cancellation failures do not replace the timeout error, but they do retire the transport so later calls fail fast.

### Single-Flight Client Initialization

`LspService::get_or_create_client` uses explicit `InitRole` election: the first caller becomes `Leader` and spawns an owned initialization task (`run_initialization_attempt`); concurrent callers for the same `{project_root}:{server_id}` key become `Waiters`. The `InitSlot` stores one leader sender plus a waiter list, and completion is fanned out to every sender with the same `Arc<LspClient>` on success or the same `SharedInitError` on failure. An `ATTEMPT_COUNTER: AtomicU64` generates monotonic attempt IDs stored in the `InitSlot`.

#### Start-Registration Barrier

The wrapper task does not begin its initialization body until its `active_init_tasks` entry has been installed. This is enforced by a one-shot start barrier:

1. The leader creates `(start_tx, start_rx)` and `(completion_tx, completion_rx)` channels.
2. The wrapper task is spawned with `start_rx` and `completion_tx` and **awaits** `start_rx` first.
3. The leader installs the `InitTaskControl` (containing `completion_rx`) into `active_init_tasks` under its own lock acquisition.
4. The leader re-validates the slot under the `initializing` lock — these are sequential lock acquisitions, not nested.
5. The leader sends on `start_tx`, releasing the wrapper to begin its body.
6. If the slot was invalidated in step 4, the leader drops `start_tx` (causing the wrapper's `start_rx.await` to resolve to `Err`), aborts the wrapper defensively, removes the just-installed `active_init_tasks` entry, and notifies any waiters via `abort_and_finalize_unstarted_task`.

This eliminates the spawn-before-registration race: a fast task cannot complete before its bookkeeping record exists.

#### Authoritative Completion Primitive

Each spawned initialization task is wrapped in `run_init_task_wrapper`, which:

1. **Awaits** `start_rx` to receive the registration-completion signal.
2. **Owns** the `completion_tx` end of the authoritative terminal signal.
3. **Executes** the inner init attempt, with `AssertUnwindSafe + catch_unwind` to convert panics into a `SharedInitError` for any waiters and an `InitTaskExit::Panicked` exit value.
4. **Explicitly removes** its `active_init_tasks` entry before sending the terminal exit (primary cleanup path).
5. **Disarms** the `ActiveTaskGuard` fallback so the guard's `Drop` is a no-op.
6. **Sends** exactly one `InitTaskExit` (`Completed`, `Panicked(msg)`, or `Cancelled`) via `completion_tx`.

The completion receiver in `InitTaskControl` is the authoritative source of truth for "the wrapper task has terminated". The receiver resolves to `Ok(exit)` on the normal path, or to `Err(RecvError)` if the sender (and therefore the wrapper) was dropped without sending — e.g. by forced abort. Shutdown awaits this receiver through `await_init_task_completions`; it never holds the real `JoinHandle` via a forwarding task.

On initialization failure, the slot is cleaned up by attempt ID (compare-and-remove prevents stale cleanup from deleting newer slots), and all waiting callers receive `SharedInitError` (preserving error category and message), allowing retries. Before a successful client is published, the init task rechecks `LifecycleState` and only inserts when the phase is still `Running` and the generation matches the captured generation; if publication is invalidated or loses to an existing client, the unpublished client is disposed via `dispose_unpublished_client(...)` with a bounded shutdown timeout. This differs from `OnceCell` which would cache the failure permanently. `SharedInitError` with `SharedInitErrorKind` enum (`ServerNotFound`, `DownloadFailed`, `LaunchFailed`, `InitializeFailed`, `Timeout`, `Cancelled`, `Protocol`, `Other`) is used for all oneshot channel results instead of raw `LspError`, making concurrent error propagation thread-safe and cloneable. The `#[cfg(test)]` `test_new()` constructor accepts injectable test factories for deterministic testing without live LSP servers.

#### Active-Task Entry Cleanup

`active_init_tasks` entries are removed through three complementary mechanisms:

1. **Explicit removal** (primary path): the wrapper acquires the `active_init_tasks` lock and removes its own entry before sending the terminal exit. This is the path for normal completion and ordinary failure. The wrapper then calls `ActiveTaskGuard::disarm()` to suppress the fallback.

2. **ActiveTaskGuard fallback**: if the wrapper is dropped before explicit removal (e.g. due to forced abort, panic propagation that bypasses explicit cleanup, or unexpected future drop), the guard's `Drop` runs and **spawns a follow-up cleanup task** that locks the map and removes the entry. This is robust to lock contention at drop time. The guard no longer relies on `try_lock` for the fallback path — that approach silently abandoned cleanup if the lock was held.

3. **Coordinator-owned drain**: `shutdown_all` is the additional safety net. After awaiting all completion receivers (via `await_init_task_completions`), the drain clears the map one final time to guarantee the postcondition regardless of which path any individual wrapper took.

This eliminates the prior defect where successful, failed, or invalidated attempts could leave stale task-control entries until shutdown drained the map.

#### Registration Lock Ordering

Between slot creation and active-task registration, the slot may be removed by a concurrent shutdown. The `Leader` branch resolves this race without nested locks:

1. Acquire `initializing` lock; check slot validity for this `attempt_id`; release `initializing` lock.
2. Acquire `active_init_tasks` lock; install `InitTaskControl`; release `active_init_tasks` lock.
3. Acquire `initializing` lock again; re-check slot validity; release `initializing` lock.
4. If still valid, send on `start_tx` to release the wrapper.
5. If invalidated at any point, run `abort_and_finalize_unstarted_task` to drop the start signal, abort the wrapper defensively, remove the active-task entry, and notify any waiters.

No path holds `active_init_tasks` while awaiting `initializing`, and no path holds either lock across task/client I/O. The two lock acquisitions are sequential, not nested.

#### Cooperative Cancellation in Test Factories

The injected test factory is wrapped in a `tokio::select!` so cancellation propagates to test factories by default:

```rust
tokio::select! {
    biased;
    res = init_fn(server, &root) => res,
    _ = cancellation.cancelled() => Err(LspError::InitializationCancelled("shutting down".into())),
}
```

The standard `blocking_factory` and similar are cancellation-aware. Tests that exercise forced abort (e.g. via a stuck factory) use factories whose inner future ignores the outer `select!`'s cancellation arm, exercising the `AbortHandle` path through `await_init_task_completions`.

### Global Map Lock Discipline

Non-mutating service methods use `clients.read().await` to avoid serializing unrelated clients behind process I/O. These methods include: `open_file`, `update_file`, `close_file`, `save_file`, `is_file_open`, `get_diagnostics_for_key`, `get_all_diagnostics_for_key`, `diagnostics_may_still_be_warming`, `get_diagnostic_snapshot_for_key`, `send_request`, `client_keys`, and `get_capabilities_for_key`. Each follows the pattern:

1. Acquire the map read lock
2. Clone the `Arc<LspClient>`
3. Release the map lock
4. Await the client operation

Write guards (`clients.write().await`) are reserved for slot election/publication (inserting a new client entry after initialization) and shutdown drain (removing clients during `shutdown_all`). This separation ensures read-heavy workloads (diagnostics, file operations, capability checks) never contend with write operations.

`close_file` and `save_file` use deterministic O(1) ownership lookup via the `document_owners` map (URI → client key) rather than searching cloned handles or relying on `HashMap` iteration order.

### Shutdown Coordination

`LspService` tracks a `LifecycleState` containing both `ServiceLifecycle` phase and a monotonic `generation: u64`. The service also holds a `tokio::sync::watch` channel (`lifecycle_tx`) that retains the latest lifecycle state for late subscribers; this replaces the previous `Notify`-based coordination which was susceptible to lost wakeups at the `ShuttingDown → Stopped` transition. `shutdown_all()` atomically transitions to `ShuttingDown` and increments the generation, broadcasting the change on the watch channel. The spawned initialization task rechecks the phase and generation before publication, preventing stale results from being published after shutdown and disposing any unpublished client that loses the race. `get_or_create_client()` rejects new client acquisition when the lifecycle is not `Running`, returning `LspError::InitializationCancelled`.

#### Quiescent Shutdown Sequence

`shutdown_all()` follows a bounded, multi-phase sequence driven by an **absolute deadline** (computed once at entry: `Instant::now() + SHUTDOWN_GLOBAL_TIMEOUT`). Each stage receives a remaining-time bound; the deadline propagates rather than being re-wrapped in a timeout that can silently abandon finalization.

1. **Transition to ShuttingDown** — atomically sets phase and increments generation; broadcasts on `lifecycle_tx` (watch channel). A second caller observing `ShuttingDown` enters the race-free `await_stopped()` path.
2. **Clear document ownership** — `document_owners` is cleared.
3. **Drain init slots** — all pending `InitSlot` entries are removed; their senders are notified at step 9.
4. **Drain active tasks** — `active_init_tasks` is drained; each entry's `InitTaskControl` (containing its `CancellationToken`, `AbortHandle`, and authoritative completion receiver) is moved into the shutdown's local vector.
5. **Concurrent cooperative cancel** — all cancellation tokens are signalled simultaneously.
6. **Aggregate grace wait** — `await_init_task_completions` awaits all completion receivers concurrently using `FuturesUnordered` under one aggregate grace deadline (`SHUTDOWN_CANCELLATION_GRACE` = 300ms, capped by the global deadline). The future for each control uses `tokio::select!` to race the receiver against the deadline. On timeout, the control (with its real receiver intact) is returned in the pending set. On receiver resolution, the exit value is logged. **No forwarding task wraps the real `JoinHandle`**: the receiver is the authoritative terminal signal.
7. **Concurrent abort of stragglers** — for any controls still pending after the grace, `AbortHandle::abort()` is called on each, then `await_init_task_completions` re-awaits the same set of completion receivers under the remaining global deadline. The receiver resolves either when the wrapper sends its terminal exit (rare under forced abort) or when the sender is dropped (the task future was dropped by the abort, closing the channel). Every aborted task's real completion is observed.
8. **Concurrent ready-client shutdown** — ready clients are drained from the map and shut down concurrently (`futures::future::join_all`). Each per-client timeout is capped by `SHUTDOWN_CLIENT_TIMEOUT` (2s) and the global deadline, so the total shutdown duration is independent of client count. Three result variants are logged: `Ok(Ok(()))` (graceful), `Ok(Err(_))` (graceful shutdown error), and `Err(_)` (timeout).
9. **Notify init-task waiters** — the senders drained in step 3 receive a `Cancelled` `SharedInitError`.
10. **Forced finalization** — if the absolute deadline has expired, a `warn!` is logged. The `active_init_tasks`, `initializing`, and `document_owners` maps are drained defensively to guarantee postconditions. This is the documented **pathological deadline fallback**: the service state is finalized after abort was requested, with unresolved task completion logged as a severe invariant failure. The shutdown contract distinguishes the **normal contract** (all task termination observed via completion receivers) from the **deadline fallback** (state forced after the global deadline, with the explicit caveat that Tokio may not deliver a terminal event for an aborted task in pathological cases).
11. **Transition to Stopped** — final lifecycle phase; broadcast on `lifecycle_tx` so concurrent waiters can return.

Total bounded duration: `SHUTDOWN_GLOBAL_TIMEOUT` (6s). Per-stage budgets are derived from the absolute deadline.

#### Concurrent Shutdown Callers

A second caller observing `ShuttingDown` enters `await_stopped()`:

1. Subscribe to the `lifecycle_tx` watch channel.
2. Re-check the current state.
3. If `Stopped`, return immediately.
4. If `ShuttingDown`, await state changes until `Stopped`.

This race-free pattern eliminates the lost-wakeup window that the previous `Notify`-based coordination had at the `ShuttingDown → Stopped` transition. Late subscribers always observe the latest retained state.

### New Tests

The tracked initialization and quiescent shutdown features are covered by targeted tests:

| Test | What it verifies |
|------|-----------------|
| `shutdown_cancels_blocked_factory` | Cooperative cancellation: a factory blocked in `initialize` is cancelled via `CancellationToken` during shutdown |
| `shutdown_aborts_uncooperative_task` | Hard abort: a task that ignores cooperative cancellation is aborted via `AbortHandle` after grace period. The `FutureExitProbe` RAII guard asserts the factory future body was actually dropped before shutdown returned. |
| `concurrent_shutdown_callers` | Two concurrent `shutdown_all()` calls both observe the final `Stopped` state via the watch channel |
| `concurrent_shutdown_lost_wakeup_boundary` | Late subscribers to the watch channel do not miss the `ShuttingDown → Stopped` transition |
| `read_lock_concurrency` | Non-mutating operations (`open_file`, `diagnostics`, etc.) use read locks and do not contend with each other |
| `publication_race_remains_safe` | Publication under shutdown races: an init task that finishes after `ShuttingDown` does not publish a stale client |
| `normal_completion_removes_active_task_entry` | Explicit cleanup path: the wrapper removes its `active_init_tasks` entry without requiring shutdown |
| `ordinary_failure_removes_active_task_entry` | Same, for ordinary initialization failures |
| `forced_abort_is_awaited` | The aborted task's completion receiver is awaited; the task body actually exits before shutdown returns. The `FutureExitProbe` proves the factory future was dropped. |
| `global_deadline_finalizes_state` | A task that does not complete within the global deadline is still drained; lifecycle reaches `Stopped` and all maps are empty |
| `fast_completion_cannot_beat_registration` | The start-registration barrier prevents a fast-completing task from racing past the `active_init_tasks` insertion. Run repeatedly in a bounded loop to expose scheduler races. |
| `cooperative_cancellation_is_observed` | The factory future body is dropped (RAII probe increments) before shutdown returns; the `InitTaskExit` resolution is observed via the authoritative receiver. |
| `many_tasks_share_one_grace_period` | The aggregate grace wait in `await_init_task_completions` is applied across all in-flight tasks; total shutdown time is bounded by one grace period rather than N × grace. |
| `no_stale_active_entries_under_contention` | Concurrent fast success attempts (single-flight) leave `active_init_tasks` empty without requiring shutdown. |
| `lock_order_no_deadlock_under_overlap` | Concurrent registration and shutdown overlap via test gates; neither path deadlocks and both complete within bounded time. |
| `global_deadline_fallback_asserts_all_signals` | A stuck factory is forcibly aborted, the abort signal is observed, all maps are drained, and the lifecycle is `Stopped` — all within the global deadline. |
| `forced_abort_after_grace_period` | Genuinely survives cooperative cancellation past the 300ms grace interval using a test-only `InitTaskBehavior::IgnoreCancellationUntilAbort` hook. Asserts the real `AbortHandle::abort()` path is reached and the factory future is dropped before shutdown returns. |
| `aggregate_grace_across_independent_tasks` | Multiple independent initialization keys (distinct roots) each with blocked factories. Confirms `active_init_tasks.len() == N` and total shutdown time is bounded near one aggregate grace period rather than N × grace. |
| `deadline_fallback_with_unresolvable_completion` | Constructs `InitTaskControl` values with receivers whose senders are intentionally retained (never resolving). Drives `await_init_task_completions` to the global deadline and verifies unresolved controls are logged/returned and state finalization continues. |
| Phase 2: initialization handshake | `production_protocol_stdio::initialization_handshake` | Real stdio init/initialized/shutdown/exit through fake server |
| Phase 2: server request during init | `production_protocol_stdio::server_requests_during_init_and_dynamic_registration` | workspace/configuration interleaved with initialize |
| Phase 2: apply-edit refusal | `production_protocol_stdio::apply_edit_refusal_keeps_client_usable` | workspace/applyEdit rejected with applied:false |
| Phase 2: concurrent responses | `production_protocol_stdio::concurrent_out_of_order_responses_and_notifications` | Multiple requests, out-of-order responses |
| Phase 2: timeout and cancellation | `production_protocol_stdio::request_timeout_and_late_response_are_dropped` | Production $/cancelRequest emission |
| Phase 2: malformed frames | `production_protocol_stdio::malformed_frames_fail_transport` | 8 malformed framing cases → transport failure |
| Phase 2: server exit | `production_protocol_stdio::server_exit_before_response_and_error_response` | Server exits without responding |
| Phase 2: typed semantic | `production_semantic_stdio::typed_semantic_requests_collect_context_and_freshness` | Hover, definition, references, symbols, completion, code actions |

The `FutureExitProbe` test-only RAII guard (`src/lsp/../service.rs`) is constructed at the top of test factory futures to prove that the future body was actually dropped. It is robust to all three exit paths (normal return, cooperative cancellation, forced abort) and is used by `shutdown_aborts_uncooperative_task`, `cooperative_cancellation_is_observed`, `forced_abort_is_awaited`, and `forced_abort_after_grace_period`.

The flaky transport test (`timeout_cancel_failure_marks_transport_failed_and_writes_writer_closed`) has been fixed by replacing OS-pipe-dependent behavior with deterministic writer injection.

### Writer Failure Propagation

The background reader tracks `ClientTransportState` (`Running` or `Failed { reason }`). All terminal transport failures (stdout EOF, server-request result/error write failure, `send_request` write failure, `send_notification` write failure, and timeout-cancel write failure) transition to `Failed` exactly once via the centralized `fail_transport()` helper. The helper atomically transitions to `Failed` (idempotent), releases the transport lock, then drains all pending requests with errors. Subsequent `send_request` / `send_notification` calls return `LspError::WriterClosed` immediately, avoiding writes to a broken pipe.

### Integral Error Code Validation

`is_structural_error()` in `client.rs` validates JSON-RPC error codes as integers using `as_i64().is_some()`, rejecting fractional codes (e.g. `3.5`) that would fail JSON-RPC error semantics. This prevents misclassification of malformed error responses.

### Limitations

- `workspace/applyEdit` is always rejected as an application-level result (`applied: false`) — servers cannot implicitly write files through Codegg
- Dynamic registrations are tracked but do not expand model-facing capability claims
- Configuration responses are bounded to the server's configured section — no environment secrets are exposed
- Server requests are handled synchronously within the background reader with a 5-second timeout. A timeout produces a JSON-RPC error response with code `-32603` (Internal error) rather than silently abandoning the request. Current handlers are fast and local.

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
    Protocol(String),
    WriterClosed(String),
    InitializationCancelled(String),
}
```

### SharedInitError

A cloneable error type used for concurrent initialization waiters. `SharedInitError` with `SharedInitErrorKind` enum (`ServerNotFound`, `DownloadFailed`, `LaunchFailed`, `InitializeFailed`, `Timeout`, `Cancelled`, `Protocol`, `Other`) carries the error category and message across threads via oneshot channels. Converts via `From<&LspError> for SharedInitError` and `into_lsp_error()` back to `LspError`. This replaces raw `LspError` in the `InitSlot` oneshot results, making concurrent initialization error propagation thread-safe and cloneable.

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

## Phase 2: Scripted Stdio Integration Testing (Complete)

The `egglsp` package carries production-harness integration tests under `tests/production_protocol_stdio.rs`, `tests/production_semantic_stdio.rs`, and `tests/production_service_stdio.rs`, plus `tests/scenario_engine.rs` wrapping the fake-server self-tests. The root crate carries composite tests in `tests/lsp_composite_stdio.rs` that bridge the gap between `egglsp`-only tests and the real root-crate collectors (`SemanticContextCollector`, `DiagnosticsCollector`, `LspOperations`). The fake LSP server binary is named `codegg-lsp-test-server` for root tests (via `CARGO_BIN_EXE_codegg-lsp-test-server`); the `egglsp` package uses `egglsp-test-server` via `CARGO_BIN_EXE_egglsp-test-server`. Both are built as `[[bin]]` targets from the `egglsp` package; they read Content-Length framed JSON-RPC from stdin, execute scripted scenarios, and write machine-readable transcripts. The fake server supports captured-ID mode for genuinely out-of-order concurrent responses, enabling deterministic testing of concurrent request handling. All integration tests use bounded condition waits (polling loops) instead of fixed sleeps.

### Architecture

```
Integration test
    |
    | creates Scenario JSON file
    v
LspWriter / frame reader
    |
    | launches child process through spawn_server
    v
egglsp-test-server binary
    |
    | reads scenario, exchanges real framed messages
    v
transcript + assertions
```

### Scenario Format

Scenarios are JSON files with steps like `ExpectRequest`, `ExpectNotification`, `AllowRequest`, `AllowNotification`, `SendNotification`, `Delay`, and `ExitNow`. Steps can trigger actions like `RespondResult`, `RespondError`, `SendRequest`, `SendRawBytes`, or grouped-frame/raw-header helpers.

### Binary Discovery

Cargo exposes the test binary to the `egglsp` package integration tests via `CARGO_BIN_EXE_egglsp-test-server`. Root-crate composite tests use `CARGO_BIN_EXE_codegg-lsp-test-server`. The `EGGLSP_TEST_SERVER` env var can override the path for CI or manual runs.

### Test Counts

- **11 production protocol tests** in `tests/production_protocol_stdio.rs` — all passing ✅
- **3 production semantic tests** in `tests/production_semantic_stdio.rs` — all passing ✅
- **5 production service tests** in `tests/production_service_stdio.rs` — all passing ✅
- **20 root composite tests** in `tests/lsp_composite_stdio.rs` — all passing ✅
- **235 unit tests** in the `egglsp` crate
- **3 scenario-engine tests** in `tests/scenario_engine.rs` — wrapper around `crates/egglsp-test-server/tests/scenario_engine.rs` for strict allow-listing, raw bytes, and grouped-frame fixtures

### Test Organization

- `tests/production_protocol_stdio.rs` — Production-harness protocol coverage for launcher-path behavior and transport edge cases
- `tests/production_semantic_stdio.rs` — Production-harness semantic and edit-preview coverage
- `tests/production_service_stdio.rs` — Production-harness LspService lifecycle coverage
- `tests/lsp_composite_stdio.rs` — 21 root-crate composite tests exercising `SemanticContextCollector`, `DiagnosticsCollector`, `LspOperations`, and security context tool orchestration against the fake server via the production `LspClient`/`LspService` stack; includes workspace-edit-preview safety tests (out-of-root, overlapping, command-only, no-edit, ambiguous, resource-operation), semantic-context collector workflow/capability-gating/failure-degradation tests, security context tool tests (orchestration with risk markers, call expansion, cycle suppression, and graceful degradation on call hierarchy error), and hunk-source-context collector test (unified diff with real LSP operations). Path comparison uses `Path::strip_prefix` for path-aware hunk normalization.
- `tests/common/harness.rs` — Reusable fake-server test harness with temp directory and scenario management
- `tests/common/production_harness.rs` — Real-project harness for production launcher-path coverage
- `tests/scenario_engine.rs` — Package-local wrapper around the fake-server self-tests

### Test Coverage Matrix (Phase 2)

| Section | Plan ID | Tests | Status |
|---------|---------|-------|--------|
| Initialization handshake | C1 | `initialization_handshake` | ✅ |
| Server requests during init + dynamic registration | C2 | `server_requests_during_init_and_dynamic_registration` | ✅ |
| Apply-edit refusal | C3 | `apply_edit_refusal_keeps_client_usable` | ✅ |
| Interleaved notifications | C4 | `concurrent_out_of_order_responses_and_notifications` | ✅ |
| Concurrent out-of-order responses | C5 | `concurrent_out_of_order_responses_and_notifications` (captured-ID for genuine out-of-order) | ✅ |
| Diagnostics lifecycle | C6 | `diagnostics_lifecycle_tracks_file_changes` | ✅ |
| Cancellation write failure | C9 | Deterministic unit test in `client.rs` (OS-pipe flake avoided) | ✅ |
| Graceful shutdown | C10 | `server_exit_before_response_and_error_response` | ✅ |
| Ungraceful shutdown / EOF | C11 | `server_exit_before_response_and_error_response` | ✅ |
| Server error response | — | `error_response_is_reported` | ✅ |
| Malformed frames | — | `malformed_frames_fail_transport` (8 cases) | ✅ |
| Unknown frames | — | `unknown_json_rpc_frames_are_ignored` | ✅ |
| Grouped/split writes | — | `grouped_frames_and_split_writes_are_processed` | ✅ |
| Timeout and cancellation | C8 | `request_timeout_and_late_response_are_dropped` | ✅ |
| Document lifecycle | D1 | `typed_semantic_requests_collect_context_and_freshness` | ✅ |
| Hover | D2 | `typed_semantic_requests_collect_context_and_freshness` | ✅ |
| Definition | D2 | `typed_semantic_requests_collect_context_and_freshness` | ✅ |
| References | D2 | `typed_semantic_requests_collect_context_and_freshness` | ✅ |
| Document symbols | D2 | `typed_semantic_requests_collect_context_and_freshness` | ✅ |
| Call hierarchy | D3 | `hierarchy_context_requests_round_trip_through_real_client` (typed `LspClient` methods: `prepare_call_hierarchy`, `incoming_calls`, `outgoing_calls`) | ✅ |
| Type hierarchy | D3 | `hierarchy_context_requests_round_trip_through_real_client` (typed `LspClient` methods: `prepare_type_hierarchy`, `supertypes`, `subtypes`) | ✅ |
| Rename (WorkspaceEdit) | D4 | `edit_round_trips_do_not_mutate_disk` | ✅ |
| Code action (edit-bearing) | D4 | `typed_semantic_requests_collect_context_and_freshness` | ✅ |
| Rename preview (composite) | D5 | `rename_preview_converts_through_production_path` | ✅ | child-process |
| Format preview (composite) | D5 | `format_preview_converts_through_production_path` | ✅ | child-process |
| Source-action preview (composite) | D5 | `code_action_source_action_preview_converts_through_production_path` | ✅ | child-process |
| Preview safety: out-of-root | D5 | `preview_safety_out_of_root_rejected` | ✅ | child-process |
| Preview safety: overlapping | D5 | `preview_safety_overlapping_edits_rejected` | ✅ | child-process |
| Preview safety: command-only | D5 | `preview_safety_command_only_code_action_rejected` | ✅ | local |
| Preview safety: no-edit | D5 | `preview_safety_no_edit_code_action_rejected` | ✅ | local |
| Preview safety: ambiguous | D5 | `preview_safety_ambiguous_source_actions_rejected` | ✅ | local |
| Preview safety: resource operation | D5 | `preview_safety_resource_operation_rejected` | ✅ | local |
| Semantic context composite | D6 | `semantic_context_collector_exercises_real_workflow` | ✅ |
| Security context composite | D6 | `semantic_context_security_review_intent_collects_security_source` (renamed from `security_context_workflow_uses_semantic_collector`) | ✅ |
| Security context tool orchestration | D6 | `security_context_tool_exercises_risk_filtering_and_call_expansion` (exercises real `LspTool::execute("securityContext")` with risk markers, call expansion, cycle suppression) | ✅ |
| Security context: call hierarchy error degradation | D6 | `security_context_tool_degrades_on_call_hierarchy_error` (outgoingCalls error is recorded, packet returned, nodes/evidence preserved) | ✅ |
| Hunk source context composite | D7 | `hunk_source_context_collector_exercises_real_workflow` | ✅ |
| Semantic context: capability gating | D6 | `semantic_context_collector_capability_gating` | ✅ |
| Semantic context: failure degradation | D6 | `semantic_context_collector_failure_degradation` | ✅ |
| LspService single-flight | — | `single_flight_init_uses_a_real_child` | ✅ |
| LspService document lifecycle | — | `document_lifecycle_ownership_tracks_open_update_save_close` | ✅ |
| LspService diagnostics | — | `service_diagnostics_warming_then_populated` | ✅ |
| LspService delayed init shutdown | — | `shutdown_during_delayed_init_cancels_callers` | ✅ |
| LspService in-flight shutdown | — | `shutdown_with_inflight_request_is_bounded` | ✅ |

Phase 2 deliberately skips the following items (deferred to Phase 3 or omitted as nondeterministic at the OS-pipe level):
- **C7** (configuration / dynamic registration with real-server matrix) — deferred to Phase 3
- **C12** (malformed framing byte-level) — covered by `malformed_frames_fail_transport` + unit tests in `writer.rs`
- **C13** (malformed JSON-RPC shapes) — covered by `classify_json_rpc_message` unit tests in `client.rs`
- **C14** (server-response write failure end-to-end) — covered by deterministic writer unit test
- **C15** (stderr drainage) — drain is in `launch::spawn_stderr_drain`; bounded by line cap (not yet a Phase 2 test)

### Running

```bash
# Run Phase 2 integration tests (parallel-safe)
cargo test -p egglsp --test production_protocol_stdio
cargo test -p egglsp --test production_semantic_stdio
cargo test -p egglsp --test production_service_stdio
cargo test -p egglsp --test scenario_engine

# Run root composite tests (semantic/security/hunk collectors + preview safety)
cargo test --test lsp_composite_stdio

# Force single-threaded to validate sequential stability
cargo test -p egglsp --tests -- --test-threads=1
```

Phase 2 tests are parallel-safe (unique tempdir per test, per-process scenario/transcript paths). The harness does not require `--test-threads=1`; that flag was only needed by the pre-Phase-2 test layout.

### Phase 2 Final Closure Notes

- **Hermetic binary strategy**: Root-crate composite tests use `codegg-lsp-test-server` (via `CARGO_BIN_EXE_codegg-lsp-test-server`), while `egglsp`-only integration tests use `egglsp-test-server` (via `CARGO_BIN_EXE_egglsp-test-server`). Both are `[[bin]]` targets from the `egglsp` package, sharing the same source. The root `Cargo.toml` declares a `[[bin]]` target pointing to the shared source.
- **Path-aware hunk normalization**: Hunk path comparison now uses `Path::strip_prefix` instead of string prefix stripping, providing correct cross-platform behavior for paths with different separators.
- **Inspection APIs**: `transport_state_snapshot()` and `pending_request_count()` are observational health APIs for diagnostics; `dynamic_registration_snapshot()` is test-support/internal.

## Phase 3: Real-Server Compatibility Matrix (Opt-in)

Phase 2 gives us confidence in the wire protocol. Phase 3 extends this confidence to real LSP servers — verifying that rust-analyzer, pyright, gopls, clangd, and typescript-language-server all work with the production launcher, frame parser, and request routing.

### Why Deferred

Real-server smoke tests are:
- **Slow** — server startup is 200ms-2s, plus indexing and warm-up
- **Non-hermetic** — require installed binaries or downloaded releases
- **Flaky** — diagnostics can take seconds to arrive, language versions vary
- **Expensive in CI** — minutes of compute for marginal additional coverage

### Target Compatibility Matrix

| Server | Language | Key Operations | Expected Behavior | Known Limitations |
|--------|----------|----------------|-------------------|-------------------|
| **rust-analyzer** | Rust | hover, definition, references, symbols, call hierarchy, rename, code actions, semanticContext, securityContext, hunkSourceContext | Full feature coverage | Initial indexing may be slow on large workspaces; diagnostics may need a warm-up delay |
| **pyright** | Python | hover, definition, references, symbols, rename | Full feature coverage | No `prepareCallHierarchy` (Python doesn't have function-level call hierarchy); `codeAction` limited to pyright's organize imports |
| **typescript-language-server** | TypeScript / JavaScript | hover, definition, references, symbols, rename, code actions | Full feature coverage | `prepareCallHierarchy` may be empty; large workspaces slow |
| **gopls** | Go | hover, definition, references, symbols, rename, code actions | Full feature coverage | Call hierarchy not yet supported by gopls; securityContext will degrade gracefully |
| **clangd** | C / C++ | hover, definition, references, symbols, rename, code actions | Full feature coverage | No call hierarchy; slow on large TUs |

### Test Profile

Real-server tests will be opt-in via a cargo feature flag:

```bash
cargo test -p egglsp --features lsp-real-server
```

Or per-server:

```bash
cargo test -p egglsp --features lsp-real-server-rust-analyzer
```

### Opt-In Mechanics

The real-server tests are **not** in default CI. They will:

1. Spawn the actual server binary (or download if not on PATH).
2. Wait for initialization and a brief warm-up period.
3. Open a small fixture file and exercise the operation under test.
4. Assert on the shape and content of the response.
5. Clean up via `kill_on_drop` and `LspService::shutdown_all()`.

The fixtures are tiny (10-50 lines) and live in `tests/fixtures/real_servers/`. Each test sets a generous timeout (30-60s) and uses a unique scratch directory.

### Open Questions for Phase 3

- Should we record golden-output JSON responses and diff against them, or use shape-only assertions?
- How do we handle servers that send unsolicited `workspace/diagnostic` (pull-mode) in addition to `textDocument/publishDiagnostics` (push-mode)?
- Do we need a separate "warm-up" phase before assertions, or can we infer readiness from the first response?
- Should real-server tests be nightly-only, or part of every PR?

## See Also

- [.opencode/skills/lsp/SKILL.md](../.opencode/skills/lsp/SKILL.md) - LSP skill guide
- [tool.md](tool.md) - LSP tool wrapper
- [plans/lsp_phase1_cleanup_and_phase2_scripted_stdio_harness.md](../plans/lsp_phase1_cleanup_and_phase2_scripted_stdio_harness.md) - Phase 1 + Phase 2 plan
