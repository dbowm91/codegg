# LSP Integration Hardening and Execution Plan

## Purpose

Make Codegg's existing LSP integration reliable enough to use as a normal agentic code-intelligence layer.

This is not a greenfield LSP plan. The repository already has a workspace crate at `crates/egglsp`, a compatibility shim at `src/lsp/mod.rs`, and a model-facing native `lsp` tool at `src/tool/lsp.rs`. The goal of this pass is to harden the current implementation, reduce model-facing noise, fix correctness hazards, and add enough diagnostics/tests that future work can build on it safely.

The target architecture remains:

```text
model / agent loop
  -> native tool registry
  -> src/tool/lsp.rs wrapper
  -> egglsp service / operations / diagnostics
  -> external language server process
```

The TUI should not own or speak raw LSP. LSP remains core/tool-owned and frontend-neutral.

## Current State Summary

Codegg already has the right broad shape:

```text
crates/egglsp/
  src/client.rs
  src/config.rs
  src/diagnostics.rs
  src/download.rs
  src/error.rs
  src/language.rs
  src/launch.rs
  src/operations.rs
  src/root.rs
  src/server.rs
  src/service.rs

src/lsp/mod.rs       # compatibility shim over egglsp
src/tool/lsp.rs      # model-facing native tool wrapper
```

`egglsp` owns the language-server client/service/operations implementation. `src/lsp/mod.rs` re-exports it for compatibility and bridges Codegg config/error types. `src/tool/lsp.rs` exposes the model-facing `lsp` tool and attaches native `egglsp` provenance through the structured tool execution path.

The tool registry already understands LSP as a configurable native-tool domain. `ToolRegistryOptions` accepts an optional `lsp_service`, and `ToolRegistry::with_options` registers the real `LspTool`, a hidden disabled stub, or a fallback-native wrapper depending on `[tool_backends.lsp]`.

The current implementation is close, but not yet clean enough for regular agent use. The main gaps are:

1. `egglsp::launch::drain_stderr` can block if a language server keeps stderr open.
2. `LspClient::initialize` spawns a notification task that logs notifications, but does not feed them through `process_notification`, so diagnostics can be lost or delayed.
3. `LspClient::send_request` only reads stdout while a request is pending, so standalone server notifications are not reliably consumed.
4. `src/tool/lsp.rs` returns raw/pretty LSP JSON for most operations, which is noisy for models.
5. The public tool schema says line numbers are 1-indexed, but the wrapper passes them directly to LSP, which is 0-indexed.
6. Several operations select `service.client_keys().first()` instead of resolving the client for the relevant file/root.
7. `incomingCalls` and `outgoingCalls` build fake call hierarchy items and should not be model-facing until implemented correctly.
8. `codeAction` is currently read-only in practice, but it returns raw action data and should remain preview-only or be hidden until a proper preview format exists.
9. There is no `codegg doctor --subsystem lsp` path.
10. Tests cover language detection and registry visibility, but do not lock down LSP tool schema, line/column conversion, diagnostics notification handling, or model-facing output shape.

## Non-Goals

Do not create a second LSP subsystem under `src/lsp/`. The authoritative implementation is `crates/egglsp`.

Do not move LSP into the TUI or any frontend. Frontends may later show LSP status/results, but they should call the existing core/tool APIs.

Do not implement mutating LSP operations in this pass. Rename, formatting, code action application, and `workspace/applyEdit` should remain out of scope unless returned as explicit preview-only data with no filesystem mutation.

Do not add completion as a model-facing tool operation. Completion is mostly an editor feature and is not the high-value agent interface.

Do not require `rust-analyzer`, network downloads, npm, pyright, or any external language server in the default test suite.

Do not expose a raw `egglsp` MCP server. Codegg's current direction is library-first, MCP-second; the stable model-facing name should remain `lsp`.

## Phase 1 — Confirm the Current LSP Wiring

Start by verifying the current wiring before changing behavior.

Run:

```bash
rg -n "egglsp|pub mod lsp|LspTool|LspService|LspOperations|DiagnosticsCollector" Cargo.toml src crates tests architecture plans
rg -n "tool_backends.*lsp|BackendDomain::Lsp|with_session_config_defaults|execute_capture" src tests architecture
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
```

Expected findings:

- `crates/egglsp` is a workspace member and direct root dependency.
- `src/lsp/mod.rs` is a compatibility shim, not the primary implementation.
- `src/tool/lsp.rs` is the model-facing wrapper.
- `ToolRegistry::with_options` is the authoritative registration point.
- `ToolRegistry::execute_capture` is the native structured execution path.

Do not proceed if the code has drifted enough that these assumptions are false. Update this plan first.

Acceptance criteria:

- Baseline commands either pass, or failures are recorded in the implementation notes before edits begin.
- No new LSP module/crate is introduced.

## Phase 2 — Fix LSP Process Lifecycle Hazards

`egglsp::launch::drain_stderr` currently reads stderr to EOF. For long-lived language servers, EOF usually does not occur until shutdown, so this can hang client creation.

Replace this with bounded or background stderr handling.

Recommended implementation:

1. Remove the blocking `drain_stderr(&mut process).await` call from `LspClient::new`.
2. Move `stderr` out of `LspProcess` into a background task at spawn time, or add a `spawn_stderr_drain(server_id, stderr)` helper.
3. The background drain should:
   - read lines or chunks asynchronously;
   - log at `debug` or `trace`;
   - cap retained/logged output per process;
   - never block initialization;
   - exit quietly when the child exits or the pipe closes.
4. Keep `kill_on_drop(true)` and explicit shutdown behavior intact.

If keeping `stderr` inside `LspProcess`, use a short timeout and bounded read only. Do not call `read_to_string` on a live server pipe.

Acceptance criteria:

- Starting an LSP client cannot hang waiting for stderr EOF.
- Server stderr still appears in debug logs when available.
- Existing launch tests still pass.
- Add or update a unit test around bounded stderr handling if practical.

## Phase 3 — Make Diagnostics Notifications Reliable

Diagnostics are one of the highest-value LSP features for Codegg, but they require reliable notification handling.

Current problem: `send_request` forwards nonmatching JSON-RPC messages to `notif_tx`, and the task spawned in `initialize` logs those messages instead of updating the diagnostics cache. Also, stdout is only read while a request is in flight.

Implement this in two steps.

### Step 3A — Extract a testable notification parser

Add a helper in `crates/egglsp/src/client.rs` or a new small module:

```rust
pub(crate) async fn process_notification_value(
    diagnostics: &tokio::sync::Mutex<HashMap<String, Vec<lsp_types::Diagnostic>>>,
    value: serde_json::Value,
) -> Result<(), LspError>
```

or an equivalent synchronous parse function plus async cache update.

It should handle at least:

```text
textDocument/publishDiagnostics
```

It should ignore unknown notifications without error.

Add pure unit tests using synthetic `publishDiagnostics` JSON. These tests must not spawn a real LSP server.

### Step 3B — Replace logging-only notification handling

Update the notification task so every received notification goes through the parser and updates the shared diagnostics cache.

Short-term acceptable version:

- nonmatching messages encountered during `send_request` are parsed and stored;
- `open_file`, `update_file`, and diagnostics queries opportunistically drain pending notifications for a bounded short interval.

Preferred version:

- split transport into a background stdout reader;
- track pending requests by request id with `oneshot` senders;
- dispatch notifications immediately to the diagnostics parser;
- keep request timeout semantics at the caller boundary.

The preferred version is cleaner and will matter once multiple LSP operations run concurrently. If the implementation model chooses the short-term version, document the limitation clearly in `crates/egglsp/src/client.rs` and in this plan's completion notes.

Acceptance criteria:

- `DiagnosticsCollector::get_diagnostics_for_file` can return diagnostics received from `publishDiagnostics` notifications.
- Unknown notifications do not fail requests.
- Diagnostics parser is covered by tests that require no external server.
- No task logs raw notification payloads at normal verbosity.

## Phase 4 — Stabilize the Model-Facing LSP Output

`src/tool/lsp.rs` should not return raw `lsp_types::*` JSON to the model. Add compact DTOs and caps.

Recommended DTOs:

```rust
struct LspToolOutput<T> {
    operation: String,
    file_path: Option<String>,
    result_count: usize,
    truncated: bool,
    results: T,
}

struct DiagnosticSummary {
    file: String,
    line: u32,
    column: u32,
    severity: String,
    source: Option<String>,
    code: Option<String>,
    message: String,
}

struct LocationSummary {
    file: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    snippet: Option<String>,
}

struct SymbolSummary {
    name: String,
    kind: String,
    file: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    children: Vec<SymbolSummary>,
}

struct HoverSummary {
    file: String,
    line: u32,
    column: u32,
    contents: String,
    truncated: bool,
}
```

Keep outputs JSON, but make them small and predictable. Convert file URIs to paths. Prefer paths relative to the workspace root or current working directory when possible. Include snippets for definitions/references if cheap, capped, and safe.

Default caps:

```text
max_hover_chars = 2_000
max_references = 100
max_symbols = 300
max_snippet_lines = 3
max_output_chars = 30_000
```

If config support is too much for this pass, use constants in `src/tool/lsp.rs` and leave config as a later extension.

Acceptance criteria:

- `goToDefinition`, `findReferences`, `hover`, and `documentSymbol` return compact summaries.
- The model no longer receives large raw `lsp_types` JSON for these operations.
- Output truncation is explicit.
- Serialization tests cover at least one sample result for each supported operation.

## Phase 5 — Fix Position Indexing and Input Validation

The current schema describes `line` as 1-indexed, but the wrapper passes values directly to LSP. LSP positions are 0-indexed.

Adopt this stable model-facing convention:

```text
line: 1-indexed
column: 1-indexed
```

Then convert at the wrapper boundary:

```rust
fn to_lsp_position(line: u32, column: u32) -> lsp_types::Position {
    lsp_types::Position {
        line: line.saturating_sub(1),
        character: column.saturating_sub(1),
    }
}
```

Also validate inputs per operation:

- `goToDefinition`, `findReferences`, `hover`: require `file_path`, `line`, and `column`.
- `documentSymbol`: require `file_path`.
- `workspaceSymbol`: require `symbol`; accept optional `file_path` for root selection.
- `codeLens`: require `file_path` if kept exposed.
- `codeAction`: hide for now or require file/range and return preview-only summaries.

Do not silently default missing line/column to zero for position-sensitive operations.

Acceptance criteria:

- Schema clearly states line and column are 1-indexed.
- Wrapper converts to LSP 0-indexing exactly once.
- Missing required fields return clear `ToolError::Execution` messages.
- Unit tests cover conversion and validation failures.

## Phase 6 — Remove First-Client Routing Assumptions

Several operations currently use `service.client_keys().await.first()` and send requests to the first available client. This is incorrect in multi-root or multi-language workspaces.

Fix by adding a service method that resolves the correct client key for an operation.

Recommended API:

```rust
impl LspService {
    pub async fn get_or_create_client_for_file(
        &self,
        file_path: &Path,
    ) -> Result<(String, PathBuf), LspError>;

    pub async fn get_or_create_client_for_root_hint(
        &self,
        root_hint: Option<&Path>,
        server_id: Option<&str>,
    ) -> Result<(String, PathBuf), LspError>;
}
```

The first may simply call the existing `get_or_create_client`. The second should be conservative; if root/server cannot be determined, return a clear error rather than picking an arbitrary client.

Update `workspaceSymbol` to accept optional `file_path`. If present, resolve the client using that file. If absent, use current directory root and the default language/server only if unambiguous. Otherwise return a helpful error telling the model to provide `file_path`.

Update `goToImplementation`, `prepareCallHierarchy`, `codeLens`, and any other direct `send_request` path to resolve the key from the relevant file.

Acceptance criteria:

- No model-facing operation selects the first arbitrary client key.
- Multi-client behavior is deterministic.
- Error messages explain how to disambiguate root/language.

## Phase 7 — Narrow or Correct the Exposed Operation Set

The current tool exposes operations that are not all correctly implemented for agent use.

For this pass, expose only:

```text
diagnostics
hover
goToDefinition
findReferences
documentSymbol
workspaceSymbol
codeLens        # optional; only if summary output is compact
```

Hide or remove from the model-facing enum for now:

```text
completion
signatureHelp
codeAction
prepareCallHierarchy
incomingCalls
outgoingCalls
goToImplementation
```

Rationale:

- `completion` is not a priority for an agent harness.
- `signatureHelp` can return verbose editor-oriented data and can be reintroduced as a capped summary later.
- `codeAction` must remain preview-only and needs a stable edit/action summary first.
- call hierarchy requires a real prepare-result -> selected-item -> incoming/outgoing flow; the current fake item construction is not correct.
- implementation lookup is useful later, but should go through the same summary/key-resolution layer as definition.

If the implementation model strongly prefers to keep `goToImplementation`, it must return the same `LocationSummary` shape as `goToDefinition` and must not use first-client routing.

Acceptance criteria:

- Model-facing schema lists only supported, reliable operations.
- `tool_search` and provider tool definitions cannot surface broken LSP operations.
- Hidden operations may remain in `egglsp::operations` for future use.

## Phase 8 — Wire Diagnostics as a First-Class Tool Operation

Add an explicit `diagnostics` operation to `src/tool/lsp.rs`.

Suggested input:

```json
{
  "operation": "diagnostics",
  "file_path": "src/main.rs"
}
```

For now, require `file_path`. Later, workspace diagnostics can be added once notification handling and workspace root lifecycle are more robust.

Implementation:

1. Resolve and validate the file path using the existing allowed-root path validation.
2. Read file contents if necessary and call `service.open_file` before fetching diagnostics.
3. Use `DiagnosticsCollector::get_diagnostics_for_file` or equivalent direct service method.
4. Return `DiagnosticSummary` values with 1-indexed line/column in model-facing output.
5. If no diagnostics have arrived yet, return an empty list plus a note like `diagnostics_may_still_be_warming = true` only if the service just opened the file.

Acceptance criteria:

- `diagnostics` is documented in the schema and tool description.
- Diagnostics output is compact and 1-indexed.
- Missing/stale diagnostics are handled honestly, not treated as proof of correctness.

## Phase 9 — Respect LSP Config More Completely

The config schema has `LspRule::Active { command, extensions, env, initialization }`, but the service primarily uses static server definitions.

Make config behavior explicit and tested.

Minimum acceptable implementation:

- `disabled` must continue working.
- `env` must continue flowing into spawned server processes.
- `initialization` must continue flowing into `initialize`.
- If `command` override is not implemented, config validation or startup error must clearly say so.

Preferred implementation:

- Add a resolved launch command abstraction:

```rust
struct ResolvedLspLaunch {
    command: PathBuf,
    args: Vec<String>,
    env: Vec<(String, String)>,
    initialization_options: Option<serde_json::Value>,
}
```

- For default server definitions, resolve the command through PATH/cache/download as today.
- For `LspRule::Active { command, .. }`, treat `command[0]` as executable and `command[1..]` as args.
- Do not auto-download when a custom command is supplied.
- Keep server id/language mapping stable.

Optional config additions, if small:

```rust
request_timeout_ms: Option<u64>
auto_download: Option<bool>
max_output_chars: Option<usize>
max_hover_chars: Option<usize>
max_references: Option<usize>
```

Do not perform a large config migration in this pass. Additive optional fields are acceptable.

Acceptance criteria:

- Custom command overrides either work or fail explicitly.
- Env and initialization options are covered by tests.
- Disabled server config remains covered by existing registry tests.

## Phase 10 — Add `codegg doctor --subsystem lsp`

Extend the existing doctor command with an LSP subsystem.

Implementation outline:

1. Add `Lsp` to `DoctorSubsystem`.
2. Update `cmd_doctor` so `All` includes LSP diagnostics.
3. Add a helper that prints:
   - loaded LSP config state;
   - configured/known server ids;
   - project root detected for current directory;
   - whether common servers are disabled;
   - whether server binaries are found in PATH or cache;
   - whether auto-download would be required.
4. Do not download servers by default from `doctor`; it should be mostly non-mutating. If a later explicit `--install` flag is added, that is a separate pass.

Example output:

```text
== LSP ==
config: enabled
root: /path/to/repo
rust-analyzer: found in PATH (/Users/.../.cargo/bin/rust-analyzer)
pyright: not found in PATH, no cached binary, install required
requests: timeout 30000ms
model tool: exposed as native lsp
```

Acceptance criteria:

- `codegg doctor --subsystem lsp` works.
- `codegg doctor --subsystem all` includes LSP.
- Doctor does not start a language server unless explicitly designed to do so.
- Doctor does not download binaries by default.

## Phase 11 — Strengthen Tests Without External Server Requirements

Default tests should remain hermetic.

Add or update tests for:

```text
crates/egglsp:
  - diagnostic notification parser from synthetic JSON
  - unknown notification ignored
  - line/language/root helpers where applicable
  - custom launch config resolution if implemented

codegg root tests:
  - lsp tool schema lists only supported operations
  - lsp tool is ReadOnly
  - disabled lsp remains hidden from definitions
  - lsp execute_structured reports egglsp provenance on validation-only calls if possible
  - line/column conversion helper maps 1 -> 0 and saturates safely
  - missing file_path/line/column validation errors are clear
```

Optional integration test:

```rust
#[tokio::test]
async fn rust_analyzer_smoke_if_enabled() {
    if std::env::var("CODEGG_LSP_INTEGRATION").ok().as_deref() != Some("1") {
        return;
    }
    // require rust-analyzer in PATH; skip if missing
}
```

Run it manually with:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test --workspace lsp -- --nocapture
```

Acceptance criteria:

- `cargo test --workspace` does not require network or installed language servers.
- LSP-specific unit tests cover parser/schema/indexing behavior.
- Optional integration test is opt-in and skips cleanly.

## Phase 12 — Documentation Cleanup

Update docs after implementation, not before.

Files to consider:

```text
architecture/native_crates.md
architecture/tool.md
AGENTS.md
crates/egglsp/src/lib.rs
crates/egglsp/src/client.rs
src/tool/lsp.rs
```

Docs should state:

- `egglsp` is the authoritative LSP implementation.
- `src/lsp/mod.rs` is only a compatibility shim.
- model-facing line/column are 1-indexed.
- exposed operations are intentionally narrow.
- diagnostics are useful fast feedback, not a replacement for `cargo check`, tests, or static analyzers.
- LSP edit-producing operations are not automatically applied.

Acceptance criteria:

- Architecture docs match the actual exposed operation set.
- No docs describe raw LSP JSON as the desired model-facing shape.
- No docs suggest the TUI owns LSP sessions.

## Validation Commands

Run these before marking the pass complete:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Also run targeted checks:

```bash
cargo test -p egglsp
cargo test --test lsp
cargo test --test tool_registry
cargo test --test tool_structured_execution
codegg doctor --subsystem lsp
```

If an optional real-server smoke test is added:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test --workspace lsp -- --nocapture
```

## Done Criteria

This pass is complete when all of the following are true:

- LSP client startup cannot hang on stderr.
- Diagnostics notifications update the diagnostics cache through a tested path.
- `lsp` tool outputs compact agent-facing summaries rather than raw LSP JSON for supported operations.
- Model-facing line and column inputs are documented and converted correctly.
- No model-facing operation routes through an arbitrary first client key.
- Broken or incomplete operations are hidden from the model-facing schema.
- `diagnostics` is a first-class LSP operation.
- `codegg doctor --subsystem lsp` exists and is non-mutating by default.
- Default tests do not require network access or installed language servers.
- Architecture docs reflect the current `egglsp`-backed implementation.

## Suggested Follow-Up Passes

After this plan is complete, consider separate plans for:

1. preview-only rename and formatting using `WorkspaceEdit` summaries;
2. proper call hierarchy flow;
3. overlay sync for proposed edits before filesystem commit;
4. model-profile-aware automatic semantic context packets;
5. optional `egglsp` MCP adapter binary, hidden behind the existing native wrapper by default.
