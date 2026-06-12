---
name: lsp
description: LSP client-side integration for Language Server Protocol support
version: 1.1.0
tags:
  - lsp
  - language-server
  - diagnostics
  - code-intelligence
  - client-side
---

# LSP (Language Server Protocol) Guide

This skill covers the LSP module for language server integration in opencode-rs.

## Overview

The LSP implementation lives in the `egglsp` workspace crate (`crates/egglsp/`). `src/lsp/mod.rs` is a thin compatibility shim that re-exports `egglsp::*` and bridges config/error types. The model-facing tool is at `src/tool/lsp.rs`.

LSP is exposed as a native tool via `LspTool`, returning compact agent-facing summaries (not raw LSP JSON). Model-facing line and column are 1-indexed; the wrapper converts to LSP 0-indexed.

## Directory Structure

```
crates/egglsp/src/          # Authoritative LSP implementation
├── client.rs               # LspClient - JSON-RPC, diagnostics cache, notification parser
├── config.rs               # LspConfig, LspRule types
├── diagnostics.rs          # DiagnosticsCollector
├── edit.rs               # Workspace edit preview, text edit application, unified diff generation
├── download.rs             # Binary download/cache
├── error.rs                # LspError
├── language.rs             # Language detection from file extensions
├── launch.rs               # Process spawning, Content-Length framing, background stderr drain
├── operations.rs           # LspOperations - goto definition, hover, etc.
├── overlay.rs              # OverlaySession, OverlayRestoreToken, semantic check preview (content or patch)
├── root.rs                 # Project root detection
├── server.rs               # 39 server definitions
├── service.rs              # LspService - client management, file-based routing

src/lsp/mod.rs              # Thin re-export shim (compatibility only)
src/tool/lsp.rs             # Model-facing LSP tool with compact DTOs
```

## Key Types

### Lsp (`mod.rs`)

Main entry point combining service, operations, and diagnostics:

```rust
pub struct Lsp {
    pub service: Arc<LspService>,
    pub operations: Arc<LspOperations>,
    pub diagnostics: Arc<DiagnosticsCollector>,
}
```

### LspClient (`client.rs`)

JSON-RPC client managing LSP server process. Uses a background reader
task for message dispatch (no more request-owned reads):

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
```

The `pending` map routes response IDs to oneshot senders. The
`_reader_task` continuously reads framed JSON-RPC messages from stdout
and classifies them via `classify_json_rpc_message`. Responses are
routed to pending senders; notifications are dispatched via
`dispatch_notification`.

**Request ID Generation:**
- Uses `AtomicU64` for wrap-around safety (was `AtomicI64`)
- `fetch_add(1, Ordering::SeqCst)` for sequential IDs
- No special wrap-around check needed with unsigned integer

### Edit Preview Types (`edit.rs`)

```rust
pub struct WorkspaceEditPreview {
    pub title: String,
    pub files: Vec<FileEditPreview>,
    pub total_files: usize,
    pub total_edits: usize,
    pub truncated: bool,
}

pub struct FileEditPreview {
    pub file: PathBuf,
    pub original_hash: String,
    pub edits: Vec<TextEditPreview>,
    pub patch: String,
    pub patch_omitted: bool,
}

pub struct TextEditPreview {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub replacement_preview: String,
}
```

These types are re-exported from `egglsp` at the crate root (e.g. `egglsp::WorkspaceEditPreview`).

### LspServerDef (`server.rs`)

Server definition with 39 server implementations:

```rust
pub struct LspServerDef {
    pub id: &'static str,           # e.g., "rust-analyzer"
    pub languages: &'static [&'static str],
    pub extensions: &'static [&'static str],
    pub repo: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub download: Option<DownloadSpec>,
}
```

## Supported LSP Servers

| Language | Server ID | Command |
|----------|-----------|---------|
| Rust | `rust-analyzer` | `rust-analyzer` |
| Python | `pyright` | `pyright-langserver --stdio` |
| JS/TS | `typescript-language-server` | `typescript-language-server --stdio` |
| Go | `gopls` | `gopls` |
| C/C++ | `clangd` | `clangd` |
| Java | `jdtls` | `jdtls` |
| C# | `omnisharp` | `OmniSharp` |
| Ruby | `ruby-lsp` | `ruby-lsp` |
| Kotlin | `kotlin-language-server` | `kotlin-language-server` |
| Scala | `metals` | `metals` |
| Dart | `dart-analysis-server` | `dart language-server --client-id codegg` |
| Swift | `swift-sourcekit` | `sourcekit-lsp` |
| Haskell | `haskell-language-server` | `haskell-language-server-wrapper --lsp` |
| Lua | `lua-language-server` | `lua-language-server` |
| PHP | `php-language-server` | `php-language-server` |
| Perl/Raku | `perl-language-server` | `perl-language-server` |
| Zig | `zls` | `zls` |
| ... and more | | |

## Key Operations

### File Lifecycle

```rust
// Open file
lsp.service.open_file(path, content).await

// Update file content
lsp.service.update_file(path, content).await

// Save file
lsp.service.save_file(path, None).await

// Close file
lsp.service.close_file(path).await
```

### Code Intelligence

```rust
// Goto definition
let locations = lsp.operations.go_to_definition(file_path, line, column).await

// Find references
let refs = lsp.operations.find_references(file_path, line, column).await

// Hover
let hover = lsp.operations.hover(file_path, line, column).await

// Document symbols
let symbols = lsp.operations.document_symbols(file_path).await

// Code actions
let actions = lsp.operations.code_actions(file_path, start_line, start_col, end_line, end_col, Vec::new(), None).await

// Completion
let completions = lsp.operations.completion(file_path, line, column, None, None).await

// Signature help
let sig = lsp.operations.signature_help(file_path, line, column).await

// Preview-only rename (returns WorkspaceEditPreview with unified diff patches; does not write)
let preview = lsp.operations.rename_preview(file_path, line, column, "new_name", Some(allowed_root)).await

// Preview-only format
let preview = lsp.operations.format_preview(file_path, Some(allowed_root)).await

// Preview-only source action (organize imports)
use egglsp::operations::SourceActionPreviewKind;
let kind = SourceActionPreviewKind::parse("source.organizeImports")?;
let preview = lsp.operations.source_action_preview(file_path, kind, Some(allowed_root)).await
```

## Tool Integration

LSP is exposed via `LspTool` in `src/tool/lsp.rs`:

```rust
pub struct LspTool {
    service: Arc<crate::lsp::service::LspService>,
    allowed_root: PathBuf,
}
```

Operations available via tool:
- `goToDefinition`
- `findReferences`
- `hover`
- `documentSymbol`
- `workspaceSymbol` (returns `WorkspaceSymbolSummary` with name, kind, file, start_line, start_column, container_name)
- `diagnostics` (returns `diagnostics_may_still_be_warming: bool` to indicate if the server may not have responded yet after a recent `didOpen`/`didChange`)
- `renamePreview` (preview-only; returns `WorkspaceEditPreview` {title, files:[{file, original_hash, edits, patch}], total_*, truncated}; never mutates)
- `formatPreview` (preview-only; same `WorkspaceEditPreview` shape)
- `sourceActionPreview` (preview-only; same `WorkspaceEditPreview` shape; accepts `action` parameter — currently only `source.organizeImports` with aliases `organizeImports`/`organize_imports`; command-only actions are rejected because command execution is disabled)
- `semanticCheckPreview` (accepts either `content` or a single-file unified diff `patch`; patch input is applied in memory against `file_path` via `OverlaySession` (`apply_overlay`/`restore`), collects diagnostics + symbols, restores disk content, never writes disk; multi-file patches are unsupported in this pass; operation-level root enforcement via `allowed_root`; returns `SemanticCheckPreview` with `diagnostics_may_still_be_warming`, `diagnostics`, `diagnostics_error`, `symbols`, `symbols_error`, `restored_disk_view`, `restore_error`; `execute_structured` sets `success=false` when `restore_error` is present)
- `semanticContext` (combines multiple LSP requests; returns `SemanticContextPacket` with bounded source excerpt + diagnostics + symbols + optional definitions/references/overlay + optional source-action hints + optional call/type hierarchy; read-only, bounded; per-section errors via `definitions_error`, `references_error`; overlay limits tracked by `overlay_diagnostics_truncated`; `result_count` includes overlay items and available source-action hints; source excerpt truncation is UTF-8-safe via char-boundary cutting; `include_source_actions` boolean input, default false, populates `source_actions` array of `SemanticSourceActionHint` objects; `include_call_hierarchy` boolean input, default false, populates `call_hierarchy` section with incoming/outgoing callers; `include_type_hierarchy` boolean input, default false, populates `type_hierarchy` section with supertypes/subtypes)
- `callHierarchy` (requires file_path, line, column; optional `direction` parameter — `incoming`, `outgoing`, or `both` (default `both`); returns `CallHierarchySummary` with items, incoming, outgoing, errors, truncated)
- `typeHierarchy` (requires file_path, line, column; optional `direction` parameter; returns `TypeHierarchySummary` with items, supertypes, subtypes, errors, truncated)
- `securityContext` (security-review context packet; returns risk markers, security-relevant diagnostics/symbols, optional definitions/references/call hierarchy, optional overlay; read-only, bounded; accepts `security_categories` filter and `max_risk_markers` cap; `include_call_hierarchy` defaults true when position provided)

**Preview-only contract**: `renamePreview` / `formatPreview` / `sourceActionPreview` (and future edit previews) produce bounded unified-diff patches for review via `WorkspaceEditPreview`. `sourceActionPreview` currently supports only `source.organizeImports`; arbitrary code actions and command execution are intentionally rejected. `CodeAction` values with `command: Some(_)` but `edit: None` are classified as command-only and rejected. `format_preview` enforces `allowed_root` at the crate layer. Large patches are structurally flagged via `FileEditPreview.patch_omitted` (not string matching). They are `ToolCategory::ReadOnly`. Actual file changes require the separate mutating `apply_patch` tool (or equivalent). `codeLens` is not exposed in the model-facing schema. Source-action hints returned via `semanticContext` with `include_source_actions: true` follow the same preview-only contract — each hint's `preview` field carries a `WorkspaceEditPreview` when the action is available and has edits, or `None` when unavailable or command-only.

### Semantic context packets

`semanticContext` is the preferred agent-facing pre-edit/pre-review context operation. It combines a bounded source excerpt with current diagnostics, document symbols, optional definition/reference information, and optional overlay diagnostics for proposed content or a single-file patch. It is read-only and never applies changes.

Input parameters:
- `file_path` (required)
- `line`, `column` (optional, both-or-neither): 1-indexed target position
- `radius` (optional, default 40, max 120): lines above/below for excerpt
- `include_references` / `include_definitions` / `include_overlay` / `include_source_actions` (optional booleans)
- `include_call_hierarchy` (optional, default false): include call hierarchy information (requires line+column); requests without a target position are rejected rather than silently omitted
- `include_type_hierarchy` (optional, default false): include type hierarchy information (requires line+column); requests without a target position are rejected rather than silently omitted
- `content` / `patch` (optional, mutually exclusive): for overlay diagnostics

Source-action hints: when `include_source_actions` is true, `semanticContext` includes a `source_actions` array of `SemanticSourceActionHint` objects. Each hint has `action` (string identifier), `available` (bool), `preview` (Option\<WorkspaceEditPreview\>), and `error` (Option\<String\>). Currently only `source.organizeImports` is supported. Hints reuse the existing `sourceActionPreview` behavior (preview-only, no command execution, no mutation). Source-action failures are non-fatal; they set `error` on the individual hint but do not fail the whole packet. Available hints affect `result_count`. A pure helper `source_action_hint_from_result` converts results to hints, and `collect_source_action_hints` iterates the hardcoded allowlist.

All sections bounded: diagnostics (100), symbols (120), references (80), overlay diagnostics (100), excerpt (32KB). Per-section errors (`definitions_error`, `references_error`) are non-None when the corresponding LSP request fails. `overlay_diagnostics_truncated` in limits tracks overlay diagnostics overflow. `result_count` includes overlay diagnostics and overlay symbols. Source excerpt truncation uses `truncate_to_byte_limit_on_char_boundary` (UTF-8-safe, no replacement characters). All sections are best-effort; individual failures do not prevent the packet from being returned.

### securityContext operation

`securityContext` is a read-only context-gathering operation for security review. It is not a vulnerability scanner and does not produce vulnerability verdicts.

**Usage guidance:** Use `securityContext` before a security review of a target symbol or proposed patch. Treat risk markers as review prompts, not findings. Use explicit mutating tools only after reviewing returned patches or context.

It provides:

- Bounded source excerpt with configurable radius (default 80, max 200)
- Deterministic risk markers via pattern matching (11 categories: auth, crypto, filesystem, network, process, unsafe, serialization, sql, secrets, path_traversal, concurrency)
- Security-relevant symbols and diagnostics (filtered by keyword matching and proximity to risk markers; filtered before capping so relevant items are not dropped)
- Optional definitions, references, call hierarchy, and overlay diagnostics
- Risk marker category filtering and configurable caps (default 80, max 200)
- Nonfatal error notes when LSP subrequests fail (diagnostics, symbols, definitions, references)

**Key properties:**
- Read-only: never writes files; patch/content input is applied only in memory through the overlay path
- Deterministic: same input produces same output
- Bounded: all sections have configurable caps
- Context, not verdict: provides risk markers with rationale, not vulnerability assessments
- Precise truncation: flags reflect filtered counts, not raw counts

**Limits:** risk markers (default 80, max 200), excerpt radius (default 80, max 200 lines), security diagnostics (80), security symbols (80), references (80).

**Input parameters:** `file_path` (required), `line`/`column` (optional, both required together), `radius` (default 80, max 200), `security_categories` (optional filter), `max_risk_markers` (default 80, max 200), `content`/`patch` (optional overlay, mutually exclusive), `include_call_hierarchy` (default true when position provided).

**Implementation:** Risk marker scanning, pattern tables, and security-relevant filtering helpers live in `src/tool/lsp_security.rs`.

**Security context presets:** `securityContext` supports optional presets via `security_preset`. Presets tune default risk categories, excerpt radius, marker count, and call-hierarchy inclusion. Supported presets: `rust_server`, `rust_cli`, `web_backend`, `dependency_review`, `unsafe_review`. Explicit input fields (`security_categories`, `radius`, `max_risk_markers`, `include_call_hierarchy`) override preset defaults. See `architecture/lsp.md` for the full preset table.

### Security call expansion

`securityContext` supports optional bounded recursive call expansion via `call_depth`:

- `call_depth`: 0 (default/off), 1, or 2. Higher values rejected.
- `max_call_nodes`: 32 (default), max 64. Caps total nodes.
- `call_direction`: `"incoming"`, `"outgoing"`, or `"both"` (default).

Expansion uses BFS with cycle detection (HashSet dedup). Edges to already-seen nodes are preserved. When caps are reached, expansion prefers returning a partial graph with `truncated=true` rather than failing the entire packet. `call_expansion.truncated` is true when nodes, edges, or per-edge ranges are dropped due to configured or internal caps (`capped_call_ranges`, `push_call_expansion_edge`, `push_call_expansion_node`). Errors are nonfatal and collected in `call_expansion.errors`.

Presets do NOT enable expansion. Only explicit `call_depth > 0` activates it.

Read-only: only LSP hierarchy requests, never writes files.

### Security review workflow

The `security-review` agent uses `securityContext` in a structured workflow (`src/security/workflow.rs`):

- **Target discovery**: Changed hunks from git diff, filtered for binary/vendor paths
- **Preset selection**: Per-file heuristics map to the 5 `securityContext` presets
- **Context strategy**: `call_depth=0` by default; escalated to 1 only for high-risk targets (unsafe, network, auth, process)
- **Synthesis rule**: Risk markers are review prompts, not findings. Findings require risk marker + changed code + evidence of flow, or preflight failure.

The workflow is invoked via the `/security-review` slash command or by spawning the `security-review` subagent.

The vertical slice entry point is `plan_security_review_from_diff(diff, repo_root)`. It parses unified diff hunks, applies path exclusions (`vendor/`, `third_party/`, `target/`, `dist/`, `build/`, `node_modules/`, `*.min.js`; notably does NOT exclude `Cargo.toml`, `Cargo.lock`, `build.rs`), selects `securityContext` presets, builds request payloads, converts risk markers to review prompts, and assembles reports with an explicit "not confirmed findings" note. In this pass, `call_depth` is always 0 and findings are always empty — risk markers become review prompts only.

### Hierarchy Output Shapes

Hierarchy operations (`callHierarchy`, `typeHierarchy`) follow a consistent shape. Both require `file_path`, `line`, and `column` (1-indexed). An optional `direction` parameter controls which callsites/type sites to retrieve.

**`HierarchyDirection`** accepts:
- `"incoming"` — callers / supertypes only
- `"outgoing"` — callees / subtypes only
- `"both"` (default) — both directions

Invalid values return an error.

Hierarchy operations are shallow and non-recursive — they prepare the target item and request only immediate relationships. Unsupported language servers may return empty sections or error fields. Prepare operations open/sync the file from disk before requesting.

#### CallHierarchySummary

Returned by `callHierarchy` and optionally embedded in `semanticContext` when `include_call_hierarchy` is true.

```json
{
  "items": ["CallHierarchyItemSummary", "..."],
  "incoming": ["CallHierarchyIncomingCallSummary", "..."],
  "outgoing": ["CallHierarchyOutgoingCallSummary", "..."],
  "errors": ["error string", "..."],
  "truncated": false
}
```

Items are the prepared call hierarchy symbols at the given position. Incoming/outgoing calls reference those items by ID. Each item summary includes `name`, `kind`, `file_path`, `start_line`, `start_column`, `end_line`, `end_column`. Each incoming/outgoing summary includes `from`/`to` (item summary) and `from_ranges`/`to_ranges` (list of `LocationSummary`).

#### TypeHierarchySummary

Returned by `typeHierarchy` and optionally embedded in `semanticContext` when `include_type_hierarchy` is true.

```json
{
  "items": ["TypeHierarchyItemSummary", "..."],
  "supertypes": ["TypeHierarchyItemSummary", "..."],
  "subtypes": ["TypeHierarchyItemSummary", "..."],
  "errors": ["error string", "..."],
  "truncated": false
}
```

Items are the prepared type hierarchy symbols at the given position. Supertypes/subtypes are flattened lists of all ancestors/descendants. Each item summary includes `name`, `kind`, `file_path`, `start_line`, `start_column`, `end_line`, `end_column`, `parents` (list of parent item summaries).

### Hierarchy behavior

`callHierarchy` and `typeHierarchy` are shallow, non-recursive operations. They prepare the target item and request immediate relationships only. `from_ranges` are capped at 32 per call; the `truncated` flag accounts for item, edge, and range truncation.

Unsupported language servers return empty sections or per-section error fields.

## Project Root Detection

The module detects project roots by looking for marker files:
- `.git`, `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`
- `build.gradle`, `CMakeLists.txt`, `Makefile`, `Gemfile`, `mix.exs`
- `tsconfig.json`, `vite.config.ts`, `next.config.js`, etc.

## Binary Download/Caching

1. Checks PATH first for server binaries
2. Falls back to cached download in `$HOME/.cache/codegg/lsp/`
3. Only rust-analyzer has download specification currently
4. Supports zip, tar.gz, tar.xz extraction
5. Sets executable permissions on Unix (0o755)

## Bug Fixes Applied (2026-05-22)

### PATH Parsing Fixed (`download.rs`)

```rust
// ❌ Before - broken on Unix (split by wrong separator)
for dir in paths.split(std::path::MAIN_SEPARATOR) { ... }

// ✅ After - uses std::env::split_paths correctly
let path_var = std::env::var("PATH").ok()?;
let paths = std::env::split_paths(&path_var);
for dir in paths { ... }
```

### PHP Server Mapping Fixed (`language.rs`)

```rust
// ❌ Before - intelephense doesn't exist in server definitions
"php" => Some("intelephense"),

// ✅ After - correct server ID
"php" => Some("php-language-server"),
```

### Request Timeout Added (`client.rs`)

```rust
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub async fn send_request(...) -> Result<...> {
    // ... request setup ...
    let result = tokio::time::timeout(Self::REQUEST_TIMEOUT, async {
        // ... read loop ...
    }).await;

    match result {
        Ok(inner) => inner,
        Err(_) => Err(LspError::RequestTimeout(...)),
    }
}
```

### Hardcoded PATH Fixed (`launch.rs`)

```rust
// ❌ Before - hardcoded PATH ignored user's environment
.env_clear()
.env("PATH", "/usr/local/bin:/usr/bin:/bin")

// ✅ After - preserves user's PATH if available
.env_clear()
if let Some(user_path) = std::env::var_os("PATH") {
    cmd.env("PATH", user_path);
} else {
    cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
}
```

### Stderr Logging (`client.rs`)

Server stderr is now logged during initialization:

```rust
let mut process = launch::spawn_server(...).await?;
let stderr_output = launch::drain_stderr(&mut process).await;
if !stderr_output.is_empty() {
    info!(server = server.id, stderr = %stderr_output, "LSP server stderr");
}
```

## Additional Bug Fixes (2026-05-22 - Session Review)

### Notification Loop Redundancy Fixed (`client.rs`)

The `send_request` method had redundant notification handling:

```rust
// ❌ Before - duplicate branches, silent ignore on send failure
if let Some(resp_id) = resp.get("id") {
    if resp_id.as_i64() == Some(id) { ... }
    let _ = self.notif_tx.send(resp_str);  // Always runs after match
} else {
    let _ = self.notif_tx.send(resp_str);  // Duplicate branch
}

// ✅ After - cleaner logic, logged send failures
if let Some(resp_id) = resp.get("id") {
    if resp_id.as_i64() == Some(id) { ... }
}
if let Err(e) = self.notif_tx.send(resp_str) {
    warn!(error = %e, "failed to send notification to channel");
}
```

### close_file Race Condition Fixed (`service.rs`)

The `close_file` method had lock handling issues that could cause race conditions:

```rust
// ❌ Before - dropped read lock before acquiring write lock (race!)
let clients = self.clients.read().await;
let key = { /* find key */ };
drop(clients);  // Lock dropped here
if let Some(key) = key {
    let mut clients = self.clients.write().await;  // Another task could modify between
    // ...
}

// ✅ After - uses single write lock, removes from opened_files
let client_idx = {
    let clients = self.clients.read().await;
    // find client index
};
// ...
let mut clients = self.clients.write().await;
if let Some(entry) = clients.values_mut().nth(client_idx) {
    let was_open = entry.client.opened_files.lock().await.contains_key(&uri_str);
    if was_open {
        let _ = entry.client.close_file(&uri).await;
        entry.client.opened_files.lock().await.remove(&uri_str);
    }
}
```

### save_file Race Condition Fixed (`service.rs`)

Similar fix for `save_file`:

```rust
// ❌ Before - dropped read lock before acquiring write lock
let clients = self.clients.read().await;
let key = { /* find key */ };
drop(clients);
if let Some(key) = key {
    let mut clients = self.clients.write().await;
    // ...
}

// ✅ After - uses single write lock
let client_idx = {
    let clients = self.clients.read().await;
    // find client index
};
// ...
let mut clients = self.clients.write().await;
if let Some(entry) = clients.values_mut().nth(client_idx) {
    return entry.client.save_file(&uri, text).await;
}
```

## Error Handling

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

## Capability Discovery

`egglsp::capability` provides a normalized boolean view of `ServerCapabilities` returned by the initialized LSP server.

### LspCapabilitySnapshot

```rust
pub struct LspCapabilitySnapshot {
    // Boolean flags for common operations
    pub goto_definition: bool,
    pub hover: bool,
    pub completion: bool,
    pub references: bool,
    pub document_symbols: bool,
    pub workspace_symbols: bool,
    pub rename: bool,
    pub code_actions: bool,
    pub signature_help: bool,
    pub formatting: bool,
    pub call_hierarchy: bool,
    pub type_hierarchy: bool,
    // ... additional flags
}
```

Built via `LspCapabilitySnapshot::from_capabilities(&ServerCapabilities)` which derives the snapshot from live server capabilities reported during `initialize`.

### Querying Support

- `snapshot.supports(LspSemanticOperation::GotoDefinition)` → `bool`
- `snapshot.fallback_reason(LspSemanticOperation::Rename)` → `Option<&'static str>` — returns `Some("server does not support rename")` when unsupported, `None` when supported

### LspSemanticOperation

Enum covering all semantic operations the tool supports. Used for querying capability snapshots and for building fallback responses.

### LspUnavailable

Structured fallback response returned when an operation is not supported by the server. Constructed via `LspCapabilitySnapshot::unavailable(op)`.

### capabilities LspTool Operation

The `capabilities` operation on `LspTool` returns a `LspCapabilitySnapshot` for the active server. Callers can use it to decide whether to attempt an operation before investing in a full request.

## Diagnostics Freshness

`egglsp::diagnostics` provides diagnostics with freshness metadata so callers can judge reliability.

### LspDiagnosticSnapshot

```rust
pub struct LspDiagnosticSnapshot {
    pub file_path: String,
    pub freshness: LspDiagnosticFreshness,
    pub source: LspDiagnosticSource,
    pub diagnostics: Vec<lsp_types::Diagnostic>,
}
```

### LspDiagnosticFreshness

| Variant | Meaning |
|---------|---------|
| `Fresh` | Diagnostics arrived after the most recent `didOpen`/`didChange`/`didSave` |
| `PossiblyStale` | No response received yet; server may still be processing |
| `Stale` | File was modified after diagnostics were last received |
| `Unavailable` | No diagnostics are available (server not started, no `publishDiagnostics` received) |

### LspDiagnosticSource

| Variant | Meaning |
|---------|---------|
| `Pushed` | Received via `textDocument/publishDiagnostics` notification |
| `Pulled` | Retrieved via `textDocument/diagnostic` request |
| `Unknown` | Source not tracked |

### Usability

- `snapshot.is_usable_evidence()` → `true` for `Fresh` and `PossiblyStale` (callers may choose to treat `PossiblyStale` as usable with a warning)
- `Stale` and `Unavailable` are explicitly flagged so callers can decide whether to re-request or skip

### Invalidation Rules

- A `didOpen` or `didChange` resets the freshness to `PossiblyStale` until the next `publishDiagnostics`
- A `didSave` resets freshness; the next `publishDiagnostics` marks it `Fresh`
- File modifications tracked via `last_opened_at` timestamps drive the `Stale` classification
- The `diagnostics_may_still_be_warming` flag on the `diagnostics` tool operation is derived from `PossiblyStale` freshness

## Shared Semantic Context API

`egglsp::semantic_context` provides request/response types shared across multiple tool operations that gather context for a file position.

### SemanticContextRequest / SemanticContextResponse

```rust
pub struct SemanticContextRequest {
    pub file_path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub intent: SemanticContextIntent,
    pub radius: Option<u32>,
    pub caps: SemanticContextCaps,
    // ... additional fields
}

pub struct SemanticContextResponse {
    pub excerpt: Option<String>,
    pub diagnostics: Vec<lsp_types::Diagnostic>,
    pub symbols: Vec<DocumentSymbol>,
    pub definitions: Option<Vec<Location>>,
    pub references: Option<Vec<Location>>,
    // ... additional sections
}
```

### SemanticContextIntent

| Variant | Usage |
|---------|-------|
| `Explain` | General code explanation; fetch hover, definitions, references |
| `EditPlanning` | Pre-edit context; diagnostics, symbols, definitions, references |
| `Review` | Code review context; diagnostics, symbols, call/type hierarchy |
| `SecurityReview` | Security review; risk markers, security diagnostics, call hierarchy |
| `TestPlanning` | Test generation context; symbols, definitions, references |
| `Navigation` | Code navigation; definitions, references, symbols |

The intent drives which optional sections are populated and which caps are applied.

### SemanticContextCaps

```rust
pub struct SemanticContextCaps {
    pub max_diagnostics: usize,
    pub max_symbols: usize,
    pub max_references: usize,
    pub max_definitions: usize,
    pub max_excerpt_lines: u32,
    // ... additional caps
}
```

Enforces bounded output. Defaults are conservative and aligned with the existing `semanticContext` operation limits.

### Unavailable Responses

`LspCapabilitySnapshot::unavailable(op)` builds a structured fallback for unsupported operations. Used internally when a requested semantic context operation cannot be served because the server lacks the required capability.

## Architecture Notes

### Client-Per-Root Pattern

`LspService` maintains a `HashMap<String, ClientEntry>` where the key is `"{project_root}:{server_id}"`. This means one LSP client per project root per language.

### Content-Length Framing

LSP messages use Content-Length headers for framing:
```
Content-Length: <bytes>\r\n\r\n<json payload>
```

### Notification Handling

Server→client notifications (like `textDocument/publishDiagnostics`) are:

1. Read by the background `_reader_task` from stdout
2. Classified via `classify_json_rpc_message` into `JsonRpcMessage::Notification`
3. Dispatched via `dispatch_notification` which updates the shared `diagnostics` map
4. Diagnostics are now updated independently of pending requests (no more "diagnostics only consumed while request is pending")

### Background Dispatcher Architecture

The background reader task is spawned during `LspClient::new()`. It:

- Continuously reads Content-Length framed JSON-RPC messages from stdout
- Classifies each message via `classify_json_rpc_message` (Response, ErrorResponse, Notification, Unknown)
- Routes responses to pending oneshot senders via the `pending` map
- Dispatches notifications via `dispatch_notification` (currently handles `textDocument/publishDiagnostics`)
- Diagnostics freshness is tracked via `last_opened_at` timestamps; the `diagnostics` operation reports `diagnostics_may_still_be_warming` when a file was recently opened or changed

Key helper functions (exported from `client.rs`):
- `classify_json_rpc_message(value) -> JsonRpcMessage`
- `dispatch_notification(diagnostics, method, params)`
- `url_to_uri(url) -> Uri`

## See Also

- [tool.md](tool.md) - LSP tool wrapper
- [architecture/lsp.md](../../architecture/lsp.md) - Architecture documentation
