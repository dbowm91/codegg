# CodeGG Embedding Host E002 — Headless host protocol v1: closure record

Status: **closed**.

Plan: [002-headless-host-protocol](../../implementation/codegg-embedding-host/002-headless-host-protocol.md).
Roadmap: [codegg-embedding-host-roadmap](../../subsystems/codegg-embedding-host-roadmap.md#7-milestones).
Stability policy: [ADR-0060](../../adrs/ADR-0061-embedding-host-wire-as-a-public-contract.md) (accepted).
Contract: [`docs/embedding.md`](../../../docs/embedding.md).
Commit: `089f9479c` on `feat/codegg-embedding-host`.

## Delivered

| Requirement | Where |
|---|---|
| `codegg host --stdio` | `Commands::Host { stdio }` in `src/main.rs`, dispatched before any project or TUI setup |
| Wire types, bounds, versioning, error taxonomy | `crates/codegg-host-protocol` (extracted in E003; `src/host/protocol.rs` re-exports it) |
| Frame reader/writer, dispatch, handshake state | `src/host/mod.rs` (`run_stdio`, `FrameWriter`, `Host::serve`) |
| Session: workspace, model, agent, policy, descriptor, one turn | `src/host/session.rs` |
| Turn: admission, event mapping, cancellation, terminal frames | `src/host/turn.rs` |
| One shared loop setup for `exec` and `host` | `src/headless.rs` (`HeadlessRuntime`, `HeadlessOptions`); `exec` keeps its exact behaviour |
| Documentation of record | `docs/embedding.md` |

## Behaviour as built

- `initialize` returns `apiVersion: 1`, `codeggVersion` and
  `["descriptor", "streaming", "cancellation", "usage"]`. A required capability
  v1 does not have is refused with `unsupported_capability`, never downgraded.
- `session/open` canonicalizes an existing absolute workspace directory, resolves
  the agent, parses the descriptor and registers it for that session only. A
  rejected descriptor names the field and never the value. A second open is
  refused without disturbing the first.
- `turn/submit` admits one turn and returns `{"turnId": …}`; a second submit while
  a turn is active is the `busy` class with `code:-32001`.
- Events come from the loop's existing global bus, filtered by session id:
  `AppEvent::TextDelta` → `turn/text`, `ToolCallStarted`/`ToolResult` →
  `turn/tool` with an 8 KiB output bound, then one `turn/usage` and one
  `turn/done`. The terminal frame is computed by the host, not read from the bus.
- Usage the provider never reported is `{"reported": false}` with no counters,
  never zero.
- Cancellation uses the loop's watch channel: `turn/cancel` acknowledges
  immediately, the turn ends with `stopReason: "cancelled"`, and the next turn is
  accepted.
- A provider failure ends the turn with `turn/error` carrying the E001 class and
  `providerStatus` when known; the process and the session stay alive.
- stdin EOF cancels the active turn, closes the session and exits 0. An
  unparseable, oversized or non-UTF-8 frame is refused and exits
  `EXIT_UNPARSEABLE_FRAME` (2).
- A descriptor session has exactly one provider: the model id is the host's own
  string, with only the descriptor's `<id>/` prefix stripped. A model id that
  contains a slash keeps it, because remapping it would send a different model
  than the host asked for.

## Evidence

### Host suite

`cargo test -p codegg --lib host:: -- --test-threads=1` — **21 passed, 0 failed**:

```
host::tests::handshake_open_stream_tools_usage_and_close ... ok
host::tests::a_busy_turn_is_refused_cancelling_is_bounded_and_the_next_turn_runs ... ok
host::tests::a_provider_error_ends_the_turn_and_the_session_stays_usable ... ok
host::tests::a_second_open_is_refused_without_smearing_the_first ... ok
host::tests::an_invalid_workspace_leaves_no_session_behind ... ok
host::tests::an_oversized_frame_is_refused_and_exits_non_zero ... ok
host::tests::an_unknown_agent_is_named_and_refused ... ok
host::tests::eof_during_a_turn_exits_zero ... ok
host::tests::invalid_utf8_is_refused_and_exits_non_zero ... ok
host::tests::open_before_initialize_is_refused ... ok
host::tests::session_close_exits_zero ... ok
host::tests::the_descriptor_secret_never_leaves_the_open_frame ... ok
host::tests::unknown_methods_notifications_responses_and_fields_are_handled ... ok
host::session::tests::* (5) ... ok
host::turn::tests::* (5) ... ok
```

The scripted-host harness runs `run_stdio_with_seam` over a duplex against an
in-process fake provider, so no child process and no network are involved.

### Wire frames

The golden/round-trip matrix in `crates/codegg-host-protocol` (20 cases) pins
every request, result and notification, the camelCase descriptor object, the
usage-absence shape, the busy frame and the full error-class table.

### `exec` regression

`cargo test -p codegg --bin codegg exec` — unchanged suite, unchanged JSON shape.

### Redaction

`the_descriptor_secret_never_leaves_the_open_frame` scans every emitted byte
(stdout frames plus captured stderr) for the descriptor's key and asserts it
appears only inside the `session/open` frame that carried it.

## Deviations from the plan

- The plan suggested moving the shared runtime out of `ExecMode`. It lives in
  `src/headless.rs` and the host uses it; `exec` still owns its own copy of the
  setup because another writer held `src/exec.rs`. Delegating `exec` to
  `HeadlessRuntime` is a mechanical follow-up and changes no behaviour.
- `src/host/protocol.rs` is now a re-export of `crates/codegg-host-protocol`
  rather than the definition, per E003's extraction.
