# Core Architecture Review

## Summary
The architecture document is mostly accurate with one line number reference that needs correction and a few cases where the document could be more precise about types.

## Verified Correct
- `InprocCoreClient` has exactly 4 fields as documented at `src/core/mod.rs:22-28`
- `CoreClient` trait definition matches at `src/core/mod.rs:13-20`
- `subscribe()` returns `mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>>` as shown at line 27 (implementation at `src/core/mod.rs:702-725`)
- `TurnSubmit` variant matches documented fields at `src/protocol/core.rs:115-123`
- `CoreRequest` enum variants (Session lifecycle, Turn lifecycle, Session data, Operational helpers) match at `src/protocol/core.rs:50-175`
- `CoreEvent` enum variants match at `src/protocol/core.rs:177-272`
- Stdio and Socket clients return empty receivers as documented at `src/core/transport/stdio.rs:88-91` and `src/core/transport/socket.rs:126-129`
- Socket reconnect-and-retry-once strategy documented at line 158 is implemented at `src/core/transport/socket.rs:50`
- Transport adapters in `src/core/transport/mod.rs` correctly export `SocketCoreClient` and `StdioCoreClient`
- Event mapping from `AppEvent` to `CoreEvent` in `map_app_event_to_core_event()` at `src/core/mod.rs:728-797`

## Discrepancies Found
- **Line 37**: "Contains 4 fields" is correct, but description does not mention the `Option<Arc<...>>` wrapper type for each field, which is an important detail for understanding nullability
- **Line 52**: `Subscribe { session_id }` in doc should specify `session_id: Option<String>` to match actual definition at `src/protocol/core.rs:52-53`
- **Line 69**: `Resume { session_id, from_event_seq }` should specify `session_id: Option<String>` to match actual at `src/protocol/core.rs:55-57`

## Bugs Identified
- No bugs found in core implementation

## Improvement Suggestions
- **Line 31**: "return an empty receiver" could be clearer - both stdio and socket `subscribe()` implementations return a channel where the receiver half is dropped (open), meaning events sent would be lost. Consider clarifying this is intentional for remote transports.
- **Line 173**: "AgentFinished, Error" should be `AppEvent::AgentFinished` and `AppEvent::Error` to be precise about the type hierarchy
- Consider documenting the behavior when `turn_id` is empty string in CoreEvent mappings (e.g., line 733 `turn_id: String::new()`)

## Stale Items in Architecture Doc
- No stale content found - document appears current with implementation