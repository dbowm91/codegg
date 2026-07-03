# LSP (Language Server Protocol)

> **Note:** This document is partially outdated. For comprehensive LSP documentation, see `architecture/lsp.md` and `.codegg/skills/lsp/SKILL.md`. The phase 2 stdio test layout now lives under `crates/egglsp/tests/`, with the fake server built as the `egglsp-test-server` bin target from the `egglsp` package.

codegg integrates with Language Server Protocol (LSP) to provide IDE-like features including diagnostics, code navigation, and intelligent completions.

## Architecture

The authoritative LSP implementation is in **`crates/egglsp/`**. The `src/lsp/` directory is a thin shim that re-exports from egglsp.

The egglsp crate consists of:

- **`src/server.rs`** - Server definitions (39 servers: clangd, rust-analyzer, gopls, pyright, typescript-language-server, etc.)
- **`src/service.rs`** - `LspService` managing LSP client lifecycle, explicit leader/waiter init election, lifecycle-validated publication, unpublished-client disposal, and quiescent shutdown driven by an absolute deadline
- **`src/client.rs`** - Low-level LSP client implementation
- **`src/operations/`** - `LspOperations` module directory: `code_actions.rs`, `completion.rs`, `formatting.rs`, `navigation.rs`, `overlay_ops.rs`, `rename.rs`, `semantic_tokens.rs`, `signature.rs`
- **`src/diagnostics.rs`** - `DiagnosticsCollector` for collecting and debouncing diagnostics

Phase 2 integration tests now live under `crates/egglsp/tests/`: the legacy fake-server suites use `FakeLspHarness`, the production-harness protocol subset uses `ProductionClientHarness`, and `scenario_engine.rs` contains inlined fake-server self-tests (no external `include!`). `egglsp::test_support` is feature-gated behind `lsp-test-support`.

## Key Components

### LspService

Manages the lifecycle of LSP clients per project/language. Uses explicit leader/waiter init election for single-flight initialization (the first caller becomes leader, concurrent callers wait on the same completion fan-out), validates lifecycle generation before publication, and uses `Arc`-based handles to avoid serialization of unrelated clients behind process I/O:

```rust
pub struct LspService {
    // Manages multiple language server clients
    clients: Arc<RwLock<HashMap<String, Arc<LspClient>>>>,
    config: LspConfig,
}
```

Client-map read/write lock discipline: non-mutating service methods (`open_file`, `update_file`, `close_file`, `save_file`, `is_file_open`, `get_diagnostics_for_key`, `get_all_diagnostics_for_key`, `diagnostics_may_still_be_warming`, `get_diagnostic_snapshot_for_key`, `send_request`, `client_keys`, `get_capabilities_for_key`) use `clients.read().await`. Write guards are reserved for client publication, insertion, removal, and shutdown drain.

Each spawned initialization task is wrapped in `run_init_task_wrapper`, which awaits a start-registration barrier and owns the `Sender` end of an authoritative terminal completion channel. The wrapper owns an `ActiveTaskGuard` drop guard as a fallback for the cleanup path; the primary cleanup is explicit removal of the `active_init_tasks` entry before the wrapper sends its terminal `InitTaskExit`. `active_init_tasks: HashMap<u64, InitTaskControl>` stores each task's `CancellationToken`, `AbortHandle`, and a `oneshot::Receiver<InitTaskExit>` that is the **authoritative** terminal signal. The wrapper cannot begin its body until the leader registration code has installed the `InitTaskControl` entry — this is enforced by a one-shot start barrier, which eliminates the spawn-before-registration race. Cooperative cancellation races each long-running stage against the cancellation signal at five stages: before download, before process spawn, before initialize request, before initialized notification, and before publication. Injected test factories are wrapped in `tokio::select!` over the factory future and the `CancellationToken`, so test factories are cancellation-aware by default.

If publication loses to an existing client or is invalidated by shutdown, the unpublished client is shut down with a bounded timeout before waiters are notified.

`shutdown_all()` is quiescent: it transitions to `ShuttingDown` and broadcasts on a `tokio::sync::watch` channel (`lifecycle_tx`), drains init slots, signals cooperative cancellation to all tracked tasks concurrently, then awaits all completion receivers concurrently via `await_init_task_completions` (using `FuturesUnordered` with `tokio::select!` over each receiver and the aggregate deadline) under one 300ms grace period. Stragglers are forcibly aborted via `AbortHandle` and re-awaited through the same authoritative completion receiver path (no forwarding task ever wraps the real `JoinHandle`). Ready clients are drained concurrently via `futures::future::join_all` with a 2s per-client timeout, then `Cancelled` `SharedInitError` is notified to any waiters, and the lifecycle transitions to `Stopped`. The entire sequence is driven by an **absolute deadline** (`Instant::now() + SHUTDOWN_GLOBAL_TIMEOUT` = 6s), so the total shutdown is bounded regardless of client count. Concurrent shutdown callers observing `ShuttingDown` enter `await_stopped()`, which subscribes to the watch channel and waits for `Stopped` — race-free with no lost-wakeup window at the `ShuttingDown → Stopped` transition.

### LspOperations

Provides code navigation and analysis:

- `go_to_definition()` - Jump to symbol definitions
- `find_references()` - Find all references to a symbol
- `hover()` - Get type/info hover for cursor position
- `document_symbols()` - List all symbols in a document
- `code_actions()` - Get available code actions/quick fixes
- `completion()` - Trigger completion at cursor
- `signature_help()` - Show function signature hints
- `code_lens()` - Get CodeLens data

### DiagnosticsCollector

Collects and manages diagnostics with 150ms debouncing:

```rust
pub struct DiagnosticsCollector {
    service: Arc<LspService>,
    last_update: Arc<Mutex<HashMap<String, Instant>>>,
}
```

## Supported Languages

Servers are automatically downloaded for:

| Language | Server |
|----------|--------|
| Rust | rust-analyzer |
| Python | pyright |
| TypeScript/JavaScript | typescript-language-server |
| Go | gopls |
| C/C++ | clangd |

## Configuration

LSP is configured via `config.json`:

```json
{
  "experimental": {
    "lsp_tool": true
  },
  "lsp": {
    "servers": {
      "rust": {
        "command": "rust-analyzer",
        "args": []
      }
    }
  }
}
```

## Integration with Tools

The `lsp` tool in the tool registry allows the agent to:

1. **Goto definition** - Jump to symbol definitions
2. **Find references** - Find all symbol references
3. **Hover** - Get type information
4. **Document symbols** - List file symbols
5. **Code actions** - Get quick fixes
6. **Semantic checks** - Run `semanticCheckPreview` with either full proposed content or a single-file unified diff patch; the patch is applied in memory only, the overlay is restored after the check, and diagnostics/restore errors stay surfaced in the result

Example usage in agent prompts:
```
Use the lsp tool to find the definition of the `processRequest` function
```

## Diagnostics Flow

1. File changes are sent to LSP server via `textDocument/didChange`
2. Server publishes diagnostics via `textDocument/publishDiagnostics`
3. `DiagnosticsCollector` receives and debounces updates (150ms)
4. TUI displays diagnostics with severity indicators

## Error Handling

- Server launch failures return `LspError::LaunchFailed`
- Invalid file paths return `LspError::LaunchFailed`
- Request timeouts send a best-effort `$/cancelRequest`; if that cancel write fails, the transport is marked failed and later calls fail fast with `LspError::WriterClosed`
- Immediate request/notification I/O failures surface as `LspError::RequestFailed`; once the transport is failed, later calls fail fast with `LspError::WriterClosed`
