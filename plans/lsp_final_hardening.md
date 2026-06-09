# LSP Final Hardening Plan

## Purpose

Close the remaining edge cases in Codegg's LSP integration after `plans/lsp_integration.md` and `plans/lsp_runtime_correctness.md`.

The repo is now in good architectural shape: `egglsp` owns the implementation, `LspClient` has a background stdout dispatcher and pending request map, model-facing LSP output is compact, `codeLens` is hidden, `workspaceSymbol` is summarized, URI decoding is URL-aware, and doctor output separates backend registration from agent exposure.

This final pass should avoid broad redesign. Focus only on the residual correctness issues:

1. `lsp diagnostics` should ensure the target file is opened/synchronized before reading the diagnostics cache.
2. diagnostics warm-up semantics should reflect actual file-open/update events.
3. pending requests should fail immediately when the background stdout reader exits.
4. stale request-owned read helpers should be removed or clearly marked inactive.
5. tests should prove these behaviors without requiring real language servers by default.

## Current State Summary

Relevant files:

```text
crates/egglsp/src/client.rs
crates/egglsp/src/diagnostics.rs
crates/egglsp/src/launch.rs
crates/egglsp/src/service.rs
src/tool/lsp.rs
tests/lsp.rs
```

Current strengths:

- `LspClient` has `PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, LspError>>>>>`.
- `LspClient::new` takes stdout and spawns `background_reader`.
- `send_request` registers pending requests and waits on a `oneshot`.
- notifications are classified and dispatched independently of request calls.
- diagnostics notifications update the diagnostics cache.
- `DiagnosticsOutput` contains `diagnostics_may_still_be_warming`.
- `src/tool/lsp.rs` returns compact summaries for exposed operations.
- `codeLens` is no longer model-facing.

Remaining known issues:

- `DiagnosticsCollector::get_diagnostics_for_file` gets/creates a client and then reads cached diagnostics, but does not open or synchronize the file before checking the cache.
- `diagnostics_may_still_be_warming` is driven by `last_opened_at`, but that timestamp is only updated by `open_file`/`update_file`; diagnostics calls that never open the file can report no diagnostics and not warming.
- if `background_reader` exits, pending requests are left to timeout instead of being failed immediately.
- `launch::read_response` and `launch::read_notification` appear to remain as request-owned read helpers even though the active path now uses the background dispatcher.

## Non-Goals

Do not change the public model-facing LSP operation set.

Do not reintroduce `codeLens`, completion, code actions, rename, formatting, or call hierarchy.

Do not add an MCP adapter.

Do not change TUI behavior.

Do not require `rust-analyzer` or any external server in the default test suite.

Do not redesign the whole process model again. The dispatcher architecture is the desired shape.

## Phase 1 — Ensure Diagnostics Opens/Synchronizes the File

`DiagnosticsCollector::get_diagnostics_for_file` must ensure the language server has been told about the file before it reads from the diagnostics cache.

Implementation steps:

1. In `crates/egglsp/src/diagnostics.rs`, before reading cached diagnostics:
   - resolve the client key with `service.get_or_create_client(file_path)`;
   - check whether the file is already open, or simply send a full-content sync idempotently;
   - read the file from disk;
   - call `service.open_file(file_path, &text).await` if not open;
   - call `service.update_file(file_path, &text).await` if already open and disk content should be refreshed.
2. Add a service helper if needed:

```rust
impl LspService {
    pub async fn is_file_open(&self, key: &str, file_path: &Path) -> Result<bool, LspError>;
    pub async fn ensure_file_open_from_disk(&self, file_path: &Path) -> Result<(String, String), LspError>;
}
```

Return `(key, uri_string)` from the helper so the diagnostics collector uses the exact URI key stored by the client.

3. Do not silently swallow file-read errors. Convert them into `LspError::RequestFailed` or a more specific existing error variant with the file path included.
4. Do not create a new file. Diagnostics is read-only and should require the target file to exist.

Preferred behavior:

```text
first lsp diagnostics(file):
  get/create client
  read file from disk
  didOpen file with full text
  mark warming=true if no diagnostics have arrived yet
  return cached diagnostics if already present
```

Acceptance criteria:

- A first diagnostics call sends `didOpen` before reading/returning diagnostics.
- Subsequent diagnostics calls do not repeatedly spam `didOpen`; use `didChange` or no-op depending on existing state.
- Missing files produce a clear tool error.
- The operation remains read-only from Codegg's filesystem perspective.

## Phase 2 — Make Warm-Up Semantics Accurate

After Phase 1, warm-up can be based on real open/update events.

Implementation steps:

1. Keep `last_opened_at` or rename it to `last_synced_at` if it now tracks both opens and updates.
2. Ensure the timestamp is set whenever diagnostics explicitly syncs the file.
3. If no diagnostics have ever been received for the URI and the last sync was recent, return `diagnostics_may_still_be_warming = true`.
4. If diagnostics have been received for the URI, even if empty, return `warming = false` after the cache contains a value for that URI. A server publishing an empty diagnostics list is a meaningful response.
5. Consider storing a separate `diagnostics_received_at: HashMap<String, Instant>` or use the cache key presence to distinguish:
   - absent key: no diagnostic response yet;
   - present key with empty vec: server reported clean.

Recommended logic:

```rust
pub async fn diagnostics_may_still_be_warming(&self, uri: &str) -> bool {
    let synced_recently = self.last_synced_at
        .lock().await
        .get(uri)
        .is_some_and(|t| t.elapsed() < Duration::from_secs(2));
    let has_received_diagnostics = self.diagnostics.lock().await.contains_key(uri);
    synced_recently && !has_received_diagnostics
}
```

Acceptance criteria:

- Empty published diagnostics means clean, not warming.
- No cache entry after a recent sync means warming.
- No recent sync means not warming unless a sync just happened in this call.
- Tests cover absent cache, empty cache entry, and nonempty cache entry.

## Phase 3 — Fail Pending Requests When Reader Exits

`background_reader` currently logs and exits when reading stdout fails. Pending requests then wait until each timeout expires. Fail them immediately instead.

Implementation steps:

1. Add a helper:

```rust
async fn fail_all_pending(pending: &PendingMap, error: LspError) {
    let mut pending = pending.lock().await;
    let drained = std::mem::take(&mut *pending);
    for (_, tx) in drained {
        let _ = tx.send(Err(error.clone()));
    }
}
```

If `LspError` is not cloneable, either derive/implement `Clone` if reasonable, or create a fresh error string for each sender.

2. In `background_reader`, when `read_framed_message` returns an error, call `fail_all_pending` before breaking.
3. Also fail pending requests on JSON stream terminal conditions, child stdout EOF, or fatal framing errors.
4. Keep late responses after timeout harmless: if no pending sender exists for a response id, log at debug and drop.

Acceptance criteria:

- Reader exit fails pending requests without waiting for the request timeout.
- Timeout cleanup still removes a pending request.
- Late response after timeout remains harmless.
- Tests cover pending failure on simulated reader exit if practical.

## Phase 4 — Remove or Fence Stale Launch Read Helpers

After the dispatcher refactor, `launch::read_response` and `launch::read_notification` are likely stale. They encourage reintroducing request-owned stdout reads.

Implementation options:

Preferred:

- Remove `read_response` and `read_notification` entirely if no code uses them.
- Keep only `send_request`, `spawn_server`, `spawn_stderr_drain`, and `terminate` in `launch.rs`.

Fallback if tests or future internals still use them:

- Move framed-message reading into a single shared helper used by `client.rs`.
- Mark request-owned helpers `#[cfg(test)]` or add documentation:

```rust
// Do not use this in production LSP request handling. Active stdout reads are owned by LspClient::background_reader.
```

Acceptance criteria:

- There is one obvious production path for stdout reads: `LspClient::background_reader`.
- No production code calls `launch::read_response` or `launch::read_notification`.
- `rg "read_response|read_notification"` shows no misleading active usage.

## Phase 5 — Strengthen Hermetic Tests

Add tests without requiring a real language server.

Recommended tests:

```text
crates/egglsp/src/client.rs:
  - classify empty diagnostics notification as Notification
  - dispatch empty diagnostics inserts an empty vec into cache
  - diagnostics_may_still_be_warming is false after empty diagnostics cache entry
  - fail_all_pending sends errors to all pending receivers

crates/egglsp/src/diagnostics.rs:
  - diagnostics output treats absent cache after sync as warming
  - diagnostics output treats empty cache entry as clean
  - missing file read produces a clear LspError

src/tool/lsp.rs or tests/lsp.rs:
  - diagnostics output includes diagnostics_may_still_be_warming field
  - codeLens remains hidden from schema
  - workspaceSymbol still returns summary shape, not raw JSON
```

For any test that would otherwise need a real process, extract pure helpers instead. For example, test warm-up logic on cache/timestamp maps rather than starting rust-analyzer.

Optional integration test remains opt-in:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test -p egglsp real_lsp -- --nocapture
```

Acceptance criteria:

- `cargo test --workspace` remains hermetic.
- New tests prove the final hardening behavior.
- No default test downloads or launches a language server.

## Phase 6 — Small Documentation Cleanup

Update only docs that would otherwise mislead future implementers.

Files to check:

```text
crates/egglsp/src/client.rs
crates/egglsp/src/diagnostics.rs
crates/egglsp/src/launch.rs
architecture/native_crates.md
plans/lsp_runtime_correctness.md
```

Docs should say:

- stdout is exclusively owned by the background reader;
- diagnostics calls sync/open the file before consulting the cache;
- `diagnostics_may_still_be_warming` means no publishDiagnostics response has arrived after a recent sync;
- empty diagnostics from the server means clean, not warming;
- stale launch read helpers are removed or test-only.

Acceptance criteria:

- No docs imply request-owned stdout reads are active.
- No docs imply empty diagnostics and no diagnostics response are the same state.

## Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted checks:

```bash
cargo test -p egglsp
cargo test --test lsp
rg "read_response|read_notification" crates/egglsp src tests
codegg doctor --subsystem lsp
```

Optional real-server smoke:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test -p egglsp real_lsp -- --nocapture
```

## Done Criteria

This final pass is complete when:

- `lsp diagnostics` opens or syncs the target file before reading diagnostics.
- warm-up semantics distinguish absent diagnostics response from an empty diagnostics response.
- pending requests fail immediately when the background reader exits.
- no production code uses stale request-owned stdout read helpers.
- tests cover the final edge cases without requiring external LSP servers.
- docs reflect the final dispatcher and diagnostics semantics.

## Stopping Point

After this pass, treat the current LSP layer as stable enough for normal Codegg use. Future LSP work should be new feature work, not integration hardening:

- preview-only rename;
- preview-only formatting;
- real call hierarchy;
- overlay sync for proposed edits;
- model-profile-aware semantic context packets.
