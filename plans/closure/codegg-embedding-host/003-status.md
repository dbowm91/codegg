# CodeGG Embedding Host E003 — SDK crate and conformance: closure record

Status: **closed**.

Plan: [003-sdk-crate-and-conformance](../../implementation/codegg-embedding-host/003-sdk-crate-and-conformance.md).
Roadmap: [codegg-embedding-host-roadmap](../../subsystems/codegg-embedding-host-roadmap.md#7-milestones).
Stability policy: [ADR-0060](../../adrs/ADR-0061-embedding-host-wire-as-a-public-contract.md).
Commit: `089f9479c` on `feat/codegg-embedding-host`.

## Delivered

| Requirement | Where |
|---|---|
| One definition of the wire | `crates/codegg-host-protocol` (`lib.rs` is the moved `src/host/protocol.rs`) |
| The binary depends on it, not a copy | `src/host/protocol.rs` is a `pub use codegg_host_protocol::*;` shim |
| Typed client | `crates/codegg-sdk` — `HostClient::spawn`, `attach`, `initialize`, `open_session`, `submit`, `cancel`, `close`, `recv`, `shutdown` |
| Bounded event delivery | `mpsc::channel(EVENT_BUFFER = 256)` + `HostError::EventOverflow` |
| Request correlation | `pending: HashMap<u64, oneshot>` fed by one reader task |
| Typed errors | `HostError::{Io, Closed, Timeout, Rpc, EventOverflow, Wire}` with `class()` and `retryable()` |
| Child lifecycle | `kill_on_drop(true)`; `Drop` starts the kill; `shutdown()` waits and reaps |
| Golden frames | 20 protocol cases (`cargo test -p codegg-host-protocol`) |
| Conformance | `crates/codegg-sdk` tests drive a scripted host over `tokio::io::duplex`: open, three text deltas, a tool notification, usage, done, busy refusal, cancel, close, timeout, closed host, unknown frames |
| Documentation | `docs/embedding.md` (full contract, worked example) and `crates/codegg-sdk/README.md` |

## Evidence

### Protocol crate

`cargo test -p codegg-host-protocol` — 20 passed. The extraction is byte-identical
to the definitions E002 shipped: the same file, the same serde attributes, the
same bounds and the same golden strings. `src/host/protocol.rs` now re-exports
them, so no path inside the binary changed.

### SDK

`cargo test -p codegg-sdk` — 6 unit tests plus 1 doctest:

```
tests::a_session_streams_ordered_events_and_a_typed_done ... ok
tests::a_busy_refusal_is_a_typed_class ... ok
tests::cancel_returns_an_acknowledgement ... ok
tests::a_closed_host_fails_pending_requests ... ok
tests::a_timeout_is_reported_as_a_timeout ... ok
tests::unknown_frames_do_not_break_correlation ... ok
```

The scripted host asserts the client's own frames: `initialize` is the first
request, `session/open` carries the descriptor object verbatim, and every
notification the host emits arrives in order.

### Single-definition proof

`grep -rn "pub enum RequestBody"` finds exactly one definition, in
`crates/codegg-host-protocol/src/lib.rs`; the binary's module is a re-export.

### Compatibility

The wire did not change in this milestone: the golden frames pass before and
after the extraction, and the host suite (21 cases) is unchanged. ACP, `exec` and
the daemon protocol are untouched.

## Known limits

- The crate starts at `0.1.0` and is not published; `apiVersion: 1` is the
  contract, the crate version is not a semver promise beyond it.
- The client owns a child process and one session; it does not multiplex.
- Language bindings other than Rust are out of scope, as the plan states.
