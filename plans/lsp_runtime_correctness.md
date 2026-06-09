# LSP Runtime Correctness Follow-Up Plan

## Purpose

Finish the LSP integration hardening by correcting the remaining runtime semantics and model-facing rough edges left after `plans/lsp_integration.md`.

The previous pass materially improved the implementation: stderr draining is bounded/backgrounded, the model-facing operation enum is narrower, `diagnostics` exists, line/column conversion exists, and tests cover more of the tool schema and parser behavior. This follow-up should make the LSP layer dependable under real language-server behavior rather than merely clean in static wrapper code.

The main objective is to make `egglsp` behave like a real asynchronous JSON-RPC client:

```text
one background stdout reader
  -> pending request map by JSON-RPC id
  -> notification dispatcher
  -> diagnostics cache updates independent of request timing
```

The model-facing `lsp` tool should then expose only compact, predictable summaries.

## Current State Summary

Relevant files:

```text
crates/egglsp/src/client.rs
crates/egglsp/src/launch.rs
crates/egglsp/src/service.rs
crates/egglsp/src/operations.rs
crates/egglsp/src/diagnostics.rs
src/tool/lsp.rs
src/main.rs
tests/lsp.rs
```

Current good state:

- `LspClient::new` no longer blocks on server stderr.
- `spawn_stderr_drain` logs bounded stderr output in the background.
- `src/tool/lsp.rs` exposes a narrower operation enum.
- `diagnostics` is now a model-facing operation.
- model-facing line and column are documented as 1-indexed.
- `to_lsp_position` converts model-facing positions to LSP positions.
- tests cover schema, validation, indexing, and diagnostic JSON parsing.
- `DoctorSubsystem::Lsp` exists.

Remaining problems:

1. `LspClient::send_request` still owns the stdout read loop, so notifications are only consumed while a request is pending.
2. Diagnostics emitted after `didOpen` or `didChange` can remain unread until some later request happens.
3. There is no pending-request map or background JSON-RPC dispatcher.
4. `workspaceSymbol` and `codeLens` still return raw LSP JSON/structs to the model.
5. `uri_to_path` in `src/tool/lsp.rs` decodes `file://` URIs incorrectly.
6. `workspaceSymbol` without `file_path` is not useful unless a client already exists.
7. `codegg doctor --subsystem lsp` reports exposure using `experimental.lsp_tool`, but actual exposure is also controlled by registry/backend filtering.
8. Tests still do not prove notification dispatch updates diagnostics without a request in flight.

## Non-Goals

Do not implement rename, formatting, code-action application, or automatic `workspace/applyEdit`.

Do not add completion as a model-facing operation.

Do not move LSP into the TUI.

Do not make default tests require installed language servers or network access.

Do not expose an `egglsp` MCP adapter in this pass.

Do not perform a broad config migration. Keep changes additive and focused.

## Phase 1 — Replace Request-Owned Stdout Reads With a Dispatcher

The current `send_request` loop writes a request, then reads stdout until the matching response appears. That is not robust enough for LSP because responses and notifications share the same stream.

Implement a background dispatcher in `crates/egglsp/src/client.rs`.

Recommended structure:

```rust
type PendingRequests = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<serde_json::Value, LspError>>>>>;

pub struct LspClient {
    pub server_id: String,
    pub root: PathBuf,
    stdin: Mutex<tokio::process::ChildStdin>,
    pending: PendingRequests,
    request_id: AtomicU64,
    capabilities: Mutex<Option<ServerCapabilities>>,
    opened_files: Mutex<HashMap<String, i32>>,
    diagnostics: Arc<Mutex<HashMap<String, Vec<lsp_types::Diagnostic>>>>,
    reader_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    child: Mutex<tokio::process::Child>,
}
```

You do not have to use exactly this shape, but separate stdout from request execution. `send_request` should no longer call `launch::read_response` directly.

> **Note (2026-06-09):** `launch::read_response` and `launch::read_notification` have been removed. stdout is exclusively owned by the background reader task; `send_request` only writes to stdin and awaits a oneshot response.

Implementation steps:

1. In `launch.rs`, keep low-level JSON-RPC framing helpers, but make them usable on split stdin/stdout handles.
2. During `LspClient::new`, take stdin/stdout/stderr/child from `LspProcess`.
3. Spawn one stdout reader task per client.
4. The reader task continuously reads framed JSON-RPC messages.
5. For messages with `id`:
   - if `error` is present, send `Err(LspError::RequestFailed(...))` to the matching pending request;
   - otherwise send the `result` value to the matching pending request;
   - if no matching pending request exists, log at debug and drop.
6. For messages with `method` and no `id`, dispatch as notifications.
7. For unknown JSON shapes, log at debug and drop.
8. `send_request` should:
   - allocate id;
   - insert a `oneshot::Sender` into `pending`;
   - write the framed request to stdin;
   - await the receiver with timeout;
   - remove/clean up the pending entry on timeout or write failure.

Important: avoid holding the pending map lock while writing stdin or awaiting the response.

Acceptance criteria:

- There is exactly one stdout reader per LSP client.
- `send_request` does not read stdout directly.
- Notifications are processed even when no request is pending.
- Request timeouts remove their pending entries.
- Dropped/closed stdout causes pending requests to fail instead of hanging forever.

## Phase 2 — Make Notification Dispatch Testable

Notification dispatch needs tests that do not spawn real servers.

Extract a pure or mostly-pure function:

```rust
fn classify_json_rpc_message(value: serde_json::Value) -> JsonRpcMessage
```

Suggested enum:

```rust
enum JsonRpcMessage {
    Response { id: u64, result: serde_json::Value },
    ErrorResponse { id: u64, code: Option<i64>, message: String },
    Notification { method: String, params: serde_json::Value },
    Unknown,
}
```

Also extract:

```rust
async fn dispatch_notification(
    diagnostics: &Mutex<HashMap<String, Vec<lsp_types::Diagnostic>>>,
    method: &str,
    params: serde_json::Value,
)
```

or an equivalent non-async parse helper plus async cache update.

Tests should cover:

- response with matching id shape;
- error response shape;
- `textDocument/publishDiagnostics` notification;
- unknown notification;
- malformed/non-object JSON;
- response with string id if the crate supports only numeric ids: either reject clearly or convert consistently.

Acceptance criteria:

- Parser/dispatcher unit tests require no external server.
- Diagnostics update path is not tied to `send_request`.
- Unknown notifications are ignored without surfacing as tool errors.

## Phase 3 — Fix Diagnostics Freshness After `didOpen` / `didChange`

After the dispatcher exists, diagnostics should arrive independently. The tool still needs honest semantics around warm-up and stale data.

Implementation steps:

1. In `LspService::open_file` and `update_file`, continue sending `didOpen`/`didChange` as notifications.
2. Track opened-file versions and optionally a `last_opened_at` or `last_changed_at` timestamp per URI.
3. In `DiagnosticsCollector::get_diagnostics_for_file`, ensure the file is opened before reading diagnostics if it is not already open.
4. Return metadata indicating whether diagnostics may still be warming if the file was opened very recently and no diagnostics have been received.

Suggested DTO internal to tool output:

```rust
struct DiagnosticsOutput {
    diagnostics_may_still_be_warming: bool,
    diagnostics: Vec<DiagnosticSummary>,
}
```

This can be the `results` payload for the `diagnostics` operation.

Acceptance criteria:

- `lsp diagnostics` does not imply “clean” immediately after opening a file.
- Empty diagnostics can be distinguished from “server may not have responded yet.”
- Existing compact diagnostic summary remains 1-indexed.

## Phase 4 — Fix URI Decoding

`src/tool/lsp.rs::uri_to_path` currently treats a URI string like a filesystem path. Replace it with URL-aware decoding.

Implementation:

```rust
fn uri_to_path(uri: &crate::lsp::lsp_types::Uri) -> String {
    let raw = uri.to_string();
    url::Url::parse(&raw)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(raw)
}
```

If importing `url` directly in `src/tool/lsp.rs` is undesirable, expose a helper from `egglsp` or `src/lsp/mod.rs`.

Add tests for:

```text
file:///tmp/a%20b.rs -> /tmp/a b.rs
non-file URI remains raw
```

Acceptance criteria:

- File URIs decode correctly.
- Percent-encoded paths decode correctly.
- Non-file URIs do not panic.

## Phase 5 — Summarize `workspaceSymbol`

`workspaceSymbol` currently returns raw JSON. Convert it to compact summaries.

Recommended model-facing DTO:

```rust
struct WorkspaceSymbolSummary {
    name: String,
    kind: String,
    file: Option<String>,
    start_line: Option<u32>,
    start_column: Option<u32>,
    container_name: Option<String>,
}
```

Handle both LSP response forms:

```rust
Vec<SymbolInformation>
Vec<WorkspaceSymbol>
```

LSP servers differ here; parse permissively. If parsing one form fails, try the other. Cap results with `MAX_SYMBOLS`.

Routing rule:

- Prefer `file_path` when provided, resolving the client through `get_or_create_client_for_file`.
- If `file_path` is absent and exactly one client exists, use that client.
- If no client exists, return a clear error: `workspaceSymbol requires file_path until an LSP client has been initialized`.
- If multiple clients exist, return a clear disambiguation error.

Acceptance criteria:

- `workspaceSymbol` no longer returns raw JSON.
- Missing `file_path` behavior is deterministic and documented.
- Result count and truncation are accurate.

## Phase 6 — Either Summarize or Hide `codeLens`

`codeLens` still returns raw `CodeLens` values. Either summarize it or remove it from the model-facing schema.

Preferred option: hide it for now.

Rationale: code lenses are usually editor UI affordances, and many contain command objects that are not useful unless Codegg supports preview-only command/action semantics.

If keeping it, use a compact DTO:

```rust
struct CodeLensSummary {
    file: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    title: Option<String>,
    command: Option<String>,
}
```

Do not return raw command arguments.

Acceptance criteria for preferred option:

- `codeLens` is removed from the model-facing enum and description.
- Internal `egglsp::operations::code_lens` may remain for future use.
- Tests are updated to reflect the narrower schema.

Acceptance criteria if summarized:

- No raw command arguments are returned.
- Output is capped and summary-shaped.

## Phase 7 — Correct Doctor’s Model-Exposure Report

`codegg doctor --subsystem lsp` currently reports exposure based mostly on `experimental.lsp_tool`. Actual model exposure depends on multiple gates:

- registry backend config (`[tool_backends.lsp]`);
- hidden disabled stubs via `Tool::expose_in_definitions()`;
- `experimental.lsp_tool` passed into `filter_tools_for_model`;
- model/profile disabled tools;
- plan mode filtering if applicable.

Doctor should not pretend to fully know per-session model/profile filtering unless it actually builds the same definitions. For a static doctor report, split the output into two lines:

```text
registry: lsp registered as native / disabled / fallback-native / unavailable
agent exposure gate: experimental.lsp_tool = true|false
model tool: exposed only when registry-visible and agent exposure gate allows it
```

Implementation steps:

1. Build a `ToolRegistry::with_config(&config)` or equivalent lightweight registry.
2. Inspect `registry.backend_report(None)` for the `lsp` row.
3. Inspect `registry.definitions()` for whether `lsp` is registry-visible.
4. Print `experimental.lsp_tool` separately.
5. Avoid starting any LSP server.

Acceptance criteria:

- Doctor distinguishes backend registration from agent exposure gating.
- Doctor does not claim the model sees `lsp` unless the same static gates allow it.
- Doctor remains non-mutating.

## Phase 8 — Tighten Service Client Resolution

`get_or_create_client_for_root_hint` currently only finds existing clients. That is acceptable if documented, but the name implies creation.

Choose one of two fixes.

Option A, rename and document:

```rust
find_existing_client_for_root_hint(...)
```

Use it only for operations that can work with an existing initialized client.

Option B, implement actual creation:

```rust
get_or_create_client_for_root_hint(root_hint, server_id)
```

If `server_id` is present, find the server definition and initialize it at the root. If `server_id` is absent and no client exists, return a clear error requiring `file_path` or `server_id`. Do not guess a language server from the current directory alone.

Preferred for this pass: Option A unless `workspaceSymbol` genuinely needs server-id support.

Acceptance criteria:

- Method names match behavior.
- No operation silently depends on a misleading “get_or_create” that does not create.
- Error messages tell the model what input to provide.

## Phase 9 — Add Dispatcher-Level Tests

Add tests for the new dispatcher without a real language server.

Possible approaches:

1. Unit-test parser/classifier/dispatcher helpers directly.
2. Create an in-memory framed-message reader/writer harness using Tokio duplex streams.
3. Use a tiny test child process only in an opt-in integration test.

Default required tests:

```text
crates/egglsp:
  classify_response_message
  classify_error_response_message
  classify_publish_diagnostics_notification
  dispatch_publish_diagnostics_updates_cache
  unknown_notification_ignored
  timeout_removes_pending_request  # if easy without real process

codegg:
  lsp_schema_excludes_codeLens_if hidden
  workspaceSymbol_summary_shape if operation remains exposed
  uri_to_path_decodes_percent_encoded_file_uri
  doctor_lsp_reports_backend_and_exposure_gate separately
```

Optional integration test:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test -p egglsp real_lsp -- --nocapture
```

The optional test may require `rust-analyzer` in PATH and should skip cleanly if absent.

Acceptance criteria:

- `cargo test --workspace` remains hermetic.
- Dispatcher behavior is tested without a real LSP server.
- Schema tests match the final exposed operation set.

## Phase 10 — Documentation Updates

Update docs after code changes.

Files to check:

```text
architecture/native_crates.md
architecture/tool.md
AGENTS.md
plans/lsp_integration.md
crates/egglsp/src/client.rs
src/tool/lsp.rs
```

Docs should state:

- `egglsp` uses a background stdout dispatcher.
- diagnostics are notification-driven and may have warm-up latency.
- `workspaceSymbol` requires a disambiguating `file_path` unless exactly one client is already active.
- `codeLens` is hidden or summarized, depending on chosen implementation.
- doctor reports registry/backend state separately from agent exposure gates.

Acceptance criteria:

- Docs no longer describe request-owned stdout reads.
- Docs match the final schema.
- Docs do not imply LSP diagnostics replace build/test/static analysis.

## Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted commands:

```bash
cargo test -p egglsp
cargo test --test lsp
cargo test --test tool_registry
cargo test --test tool_structured_execution
codegg doctor --subsystem lsp
```

Optional real-server smoke:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test -p egglsp real_lsp -- --nocapture
```

## Done Criteria

This pass is complete when:

- LSP stdout is read by a background dispatcher, not by each request.
- Notifications update diagnostics without requiring another request to be pending.
- Diagnostics output honestly reports warm-up/staleness ambiguity.
- File URI decoding is URL-aware and tested.
- `workspaceSymbol` returns compact summaries, not raw JSON.
- `codeLens` is either hidden or compactly summarized without raw command arguments.
- Doctor reports backend registration and exposure gates separately.
- Client-resolution helper names match their behavior.
- Default tests remain hermetic.

## Suggested Next Passes

After this pass, the next useful LSP plans are:

1. preview-only rename and formatting via `WorkspaceEdit` summaries;
2. overlay sync for proposed edits before filesystem commit;
3. call hierarchy with the real prepare/select/incoming/outgoing flow;
4. model-profile-aware semantic context packets;
5. optional `egglsp` MCP adapter hidden behind the native `lsp` wrapper by default.
