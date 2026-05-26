# Client Architecture Review

## Summary

The client architecture documentation is accurate and well-maintained. All major claims were verified against the source code. Minor stale items noted around line number references.

## Verified Correct

- **Module exports** (`src/client/mod.rs:1-4`): `run_attach` correctly exported as public API
- **Health check 10s timeout** (`src/client/sdk.rs:40`): `timeout(Duration::from_secs(10))` confirmed
- **WebSocket 30s timeout** (`src/client/attach.rs:43`): `timeout(Duration::from_secs(30), connect_async(...))` confirmed
- **Retry logic** (`src/client/attach.rs:36-66`): 3 attempts max, exponential backoff with `2u64.saturating_pow((attempt - 1) as u32)` starting at 2s confirmed
- **Resume handshake** (`src/client/attach.rs:72-75`): Sends `TuiMessage::Resume { from_event_seq: 0 }` after connect confirmed
- **catch_unwind usage** (`src/client/attach.rs:86,114-116`): Event handling wrapped in `catch_unwind` confirmed
- **Two background tasks** (`src/client/attach.rs:85-127`): `event_task` and `send_task` spawned confirmed
- **TuiMessage protocol** (`src/protocol/tui.rs`): All variants match documentation, including `#[serde(rename = "resync_required")]` at line 69
- **handle_remote_event location** (`src/tui/app/mod.rs:794`): Confirmed at correct location (not in client module as noted)
- **Error enum** (`src/error.rs`): `ClientError` variants match documentation

## Discrepancies Found

- **RemoteClient location** (doc: `sdk.rs`, code): `RemoteClient` struct is defined in `src/client/sdk.rs:7-10` - correct, but the documentation could be clearer that it lives in the same module as `attach.rs`

## Stale Items in Architecture Doc

- **Line reference for RemoteClient** (doc: line 63, actual: ~line 7): The struct definition is at `src/client/sdk.rs:7`, not line 63 as might be inferred from the snippet format
- **TuiMessage enum location** (doc: `src/protocol/tui.rs`): Correct, but doc shows it inline while actual file has 82 lines with `QuestionSpec` struct following

## Bugs Identified

None. The client implementation appears solid.

## Improvement Suggestions

- **add `new_remote` to docs**: The `tui::App::new_remote()` call at `src/client/attach.rs:77` could be documented - currently no explicit public API for creating remote App instances noted in architecture
- **Consider documenting RemoteTuiMessage type**: The doc references `TuiMessage` but in `src/tui/app/mod.rs:798` the code uses `RemoteTuiMessage` - this appears to be an alias or re-export that could be clarified
